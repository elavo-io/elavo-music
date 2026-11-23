//! The player: the queue, the audio backend, and the floating pill bar that
//! shows and controls them.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use elavo_music_core::util::clock;
use elavo_music_core::{ArtCache, Queue, Repeat, Track};
use gtk::glib;
use gtk::prelude::*;

use crate::backend::{new_backend, Backend};
use crate::keys::MediaAction;
use crate::widgets::{art_path, cover, label};

struct Ui {
    root: gtk::Box,
    shuffle: gtk::ToggleButton,
    prev: gtk::Button,
    play: gtk::Button,
    next: gtk::Button,
    repeat: gtk::Button,
    cover_holder: gtk::Box,
    title: gtk::Label,
    artist: gtk::Label,
    elapsed: gtk::Label,
    remaining: gtk::Label,
    progress: gtk::Scale,
    volume: gtk::Scale,
    vol: gtk::Box,
}

pub struct Player {
    backend: RefCell<Box<dyn Backend>>,
    queue: RefCell<Queue>,
    art: Rc<ArtCache>,
    /// Called with a song's id once it has played long enough to count.
    on_counted: Box<dyn Fn(i64)>,
    ui: Ui,
    counted: Cell<bool>,
    last_seek: Cell<Option<Instant>>,
}

fn icon_button(icon: &str, tooltip: &str, classes: &[&str]) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(classes.iter().map(|c| c.to_string()).collect::<Vec<_>>())
        .build()
}

impl Player {
    pub fn new(art: Rc<ArtCache>, on_counted: impl Fn(i64) + 'static) -> Rc<Player> {
        let shuffle = gtk::ToggleButton::builder()
            .icon_name("media-playlist-shuffle-symbolic")
            .tooltip_text("Shuffle")
            .valign(gtk::Align::Center)
            .css_classes(["music-transport", "music-toggle"])
            .build();
        let prev = icon_button(
            "media-skip-backward-symbolic",
            "Previous",
            &["music-transport"],
        );
        let play = icon_button(
            "media-playback-start-symbolic",
            "Play",
            &["music-transport", "music-play"],
        );
        let next = icon_button("media-skip-forward-symbolic", "Next", &["music-transport"]);
        let repeat = icon_button(
            "media-playlist-repeat-symbolic",
            "Repeat",
            &["music-transport", "music-toggle"],
        );

        let cover_holder = gtk::Box::builder().valign(gtk::Align::Center).build();
        let title = label("Not Playing", &["music-now-title"]);
        let artist = label("", &["music-now-artist"]);
        let elapsed = label("0:00", &["music-time"]);
        elapsed.set_xalign(1.0);
        elapsed.set_width_chars(5);
        let remaining = label("-0:00", &["music-time"]);
        remaining.set_width_chars(5);
        let progress = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.001);
        progress.set_draw_value(false);
        progress.set_hexpand(true);
        progress.set_sensitive(false);
        progress.add_css_class("music-progress");

        let scrub = gtk::Box::builder().spacing(8).build();
        scrub.append(&elapsed);
        scrub.append(&progress);
        scrub.append(&remaining);
        let centre = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .spacing(0)
            .build();
        centre.append(&title);
        centre.append(&artist);
        centre.append(&scrub);

        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
        volume.set_draw_value(false);
        volume.set_value(0.8);
        volume.set_width_request(96);
        volume.set_valign(gtk::Align::Center);
        volume.add_css_class("music-volume");
        let vol_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
        let backend = new_backend();
        if backend.is_silent() {
            vol_icon.set_tooltip_text(Some(
                "This build has no audio output (built without the playback feature)",
            ));
        }
        vol_icon.add_css_class("music-vol-icon");

        let root = gtk::Box::builder()
            .spacing(6)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .css_classes(["music-pill"])
            .build();
        root.append(&shuffle);
        root.append(&prev);
        root.append(&play);
        root.append(&next);
        root.append(&repeat);
        let art_slot = gtk::Box::builder()
            .margin_start(10)
            .margin_end(10)
            .valign(gtk::Align::Center)
            .build();
        art_slot.append(&cover_holder);
        root.append(&art_slot);
        root.append(&centre);
        let vol = gtk::Box::builder()
            .spacing(6)
            .margin_start(14)
            .valign(gtk::Align::Center)
            .build();
        vol.append(&vol_icon);
        vol.append(&volume);
        root.append(&vol);

