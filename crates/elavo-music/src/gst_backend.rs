//! GStreamer playback through playbin3. Built with `--features playback`.
//!
//! Gapless (about-to-finish), ReplayGain and the equalizer are the next
//! steps; this plays one song at a time and reports when it ends.

use std::path::Path;

use gst::prelude::*;
use gstreamer as gst;
use gtk::gio;
use gtk::prelude::FileExt;

/// Audio sink used when `ELAVO_MUSIC_AUDIO_SINK` is unset. A real session
/// should leave the variable unset so PipeWire or Pulse is used. The nested
/// test session always sets it to `fakesink`, so nothing is sent to a
/// speaker.
const DEFAULT_AUDIO_SINK: &str = "autoaudiosink";

pub struct GstBackend {
    playbin: gst::Element,
    duration_hint: u64,
    finished: bool,
    /// Applied once the pipeline has prerolled; seeking before that fails.
    pending_seek: Option<u64>,
    last_error: Option<String>,
}

fn make_sink(name: &str) -> Option<gst::Element> {
    gst::ElementFactory::make(name).build().ok()
}

fn audio_sink_name() -> String {
    std::env::var("ELAVO_MUSIC_AUDIO_SINK")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_AUDIO_SINK.to_string())
}

impl GstBackend {
    pub fn new() -> Option<GstBackend> {
        Self::with_audio_sink(&audio_sink_name())
    }

    /// A backend that forces a named audio sink (`fakesink` in tests, so
    /// nothing reaches a speaker).
    pub fn with_audio_sink(audio_sink: &str) -> Option<GstBackend> {
        gst::init().ok()?;
        let playbin = gst::ElementFactory::make("playbin3").build().ok()?;
        if let Some(sink) = make_sink("fakesink") {
            playbin.set_property("video-sink", &sink);
        }
        match make_sink(audio_sink) {
            Some(sink) => {
                if audio_sink == "fakesink" {
                    sink.set_property("sync", true);
                }
                playbin.set_property("audio-sink", &sink);
                eprintln!("elavo-music: audio sink is {audio_sink}");
            }
            None => {
                eprintln!(
                    "elavo-music: no element named {audio_sink}; using the default audio sink"
                );
            }
        }
        Some(GstBackend {
            playbin,
            duration_hint: 0,
            finished: false,
            pending_seek: None,
            last_error: None,
        })
    }

    #[cfg(test)]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn retry_seek(&mut self) {
        let Some(ms) = self.pending_seek else { return };
        if self.do_seek(ms) {
            self.pending_seek = None;
        }
    }

    fn do_seek(&self, ms: u64) -> bool {
        self.playbin
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                gst::ClockTime::from_mseconds(ms),
            )
            .is_ok()
    }

    fn poll_bus(&mut self) {
        let Some(bus) = self.playbin.bus() else {
            return;
        };
        while let Some(message) = bus.pop() {
            match message.view() {
                gst::MessageView::Eos(_) => {
                    eprintln!("elavo-music: end of track");
                    self.finished = true;
                    // Leave the pipeline paused so is_playing() follows
                    // GStreamer after EOS, not a leftover Playing state.
                    let _ = self.playbin.set_state(gst::State::Paused);
                }
                gst::MessageView::Error(err) => {
                    let debug = err.debug();
                    let text = match debug {
                        Some(d) if !d.is_empty() => format!("{} ({d})", err.error()),
                        _ => err.error().to_string(),
                    };
                    eprintln!("elavo-music: playback error: {text}");
                    self.last_error = Some(text);
                    self.finished = true;
                    let _ = self.playbin.set_state(gst::State::Paused);
                }
                gst::MessageView::AsyncDone(_) => self.retry_seek(),
                _ => {}
            }
        }
        self.retry_seek();
    }

    /// Wait until the pipeline reaches `want`, or `timeout_ms` elapses.
    #[cfg(test)]
    fn wait_state(&self, want: gst::State, timeout_ms: u64) -> bool {
        let (_, state, _) = self
            .playbin
            .state(gst::ClockTime::from_mseconds(timeout_ms));
        state == want
    }
}

