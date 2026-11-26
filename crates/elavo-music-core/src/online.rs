//! Optional online lookups: song details from MusicBrainz and cover art
//! from the Cover Art Archive. **Nothing here runs unless the person has
//! turned it on** (`Settings::online_metadata`). It sends only a song's
//! title, artist, album and length to musicbrainz.org, and fetches pictures
//! from coverartarchive.org (which redirects to archive.org). There is no
//! other service, no account, no key and no tracking.
//!
//! Rules it follows:
//! - one MusicBrainz request a second at most, as their API terms ask;
//! - a User-Agent that names the app, its version and where to reach it;
//! - results go into the library database and the art cache, never into the
//!   song files, and only fill blanks (see `Library::apply_online`);
//! - HTTPS only.
//!
//! AcoustID would identify a song by its sound but needs an application key
//! and a fingerprint tool, so lookups are by tags only.

use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::art::ArtCache;
use crate::db::Library;
use crate::model::{OnlineMeta, Track};
use crate::settings::OnlineChoice;
use crate::tags::UNKNOWN_ARTIST;

/// Names the app, its version and a place to reach its maker, as
/// MusicBrainz requires.
pub const USER_AGENT: &str = concat!(
    "ElavoMusic/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/elavo-io/elavo-music )"
);

pub const MUSICBRAINZ: &str = "https://musicbrainz.org/ws/2";
pub const COVER_ART_ARCHIVE: &str = "https://coverartarchive.org";

/// MusicBrainz allows one request a second; a little over, to be safe.
pub const MIN_GAP: Duration = Duration::from_millis(1100);
/// A song that matched nothing is looked up again after this long.
pub const RETRY_AFTER_SECS: i64 = 30 * 24 * 3600;
/// Pictures larger than this are not stored.
pub const MAX_ART_BYTES: usize = 8 * 1024 * 1024;
/// Stop a batch after this many failed requests in a row (network down,
/// or the service is asking for a pause).
pub const MAX_CONSECUTIVE_ERRORS: u32 = 3;

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// One blocking GET. A trait so tests can answer from memory.
pub trait Http {
    fn get(&self, url: &str, accept: &str) -> Result<HttpResponse, String>;
}

/// The real thing: reqwest's blocking client, HTTPS only.
pub struct ReqwestHttp {
    client: reqwest::blocking::Client,
}

impl ReqwestHttp {
    pub fn new() -> Result<ReqwestHttp, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .https_only(true)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(ReqwestHttp { client })
    }
}

impl Http for ReqwestHttp {
    fn get(&self, url: &str, accept: &str) -> Result<HttpResponse, String> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, accept)
            .send()
            .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|n| n as usize > MAX_ART_BYTES)
        {
            return Err("response too large".into());
        }
        let body = response.bytes().map_err(|e| e.to_string())?;
        if body.len() > MAX_ART_BYTES {
            return Err("response too large".into());
        }
        Ok(HttpResponse { status, body: body.to_vec() })
    }
}

/// Time, so the rate limit can be tested without waiting.
pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, duration: Duration);
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Keeps requests at least `gap` apart.
pub struct Throttle {
    gap: Duration,
    last: Cell<Option<Instant>>,
}

impl Throttle {
    pub fn new(gap: Duration) -> Throttle {
        Throttle { gap, last: Cell::new(None) }
    }

