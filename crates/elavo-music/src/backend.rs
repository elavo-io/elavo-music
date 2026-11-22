//! Audio output behind one small trait. With the `playback` feature the
//! backend is GStreamer's playbin3; without it, a silent one that keeps the
//! clock, queue and bar working so the rest of Music can be built and tested
//! on a machine without GStreamer's development files.

use std::path::Path;
use std::time::Instant;

pub trait Backend {
    fn load(&mut self, path: &Path, duration_ms: u64);
    fn play(&mut self);
    fn pause(&mut self);
    fn is_playing(&self) -> bool;
    fn position_ms(&self) -> u64;
    fn duration_ms(&self) -> u64;
    fn seek_ms(&mut self, ms: u64);
    fn set_volume(&mut self, volume: f64);
    /// True once each time the loaded song ends on its own (or fails).
    fn take_finished(&mut self) -> bool;
    fn is_silent(&self) -> bool {
        false
    }
}

pub fn new_backend() -> Box<dyn Backend> {
    #[cfg(feature = "playback")]
    {
        if let Some(backend) = crate::gst_backend::GstBackend::new() {
            return Box::new(backend);
        }
        eprintln!("elavo-music: GStreamer could not start; playing silently");
    }
    #[cfg(not(feature = "playback"))]
    eprintln!("elavo-music: built without the `playback` feature; no sound (see README)");
    Box::new(SilentBackend::default())
}

/// Plays nothing, but advances a clock like a real player would.
#[derive(Default)]
pub struct SilentBackend {
    base_ms: u64,
    since: Option<Instant>,
    duration_ms: u64,
    finished: bool,
}

impl SilentBackend {
    fn elapsed(&self) -> u64 {
        let running = self
            .since
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        (self.base_ms + running).min(self.duration_ms)
    }
}

impl Backend for SilentBackend {
    fn load(&mut self, _path: &Path, duration_ms: u64) {
        self.base_ms = 0;
        self.since = None;
        self.duration_ms = duration_ms;
        self.finished = false;
    }

    fn play(&mut self) {
        if self.since.is_none() {
            self.since = Some(Instant::now());
        }
    }

    fn pause(&mut self) {
        self.base_ms = self.elapsed();
        self.since = None;
    }

    fn is_playing(&self) -> bool {
        self.since.is_some()
    }

    fn position_ms(&self) -> u64 {
        self.elapsed()
    }

    fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    fn seek_ms(&mut self, ms: u64) {
        self.base_ms = ms.min(self.duration_ms);
        if self.since.is_some() {
            self.since = Some(Instant::now());
        }
    }

    fn set_volume(&mut self, _volume: f64) {}

    fn take_finished(&mut self) -> bool {
        if self.since.is_some() && self.elapsed() >= self.duration_ms {
            self.base_ms = self.duration_ms;
            self.since = None;
            return true;
        }
        false
    }

    fn is_silent(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn silent_clock_runs_pauses_and_seeks() {
        let mut b = SilentBackend::default();
        b.load(Path::new("x"), 10_000);
        assert!(!b.is_playing());
        assert_eq!(b.position_ms(), 0);
        b.play();
        sleep(Duration::from_millis(30));
        assert!(b.position_ms() >= 25);
        b.pause();
        let held = b.position_ms();
        sleep(Duration::from_millis(20));
        assert_eq!(b.position_ms(), held);
        b.seek_ms(4000);
        assert_eq!(b.position_ms(), 4000);
        b.seek_ms(99_000);
        assert_eq!(b.position_ms(), 10_000, "clamped");
    }

    #[test]
    fn silent_reports_the_end_once() {
        let mut b = SilentBackend::default();
        b.load(Path::new("x"), 20);
        b.play();
        sleep(Duration::from_millis(60));
        assert!(b.take_finished(), "the end is reported");
        assert!(!b.is_playing());
        assert!(!b.take_finished(), "reported once");
    }
}