impl super::backend::Backend for GstBackend {
    fn load(&mut self, path: &Path, duration_ms: u64) {
        let _ = self.playbin.set_state(gst::State::Null);
        let uri = gio::File::for_path(path).uri();
        self.playbin.set_property("uri", uri.as_str());
        self.duration_hint = duration_ms;
        self.finished = false;
        self.pending_seek = None;
        self.last_error = None;
        eprintln!("elavo-music: load {uri}");
        let _ = self.playbin.set_state(gst::State::Paused);
    }

    fn play(&mut self) {
        if let Err(err) = self.playbin.set_state(gst::State::Playing) {
            eprintln!("elavo-music: could not play: {err}");
            self.last_error = Some(err.to_string());
            self.finished = true;
        }
        self.poll_bus();
    }

    fn pause(&mut self) {
        if let Err(err) = self.playbin.set_state(gst::State::Paused) {
            eprintln!("elavo-music: could not pause: {err}");
            self.last_error = Some(err.to_string());
        }
        self.poll_bus();
    }

    fn is_playing(&self) -> bool {
        // Prefer the pending target so an in-flight pause or play is
        // reflected immediately, not a leftover local flag.
        let (_, state, pending) = self.playbin.state(gst::ClockTime::ZERO);
        if pending != gst::State::VoidPending {
            pending == gst::State::Playing
        } else {
            state == gst::State::Playing
        }
    }

    fn position_ms(&self) -> u64 {
        self.playbin
            .query_position::<gst::ClockTime>()
            .map(|t| t.mseconds())
            .unwrap_or(0)
    }

    fn duration_ms(&self) -> u64 {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|t| t.mseconds())
            .unwrap_or(self.duration_hint)
    }

    fn seek_ms(&mut self, ms: u64) {
        if self.do_seek(ms) {
            self.pending_seek = None;
        } else {
            self.pending_seek = Some(ms);
        }
    }

    fn set_volume(&mut self, volume: f64) {
        self.playbin.set_property("volume", volume.clamp(0.0, 1.0));
    }

    fn take_finished(&mut self) -> bool {
        self.poll_bus();
        if self.finished {
            self.finished = false;
            return true;
        }
        false
    }
}

impl Drop for GstBackend {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// GStreamer's plugin registry is process-global; don't start two
    /// pipelines at once from the test thread pool.
    static LOCK: Mutex<()> = Mutex::new(());

    fn backend() -> GstBackend {
        GstBackend::with_audio_sink("fakesink").expect("GStreamer playbin3")
    }

