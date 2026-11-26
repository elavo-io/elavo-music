//! The Preferences window. For now it holds one thing: whether Music may
//! look songs up online. It is off until the person turns it on.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;

pub const SWITCH_TITLE: &str = "Look up song info and album art online";
pub const EXPLANATION: &str = "Fills in what your songs are missing, such as the album, the year and the cover, by looking them up on MusicBrainz and the Cover Art Archive. A song's title, artist, album and length are sent to musicbrainz.org, and covers come from coverartarchive.org. No account, no tracking, and nothing else leaves your computer. Your music files are never changed: what is found is kept in Music's own library.";

pub struct Prefs {
    pub window: gtk::Window,
    switch: gtk::Switch,
    status: gtk::Label,
    lookup_now: gtk::Button,
    quiet: Cell<bool>,
}

impl Prefs {
    pub fn new(
        parent: &gtk::Window,
        online: bool,
        on_toggle: impl Fn(bool) + 'static,
        on_lookup_now: impl Fn() + 'static,
    ) -> Rc<Prefs> {
        let title = gtk::Label::builder()
            .label(SWITCH_TITLE)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["music-now-title"])
            .build();
        let switch = gtk::Switch::builder()
            .active(online)
            .valign(gtk::Align::Center)
            .build();
        let head = gtk::Box::builder().spacing(12).build();
        head.append(&title);
        head.append(&switch);

        let explanation = gtk::Label::builder()
            .label(EXPLANATION)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(52)
            .css_classes(["music-now-artist"])
            .build();
        let status = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["music-now-artist"])
            .build();
        let lookup_now = gtk::Button::builder()
            .label("Look Up Now")
            .valign(gtk::Align::Center)
            .sensitive(online)
            .css_classes(["music-pill-button", "music-accent-button"])
            .build();
        let foot = gtk::Box::builder().spacing(12).build();
        foot.append(&status);
        foot.append(&lookup_now);

        let section = gtk::Label::builder()
            .label("Online")
            .xalign(0.0)
            .css_classes(["music-section-title"])
            .build();
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(20)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        body.append(&section);
        body.append(&head);
        body.append(&explanation);
        body.append(&foot);

        let window = gtk::Window::builder()
            .title("Preferences")
            .transient_for(parent)
            .default_width(460)
            .resizable(false)
            .hide_on_close(true)
            .css_classes(["music-window"])
            .child(&body)
            .build();

        let prefs = Rc::new(Prefs {
            window,
            switch,
            status,
            lookup_now,
            quiet: Cell::new(false),
        });
        {
            let this = Rc::downgrade(&prefs);
            prefs.switch.connect_active_notify(move |switch| {
                let Some(prefs) = this.upgrade() else { return };
                if prefs.quiet.get() {
                    return;
                }
                prefs.lookup_now.set_sensitive(switch.is_active());
                on_toggle(switch.is_active());
            });
        }
        prefs.lookup_now.connect_clicked(move |_| on_lookup_now());
        prefs
    }

    /// Show a choice made elsewhere without calling the toggle handler.
    pub fn set_online(&self, on: bool) {
        self.quiet.set(true);
        self.switch.set_active(on);
        self.lookup_now.set_sensitive(on);
        self.quiet.set(false);
    }

    pub fn set_status(&self, text: &str) {
        self.status.set_label(text);
    }

    /// While a lookup runs, the button cannot be pressed again.
    pub fn set_busy(&self, busy: bool) {
        self.lookup_now
            .set_sensitive(!busy && self.switch.is_active());
    }

    pub fn present(&self) {
        self.window.present();
    }
}

/// The first-run question, a card that slides in over the top of the
/// content. It is not a dialog: nothing about it blocks the window, and
/// it is not shown again once answered. `answer(true)` is "Look Up Online".
pub fn prompt_card(answer: impl Fn(bool) + 'static) -> gtk::Revealer {
    let title = gtk::Label::builder()
        .label("Look up song info and album art online?")
        .xalign(0.0)
        .wrap(true)
        .css_classes(["music-now-title"])
        .build();
    let body = gtk::Label::builder()
        .label(EXPLANATION)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(60)
        .css_classes(["music-now-artist"])
        .build();
    let no = gtk::Button::builder()
        .label("No Thanks")
        .css_classes(["music-pill-button"])
        .build();
    let yes = gtk::Button::builder()
        .label("Look Up Online")
        .css_classes(["music-pill-button", "music-accent-button"])
        .build();
    let buttons = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::End)
        .margin_top(4)
        .build();
    buttons.append(&no);
    buttons.append(&yes);
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .width_request(480)
        .css_classes(["music-prompt"])
        .build();
    card.append(&title);
    card.append(&body);
    card.append(&buttons);
    let revealer = gtk::Revealer::builder()
        .child(&card)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Start)
        .build();
    let answer = Rc::new(answer);
    for (button, yes_clicked) in [(&no, false), (&yes, true)] {
        let revealer = revealer.clone();
        let answer = answer.clone();
        button.connect_clicked(move |_| {
            revealer.set_reveal_child(false);
            answer(yes_clicked);
        });
    }
    revealer
}
