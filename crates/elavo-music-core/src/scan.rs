//! Scanning a folder into the library. A rescan reads only files whose size
//! or modification time changed, and drops songs whose files are gone.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::art::ArtCache;
use crate::db::Library;
use crate::model::Track;
use crate::tags;

#[derive(Debug, Default, PartialEq)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// Files that could not be read, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

/// Every audio file under `root`, depth first. Hidden folders are skipped
/// and symlinked folders are not followed, so a loop cannot trap the scan.
pub fn find_audio(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !hidden {
                    stack.push(path);
                }
            } else if !hidden && tags::is_audio(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn stamp(path: &Path) -> Option<(u64, i64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

/// Scans `root` into `lib`. `progress` is called with the number of files
/// looked at so far.
pub fn scan_folder(
    lib: &Library,
    root: &Path,
    art: &ArtCache,
    mut progress: impl FnMut(usize),
) -> Result<ScanReport, rusqlite::Error> {
    let files = find_audio(root);
    let known = lib.known_files()?;
    let mut report = ScanReport::default();
    let mut seen: HashSet<String> = HashSet::new();
    let added_at = now();

    lib.batch(|lib| {
        for (n, path) in files.iter().enumerate() {
            progress(n + 1);
            let key = path.to_string_lossy().into_owned();
            seen.insert(key.clone());
            let Some((size, mtime)) = stamp(path) else { continue };
            let existing = known.get(&key);
            if existing == Some(&(size, mtime)) {
                report.unchanged += 1;
                continue;
            }
            let parsed = match tags::read(path) {
                Ok(parsed) => parsed,
                Err(err) => {
                    report.failed.push((path.clone(), err.to_string()));
                    continue;
                }
            };
            let art_key = parsed
                .picture
                .as_ref()
                .and_then(|(data, mime)| art.store(data, mime.as_deref()).ok())
                .or_else(|| {
                    let folder = ArtCache::folder_art(path.parent()?)?;
                    let data = fs::read(&folder).ok()?;
                    art.store(&data, None).ok()
                });
            let id = lib.upsert_track(&Track {
                path: key,
                size,
                mtime,
                title: parsed.title,
                artist: parsed.artist,
                album_artist: parsed.album_artist,
                album: parsed.album,
                genre: parsed.genre,
                year: parsed.year,
                track_no: parsed.track_no,
                disc_no: parsed.disc_no,
                duration_ms: parsed.duration_ms,
                added_at,
                art: art_key,
                ..Track::default()
            })?;
            // A rewrite of the song's row must not lose what an online
            // lookup filled in for it.
            lib.apply_online(id)?;
            if existing.is_some() {
                report.updated += 1;
            } else {
                report.added += 1;
            }
        }
        // Songs under this folder whose files are gone.
        let prefix = {
            let mut p = root.to_string_lossy().into_owned();
            if !p.ends_with('/') {
                p.push('/');
            }
            p
        };
        for path in known.keys() {
            if path.starts_with(&prefix) && !seen.contains(path) && lib.remove_path(path)? {
                report.removed += 1;
            }
        }
        Ok(())
    })?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::config::WriteOptions;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::prelude::*;
    use lofty::tag::{Tag, TagType};

    /// A valid 16-bit mono WAV of silence, `ms` long.
    fn wav(ms: u32) -> Vec<u8> {
        let rate = 8000u32;
        let samples = rate * ms / 1000;
        let data_len = samples * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(out.len() + data_len as usize, 0);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn write_song(path: &Path, title: &str, artist: &str, album: &str, track: u32, picture: Option<&[u8]>) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, wav(1500)).unwrap();
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.into());
        tag.set_artist(artist.into());
        tag.set_album(album.into());
        tag.set_genre("Test".into());
        tag.set_track(track);
        if let Some(bytes) = picture {
            tag.push_picture(Picture::unchecked(bytes.to_vec()).pic_type(PictureType::CoverFront).mime_type(MimeType::Png).build());
        }
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 13, 10, 26, 10, 1, 2, 3, 4];

    #[test]
    fn scans_tags_duration_and_artwork() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        write_song(&music.join("Alpha/First/01.wav"), "Dawn", "Alpha", "First", 1, Some(PNG));
        write_song(&music.join("Alpha/First/02.wav"), "Dusk", "Alpha", "First", 2, Some(PNG));
        fs::write(music.join("Alpha/First/readme.txt"), "x").unwrap();
        let lib = Library::open_in_memory().unwrap();
        let art = ArtCache::new(dir.path().join("art"));

        let report = scan_folder(&lib, &music, &art, |_| {}).unwrap();
        assert_eq!(report.added, 2, "{report:?}");
        assert!(report.failed.is_empty(), "{:?}", report.failed);

        let tracks = lib.album_tracks("Alpha", "First").unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "Dawn");
        assert_eq!(tracks[0].track_no, 1);
        assert_eq!(tracks[0].genre, "Test");
        assert!((1400..=1600).contains(&tracks[0].duration_ms), "{}", tracks[0].duration_ms);
        let key = tracks[0].art.clone().expect("artwork");
        assert_eq!(tracks[1].art.as_deref(), Some(key.as_str()), "same picture, one cache file");
        assert_eq!(fs::read(art.path(&key)).unwrap(), PNG);
    }

    #[test]
    fn rescan_skips_unchanged_updates_changed_and_drops_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        let one = music.join("one.wav");
        let two = music.join("two.wav");
        write_song(&one, "One", "A", "X", 1, None);
        write_song(&two, "Two", "A", "X", 2, None);
        let lib = Library::open_in_memory().unwrap();
        let art = ArtCache::new(dir.path().join("art"));
        scan_folder(&lib, &music, &art, |_| {}).unwrap();

        let again = scan_folder(&lib, &music, &art, |_| {}).unwrap();
        assert_eq!((again.added, again.updated, again.unchanged, again.removed), (0, 0, 2, 0));

        // Retag one (a different size), delete the other.
        write_song(&one, "One Retitled", "A", "X", 1, Some(PNG));
        fs::remove_file(&two).unwrap();
        let third = scan_folder(&lib, &music, &art, |_| {}).unwrap();
        assert_eq!((third.added, third.updated, third.removed), (0, 1, 1), "{third:?}");
        let all = lib.tracks(&Default::default(), crate::db::TrackOrder::Library).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "One Retitled");
    }

    #[test]
    fn untagged_file_gets_library_defaults_and_folder_art() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        fs::create_dir_all(&music).unwrap();
        fs::write(music.join("loose song.wav"), wav(500)).unwrap();
        fs::write(music.join("cover.png"), PNG).unwrap();
        let lib = Library::open_in_memory().unwrap();
        let art = ArtCache::new(dir.path().join("art"));
        scan_folder(&lib, &music, &art, |_| {}).unwrap();
        let t = &lib.tracks(&Default::default(), crate::db::TrackOrder::Library).unwrap()[0];
        assert_eq!(t.title, "loose song");
        assert_eq!(t.artist, tags::UNKNOWN_ARTIST);
        assert_eq!(t.album, tags::UNKNOWN_ALBUM);
        assert!(t.art.is_some(), "cover.png beside the file");
    }

    #[test]
    fn unreadable_files_are_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        fs::create_dir_all(&music).unwrap();
        fs::write(music.join("broken.mp3"), b"not audio at all").unwrap();
        write_song(&music.join("good.wav"), "Good", "A", "B", 1, None);
        let lib = Library::open_in_memory().unwrap();
        let art = ArtCache::new(dir.path().join("art"));
        let report = scan_folder(&lib, &music, &art, |_| {}).unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(report.failed.len(), 1);
    }

    #[test]
    fn find_audio_skips_hidden_and_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        for f in ["a/1.mp3", "a/b/2.FLAC", ".hidden/3.mp3", "a/cover.jpg", "a/.4.mp3"] {
            fs::write(root.join(f), b"x").unwrap();
        }
        let found: Vec<_> = find_audio(root).into_iter().map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(found, ["a/1.mp3", "a/b/2.FLAC"]);
    }
}
