//! Plain data types shared by the database, the scanner and the UI.

/// One song in the library, as stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub genre: String,
    /// 0 when the file has no year.
    pub year: u32,
    pub track_no: u32,
    pub disc_no: u32,
    pub duration_ms: u64,
    pub plays: u32,
    /// 0 to 100, in steps of 20 for five stars.
    pub rating: u8,
    /// Unix seconds when the song first entered the library.
    pub added_at: i64,
    /// A key into the artwork cache, if the song or its folder has a picture.
    pub art: Option<String>,
}

/// An album: the songs that share an album artist and a title.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Album {
    pub album_artist: String,
    pub title: String,
    pub year: u32,
    pub genre: String,
    pub track_count: u32,
    pub duration_ms: u64,
    pub art: Option<String>,
    pub added_at: i64,
}

/// An album artist and how much of the library is theirs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Artist {
    pub name: String,
    pub album_count: u32,
    pub track_count: u32,
}

/// What a view shows: the "Find in" text, the genre picked in the filter
/// menu, and (for the detail pages) one album artist or album.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub text: String,
    pub genre: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
}

impl Filter {
    pub fn text(text: &str) -> Self {
        Filter {
            text: text.trim().to_string(),
            ..Filter::default()
        }
    }
}

/// What an online lookup found for one song. It lives in the library
/// database beside the song, never in the song's file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnlineMeta {
    pub track_id: i64,
    /// Unix seconds of the lookup.
    pub checked_at: i64,
    /// False when the lookup ran and found nothing.
    pub matched: bool,
    pub recording_mbid: String,
    pub release_mbid: String,
    pub release_group_mbid: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// 0 when unknown.
    pub year: u32,
    /// A key into the artwork cache, if cover art was found.
    pub art: Option<String>,
}
