//! Music's own default accent: the pinkish red Apple Music used until
//! recently. Under System Settings' Automatic accent (not built yet) an app
//! that declares a default has it respected; any color the person picks
//! overrides it.
//!
//! The same values are declared for the system in `X-Elavo-DefaultAccent` in
//! `data/io.elavo.Music.desktop`; keep the two in step.
//!
//! TODO(accent): PLACEHOLDER VALUES. They are close to Apple's red, not
//! measured. Replace both with colors sampled from Kevin's Apple Music
//! screenshots (the selected row fill and the sidebar icons, one light and
//! one dark; dark is slightly pinker).

pub const DEFAULT_ACCENT_LIGHT: &str = "#FA233B"; // PLACEHOLDER
pub const DEFAULT_ACCENT_DARK: &str = "#FC3C5B"; // PLACEHOLDER

/// The stylesheet that sets `--music-accent` for the current color scheme.
/// `style.css` uses the variable everywhere the accent shows.
pub fn css(dark: bool) -> String {
    let color = if dark {
        DEFAULT_ACCENT_DARK
    } else {
        DEFAULT_ACCENT_LIGHT
    };
    format!("window.music-window {{ --music-accent: {color}; }}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_picks_by_scheme() {
        assert!(css(true).contains(DEFAULT_ACCENT_DARK));
        assert!(css(false).contains(DEFAULT_ACCENT_LIGHT));
    }

    #[test]
    fn placeholders_are_hex_colors() {
        for c in [DEFAULT_ACCENT_LIGHT, DEFAULT_ACCENT_DARK] {
            assert_eq!(c.len(), 7);
            assert!(c.starts_with('#') && c[1..].chars().all(|d| d.is_ascii_hexdigit()));
        }
    }
}