        let ui = Ui {
            root,
            shuffle,
            prev,
            play,
            next,
            repeat,
            cover_holder,
            title,
            artist,
            elapsed,
            remaining,
            progress,
            volume,
            vol,
        };
        let player = Rc::new(Player {
            backend: RefCell::new(backend),
            queue: RefCell::new(Queue::new()),
            art,
            on_counted: Box::new(on_counted),
            ui,
            counted: Cell::new(false),
            last_seek: Cell::new(None),
        });
        player.backend.borrow_mut().set_volume(0.8);
        player.show_idle();
        player.wire();
        player
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.ui.root
    }

    /// Fit the floating bar into the content column. The bar stays at its
    /// usual width when there is room, then shrinks, hiding the volume
    /// control, shuffle, repeat, and time labels before it would overflow.
    pub fn adapt_to_width(&self, content_width: i32) {
        let width = (content_width - 36).clamp(300, 780);
        self.ui.root.set_width_request(width);
        self.ui.vol.set_visible(width >= 640);
        self.ui.shuffle.set_visible(width >= 520);
        self.ui.repeat.set_visible(width >= 520);
        self.ui.elapsed.set_visible(width >= 400);
        self.ui.remaining.set_visible(width >= 400);
    }

    fn wire(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        fn bind(
            this: &std::rc::Weak<Player>,
            f: impl Fn(&Rc<Player>) + 'static,
        ) -> impl Fn(&gtk::Button) + 'static {
            let this = this.clone();
            move |_| {
                if let Some(p) = this.upgrade() {
                    f(&p);
                }
            }
        }
        self.ui.play.connect_clicked(bind(&this, |p| p.toggle()));
        self.ui.next.connect_clicked(bind(&this, |p| p.next(false)));
        self.ui.prev.connect_clicked(bind(&this, |p| p.previous()));
        self.ui
            .repeat
            .connect_clicked(bind(&this, |p| p.cycle_repeat()));
        {
            let this = this.clone();
            self.ui.shuffle.connect_toggled(move |b| {
                if let Some(p) = this.upgrade() {
                    p.queue.borrow_mut().set_shuffle(b.is_active());
                }
            });
        }
        {
            let this = this.clone();
            self.ui.progress.connect_change_value(move |_, _, value| {
                if let Some(p) = this.upgrade() {
                    p.seek_fraction(value.clamp(0.0, 1.0));
                }
                glib::Propagation::Proceed
            });
        }
        {
            let this = this.clone();
            self.ui.volume.connect_value_changed(move |s| {
                if let Some(p) = this.upgrade() {
                    p.backend.borrow_mut().set_volume(s.value());
                }
            });
        }
        let this = this.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || match this.upgrade() {
            Some(p) => {
                p.tick();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    /// Play `tracks` from `start`.
    pub fn play_tracks(&self, tracks: Vec<Track>, start: usize) {
        self.queue.borrow_mut().set(tracks, start);
        self.start_current();
    }

    /// Play `tracks` shuffled, whichever song comes up first.
    pub fn shuffle_tracks(&self, tracks: Vec<Track>) {
        self.ui.shuffle.set_active(true);
        self.queue.borrow_mut().set_shuffle(true);
        let start = if tracks.is_empty() {
            0
        } else {
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as usize)
                .unwrap_or(0))
                % tracks.len()
        };
        self.play_tracks(tracks, start);
    }

    fn start_current(&self) {
        let Some(track) = self.queue.borrow().current().cloned() else {
            self.show_idle();
            return;
        };
        {
            let mut backend = self.backend.borrow_mut();
            backend.load(Path::new(&track.path), track.duration_ms);
            backend.play();
        }
        self.counted.set(false);
        self.show_track(&track);
        self.refresh();
    }

    pub fn handle_media(self: &Rc<Self>, action: MediaAction) {
        match action {
            MediaAction::Toggle => self.toggle(),
            MediaAction::Pause => {
                if self.backend.borrow().is_playing() {
                    self.toggle();
                }
            }
            MediaAction::Next => self.next(false),
            MediaAction::Previous => self.previous(),
        }
    }

    pub fn toggle(self: &Rc<Self>) {
        if self.queue.borrow().current().is_none() {
            return;
        }
        {
            let mut backend = self.backend.borrow_mut();
            if backend.is_playing() {
                eprintln!("elavo-music: pause");
                backend.pause();
            } else {
                eprintln!("elavo-music: play");
                backend.play();
            }
        }
        // Rewriting the play button (icon, tooltip) in the middle of its
        // own Space/Return activation can leave it in a pressed state that
        // then ignores pointer clicks. Finish the event first.
        let play = self.ui.play.clone();
        let this = Rc::downgrade(self);
        glib::idle_add_local(move || {
            play.unset_state_flags(gtk::StateFlags::ACTIVE);
            if let Some(p) = this.upgrade() {
                p.refresh();
            }
            glib::ControlFlow::Break
        });
    }

    pub fn next(&self, auto: bool) {
        let moved = self.queue.borrow_mut().next(auto).is_some();
        if moved {
            self.start_current();
        } else {
            // The queue is over: stop at the start of the last song.
            let mut backend = self.backend.borrow_mut();
            backend.pause();
            backend.seek_ms(0);
            drop(backend);
            self.refresh();
        }
    }

    pub fn previous(&self) {
        // Like every player: a song well under way restarts first.
        if self.backend.borrow().position_ms() > 3000 {
            self.backend.borrow_mut().seek_ms(0);
            return;
        }
        if self.queue.borrow_mut().previous().is_some() {
            self.start_current();
        }
    }

    fn cycle_repeat(&self) {
        let next = self.queue.borrow().repeat.cycle();
        self.queue.borrow_mut().repeat = next;
        self.refresh();
    }

    fn seek_fraction(&self, fraction: f64) {
        let duration = self.backend.borrow().duration_ms();
        if duration > 0 && self.queue.borrow().current().is_some() {
            self.backend
                .borrow_mut()
                .seek_ms((duration as f64 * fraction) as u64);
            self.last_seek.set(Some(Instant::now()));
            self.tick_labels();
        }
    }

    fn show_idle(&self) {
        let ui = &self.ui;
        ui.title.set_label("Not Playing");
        ui.artist.set_label("");
        ui.artist.set_visible(false);
        while let Some(old) = ui.cover_holder.first_child() {
            ui.cover_holder.remove(&old);
        }
        ui.cover_holder.append(&cover(None, 44));
        ui.progress.set_value(0.0);
        ui.progress.set_sensitive(false);
        ui.elapsed.set_label("0:00");
        ui.remaining.set_label("-0:00");
        self.refresh();
    }

    fn show_track(&self, track: &Track) {
        let ui = &self.ui;
        ui.title.set_label(&track.title);
        let by = if track.album.is_empty() {
            track.artist.clone()
        } else {
            format!("{} — {}", track.artist, track.album)
        };
        ui.artist.set_label(&by);
        ui.artist.set_visible(true);
        while let Some(old) = ui.cover_holder.first_child() {
            ui.cover_holder.remove(&old);
        }
        ui.cover_holder
            .append(&cover(art_path(&self.art, &track.art), 44));
        ui.progress.set_sensitive(true);
    }

    /// Button states: icons, tooltips, and what can be pressed.
    fn refresh(&self) {
        let ui = &self.ui;
        let queue = self.queue.borrow();
        let has = queue.current().is_some();
        let playing = self.backend.borrow().is_playing();
        ui.play.set_sensitive(has);
        ui.prev.set_sensitive(has);
        ui.next.set_sensitive(has);
        ui.play.set_icon_name(if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        ui.play
            .set_tooltip_text(Some(if playing { "Pause" } else { "Play" }));
        let (icon, tip, on) = match queue.repeat {
            Repeat::Off => ("media-playlist-repeat-symbolic", "Repeat: Off", false),
            Repeat::All => ("media-playlist-repeat-symbolic", "Repeat: All", true),
            Repeat::One => ("media-playlist-repeat-song-symbolic", "Repeat: One", true),
        };
        ui.repeat.set_icon_name(icon);
        ui.repeat.set_tooltip_text(Some(tip));
        if on {
            ui.repeat.add_css_class("active");
        } else {
            ui.repeat.remove_css_class("active");
        }
    }

    fn tick_labels(&self) {
        let (pos, dur) = {
            let backend = self.backend.borrow();
            (backend.position_ms(), backend.duration_ms())
        };
        self.ui.elapsed.set_label(&clock(pos));
        self.ui
            .remaining
            .set_label(&format!("-{}", clock(dur.saturating_sub(pos))));
        let recently_dragged = self
            .last_seek
            .get()
            .is_some_and(|t| t.elapsed() < Duration::from_millis(600));
        if dur > 0 && !recently_dragged {
            self.ui
                .progress
                .set_value((pos as f64 / dur as f64).clamp(0.0, 1.0));
        }
    }

    fn tick(&self) {
        let has = self.queue.borrow().current().is_some();
        if !has {
            return;
        }
        let finished = self.backend.borrow_mut().take_finished();
        if finished {
            self.next(true);
            return;
        }
        self.tick_labels();
        if !self.counted.get() {
            let (pos, dur) = {
                let b = self.backend.borrow();
                (b.position_ms(), b.duration_ms())
            };
            if pos >= (dur / 2).clamp(1, 30_000) && self.backend.borrow().is_playing() {
                self.counted.set(true);
                if let Some(track) = self.queue.borrow().current() {
                    eprintln!(
                        "elavo-music: counted a play of {} (id {})",
                        track.title, track.id
                    );
                    (self.on_counted)(track.id);
                }
            }
        }
        self.refresh();
        if std::env::var("ELAVO_MUSIC_DEBUG").is_ok() {
            let (pos, dur, title) = {
                let b = self.backend.borrow();
                let title = self
                    .queue
                    .borrow()
                    .current()
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                (b.position_ms(), b.duration_ms(), title)
            };
            // One line a second is enough to see that the clock is moving.
            if pos % 1000 < 280 {
                eprintln!("elavo-music: position {pos} / {dur} ms ({title})");
            }
        }
    }
}

