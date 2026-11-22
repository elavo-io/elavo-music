//! Where Music keeps things. The XDG variables are honoured, so a test
//! session can point all of it somewhere private.

use std::env;
use std::path::PathBuf;

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match env::var_os(var) {
        Some(value) if !value.is_empty() && PathBuf::from(&value).is_absolute() => {
            PathBuf::from(value)
        }
        _ => home().join(fallback),
    }
}

/// `~/.local/share/elavo-music`, with the library database in it.
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("elavo-music")
}

pub fn database() -> PathBuf {
    data_dir().join("library.db")
}

/// `~/.cache/elavo-music`, with the artwork cache in it.
pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("elavo-music")
}

pub fn art_dir() -> PathBuf {
    cache_dir().join("art")
}

/// `~/.config/elavo/music.conf`, next to the other Elavo apps' files.
pub fn settings_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("elavo").join("music.conf")
}

/// The folder scanned by default: the XDG music folder, else `~/Music`.
pub fn default_music_dir() -> PathBuf {
    if let Some(dir) = user_dirs_music() {
        return dir;
    }
    home().join("Music")
}

fn user_dirs_music() -> Option<PathBuf> {
    let file = xdg("XDG_CONFIG_HOME", ".config").join("user-dirs.dirs");
    let text = std::fs::read_to_string(file).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("XDG_MUSIC_DIR=") {
            let value = value.trim().trim_matches('"');
            let value = value.replace("$HOME", &home().to_string_lossy());
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return Some(path);
            }
        }
    }
    None
}