    fn wait_pos(b: &mut GstBackend, min_ms: u64, timeout_ms: u64) -> u64 {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let _ = b.take_finished();
            let pos = b.position_ms();
            if pos >= min_ms || Instant::now() >= deadline {
                return pos;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_finished(b: &mut GstBackend, timeout_ms: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            if b.take_finished() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A valid 16-bit mono 8 kHz WAV of a short burst, `ms` long.
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
        // A quiet square-ish tone so it is not a silent buffer.
        for i in 0..samples {
            let sample: i16 = if (i / 10) % 2 == 0 { 800 } else { -800 };
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    fn write_wav(dir: &Path, ms: u32) -> PathBuf {
        let path = dir.join(format!("tone-{ms}.wav"));
        std::fs::write(&path, wav(ms)).unwrap();
        path
    }

    fn gst_launch(dir: &Path, name: &str, encode: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let uri_src = "audiotestsrc wave=sine freq=440 num-buffers=50";
        let mut args = vec!["-q".to_string()];
        args.extend(uri_src.split(' ').map(str::to_string));
        args.push("!".into());
        args.push("audioconvert".into());
        for part in encode {
            args.push("!".into());
            args.push((*part).into());
        }
        args.push("!".into());
        args.push("filesink".into());
        args.push(format!("location={}", path.display()));
        let status = Command::new("gst-launch-1.0")
            .args(&args)
            .status()
            .expect("gst-launch-1.0");
        assert!(status.success(), "gst-launch failed for {name}: {args:?}");
        assert!(
            path.is_file() && path.metadata().unwrap().len() > 0,
            "{name} was not written"
        );
        path
    }

    fn play(b: &mut GstBackend, path: &Path, hint_ms: u64) {
        b.load(path, hint_ms);
        assert!(
            b.wait_state(gst::State::Paused, 4000) || b.wait_state(gst::State::Playing, 100),
            "preroll failed for {} (error: {:?})",
            path.display(),
            b.last_error()
        );
        b.play();
        assert!(b.is_playing(), "play() did not start {}", path.display());
    }

    #[test]
    fn wav_plays_pauses_seeks_and_ends() {
        let _lock = LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = write_wav(dir.path(), 2000);
        let mut b = backend();

        play(&mut b, &path, 2000);
        let mid = wait_pos(&mut b, 250, 3000);
        assert!(mid >= 250, "position did not advance (got {mid} ms)");
        assert!(
            b.last_error().is_none(),
            "unexpected error: {:?}",
            b.last_error()
        );

        b.pause();
        assert!(!b.is_playing());
        let held = b.position_ms();
        std::thread::sleep(Duration::from_millis(200));
        let later = b.position_ms();
        assert!(
            later.abs_diff(held) < 80,
            "position moved while paused ({held} -> {later})"
        );

        b.play();
        b.seek_ms(1_200);
        let sought = wait_pos(&mut b, 1_150, 2000);
        assert!(sought >= 1_150, "seek did not take (got {sought} ms)");
        assert!(sought < 1_900, "seek overshot ({sought} ms)");

        // Jump near the end and wait for EOS.
        b.seek_ms(1_850);
        assert!(wait_finished(&mut b, 3000), "end of track was not reported");
        assert!(!b.is_playing());
        assert!(!wait_finished(&mut b, 200), "EOS must be reported once");
    }

    #[test]
    fn pending_seek_applies_after_preroll() {
        let _lock = LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = write_wav(dir.path(), 2000);
        let mut b = backend();
        b.load(&path, 2000);
        // Seek before the pipeline is ready, the way the screenshot hook does.
        b.seek_ms(900);
        b.play();
        let pos = wait_pos(&mut b, 850, 3000);
        assert!(pos >= 850, "pending seek was dropped (got {pos} ms)");
        assert!(pos < 1_600, "pending seek overshot ({pos} ms)");
    }

    #[test]
    fn mp3_and_flac_decode() {
        let _lock = LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mp3 = gst_launch(dir.path(), "tone.mp3", &["lamemp3enc"]);
        let flac = gst_launch(dir.path(), "tone.flac", &["flacenc"]);
        let mut b = backend();
        for path in [mp3, flac] {
            play(&mut b, &path, 1500);
            let pos = wait_pos(&mut b, 200, 3000);
            assert!(
                pos >= 200,
                "{}: position {pos} ms, error {:?}",
                path.display(),
                b.last_error()
            );
            assert!(
                b.last_error().is_none(),
                "{}: {}",
                path.display(),
                b.last_error().unwrap()
            );
            b.pause();
        }
    }

    #[test]
    fn aac_needs_libav_plugin() {
        let _lock = LOCK.lock().unwrap();
        let path = PathBuf::from("/tmp/elavo-music-gst-samples/tone.m4a");
        if !path.is_file() {
            eprintln!("skipping AAC decode check: {} is missing", path.display());
            return;
        }
        let mut b = backend();
        b.load(&path, 2000);
        let prerolled = b.wait_state(gst::State::Paused, 3000);
        b.play();
        let _ = wait_pos(&mut b, 150, 2000);
        let _ = wait_finished(&mut b, 500);
        if prerolled && b.last_error().is_none() && b.position_ms() > 0 {
            // gstreamer1-plugin-libav (or another AAC decoder) is installed.
            return;
        }
        let err = b.last_error().unwrap_or("").to_lowercase();
        assert!(
            !prerolled
                || err.contains("plugin")
                || err.contains("not-linked")
                || err.contains("stream"),
            "expected a missing-plugin error for AAC, got preroll={prerolled} error={err:?}"
        );
    }
}
