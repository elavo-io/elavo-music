//! Settings. Only the model for now: the window to change them comes later.
//! They live in `~/.config/elavo/music.conf` as `key=value` lines.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The format a ripped disc is encoded to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RipFormat {
    /// AAC in an `.m4a`. The default: it sounds better than MP3 at the same
    /// size and an iPod Classic plays it without a conversion on every sync.
    #[default]
    Aac,
    Mp3,
    /// Apple Lossless: lossless, and still playable on an iPod Classic.
    Alac,
    Flac,
}

impl RipFormat {
    pub const ALL: [RipFormat; 4] = [RipFormat::Aac, RipFormat::Mp3, RipFormat::Alac, RipFormat::Flac];

    pub fn label(self) -> &'static str {
        match self {
            RipFormat::Aac => "AAC",
            RipFormat::Mp3 => "MP3",
            RipFormat::Alac => "Apple Lossless (ALAC)",
            RipFormat::Flac => "FLAC",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            RipFormat::Aac | RipFormat::Alac => "m4a",
            RipFormat::Mp3 => "mp3",
            RipFormat::Flac => "flac",
        }
    }

    pub fn is_lossless(self) -> bool {
        matches!(self, RipFormat::Alac | RipFormat::Flac)
    }

    fn key(self) -> &'static str {
        match self {
            RipFormat::Aac => "aac",
            RipFormat::Mp3 => "mp3",
            RipFormat::Alac => "alac",
            RipFormat::Flac => "flac",
        }
    }

    fn from_key(key: &str) -> Option<RipFormat> {
        RipFormat::ALL.into_iter().find(|f| f.key() == key)
    }
}

/// Whether Music may look songs up online (MusicBrainz and the Cover Art
/// Archive). Until the person answers, it is `Ask`, which sends nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OnlineChoice {
    /// Not asked yet. Nothing is sent.
    #[default]
    Ask,
    On,
    Off,
}

impl OnlineChoice {
    fn key(self) -> &'static str {
        match self {
            OnlineChoice::Ask => "ask",
            OnlineChoice::On => "on",
            OnlineChoice::Off => "off",
        }
    }

    fn from_key(key: &str) -> OnlineChoice {
        match key {
            "on" => OnlineChoice::On,
            "off" => OnlineChoice::Off,
            _ => OnlineChoice::Ask,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RipSettings {
    pub format: RipFormat,
    /// Target (or, with `vbr`, average) bitrate for the lossy formats.
    pub bitrate_kbps: u32,
    pub vbr: bool,
}

impl Default for RipSettings {
    /// AAC at 256 kbps, variable bitrate.
    fn default() -> Self {
        RipSettings { format: RipFormat::Aac, bitrate_kbps: 256, vbr: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub music_folder: PathBuf,
    pub rip: RipSettings,
    pub online_metadata: OnlineChoice,
}

impl Settings {
    pub fn new(music_folder: PathBuf) -> Settings {
        Settings { music_folder, rip: RipSettings::default(), online_metadata: OnlineChoice::default() }
    }

    pub fn parse(text: &str, default_folder: PathBuf) -> Settings {
        let mut settings = Settings::new(default_folder);
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "music-folder" if !value.is_empty() => settings.music_folder = PathBuf::from(value),
                "rip-format" => {
                    if let Some(f) = RipFormat::from_key(value) {
                        settings.rip.format = f;
                    }
                }
                "rip-bitrate" => {
                    if let Ok(n) = value.parse::<u32>() {
                        settings.rip.bitrate_kbps = n.clamp(64, 320);
                    }
                }
                "rip-vbr" => settings.rip.vbr = value != "false",
                "online-metadata" => settings.online_metadata = OnlineChoice::from_key(value),
                _ => {}
            }
        }
        settings
    }

    pub fn serialize(&self) -> String {
        format!(
            "music-folder={}\nrip-format={}\nrip-bitrate={}\nrip-vbr={}\nonline-metadata={}\n",
            self.music_folder.display(),
            self.rip.format.key(),
            self.rip.bitrate_kbps,
            self.rip.vbr,
            self.online_metadata.key()
        )
    }

    pub fn load(path: &Path, default_folder: PathBuf) -> Settings {
        match fs::read_to_string(path) {
            Ok(text) => Settings::parse(&text, default_folder),
            Err(_) => Settings::new(default_folder),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.serialize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_aac_256_vbr() {
        let s = Settings::new("/home/k/Music".into());
        assert_eq!(s.rip.format, RipFormat::Aac);
        assert_eq!(s.rip.bitrate_kbps, 256);
        assert!(s.rip.vbr);
        assert_eq!(s.rip.format.extension(), "m4a");
        assert!(!s.rip.format.is_lossless());
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("elavo").join("music.conf");
        let mut s = Settings::new("/srv/music".into());
        s.rip = RipSettings { format: RipFormat::Alac, bitrate_kbps: 192, vbr: false };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path, "/x".into()), s);
    }

    #[test]
    fn bad_values_fall_back_to_defaults() {
        let s = Settings::parse("rip-format=wav\nrip-bitrate=banana\nnonsense\nrip-bitrate=9999\n", "/m".into());
        assert_eq!(s.rip.format, RipFormat::Aac);
        assert_eq!(s.rip.bitrate_kbps, 320, "clamped");
        assert_eq!(s.music_folder, PathBuf::from("/m"));
        assert_eq!(Settings::load(Path::new("/definitely/not/here"), "/m".into()), Settings::new("/m".into()));
    }

    #[test]
    fn online_lookups_are_off_until_asked() {
        let s = Settings::new("/m".into());
        assert_eq!(s.online_metadata, OnlineChoice::Ask);
        assert_eq!(Settings::parse("", "/m".into()).online_metadata, OnlineChoice::Ask);
        assert_eq!(Settings::parse("online-metadata=banana", "/m".into()).online_metadata, OnlineChoice::Ask);
        for choice in [OnlineChoice::On, OnlineChoice::Off, OnlineChoice::Ask] {
            let mut s = Settings::new("/m".into());
            s.online_metadata = choice;
            assert_eq!(Settings::parse(&s.serialize(), "/m".into()).online_metadata, choice);
        }
        // An older file without the key stays at Ask.
        assert_eq!(
            Settings::parse("music-folder=/x\nrip-format=mp3\n", "/m".into()).online_metadata,
            OnlineChoice::Ask
        );
    }

    #[test]
    fn every_format_is_selectable() {
        for f in RipFormat::ALL {
            let s = Settings::parse(&format!("rip-format={}", f.key()), "/m".into());
            assert_eq!(s.rip.format, f);
        }
    }
}
