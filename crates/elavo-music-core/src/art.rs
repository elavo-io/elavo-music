//! The artwork cache: picture bytes from tags and folders, stored once by
//! content under `~/.cache/elavo-music/art`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct ArtCache {
    dir: PathBuf,
}

/// FNV-1a, 64 bit. Only names cache files, so it need not be cryptographic.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// The file extension for a picture, from its MIME type or, failing that,
/// its first bytes.
fn extension(mime: Option<&str>, data: &[u8]) -> &'static str {
    match mime {
        Some("image/png") => return "png",
        Some("image/jpeg") | Some("image/jpg") => return "jpg",
        Some("image/webp") => return "webp",
        Some("image/gif") => return "gif",
        _ => {}
    }
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if data.starts_with(&[0xff, 0xd8]) {
        "jpg"
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        "webp"
    } else {
        "img"
    }
}

const FOLDER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const FOLDER_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

impl ArtCache {
    pub fn new(dir: PathBuf) -> ArtCache {
        ArtCache { dir }
    }

    /// Stores the picture and returns its key (a file name in the cache).
    pub fn store(&self, data: &[u8], mime: Option<&str>) -> io::Result<String> {
        let key = format!("{:016x}.{}", fnv1a(data), extension(mime, data));
        let path = self.dir.join(&key);
        if !path.exists() {
            fs::create_dir_all(&self.dir)?;
            let tmp = self.dir.join(format!("{key}.part"));
            fs::write(&tmp, data)?;
            fs::rename(&tmp, &path)?;
        }
        Ok(key)
    }

    pub fn path(&self, key: &str) -> PathBuf {
        self.dir.join(key)
    }

    /// `cover.jpg`, `folder.png` and the like beside the songs of an album.
    pub fn folder_art(dir: &Path) -> Option<PathBuf> {
        let mut best: Option<(usize, PathBuf)> = None;
        for entry in fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            let stem = path.file_stem()?.to_string_lossy().to_lowercase();
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
            let Some(ext) = ext else { continue };
            if !FOLDER_EXTS.contains(&ext.as_str()) {
                continue;
            }
            if let Some(rank) = FOLDER_NAMES.iter().position(|n| *n == stem) {
                if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                    best = Some((rank, path));
                }
            }
        }
        best.map(|(_, path)| path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3];

    #[test]
    fn store_is_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtCache::new(dir.path().join("art"));
        let a = cache.store(PNG, None).unwrap();
        let b = cache.store(PNG, Some("image/png")).unwrap();
        assert_eq!(a, b);
        assert!(a.ends_with(".png"));
        assert_eq!(fs::read(cache.path(&a)).unwrap(), PNG);
        let other = cache.store(&[0xff, 0xd8, 9], Some("image/jpeg")).unwrap();
        assert_ne!(a, other);
        assert!(other.ends_with(".jpg"));
        assert_eq!(fs::read_dir(dir.path().join("art")).unwrap().count(), 2);
    }

    #[test]
    fn folder_art_prefers_cover() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ArtCache::folder_art(dir.path()), None);
        fs::write(dir.path().join("Folder.JPG"), b"x").unwrap();
        fs::write(dir.path().join("notes.png"), b"x").unwrap();
        assert_eq!(ArtCache::folder_art(dir.path()).unwrap().file_name().unwrap(), "Folder.JPG");
        fs::write(dir.path().join("cover.png"), b"x").unwrap();
        assert_eq!(ArtCache::folder_art(dir.path()).unwrap().file_name().unwrap(), "cover.png");
    }
}
