//! Reading tags and artwork from audio files, with lofty.

use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};

/// File extensions the scanner picks up.
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "wv"];

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// What a file says about itself, with blanks filled in the way a library
/// shows them: "Unknown Artist", "Unknown Album", the file name for a title.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    pub year: u32,
    pub track_no: u32,
    pub disc_no: u32,
    pub duration_ms: u64,
    pub picture: Option<(Vec<u8>, Option<String>)>,
}

pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
pub const UNKNOWN_ALBUM: &str = "Unknown Album";

fn clean(value: Option<impl AsRef<str>>) -> String {
    value.map(|v| v.as_ref().trim().to_string()).unwrap_or_default()
}

fn best_picture(tag: &Tag) -> Option<(Vec<u8>, Option<String>)> {
    let pictures = tag.pictures();
    let pic = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())?;
    Some((pic.data().to_vec(), pic.mime_type().map(|m| m.as_str().to_string())))
}

pub fn read(path: &Path) -> Result<Parsed, lofty::error::FileParseError> {
    let tagged = lofty::read_from_path(path)?;
    let duration_ms = tagged.properties().duration().as_millis() as u64;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let mut parsed = Parsed { duration_ms, ..Parsed::default() };
    if let Some(tag) = tag {
        parsed.title = clean(tag.title());
        parsed.artist = clean(tag.artist());
        parsed.album = clean(tag.album());
        parsed.genre = clean(tag.genre());
        parsed.album_artist = clean(tag.get_string(ItemKey::AlbumArtist));
        parsed.year = tag.date().map(|d| u32::from(d.year)).unwrap_or(0);
        parsed.track_no = tag.track().unwrap_or(0);
        parsed.disc_no = tag.disk().unwrap_or(0);
        parsed.picture = best_picture(tag);
    }
    Ok(normalize(parsed, path))
}

/// Fills blanks. Kept apart from `read` so it can be tested without a file.
pub fn normalize(mut p: Parsed, path: &Path) -> Parsed {
    if p.title.is_empty() {
        p.title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
    }
    if p.artist.is_empty() {
        p.artist = if p.album_artist.is_empty() { UNKNOWN_ARTIST.into() } else { p.album_artist.clone() };
    }
    if p.album_artist.is_empty() {
        p.album_artist = p.artist.clone();
    }
    if p.album.is_empty() {
        p.album = UNKNOWN_ALBUM.into();
    }
    if p.disc_no == 0 {
        p.disc_no = 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_extensions_ignore_case() {
        assert!(is_audio(Path::new("/m/a.MP3")));
        assert!(is_audio(Path::new("/m/a.flac")));
        assert!(!is_audio(Path::new("/m/a.jpg")));
        assert!(!is_audio(Path::new("/m/noext")));
    }

    #[test]
    fn normalize_fills_blanks() {
        let p = normalize(Parsed::default(), Path::new("/m/01 Some Song.mp3"));
        assert_eq!(p.title, "01 Some Song");
        assert_eq!(p.artist, UNKNOWN_ARTIST);
        assert_eq!(p.album_artist, UNKNOWN_ARTIST);
        assert_eq!(p.album, UNKNOWN_ALBUM);
        assert_eq!(p.disc_no, 1);
    }

    #[test]
    fn album_artist_stands_in_for_a_missing_artist_and_back() {
        let only_aa = normalize(Parsed { album_artist: "Band".into(), ..Parsed::default() }, Path::new("x.mp3"));
        assert_eq!(only_aa.artist, "Band");
        let only_artist = normalize(Parsed { artist: "Solo".into(), ..Parsed::default() }, Path::new("x.mp3"));
        assert_eq!(only_artist.album_artist, "Solo");
        let both = normalize(
            Parsed { artist: "Guest".into(), album_artist: "Host".into(), ..Parsed::default() },
            Path::new("x.mp3"),
        );
        assert_eq!((both.artist.as_str(), both.album_artist.as_str()), ("Guest", "Host"));
    }
}
