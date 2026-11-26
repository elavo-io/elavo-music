//! The global menu. The Folio panel draws the bold application menu (About,
//! Hide, Quit) and the Window menu itself, so Music exports only its own
//! menus: File, Edit, Controls and View. It has no menu named Music and no
//! Quit of its own.

use gtk::gio;
use gtk::prelude::*;

/// Every `win.` action the menu uses, so the window can be checked to
/// provide them all.
pub const ACTIONS: &[&str] = &[
    "add-to-library",
    "rescan",
    "find",
    "preferences",
    "play-pause",
    "next",
    "previous",
    "shuffle",
    "repeat",
    "volume-up",
    "volume-down",
    "view",
    "show-genres",
];

/// Keyboard shortcuts for the actions, as `(detailed action, accelerators)`.
/// Space is not here: the window handles it itself so a search can still
/// contain spaces; the menu just shows it.
pub const ACCELS: &[(&str, &[&str])] = &[
    ("win.add-to-library", &["<Control>o"]),
    ("win.rescan", &["<Control>r"]),
    ("win.find", &["<Control>f"]),
    ("win.preferences", &["<Control>comma"]),
    ("win.next", &["<Control>Right"]),
    ("win.previous", &["<Control>Left"]),
    ("win.volume-up", &["<Control>Up"]),
    ("win.volume-down", &["<Control>Down"]),
    ("win.show-genres", &["<Control>g"]),
    ("win.view::recent", &["<Control>1"]),
    ("win.view::artists", &["<Control>2"]),
    ("win.view::albums", &["<Control>3"]),
    ("win.view::songs", &["<Control>4"]),
];

pub fn install_accels(app: &gtk::Application) {
    for (action, accels) in ACCELS {
        app.set_accels_for_action(action, accels);
    }
}

fn item(label: &str, action: &str) -> gio::MenuItem {
    gio::MenuItem::new(Some(label), Some(action))
}

fn choice(label: &str, action: &str, target: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), None);
    item.set_action_and_target_value(Some(action), Some(&target.to_variant()));
    item
}

