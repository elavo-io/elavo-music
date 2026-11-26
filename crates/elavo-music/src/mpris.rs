//! MPRIS2: Music on the session bus as `org.mpris.MediaPlayer2.io.elavo.Music`.
//!
//! This is how anything outside Music controls it: the desktop's media
//! keys (`elavo-media-key` in the image, which labwc runs for the play,
//! pause, next and previous keys), media widgets, `playerctl`, and the
//! panel. Without it the media keys only reached Music while its window
//! had the keyboard focus, because a Wayland client sees keys only then.
//!
//! It uses GIO's D-Bus support, so it adds no dependency.

use std::cell::RefCell;
use std::path::Path;
use std::rc::{Rc, Weak};

use elavo_music_core::{Repeat, Track};
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::player::{Player, PlayerState};

pub const BUS_NAME: &str = "org.mpris.MediaPlayer2.io.elavo.Music";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

const INTROSPECTION: &str = r#"
<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek"><arg direction="in" name="Offset" type="x"/></method>
    <method name="SetPosition">
      <arg direction="in" name="TrackId" type="o"/>
      <arg direction="in" name="Position" type="x"/>
    </method>
    <method name="OpenUri"><arg direction="in" name="Uri" type="s"/></method>
    <signal name="Seeked"><arg name="Position" type="x"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="LoopStatus" type="s" access="readwrite"/>
    <property name="Rate" type="d" access="read"/>
    <property name="Shuffle" type="b" access="readwrite"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Volume" type="d" access="readwrite"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>"#;

/// `Playing`, `Paused`, or `Stopped` when there is no song.
pub fn playback_status(state: &PlayerState) -> &'static str {
    if !state.has_track {
        "Stopped"
    } else if state.playing {
        "Playing"
    } else {
        "Paused"
    }
}

pub fn loop_status(repeat: Repeat) -> &'static str {
    match repeat {
        Repeat::Off => "None",
        Repeat::One => "Track",
        Repeat::All => "Playlist",
    }
}

pub fn repeat_from_loop_status(status: &str) -> Option<Repeat> {
    match status {
        "None" => Some(Repeat::Off),
        "Track" => Some(Repeat::One),
        "Playlist" => Some(Repeat::All),
        _ => None,
    }
}

pub fn track_object_path(id: i64) -> String {
    format!("/io/elavo/Music/track/{id}")
}

fn strings(items: &[&str]) -> glib::Variant {
    items
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .to_variant()
}

/// The `Metadata` property: `a{sv}` with the keys media widgets read.
pub fn metadata(track: Option<&Track>, art: Option<&Path>) -> glib::Variant {
    let dict = glib::VariantDict::new(None);
    let object = |path: &str| {
        glib::variant::ObjectPath::try_from(path.to_string())
            .expect("a valid object path")
            .to_variant()
    };
    match track {
        None => dict.insert_value("mpris:trackid", &object(NO_TRACK)),
        Some(track) => {
            dict.insert_value("mpris:trackid", &object(&track_object_path(track.id)));
            dict.insert_value(
                "mpris:length",
                &((track.duration_ms as i64) * 1000).to_variant(),
            );
            dict.insert_value("xesam:title", &track.title.to_variant());
            dict.insert_value("xesam:album", &track.album.to_variant());
            dict.insert_value("xesam:artist", &strings(&[&track.artist]));
            dict.insert_value("xesam:albumArtist", &strings(&[&track.album_artist]));
            if !track.genre.is_empty() {
                dict.insert_value("xesam:genre", &strings(&[&track.genre]));
            }
            if track.track_no > 0 {
                dict.insert_value("xesam:trackNumber", &(track.track_no as i32).to_variant());
            }
            if track.disc_no > 0 {
                dict.insert_value("xesam:discNumber", &(track.disc_no as i32).to_variant());
            }
            dict.insert_value(
                "xesam:url",
                &gio::File::for_path(&track.path)
                    .uri()
                    .to_string()
                    .to_variant(),
            );
            if let Some(art) = art {
                dict.insert_value(
                    "mpris:artUrl",
                    &gio::File::for_path(art).uri().to_string().to_variant(),
                );
            }
        }
    }
    dict.end()
}

