//! Keyboard media controls: Space (when not typing) and the XF86 audio keys.
//!
//! Space is also the default activation key for a focused GtkButton and for
//! some list widgets. Handling it on the window in the capture phase, and
//! ignoring typematic repeats, keeps it from restarting the selected song or
//! leaving the play/pause button stuck after a key press.

use gtk::gdk::Key;
use gtk::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    Toggle,
    Pause,
    Next,
    Previous,
}

/// What a key should do, if anything.
///
/// Media keys work even while Find is focused. Space and keypad Space toggle
/// playback only when the focus is not an editable widget, so a search can
/// still contain spaces. Repeats are ignored so holding a key does not flutter
/// between play and pause.
pub fn media_key_action(key: Key, focus_is_editable: bool, is_repeat: bool) -> Option<MediaAction> {
    if is_repeat {
        return None;
    }
    if key == Key::space || key == Key::KP_Space {
        if focus_is_editable {
            return None;
        }
        return Some(MediaAction::Toggle);
    }
    if key == Key::AudioPlay {
        return Some(MediaAction::Toggle);
    }
    if key == Key::AudioPause || key == Key::AudioStop {
        return Some(MediaAction::Pause);
    }
    if key == Key::AudioNext {
        return Some(MediaAction::Next);
    }
    if key == Key::AudioPrev {
        return Some(MediaAction::Previous);
    }
    None
}

/// True when `widget` or one of its ancestors is a text field.
pub fn focus_is_editable(widget: &gtk::Widget) -> bool {
    let mut current = Some(widget.clone());
    while let Some(w) = current {
        if w.is::<gtk::Text>()
            || w.is::<gtk::Entry>()
            || w.is::<gtk::SearchEntry>()
            || w.is::<gtk::TextView>()
            || w.is::<gtk::SpinButton>()
        {
            return true;
        }
        current = w.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_toggles_unless_editing_or_repeat() {
        assert_eq!(
            media_key_action(Key::space, false, false),
            Some(MediaAction::Toggle)
        );
        assert_eq!(
            media_key_action(Key::KP_Space, false, false),
            Some(MediaAction::Toggle)
        );
        assert_eq!(media_key_action(Key::space, true, false), None);
        assert_eq!(media_key_action(Key::space, false, true), None);
        assert_eq!(media_key_action(Key::KP_Space, true, false), None);
    }

    #[test]
    fn media_keys_work_while_editing() {
        assert_eq!(
            media_key_action(Key::AudioPlay, true, false),
            Some(MediaAction::Toggle)
        );
        assert_eq!(
            media_key_action(Key::AudioPause, true, false),
            Some(MediaAction::Pause)
        );
        assert_eq!(
            media_key_action(Key::AudioStop, false, false),
            Some(MediaAction::Pause)
        );
        assert_eq!(
            media_key_action(Key::AudioNext, true, false),
            Some(MediaAction::Next)
        );
        assert_eq!(
            media_key_action(Key::AudioPrev, false, false),
            Some(MediaAction::Previous)
        );
        assert_eq!(media_key_action(Key::AudioPlay, false, true), None);
        assert_eq!(media_key_action(Key::a, false, false), None);
    }
}
