//! The library database: one SQLite file under `~/.local/share/elavo-music`.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, params_from_iter, Connection, Row};

use crate::model::{Album, Artist, Filter, Track};

pub type Result<T> = rusqlite::Result<T>;

const SCHEMA_VERSION: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackOrder {
    /// Artist, album, disc, track: the order of a library.
    Library,
    Title,
    RecentlyAdded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlbumOrder {
    Title,
    Artist,
    RecentlyAdded,
}

pub struct Library {
    conn: Connection,
}

const TRACK_COLUMNS: &str = "id, path, size, mtime, title, artist, album_artist, album, genre, \
     year, track_no, disc_no, duration_ms, plays, rating, added_at, art";

fn track_from_row(row: &Row) -> Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        path: row.get(1)?,
        size: row.get::<_, i64>(2)? as u64,
        mtime: row.get(3)?,
        title: row.get(4)?,
        artist: row.get(5)?,
        album_artist: row.get(6)?,
        album: row.get(7)?,
        genre: row.get(8)?,
        year: row.get::<_, i64>(9)? as u32,
        track_no: row.get::<_, i64>(10)? as u32,
        disc_no: row.get::<_, i64>(11)? as u32,
        duration_ms: row.get::<_, i64>(12)? as u64,
        plays: row.get::<_, i64>(13)? as u32,
        rating: row.get::<_, i64>(14)? as u8,
        added_at: row.get(15)?,
        art: row.get(16)?,
    })
}