/// What `Position` and `Seek` need from the player; the rest of the
/// player's surface is used directly.
fn micros(ms: u64) -> i64 {
    (ms as i64).saturating_mul(1000)
}

struct Service {
    app: gtk::Application,
    window: gtk::ApplicationWindow,
    player: Rc<Player>,
}

impl Service {
    fn root_property(&self, name: &str) -> glib::Variant {
        match name {
            "CanQuit" => false.to_variant(),
            "CanRaise" => true.to_variant(),
            "HasTrackList" => false.to_variant(),
            "Identity" => "Music".to_variant(),
            "DesktopEntry" => "io.elavo.Music".to_variant(),
            "SupportedUriSchemes" => strings(&["file"]),
            // Anything Music can play depends on the GStreamer plugins
            // installed, so it does not promise a list.
            "SupportedMimeTypes" => strings(&[]),
            _ => false.to_variant(),
        }
    }

    fn player_property(&self, name: &str) -> glib::Variant {
        let state = self.player.state();
        match name {
            "PlaybackStatus" => playback_status(&state).to_variant(),
            "LoopStatus" => loop_status(state.repeat).to_variant(),
            "Rate" | "MinimumRate" | "MaximumRate" => 1.0f64.to_variant(),
            "Shuffle" => state.shuffle.to_variant(),
            "Metadata" => metadata(
                self.player.current_track().as_ref(),
                self.player.current_art_file().as_deref(),
            ),
            "Volume" => self.player.volume().to_variant(),
            "Position" => micros(self.player.position_ms()).to_variant(),
            "CanGoNext" | "CanGoPrevious" | "CanPlay" | "CanPause" | "CanSeek" => {
                state.has_track.to_variant()
            }
            "CanControl" => true.to_variant(),
            _ => false.to_variant(),
        }
    }

