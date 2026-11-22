//! Small formatting helpers.

/// `3:45`, or `1:02:03` for an hour or more.
pub fn clock(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `45 seconds`, `12 minutes`, `1 hour, 5 minutes`: a length for an album header.
pub fn length_words(ms: u64) -> String {
    let minutes = (ms + 30_000) / 60_000;
    let plural = |n: u64, word: &str| {
        if n == 1 {
            format!("{n} {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    if ms < 60_000 {
        return plural(ms / 1000, "second");
    }
    let (h, m) = (minutes / 60, minutes % 60);
    match (h, m) {
        (0, m) => plural(m, "minute"),
        (h, 0) => plural(h, "hour"),
        (h, m) => format!("{}, {}", plural(h, "hour"), plural(m, "minute")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_formats() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(59_999), "0:59");
        assert_eq!(clock(225_000), "3:45");
        assert_eq!(clock(3_723_000), "1:02:03");
    }

    #[test]
    fn length_words_formats() {
        assert_eq!(length_words(1000), "1 second");
        assert_eq!(length_words(45_000), "45 seconds");
        assert_eq!(length_words(12 * 60_000), "12 minutes");
        assert_eq!(length_words(60_000), "1 minute");
        assert_eq!(length_words(3_600_000), "1 hour");
        assert_eq!(length_words(65 * 60_000), "1 hour, 5 minutes");
    }
}