impl Player {
    /// Seek to a time in the current song (screenshot hook).
    pub fn seek_ms_for_demo(&self, ms: u64) {
        self.backend.borrow_mut().seek_ms(ms);
        self.tick_labels();
    }

    /// Run a comma-separated hook list used by the nested test session:
    /// `next`, `prev`, `pause`, `play`, and `wait:MS`.
    pub fn run_then_script(self: &Rc<Self>, script: &str) {
        let steps: Vec<String> = script
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self::then_step(self.clone(), steps, 0);
    }

    fn then_step(player: Rc<Player>, steps: Vec<String>, index: usize) {
        if index >= steps.len() {
            return;
        }
        let step = steps[index].clone();
        let delay = step
            .strip_prefix("wait:")
            .and_then(|ms| ms.parse().ok())
            .unwrap_or(1);
        glib::timeout_add_local(Duration::from_millis(delay.max(1)), move || {
            if !step.starts_with("wait:") {
                let before = player
                    .queue
                    .borrow()
                    .current()
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                let pos = player.backend.borrow().position_ms();
                match step.as_str() {
                    "next" => player.next(false),
                    "prev" | "previous" => player.previous(),
                    "pause" => {
                        if player.backend.borrow().is_playing() {
                            player.toggle();
                        }
                    }
                    "play" => {
                        if !player.backend.borrow().is_playing() {
                            player.toggle();
                        }
                    }
                    other => eprintln!("elavo-music: unknown then step {other}"),
                }
                let after = player
                    .queue
                    .borrow()
                    .current()
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                let pos2 = player.backend.borrow().position_ms();
                eprintln!(
                    "elavo-music: then {step} ({before} at {pos} ms) -> {after} at {pos2} ms"
                );
            }
            Self::then_step(player.clone(), steps.clone(), index + 1);
            glib::ControlFlow::Break
        });
    }
}