    fn set_player_property(&self, name: &str, value: &glib::Variant) -> bool {
        match name {
            "LoopStatus" => match value.str().and_then(repeat_from_loop_status) {
                Some(repeat) => {
                    self.player.set_repeat(repeat);
                    true
                }
                None => false,
            },
            "Shuffle" => match value.get::<bool>() {
                Some(on) => {
                    self.player.set_shuffle(on);
                    true
                }
                None => false,
            },
            "Volume" => match value.get::<f64>() {
                Some(volume) if volume.is_finite() => {
                    self.player.set_volume(volume);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn player_method(
        &self,
        connection: &gio::DBusConnection,
        method: &str,
        params: &glib::Variant,
    ) {
        let player = &self.player;
        match method {
            "Next" => player.next(false),
            "Previous" => player.previous(),
            "Pause" => player.pause(),
            "Play" => player.play(),
            "PlayPause" => player.handle_media(crate::keys::MediaAction::Toggle),
            "Stop" => player.stop(),
            "Seek" => {
                if let Some((offset,)) = params.get::<(i64,)>() {
                    let now = player.position_ms() as i64;
                    let target = (now + offset / 1000).max(0) as u64;
                    player.seek_ms(target);
                    self.emit_seeked(connection);
                }
            }
            "SetPosition" => {
                if let Some((track, position)) = params.get::<(String, i64)>() {
                    let current = player.current_track().map(|t| track_object_path(t.id));
                    if current.as_deref() == Some(track.as_str()) && position >= 0 {
                        player.seek_ms((position / 1000) as u64);
                        self.emit_seeked(connection);
                    }
                }
            }
            // Music plays what is in its library; opening a loose file is
            // not supported yet.
            "OpenUri" => {}
            _ => {}
        }
    }

    fn emit_seeked(&self, connection: &gio::DBusConnection) {
        let _ = connection.emit_signal(
            None,
            OBJECT_PATH,
            PLAYER,
            "Seeked",
            Some(&(micros(self.player.position_ms()),).to_variant()),
        );
    }
}

/// Own the MPRIS name and keep it in step with the player. Does nothing
/// useful when there is no session bus.
pub fn start(app: &gtk::Application, window: &gtk::ApplicationWindow, player: &Rc<Player>) {
    let service = Rc::new(Service {
        app: app.clone(),
        window: window.clone(),
        player: player.clone(),
    });
    let connection: Rc<RefCell<Option<gio::DBusConnection>>> = Rc::default();

    let node = match gio::DBusNodeInfo::for_xml(INTROSPECTION) {
        Ok(node) => node,
        Err(err) => {
            eprintln!("elavo-music: MPRIS description is invalid: {err}");
            return;
        }
    };

    let acquired = {
        let service = service.clone();
        let connection = connection.clone();
        move |conn: gio::DBusConnection, _: &str| {
            for (interface, root) in [(ROOT, true), (PLAYER, false)] {
                let Some(info) = node.lookup_interface(interface) else {
                    continue;
                };
                let (get, set, call) = (service.clone(), service.clone(), service.clone());
                let registered = conn
                    .register_object(OBJECT_PATH, &info)
                    .method_call(
                        move |conn, _sender, _path, _iface, method, params, invocation| {
                            if root {
                                if method == "Raise" {
                                    call.window.present();
                                }
                            } else {
                                call.player_method(&conn, method, &params);
                            }
                            invocation.return_value(None);
                        },
                    )
                    .property(move |_conn, _sender, _path, _iface, name| {
                        if root {
                            get.root_property(name)
                        } else {
                            get.player_property(name)
                        }
                    })
                    .set_property(move |_conn, _sender, _path, _iface, name, value| {
                        !root && set.set_player_property(name, &value)
                    })
                    .build();
                if let Err(err) = registered {
                    eprintln!("elavo-music: cannot export {interface}: {err}");
                }
            }
            *connection.borrow_mut() = Some(conn);
        }
    };
    let owner = gio::bus_own_name(
        gio::BusType::Session,
        BUS_NAME,
        gio::BusNameOwnerFlags::NONE,
        acquired,
        |_, name| eprintln!("elavo-music: MPRIS ready as {name}"),
        |_, name| eprintln!("elavo-music: could not own {name}; media keys will not reach Music"),
    );
    // The name is held until the process ends; an OwnerId does not release
    // it when dropped, so there is nothing to keep alive.
    let _ = owner;

    // Tell listeners what changed.
    let weak: Weak<Service> = Rc::downgrade(&service);
    player.connect_changed(move |_| {
        let Some(service) = weak.upgrade() else {
            return;
        };
        let Some(conn) = connection.borrow().clone() else {
            return;
        };
        let changed = glib::VariantDict::new(None);
        for name in [
            "PlaybackStatus",
            "LoopStatus",
            "Shuffle",
            "Metadata",
            "Volume",
            "CanGoNext",
            "CanGoPrevious",
            "CanPlay",
            "CanPause",
            "CanSeek",
        ] {
            changed.insert_value(name, &service.player_property(name));
        }
        let parameters = glib::Variant::tuple_from_iter([
            PLAYER.to_variant(),
            changed.end(),
            Vec::<String>::new().to_variant(),
        ]);
        let _ = conn.emit_signal(
            None,
            OBJECT_PATH,
            PROPERTIES,
            "PropertiesChanged",
            Some(&parameters),
        );
    });
    let _ = &service.app;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Track {
        Track {
            id: 42,
            path: "/home/k/Music/88rising/Walking.mp3".into(),
            title: "Walking (feat. Swae Lee & Major Lazer)".into(),
            artist: "88rising".into(),
            album_artist: "88rising".into(),
            album: "Head In The Clouds II".into(),
            genre: "Hip-Hop".into(),
            track_no: 7,
            disc_no: 1,
            duration_ms: 166_000,
            ..Track::default()
        }
    }

    fn state(playing: bool, has_track: bool) -> PlayerState {
        PlayerState {
            playing,
            has_track,
            track_id: 0,
            shuffle: false,
            repeat: Repeat::Off,
            volume_percent: 80,
        }
    }

    #[test]
    fn the_bus_name_is_a_valid_mpris_name() {
        assert!(BUS_NAME.starts_with("org.mpris.MediaPlayer2."));
        assert!(gio::dbus_is_name(BUS_NAME));
        // A media widget finds the app by this.
        assert_eq!(
            BUS_NAME.trim_start_matches("org.mpris.MediaPlayer2."),
            "io.elavo.Music"
        );
    }

    #[test]
    fn introspection_parses_and_has_both_interfaces() {
        let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).unwrap();
        assert!(node.lookup_interface(ROOT).is_some());
        let player = node.lookup_interface(PLAYER).unwrap();
        for method in [
            "Next",
            "Previous",
            "Pause",
            "PlayPause",
            "Stop",
            "Play",
            "Seek",
            "SetPosition",
            "OpenUri",
        ] {
            assert!(player.lookup_method(method).is_some(), "{method}");
        }
        for property in [
            "PlaybackStatus",
            "Metadata",
            "Volume",
            "Position",
            "CanControl",
            "LoopStatus",
            "Shuffle",
        ] {
            assert!(player.lookup_property(property).is_some(), "{property}");
        }
    }

    #[test]
    fn status_follows_the_player() {
        assert_eq!(playback_status(&state(false, false)), "Stopped");
        assert_eq!(playback_status(&state(true, false)), "Stopped");
        assert_eq!(playback_status(&state(false, true)), "Paused");
        assert_eq!(playback_status(&state(true, true)), "Playing");
    }

    #[test]
    fn loop_status_round_trips() {
        for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
            assert_eq!(repeat_from_loop_status(loop_status(repeat)), Some(repeat));
        }
        assert_eq!(repeat_from_loop_status("Sometimes"), None);
    }

    #[test]
    fn metadata_carries_what_widgets_show() {
        let art = Path::new("/home/k/.cache/elavo-music/art/ab.jpg");
        let meta = metadata(Some(&song()), Some(art));
        assert_eq!(meta.type_().to_string(), "a{sv}");
        let dict = gio::glib::VariantDict::new(Some(&meta));
        let get = |key: &str| {
            dict.lookup_value(key, None)
                .unwrap_or_else(|| panic!("no {key}"))
        };
        assert_eq!(get("mpris:trackid").str(), Some("/io/elavo/Music/track/42"));
        assert_eq!(get("mpris:length").get::<i64>(), Some(166_000_000));
        assert_eq!(
            get("xesam:title").str(),
            Some("Walking (feat. Swae Lee & Major Lazer)")
        );
        assert_eq!(get("xesam:album").str(), Some("Head In The Clouds II"));
        assert_eq!(
            get("xesam:artist").get::<Vec<String>>(),
            Some(vec!["88rising".to_string()])
        );
        assert_eq!(get("xesam:trackNumber").get::<i32>(), Some(7));
        assert_eq!(
            get("mpris:artUrl").str(),
            Some("file:///home/k/.cache/elavo-music/art/ab.jpg")
        );
        assert_eq!(
            get("xesam:url").str(),
            Some("file:///home/k/Music/88rising/Walking.mp3")
        );
    }

    #[test]
    fn metadata_without_a_song_is_the_no_track_marker() {
        let meta = metadata(None, None);
        let dict = gio::glib::VariantDict::new(Some(&meta));
        assert_eq!(
            dict.lookup_value("mpris:trackid", None).unwrap().str(),
            Some(NO_TRACK)
        );
        assert!(dict.lookup_value("xesam:title", None).is_none());
        let mut bare = song();
        bare.genre.clear();
        bare.track_no = 0;
        let meta = metadata(Some(&bare), None);
        let dict = gio::glib::VariantDict::new(Some(&meta));
        assert!(dict.lookup_value("xesam:genre", None).is_none());
        assert!(dict.lookup_value("xesam:trackNumber", None).is_none());
        assert!(dict.lookup_value("mpris:artUrl", None).is_none());
    }
}