/// Escapes `%`, `_` and `\` so the text is matched literally by LIKE.
fn like_pattern(text: &str) -> String {
    let mut out = String::from("%");
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// The WHERE clause for a filter, and its parameters in order.
fn where_clause(filter: &Filter) -> (String, Vec<String>) {
    let mut parts = Vec::new();
    let mut args = Vec::new();
    for word in filter.text.split_whitespace() {
        parts.push(
            "(title LIKE ? ESCAPE '\\' OR artist LIKE ? ESCAPE '\\' OR album_artist LIKE ? \
             ESCAPE '\\' OR album LIKE ? ESCAPE '\\' OR genre LIKE ? ESCAPE '\\')"
                .to_string(),
        );
        let pattern = like_pattern(word);
        args.extend(std::iter::repeat_n(pattern, 5));
    }
    if let Some(genre) = &filter.genre {
        parts.push("genre = ? COLLATE NOCASE".to_string());
        args.push(genre.clone());
    }
    if let Some(artist) = &filter.album_artist {
        parts.push("album_artist = ?".to_string());
        args.push(artist.clone());
    }
    if let Some(album) = &filter.album {
        parts.push("album = ?".to_string());
        args.push(album.clone());
    }
    if parts.is_empty() {
        (String::new(), args)
    } else {
        (format!("WHERE {}", parts.join(" AND ")), args)
    }
}

impl Library {
    pub fn open(path: &Path) -> Result<Library> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Library> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Library> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let lib = Library { conn };
        lib.migrate()?;
        Ok(lib)
    }

    fn migrate(&self) -> Result<()> {
        let version: i32 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "CREATE TABLE tracks (
                    id INTEGER PRIMARY KEY,
                    path TEXT NOT NULL UNIQUE,
                    size INTEGER NOT NULL,
                    mtime INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    artist TEXT NOT NULL,
                    album_artist TEXT NOT NULL,
                    album TEXT NOT NULL,
                    genre TEXT NOT NULL,
                    year INTEGER NOT NULL DEFAULT 0,
                    track_no INTEGER NOT NULL DEFAULT 0,
                    disc_no INTEGER NOT NULL DEFAULT 0,
                    duration_ms INTEGER NOT NULL DEFAULT 0,
                    plays INTEGER NOT NULL DEFAULT 0,
                    rating INTEGER NOT NULL DEFAULT 0,
                    added_at INTEGER NOT NULL,
                    art TEXT
                );
                CREATE INDEX tracks_album ON tracks (album_artist, album);
                CREATE INDEX tracks_genre ON tracks (genre);",
            )?;
        }
        if version < SCHEMA_VERSION {
            self.conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    /// Runs `work` in one transaction; a scan of thousands of files is
    /// minutes slower without it.
    pub fn batch<R>(&self, work: impl FnOnce(&Library) -> Result<R>) -> Result<R> {
        self.conn.execute_batch("BEGIN")?;
        match work(self) {
            Ok(value) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    /// Adds a song, or refreshes the tags of the one at the same path. Plays,
    /// rating and the date added are kept.
    pub fn upsert_track(&self, track: &Track) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO tracks (path, size, mtime, title, artist, album_artist, album, genre,
                                 year, track_no, disc_no, duration_ms, plays, rating, added_at, art)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(path) DO UPDATE SET
                size = excluded.size, mtime = excluded.mtime, title = excluded.title,
                artist = excluded.artist, album_artist = excluded.album_artist,
                album = excluded.album, genre = excluded.genre, year = excluded.year,
                track_no = excluded.track_no, disc_no = excluded.disc_no,
                duration_ms = excluded.duration_ms, art = excluded.art",
            params![
                track.path,
                track.size as i64,
                track.mtime,
                track.title,
                track.artist,
                track.album_artist,
                track.album,
                track.genre,
                track.year as i64,
                track.track_no as i64,
                track.disc_no as i64,
                track.duration_ms as i64,
                track.plays as i64,
                track.rating as i64,
                track.added_at,
                track.art,
            ],
        )?;
        self.conn
            .query_row("SELECT id FROM tracks WHERE path = ?1", [&track.path], |r| r.get(0))
    }

    /// Every stored path with its size and modification time, for the
    /// rescan that skips files that have not changed.
    pub fn known_files(&self) -> Result<HashMap<String, (u64, i64)>> {
        let mut stmt = self.conn.prepare("SELECT path, size, mtime FROM tracks")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)?)))
        })?;
        rows.collect()
    }

    pub fn remove_path(&self, path: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM tracks WHERE path = ?1", [path])? > 0)
    }

    pub fn count(&self) -> Result<u64> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    pub fn track(&self, id: i64) -> Result<Option<Track>> {
        let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map([id], track_from_row)?;
        rows.next().transpose()
    }

    pub fn tracks(&self, filter: &Filter, order: TrackOrder) -> Result<Vec<Track>> {
        let (clause, args) = where_clause(filter);
        let order = match order {
            TrackOrder::Library => {
                "album_artist COLLATE NOCASE, album COLLATE NOCASE, disc_no, track_no, \
                 title COLLATE NOCASE"
            }
            TrackOrder::Title => "title COLLATE NOCASE, artist COLLATE NOCASE",
            TrackOrder::RecentlyAdded => "added_at DESC, album_artist COLLATE NOCASE, album, disc_no, track_no",
        };
        let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks {clause} ORDER BY {order}");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), track_from_row)?;
        rows.collect()
    }

    /// The songs of one album, in disc and track order.
    pub fn album_tracks(&self, album_artist: &str, album: &str) -> Result<Vec<Track>> {
        let filter = Filter {
            album_artist: Some(album_artist.to_string()),
            album: Some(album.to_string()),
            ..Filter::default()
        };
        let (clause, args) = where_clause(&filter);
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks {clause} ORDER BY disc_no, track_no, title COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), track_from_row)?;
        rows.collect()
    }

    pub fn albums(&self, filter: &Filter, order: AlbumOrder, limit: Option<u32>) -> Result<Vec<Album>> {
        let (clause, args) = where_clause(filter);
        let order = match order {
            AlbumOrder::Title => "album COLLATE NOCASE, album_artist COLLATE NOCASE",
            AlbumOrder::Artist => "album_artist COLLATE NOCASE, MIN(year), album COLLATE NOCASE",
            AlbumOrder::RecentlyAdded => "MAX(added_at) DESC, album_artist COLLATE NOCASE, album COLLATE NOCASE",
        };
        let limit = limit.map(|n| format!("LIMIT {n}")).unwrap_or_default();
        let sql = format!(
            "SELECT album_artist, album, MAX(year), MIN(genre), COUNT(*), SUM(duration_ms),
                    MAX(art), MAX(added_at)
             FROM tracks {clause} GROUP BY album_artist, album ORDER BY {order} {limit}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), |r| {
            Ok(Album {
                album_artist: r.get(0)?,
                title: r.get(1)?,
                year: r.get::<_, i64>(2)? as u32,
                genre: r.get(3)?,
                track_count: r.get::<_, i64>(4)? as u32,
                duration_ms: r.get::<_, i64>(5)? as u64,
                art: r.get(6)?,
                added_at: r.get(7)?,
            })
        })?;
        rows.collect()
    }

    pub fn artists(&self, filter: &Filter) -> Result<Vec<Artist>> {
        let (clause, args) = where_clause(filter);
        let sql = format!(
            "SELECT album_artist, COUNT(DISTINCT album), COUNT(*) FROM tracks {clause}
             GROUP BY album_artist ORDER BY album_artist COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), |r| {
            Ok(Artist {
                name: r.get(0)?,
                album_count: r.get::<_, i64>(1)? as u32,
                track_count: r.get::<_, i64>(2)? as u32,
            })
        })?;
        rows.collect()
    }

    /// Genres in use, for the filter menu. Blank genres are left out.
    pub fn genres(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT genre FROM tracks WHERE genre <> '' GROUP BY genre COLLATE NOCASE
             ORDER BY genre COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    }

    pub fn record_play(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE tracks SET plays = plays + 1 WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(path: &str, title: &str, artist: &str, album: &str, genre: &str, track_no: u32) -> Track {
        Track {
            path: path.into(),
            size: 10,
            mtime: 100,
            title: title.into(),
            artist: artist.into(),
            album_artist: artist.into(),
            album: album.into(),
            genre: genre.into(),
            year: 2001,
            track_no,
            disc_no: 1,
            duration_ms: 60_000,
            added_at: 1000,
            ..Track::default()
        }
    }

    fn sample() -> Library {
        let lib = Library::open_in_memory().unwrap();
        for t in [
            song("/m/a1.mp3", "Dawn", "Alpha", "First Light", "Ambient", 1),
            song("/m/a2.mp3", "Dusk", "Alpha", "First Light", "Ambient", 2),
            song("/m/b1.mp3", "Rock 100%", "Beta", "Loud", "Rock", 1),
            song("/m/c1.mp3", "Calm", "Gamma", "Quiet", "ambient", 1),
        ] {
            lib.upsert_track(&t).unwrap();
        }
        lib
    }

    #[test]
    fn upsert_inserts_then_updates_in_place() {
        let lib = sample();
        assert_eq!(lib.count().unwrap(), 4);
        let id = lib.upsert_track(&song("/m/a1.mp3", "Dawn", "Alpha", "First Light", "Ambient", 1)).unwrap();
        lib.record_play(id).unwrap();
        lib.record_play(id).unwrap();
        let mut again = song("/m/a1.mp3", "Dawn (Remaster)", "Alpha", "First Light", "Ambient", 1);
        again.added_at = 5000;
        let same = lib.upsert_track(&again).unwrap();
        assert_eq!(id, same);
        assert_eq!(lib.count().unwrap(), 4);
        let t = lib.track(id).unwrap().unwrap();
        assert_eq!(t.title, "Dawn (Remaster)");
        assert_eq!(t.plays, 2, "plays survive a retag");
        assert_eq!(t.added_at, 1000, "date added survives a retag");
    }

    #[test]
    fn tracks_filter_by_text_and_genre() {
        let lib = sample();
        let all = lib.tracks(&Filter::default(), TrackOrder::Library).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].title, "Dawn");

        let hit = lib.tracks(&Filter::text("dusk"), TrackOrder::Library).unwrap();
        assert_eq!(hit.len(), 1);
        // Several words must all match, in any column.
        let both = lib.tracks(&Filter::text("alpha light"), TrackOrder::Library).unwrap();
        assert_eq!(both.len(), 2);
        let none = lib.tracks(&Filter::text("alpha loud"), TrackOrder::Library).unwrap();
        assert!(none.is_empty());
        // % is matched literally.
        let pct = lib.tracks(&Filter::text("100%"), TrackOrder::Library).unwrap();
        assert_eq!(pct.len(), 1);
        let wild = lib.tracks(&Filter::text("%"), TrackOrder::Library).unwrap();
        assert_eq!(wild.len(), 1);

        let genre = Filter { genre: Some("Ambient".into()), ..Filter::default() };
        assert_eq!(lib.tracks(&genre, TrackOrder::Title).unwrap().len(), 3, "genre ignores case");
    }

    #[test]
    fn albums_and_artists_group() {
        let lib = sample();
        let albums = lib.albums(&Filter::default(), AlbumOrder::Title, None).unwrap();
        assert_eq!(albums.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["First Light", "Loud", "Quiet"]);
        let first = &albums[0];
        assert_eq!(first.track_count, 2);
        assert_eq!(first.duration_ms, 120_000);
        let artists = lib.artists(&Filter::default()).unwrap();
        assert_eq!(artists.len(), 3);
        assert_eq!(artists[0], Artist { name: "Alpha".into(), album_count: 1, track_count: 2 });
        let limited = lib.albums(&Filter::default(), AlbumOrder::Artist, Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn recently_added_orders_newest_first() {
        let lib = sample();
        let mut newer = song("/m/d1.mp3", "New", "Delta", "Fresh", "Pop", 1);
        newer.added_at = 9000;
        lib.upsert_track(&newer).unwrap();
        let albums = lib.albums(&Filter::default(), AlbumOrder::RecentlyAdded, Some(1)).unwrap();
        assert_eq!(albums[0].title, "Fresh");
    }

    #[test]
    fn album_tracks_in_disc_and_track_order() {
        let lib = Library::open_in_memory().unwrap();
        let mut t = song("/m/2.mp3", "Two", "A", "Al", "G", 2);
        lib.upsert_track(&t).unwrap();
        t = song("/m/1.mp3", "One", "A", "Al", "G", 1);
        lib.upsert_track(&t).unwrap();
        t = song("/m/d2.mp3", "Disc Two", "A", "Al", "G", 1);
        t.disc_no = 2;
        lib.upsert_track(&t).unwrap();
        let titles: Vec<_> = lib.album_tracks("A", "Al").unwrap().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["One", "Two", "Disc Two"]);
    }

    #[test]
    fn known_files_and_removal() {
        let lib = sample();
        let known = lib.known_files().unwrap();
        assert_eq!(known.get("/m/a1.mp3"), Some(&(10, 100)));
        assert!(lib.remove_path("/m/a1.mp3").unwrap());
        assert!(!lib.remove_path("/m/a1.mp3").unwrap());
        assert_eq!(lib.count().unwrap(), 3);
    }

    #[test]
    fn genres_are_distinct_ignoring_case() {
        let lib = sample();
        let genres = lib.genres().unwrap();
        assert_eq!(genres.len(), 2, "{genres:?}");
    }

    #[test]
    fn batch_rolls_back_on_error() {
        let lib = Library::open_in_memory().unwrap();
        let result: Result<()> = lib.batch(|l| {
            l.upsert_track(&song("/m/x.mp3", "X", "A", "B", "G", 1))?;
            Err(rusqlite::Error::QueryReturnedNoRows)
        });
        assert!(result.is_err());
        assert_eq!(lib.count().unwrap(), 0);
    }

    #[test]
    fn database_persists_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("library.db");
        {
            let lib = Library::open(&path).unwrap();
            lib.upsert_track(&song("/m/x.mp3", "X", "A", "B", "G", 1)).unwrap();
        }
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.count().unwrap(), 1);
    }
}
