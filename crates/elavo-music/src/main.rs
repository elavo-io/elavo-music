//! Elavo Music: a local music library and player, shown as "Music".

mod accent;
mod albums;
mod app;
mod artists;
mod backend;
mod keys;
mod menu;
mod mpris;
mod player;
mod prefs;
mod songs;
mod widgets;
mod window;

#[cfg(feature = "playback")]
mod gst_backend;

use gtk::gio;
use gtk::glib;

fn main() -> glib::ExitCode {
    gio::resources_register_include!("elavo-music.gresource").expect("compiled resources");
    app::run()
}