pub fn build() -> gio::Menu {
    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    file.append_item(&item("Add to Library…", "win.add-to-library"));
    file.append_item(&item("Rescan Library", "win.rescan"));
    menu.append_submenu(Some("File"), &file);

    let edit = gio::Menu::new();
    edit.append_item(&item("Find", "win.find"));
    let prefs = gio::Menu::new();
    prefs.append_item(&item("Preferences…", "win.preferences"));
    edit.append_section(None, &prefs);
    menu.append_submenu(Some("Edit"), &edit);

    let controls = gio::Menu::new();
    let play = item("Play/Pause", "win.play-pause");
    // Shown only; the window's key handler does the work.
    play.set_attribute_value("accel", Some(&"space".to_variant()));
    controls.append_item(&play);
    controls.append_item(&item("Next", "win.next"));
    controls.append_item(&item("Previous", "win.previous"));
    let modes = gio::Menu::new();
    modes.append_item(&item("Shuffle", "win.shuffle"));
    controls.append_section(None, &modes);
    let repeat = gio::Menu::new();
    repeat.append_item(&choice("Repeat Off", "win.repeat", "off"));
    repeat.append_item(&choice("Repeat All", "win.repeat", "all"));
    repeat.append_item(&choice("Repeat One", "win.repeat", "one"));
    controls.append_section(None, &repeat);
    let volume = gio::Menu::new();
    volume.append_item(&item("Volume Up", "win.volume-up"));
    volume.append_item(&item("Volume Down", "win.volume-down"));
    controls.append_section(None, &volume);
    menu.append_submenu(Some("Controls"), &controls);

    let view = gio::Menu::new();
    let views = gio::Menu::new();
    views.append_item(&choice("Recently Added", "win.view", "recent"));
    views.append_item(&choice("Artists", "win.view", "artists"));
    views.append_item(&choice("Albums", "win.view", "albums"));
    views.append_item(&choice("Songs", "win.view", "songs"));
    view.append_section(None, &views);
    let filter = gio::Menu::new();
    filter.append_item(&item("Filter by Genre…", "win.show-genres"));
    view.append_section(None, &filter);
    menu.append_submenu(Some("View"), &view);

    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every labelled item, with its action, however deeply nested.
    fn collect(model: &gio::MenuModel, out: &mut Vec<(String, Option<String>)>) {
        for i in 0..model.n_items() {
            let label = model
                .item_attribute_value(i, "label", None)
                .and_then(|v| v.get::<String>());
            let action = model
                .item_attribute_value(i, "action", None)
                .and_then(|v| v.get::<String>());
            if let Some(label) = label {
                out.push((label, action));
            }
            for link in [gio::MENU_LINK_SUBMENU, gio::MENU_LINK_SECTION] {
                if let Some(sub) = model.item_link(i, link) {
                    collect(&sub, out);
                }
            }
        }
    }

    fn top_menus() -> Vec<String> {
        let menu = build();
        (0..menu.n_items())
            .filter_map(|i| {
                menu.item_attribute_value(i, "label", None)
                    .and_then(|v| v.get::<String>())
            })
            .collect()
    }

    fn all_items() -> Vec<(String, Option<String>)> {
        let menu = build();
        let mut items = Vec::new();
        collect(menu.upcast_ref(), &mut items);
        items
    }

    #[test]
    fn exports_no_music_menu_and_no_quit() {
        let tops = top_menus();
        assert_eq!(tops, ["File", "Edit", "Controls", "View"]);
        assert!(
            !tops.iter().any(|t| t == "Music"),
            "the panel already draws a menu named Music"
        );
        assert!(
            !tops.iter().any(|t| t == "Window"),
            "the panel adds the Window menu"
        );
        for (label, action) in all_items() {
            assert!(
                !label.starts_with("Quit"),
                "the panel provides Quit: {label}"
            );
            assert!(
                !label.starts_with("Hide"),
                "the panel provides Hide: {label}"
            );
            assert_ne!(action.as_deref(), Some("app.quit"));
        }
    }

    #[test]
    fn has_no_local_ai_items() {
        let words = |s: &str| {
            s.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        for (label, action) in all_items() {
            assert!(!words(&label).iter().any(|w| w == "ai"), "AI item: {label}");
            assert!(!words(&action.unwrap_or_default()).iter().any(|w| w == "ai"));
        }
        assert!(!top_menus()
            .iter()
            .any(|t| words(t).iter().any(|w| w == "ai")));
    }

    #[test]
    fn every_action_is_a_known_window_action() {
        let menus = top_menus();
        for (label, action) in all_items() {
            if action.is_none() && menus.contains(&label) {
                continue; // a top-level menu title, not a command
            }
            let action = action.unwrap_or_else(|| panic!("{label} has no action"));
            let name = action
                .strip_prefix("win.")
                .unwrap_or_else(|| panic!("{label}: {action} is not a window action"));
            assert!(ACTIONS.contains(&name), "{label}: unknown action {name}");
        }
        for name in ACTIONS {
            assert!(
                all_items()
                    .iter()
                    .any(|(_, a)| a.as_deref() == Some(&format!("win.{name}"))),
                "{name} is in ACTIONS but not in the menu"
            );
        }
    }

    #[test]
    fn controls_menu_has_the_playback_items() {
        let items = all_items();
        for needed in [
            "win.play-pause",
            "win.next",
            "win.previous",
            "win.shuffle",
            "win.repeat",
            "win.volume-up",
            "win.volume-down",
        ] {
            assert!(
                items.iter().any(|(_, a)| a.as_deref() == Some(needed)),
                "Controls lacks {needed}"
            );
        }
    }

    #[test]
    fn accelerators_point_at_real_actions() {
        for (detailed, _) in ACCELS {
            let name = detailed
                .strip_prefix("win.")
                .unwrap()
                .split("::")
                .next()
                .unwrap();
            assert!(ACTIONS.contains(&name), "{detailed}");
        }
    }
}