    /// Blocks until a request may go out, then counts it.
    pub fn wait(&self, clock: &dyn Clock) {
        if let Some(last) = self.last.get() {
            let since = clock.now().saturating_duration_since(last);
            if since < self.gap {
                clock.sleep(self.gap - since);
            }
        }
        self.last.set(Some(clock.now()));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FetchError {
    /// The network is down, the service said to slow down, or it errored.
    /// Worth trying again later.
    Transient(String),
    /// The reply was not what the service documents.
    BadReply(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Transient(m) => write!(f, "online lookup failed: {m}"),
            FetchError::BadReply(m) => write!(f, "unexpected reply: {m}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// What to look for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
}

impl Query {
    pub fn from_track(track: &Track) -> Query {
        let known = |s: &str, unknown: &str| if s == unknown { String::new() } else { s.to_string() };
        Query {
            title: track.title.clone(),
            artist: known(&track.artist, UNKNOWN_ARTIST),
            album: known(&track.album, crate::tags::UNKNOWN_ALBUM),
            duration_ms: track.duration_ms,
        }
    }
}

/// A song MusicBrainz recognised.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub recording_id: String,
    pub title: String,
    pub artist: String,
    pub release_id: String,
    pub release_group_id: String,
    pub album: String,
    pub year: u32,
}

/// Percent-encodes everything but unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A phrase for a Lucene query: quoted, with quotes and backslashes escaped.
fn phrase(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn normalized(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The recording search URL for a query. `with_album` adds the album to it.
pub fn recording_url(base: &str, query: &Query, with_album: bool) -> String {
    let mut lucene = format!("recording:{}", phrase(&query.title));
    if !query.artist.is_empty() {
        lucene.push_str(&format!(" AND artist:{}", phrase(&query.artist)));
    }
    if with_album && !query.album.is_empty() {
        lucene.push_str(&format!(" AND release:{}", phrase(&query.album)));
    }
    format!("{base}/recording?query={}&fmt=json&limit=10", encode(&lucene))
}

fn artist_credit(value: &Value) -> String {
    let mut out = String::new();
    for part in value.as_array().into_iter().flatten() {
        let name = part
            .get("name")
            .or_else(|| part.pointer("/artist/name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        out.push_str(name);
        out.push_str(part.get("joinphrase").and_then(Value::as_str).unwrap_or(""));
    }
    out
}

fn year_of(date: &str) -> u32 {
    date.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0)
}

/// The best recording in a search reply, if any is good enough.
///
/// A match needs a score of 90 or more and, when both lengths are known,
/// to be within five seconds of the file. With no artist to go on, only a
/// perfect score with a matching length is trusted.
pub fn pick(reply: &Value, query: &Query) -> Option<Found> {
    let artist_known = !query.artist.is_empty();
    let mut best: Option<(i64, u64, &Value)> = None;
    for rec in reply.get("recordings")?.as_array()? {
        let score = rec.get("score").and_then(Value::as_i64).unwrap_or(0);
        if score < 90 || (!artist_known && score < 100) {
            continue;
        }
        let length = rec.get("length").and_then(Value::as_u64);
        let gap = match (length, query.duration_ms) {
            (Some(len), dur) if dur > 0 => {
                let gap = len.abs_diff(dur);
                if gap > 5000 {
                    continue;
                }
                gap
            }
            _ if !artist_known => continue,
            _ => 0,
        };
        let better = match &best {
            None => true,
            Some((s, g, _)) => score > *s || (score == *s && gap < *g),
        };
        if better {
            best = Some((score, gap, rec));
        }
    }
    let (_, _, rec) = best?;
    let releases = rec.get("releases").and_then(Value::as_array);
    let wanted = normalized(&query.album);
    let release = releases.and_then(|list| {
        list.iter()
            .find(|r| {
                !wanted.is_empty() && normalized(r.get("title").and_then(Value::as_str).unwrap_or("")) == wanted
            })
            .or_else(|| list.iter().find(|r| r.get("status").and_then(Value::as_str) == Some("Official")))
            .or_else(|| list.first())
    });
    let text = |v: Option<&Value>, key: &str| {
        v.and_then(|v| v.get(key)).and_then(Value::as_str).unwrap_or("").to_string()
    };
    Some(Found {
        recording_id: text(Some(rec), "id"),
        title: text(Some(rec), "title"),
        artist: artist_credit(rec.get("artist-credit").unwrap_or(&Value::Null)),
        release_id: text(release, "id"),
        release_group_id: release
            .and_then(|r| r.pointer("/release-group/id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        album: text(release, "title"),
        year: year_of(&text(release, "date")),
    })
}

fn looks_like_image(data: &[u8]) -> bool {
    data.starts_with(&[0x89, b'P', b'N', b'G']) || data.starts_with(&[0xff, 0xd8])
}

pub struct Online<H: Http, C: Clock> {
    http: H,
    clock: C,
    throttle: Throttle,
    musicbrainz: String,
    cover_art: String,
}

impl Online<ReqwestHttp, SystemClock> {
    pub fn new() -> Result<Self, String> {
        Ok(Online::with(ReqwestHttp::new()?, SystemClock))
    }
}

impl<H: Http, C: Clock> Online<H, C> {
    pub fn with(http: H, clock: C) -> Self {
        Online {
            http,
            clock,
            throttle: Throttle::new(MIN_GAP),
            musicbrainz: MUSICBRAINZ.into(),
            cover_art: COVER_ART_ARCHIVE.into(),
        }
    }

    fn musicbrainz_json(&self, url: &str) -> Result<Value, FetchError> {
        self.throttle.wait(&self.clock);
        let response = self
            .http
            .get(url, "application/json")
            .map_err(FetchError::Transient)?;
        match response.status {
            200 => serde_json::from_slice(&response.body).map_err(|e| FetchError::BadReply(e.to_string())),
            // 503 is how MusicBrainz asks for a pause; 429 likewise.
            429 | 500..=599 => Err(FetchError::Transient(format!("HTTP {}", response.status))),
            other => Err(FetchError::BadReply(format!("HTTP {other}"))),
        }
    }

    /// Looks the song up by its tags. With an album known, the search
    /// includes it first and, if that finds nothing, runs once more without.
    pub fn find_recording(&self, query: &Query) -> Result<Option<Found>, FetchError> {
        if query.title.trim().is_empty() {
            return Ok(None);
        }
        let first = self.musicbrainz_json(&recording_url(&self.musicbrainz, query, true))?;
        if let Some(found) = pick(&first, query) {
            return Ok(Some(found));
        }
        if query.album.is_empty() {
            return Ok(None);
        }
        let second = self.musicbrainz_json(&recording_url(&self.musicbrainz, query, false))?;
        Ok(pick(&second, query))
    }

    /// The front cover of a release, else of its release group. `None`
    /// when the archive has none.
    pub fn front_cover(&self, release_id: &str, release_group_id: &str) -> Result<Option<Vec<u8>>, FetchError> {
        let mut urls = Vec::new();
        if !release_id.is_empty() {
            urls.push(format!("{}/release/{}/front-500", self.cover_art, release_id));
        }
        if !release_group_id.is_empty() {
            urls.push(format!("{}/release-group/{}/front-500", self.cover_art, release_group_id));
        }
        for url in urls {
            let response = self
                .http
                .get(&url, "image/jpeg, image/png")
                .map_err(FetchError::Transient)?;
            match response.status {
                200 if looks_like_image(&response.body) && response.body.len() <= MAX_ART_BYTES => {
                    return Ok(Some(response.body));
                }
                200 => return Err(FetchError::BadReply("not a picture".into())),
                404 => continue,
                429 | 500..=599 => return Err(FetchError::Transient(format!("HTTP {}", response.status))),
                other => return Err(FetchError::BadReply(format!("HTTP {other}"))),
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct BatchReport {
    pub looked_up: usize,
    pub matched: usize,
    pub not_found: usize,
    pub art_added: usize,
    /// Songs skipped because the lookup failed; they stay pending.
    pub failed: usize,
    /// True if the batch stopped before the end (cancelled, or the
    /// network or service kept failing).
    pub stopped_early: bool,
}

/// Looks up songs that have not had a lookup, one at a time, only if
/// `choice` is `On`. With anything else it returns at once and sends nothing.
///
/// `cancel` is asked before each song; `progress(done, total)` after each.
#[allow(clippy::too_many_arguments)]
pub fn run_batch<H: Http, C: Clock>(
    choice: OnlineChoice,
    online: &Online<H, C>,
    lib: &Library,
    art: &ArtCache,
    limit: usize,
    now: i64,
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<BatchReport, rusqlite::Error> {
    let mut report = BatchReport::default();
    if choice != OnlineChoice::On {
        return Ok(report);
    }
    let pending = lib.pending_online(now, RETRY_AFTER_SECS, limit)?;
    let total = pending.len();
    // Pictures already fetched this run, by release.
    let mut covers: HashMap<String, Option<String>> = HashMap::new();
    let mut errors_in_a_row = 0;
    for (n, track) in pending.iter().enumerate() {
        if cancel() {
            report.stopped_early = true;
            break;
        }
        let query = Query::from_track(track);
        let found = match online.find_recording(&query) {
            Ok(found) => found,
            Err(err) => {
                eprintln!("elavo-music: {err}");
                report.failed += 1;
                errors_in_a_row += 1;
                if errors_in_a_row >= MAX_CONSECUTIVE_ERRORS {
                    report.stopped_early = true;
                    break;
                }
                progress(n + 1, total);
                continue;
            }
        };
        errors_in_a_row = 0;
        report.looked_up += 1;
        let mut meta = OnlineMeta { track_id: track.id, checked_at: now, ..OnlineMeta::default() };
        if let Some(found) = found {
            report.matched += 1;
            meta.matched = true;
            meta.recording_mbid = found.recording_id.clone();
            meta.release_mbid = found.release_id.clone();
            meta.release_group_mbid = found.release_group_id.clone();
            meta.title = found.title.clone();
            meta.artist = found.artist.clone();
            meta.album = found.album.clone();
            meta.year = found.year;
            let cover_key = format!("{}/{}", found.release_id, found.release_group_id);
            let has_art = lib.track(track.id)?.is_some_and(|t| t.art.is_some());
            if !found.release_id.is_empty() || !found.release_group_id.is_empty() {
                let key = match covers.get(&cover_key) {
                    Some(key) => key.clone(),
                    None => {
                        let fetched = if has_art {
                            None
                        } else {
                            match online.front_cover(&found.release_id, &found.release_group_id) {
                                Ok(Some(bytes)) => art.store(&bytes, None).ok(),
                                Ok(None) => None,
                                Err(err) => {
                                    eprintln!("elavo-music: {err}");
                                    None
                                }
                            }
                        };
                        covers.insert(cover_key, fetched.clone());
                        fetched
                    }
                };
                meta.art = key;
            }
        } else {
            report.not_found += 1;
        }
        lib.save_online(&meta)?;
        if lib.apply_online(track.id)? && meta.art.is_some() {
            report.art_added += 1;
        }
        progress(n + 1, total);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    /// One recorded request: (URL, User-Agent).
    type Sent = (String, String);

    /// Answers from a list of (URL fragment, status, body), in order of
    /// first match, and records every request.
    struct MockHttp {
        routes: Vec<(String, u16, Vec<u8>)>,
        requests: Rc<RefCell<Vec<(String, String)>>>,
    }

    impl MockHttp {
        fn new(routes: Vec<(&str, u16, Vec<u8>)>) -> (MockHttp, Rc<RefCell<Vec<Sent>>>) {
            let requests = Rc::new(RefCell::new(Vec::new()));
            (
                MockHttp {
                    routes: routes.into_iter().map(|(u, s, b)| (u.to_string(), s, b)).collect(),
                    requests: requests.clone(),
                },
                requests,
            )
        }
    }

    impl Http for MockHttp {
        fn get(&self, url: &str, accept: &str) -> Result<HttpResponse, String> {
            self.requests.borrow_mut().push((url.to_string(), accept.to_string()));
            for (fragment, status, body) in &self.routes {
                if url.contains(fragment.as_str()) {
                    return Ok(HttpResponse { status: *status, body: body.clone() });
                }
            }
            Ok(HttpResponse { status: 404, body: Vec::new() })
        }
    }

    /// A clock that only moves when told to sleep.
    #[derive(Clone)]
    struct FakeClock {
        start: Instant,
        offset: Rc<Cell<Duration>>,
        sleeps: Rc<RefCell<Vec<Duration>>>,
    }

    impl FakeClock {
        fn new() -> FakeClock {
            FakeClock { start: Instant::now(), offset: Rc::default(), sleeps: Rc::default() }
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.start + self.offset.get()
        }
        fn sleep(&self, duration: Duration) {
            self.offset.set(self.offset.get() + duration);
            self.sleeps.borrow_mut().push(duration);
        }
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3, 4];

    fn reply(score: i64, length: u64) -> String {
        format!(
            r#"{{"recordings":[{{"id":"rec-1","score":{score},"title":"Walking","length":{length},
            "artist-credit":[{{"name":"88rising","joinphrase":" & "}},{{"name":"Swae Lee","joinphrase":""}}],
            "releases":[
              {{"id":"rel-bootleg","title":"Other","status":"Bootleg","date":"2019","release-group":{{"id":"rg-x"}}}},
              {{"id":"rel-1","title":"Head In The Clouds II","status":"Official","date":"2019-07-12","release-group":{{"id":"rg-1"}}}}
            ]}}]}}"#
        )
    }

    fn query() -> Query {
        Query {
            title: "Walking".into(),
            artist: "88rising".into(),
            album: "Head In The Clouds II".into(),
            duration_ms: 166_000,
        }
    }

    fn mock(routes: Vec<(&str, u16, Vec<u8>)>) -> (Online<MockHttp, FakeClock>, FakeClock, Rc<RefCell<Vec<Sent>>>) {
        let (http, requests) = MockHttp::new(routes);
        let clock = FakeClock::new();
        (Online::with(http, clock.clone()), clock, requests)
    }

    fn track(path: &str, title: &str, artist: &str, album: &str) -> Track {
        Track {
            path: path.into(),
            size: 1,
            mtime: 1,
            title: title.into(),
            artist: artist.into(),
            album_artist: artist.into(),
            album: album.into(),
            genre: String::new(),
            duration_ms: 166_000,
            disc_no: 1,
            added_at: 1,
            ..Track::default()
        }
    }

    #[test]
    fn user_agent_names_the_app_version_and_contact() {
        assert!(USER_AGENT.starts_with("ElavoMusic/"));
        assert!(USER_AGENT.contains(env!("CARGO_PKG_VERSION")));
        assert!(USER_AGENT.contains("https://github.com/elavo-io/elavo-music"));
    }

    #[test]
    fn search_url_quotes_and_escapes() {
        let q = Query { title: r#"Say "Hi""#.into(), artist: "AC/DC".into(), album: "Back & Forth".into(), duration_ms: 0 };
        let url = recording_url(MUSICBRAINZ, &q, true);
        assert!(url.starts_with("https://musicbrainz.org/ws/2/recording?query="));
        assert!(url.ends_with("&fmt=json&limit=10"));
        assert!(!url.contains(' ') && !url.contains('"'));
        assert_eq!(url.matches('&').count(), 2, "only fmt and limit; the album's & is encoded");
        assert!(url.contains("recording%3A%22Say%20%5C%22Hi%5C%22%22"));
        assert!(url.contains("AND%20artist%3A%22AC%2FDC%22"));
        assert!(url.contains("AND%20release%3A%22Back%20%26%20Forth%22"));
        assert!(!recording_url(MUSICBRAINZ, &q, false).contains("release"));
    }

    #[test]
    fn picks_the_matching_release_and_joins_the_credit() {
        let json: Value = serde_json::from_str(&reply(100, 165_000)).unwrap();
        let found = pick(&json, &query()).unwrap();
        assert_eq!(found.recording_id, "rec-1");
        assert_eq!(found.artist, "88rising & Swae Lee");
        assert_eq!(found.release_id, "rel-1", "the release named like the album");
        assert_eq!(found.release_group_id, "rg-1");
        assert_eq!(found.album, "Head In The Clouds II");
        assert_eq!(found.year, 2019);
    }

    #[test]
    fn prefers_an_official_release_when_no_title_matches() {
        let json: Value = serde_json::from_str(&reply(100, 166_000)).unwrap();
        let q = Query { album: "Something Else".into(), ..query() };
        assert_eq!(pick(&json, &q).unwrap().release_id, "rel-1");
    }

    #[test]
    fn rejects_weak_scores_and_wrong_lengths() {
        let weak: Value = serde_json::from_str(&reply(80, 166_000)).unwrap();
        assert!(pick(&weak, &query()).is_none());
        let long: Value = serde_json::from_str(&reply(100, 240_000)).unwrap();
        assert!(pick(&long, &query()).is_none(), "a different length is a different recording");
        let close: Value = serde_json::from_str(&reply(95, 169_000)).unwrap();
        assert!(pick(&close, &query()).is_some());
    }

    #[test]
    fn without_an_artist_only_a_perfect_match_with_a_length_counts() {
        let q = Query { artist: String::new(), album: String::new(), ..query() };
        let good: Value = serde_json::from_str(&reply(100, 166_000)).unwrap();
        assert!(pick(&good, &q).is_some());
        let good_not_perfect: Value = serde_json::from_str(&reply(95, 166_000)).unwrap();
        assert!(pick(&good_not_perfect, &q).is_none());
        let no_length = Query { duration_ms: 0, ..q };
        assert!(pick(&good, &no_length).is_none());
    }

    #[test]
    fn requests_are_a_second_apart() {
        let (online, clock, requests) = mock(vec![("recording?query", 200, reply(100, 166_000).into_bytes())]);
        for _ in 0..3 {
            online.find_recording(&query()).unwrap().unwrap();
        }
        assert_eq!(requests.borrow().len(), 3);
        let sleeps = clock.sleeps.borrow();
        assert_eq!(sleeps.len(), 2, "no wait before the first request");
        assert!(sleeps.iter().all(|s| *s >= Duration::from_millis(1000)), "{sleeps:?}");
        assert!(MIN_GAP >= Duration::from_secs(1));
    }

    #[test]
    fn a_search_without_the_album_runs_once_more_if_the_first_finds_nothing() {
        let empty = br#"{"recordings":[]}"#.to_vec();
        let (online, clock, requests) = mock(vec![("release%3A", 200, empty), ("recording?query", 200, reply(100, 166_000).into_bytes())]);
        let found = online.find_recording(&query()).unwrap();
        assert!(found.is_some());
        let requests = requests.borrow();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].0.contains("release%3A") && !requests[1].0.contains("release%3A"));
        assert_eq!(clock.sleeps.borrow().len(), 1, "the second search waited its turn too");
        assert!(requests.iter().all(|(u, a)| u.starts_with("https://") && a == "application/json"));
    }

    #[test]
    fn busy_service_is_a_transient_error() {
        let (online, _, _) = mock(vec![("recording?query", 503, Vec::new())]);
        assert!(matches!(online.find_recording(&query()), Err(FetchError::Transient(_))));
        let (online, _, _) = mock(vec![("recording?query", 200, b"<html>".to_vec())]);
        assert!(matches!(online.find_recording(&query()), Err(FetchError::BadReply(_))));
    }

    #[test]
    fn cover_falls_back_to_the_release_group_and_may_be_absent() {
        let (online, _, requests) = mock(vec![("release-group/rg-1/front-500", 200, PNG.to_vec())]);
        assert_eq!(online.front_cover("rel-1", "rg-1").unwrap().unwrap(), PNG);
        let urls: Vec<String> = requests.borrow().iter().map(|r| r.0.clone()).collect();
        assert_eq!(
            urls,
            [
                "https://coverartarchive.org/release/rel-1/front-500",
                "https://coverartarchive.org/release-group/rg-1/front-500"
            ]
        );
        let (none, _, _) = mock(vec![]);
        assert_eq!(none.front_cover("rel-1", "rg-1").unwrap(), None);
        let (html, _, _) = mock(vec![("front-500", 200, b"<html>".to_vec())]);
        assert!(matches!(html.front_cover("rel-1", ""), Err(FetchError::BadReply(_))));
    }

    fn library_with(tracks: &[Track]) -> (Library, Vec<i64>) {
        let lib = Library::open_in_memory().unwrap();
        let ids = tracks.iter().map(|t| lib.upsert_track(t).unwrap()).collect();
        (lib, ids)
    }

    fn run(
        choice: OnlineChoice,
        online: &Online<MockHttp, FakeClock>,
        lib: &Library,
        art: &ArtCache,
    ) -> BatchReport {
        run_batch(choice, online, lib, art, 100, 5_000, &|| false, &mut |_, _| {}).unwrap()
    }

    #[test]
    fn nothing_is_sent_unless_online_lookups_are_on() {
        let (lib, _) = library_with(&[track("/m/a.mp3", "Walking", "88rising", "Head")]);
        let dir = tempfile::tempdir().unwrap();
        let art = ArtCache::new(dir.path().join("art"));
        for choice in [OnlineChoice::Ask, OnlineChoice::Off] {
            let (online, _, requests) = mock(vec![("recording?query", 200, reply(100, 166_000).into_bytes())]);
            let report = run(choice, &online, &lib, &art);
            assert_eq!(report, BatchReport::default());
            assert!(requests.borrow().is_empty(), "{choice:?} must not touch the network");
        }
        assert_eq!(OnlineChoice::default(), OnlineChoice::Ask);
    }

    #[test]
    fn a_lookup_fills_blanks_and_art_without_touching_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("song.mp3");
        std::fs::write(&file, b"not really an mp3").unwrap();
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        let mut a = track(file.to_str().unwrap(), "Walking", "88rising", "Unknown Album");
        a.album = UNKNOWN_ALBUM_FOR_TEST.into();
        let mut b = track("/m/b.mp3", "Second", "88rising", UNKNOWN_ALBUM_FOR_TEST);
        b.duration_ms = 100;
        let (lib, ids) = library_with(&[a, b]);
        let art = ArtCache::new(dir.path().join("art"));
        let (online, _, requests) = mock(vec![
            ("recording?query", 200, reply(100, 166_000).into_bytes()),
            ("release/rel-1/front-500", 200, PNG.to_vec()),
        ]);
        let report = run(OnlineChoice::On, &online, &lib, &art);
        assert_eq!(report.matched, 1);
        assert_eq!(report.not_found, 1, "the second song's length does not fit");
        assert_eq!(report.art_added, 1);
        let t = lib.track(ids[0]).unwrap().unwrap();
        assert_eq!(t.album, "Head In The Clouds II");
        assert_eq!(t.year, 2019);
        assert_eq!(t.title, "Walking", "the title the file has is kept");
        let key = t.art.unwrap();
        assert_eq!(std::fs::read(art.path(&key)).unwrap(), PNG);
        // The matched song's metadata is stored; the file is untouched.
        let meta = lib.online_meta(ids[0]).unwrap().unwrap();
        assert!(meta.matched && meta.release_mbid == "rel-1" && meta.recording_mbid == "rec-1");
        assert_eq!(std::fs::read(&file).unwrap(), b"not really an mp3");
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), before);
        assert_eq!(lib.online_counts().unwrap(), (1, 1));
        // Nothing is asked twice.
        let n = requests.borrow().len();
        let again = run(OnlineChoice::On, &online, &lib, &art);
        assert_eq!(again.looked_up, 0);
        assert_eq!(requests.borrow().len(), n);
    }

    const UNKNOWN_ALBUM_FOR_TEST: &str = crate::tags::UNKNOWN_ALBUM;

    #[test]
    fn a_picture_the_file_has_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = track("/m/a.mp3", "Walking", "88rising", "Head In The Clouds II");
        t.art = Some("own.jpg".into());
        let (lib, ids) = library_with(&[t]);
        let art = ArtCache::new(dir.path().join("art"));
        let (online, _, requests) = mock(vec![
            ("recording?query", 200, reply(100, 166_000).into_bytes()),
            ("front-500", 200, PNG.to_vec()),
        ]);
        run(OnlineChoice::On, &online, &lib, &art);
        assert_eq!(lib.track(ids[0]).unwrap().unwrap().art.as_deref(), Some("own.jpg"));
        assert!(!requests.borrow().iter().any(|r| r.0.contains("coverartarchive")), "no download for a song that has a picture");
    }

    #[test]
    fn the_whole_album_shares_one_picture_and_one_download() {
        let dir = tempfile::tempdir().unwrap();
        let tracks: Vec<Track> = (1..=3)
            .map(|n| track(&format!("/m/{n}.mp3"), "Walking", "88rising", "Head In The Clouds II"))
            .collect();
        let (lib, ids) = library_with(&tracks);
        let art = ArtCache::new(dir.path().join("art"));
        let (online, _, requests) = mock(vec![
            ("recording?query", 200, reply(100, 166_000).into_bytes()),
            ("release/rel-1/front-500", 200, PNG.to_vec()),
        ]);
        run(OnlineChoice::On, &online, &lib, &art);
        let keys: Vec<_> = ids.iter().map(|id| lib.track(*id).unwrap().unwrap().art).collect();
        assert!(keys.iter().all(|k| k.is_some() && *k == keys[0]), "{keys:?}");
        assert_eq!(requests.borrow().iter().filter(|r| r.0.contains("coverartarchive")).count(), 1);
    }

    #[test]
    fn the_batch_stops_when_the_service_keeps_failing_and_keeps_songs_pending() {
        let tracks: Vec<Track> = (1..=6).map(|n| track(&format!("/m/{n}.mp3"), "Walking", "88rising", "Head")).collect();
        let (lib, _) = library_with(&tracks);
        let dir = tempfile::tempdir().unwrap();
        let art = ArtCache::new(dir.path().join("art"));
        let (online, _, requests) = mock(vec![("recording?query", 503, Vec::new())]);
        let report = run(OnlineChoice::On, &online, &lib, &art);
        assert!(report.stopped_early);
        assert_eq!(report.failed, 3);
        assert_eq!(requests.borrow().len(), 3, "gave up after three failures instead of hammering");
        assert_eq!(lib.pending_online(5_000, RETRY_AFTER_SECS, 100).unwrap().len(), 6);
    }

    #[test]
    fn cancelling_stops_before_the_next_song() {
        let tracks: Vec<Track> = (1..=4).map(|n| track(&format!("/m/{n}.mp3"), "Walking", "88rising", "Head")).collect();
        let (lib, _) = library_with(&tracks);
        let dir = tempfile::tempdir().unwrap();
        let art = ArtCache::new(dir.path().join("art"));
        let (online, _, requests) = mock(vec![("recording?query", 200, br#"{"recordings":[]}"#.to_vec())]);
        let calls = Cell::new(0);
        let report = run_batch(
            OnlineChoice::On, &online, &lib, &art, 100, 5_000,
            &|| { calls.set(calls.get() + 1); calls.get() > 2 },
            &mut |_, _| {},
        ).unwrap();
        assert!(report.stopped_early);
        // Two songs, each searched with and without the album.
        assert_eq!(requests.borrow().len(), 4);
    }

    #[test]
    fn songs_with_no_match_are_retried_only_after_a_month() {
        let (lib, ids) = library_with(&[track("/m/a.mp3", "Nothing", "Nobody", "Void")]);
        lib.save_online(&OnlineMeta { track_id: ids[0], checked_at: 1_000, matched: false, ..OnlineMeta::default() }).unwrap();
        assert!(lib.pending_online(1_000 + 3600, RETRY_AFTER_SECS, 10).unwrap().is_empty());
        assert_eq!(lib.pending_online(1_000 + RETRY_AFTER_SECS + 1, RETRY_AFTER_SECS, 10).unwrap().len(), 1);
    }

    #[test]
    fn found_info_survives_a_rescan_and_goes_with_the_song() {
        let mut t = track("/m/a.mp3", "Walking", "88rising", crate::tags::UNKNOWN_ALBUM);
        let (lib, ids) = library_with(&[t.clone()]);
        lib.save_online(&OnlineMeta {
            track_id: ids[0],
            checked_at: 1,
            matched: true,
            album: "Head In The Clouds II".into(),
            year: 2019,
            art: Some("cover.png".into()),
            ..OnlineMeta::default()
        })
        .unwrap();
        assert!(lib.apply_online(ids[0]).unwrap());
        assert!(!lib.apply_online(ids[0]).unwrap(), "nothing left to fill");
        // A rescan rewrites the row from the file's own (blank) tags.
        t.size = 2;
        lib.upsert_track(&t).unwrap();
        assert_eq!(lib.track(ids[0]).unwrap().unwrap().album, crate::tags::UNKNOWN_ALBUM);
        assert!(lib.apply_online(ids[0]).unwrap());
        let back = lib.track(ids[0]).unwrap().unwrap();
        assert_eq!((back.album.as_str(), back.year, back.art.as_deref()), ("Head In The Clouds II", 2019, Some("cover.png")));
        lib.remove_path("/m/a.mp3").unwrap();
        assert_eq!(lib.online_counts().unwrap(), (0, 0), "the lookup goes when the song does");
    }

    #[test]
    fn an_older_database_gains_the_lookup_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE tracks (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
                  mtime INTEGER NOT NULL, title TEXT NOT NULL, artist TEXT NOT NULL, album_artist TEXT NOT NULL,
                  album TEXT NOT NULL, genre TEXT NOT NULL, year INTEGER NOT NULL DEFAULT 0,
                  track_no INTEGER NOT NULL DEFAULT 0, disc_no INTEGER NOT NULL DEFAULT 0,
                  duration_ms INTEGER NOT NULL DEFAULT 0, plays INTEGER NOT NULL DEFAULT 0,
                  rating INTEGER NOT NULL DEFAULT 0, added_at INTEGER NOT NULL, art TEXT);
                 INSERT INTO tracks (path,size,mtime,title,artist,album_artist,album,genre,added_at)
                  VALUES ('/m/old.mp3',1,1,'Old','A','A','B','',1);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.count().unwrap(), 1, "the songs are kept");
        assert_eq!(lib.pending_online(10, RETRY_AFTER_SECS, 10).unwrap().len(), 1);
    }
}
