# Elavo Music

Music is the Elavo desktop's music app: a library of the songs in your Music folder, with a floating player bar. The app is Elavo Music, the app id is `io.elavo.Music`, and the name everywhere in the OS (menus, launcher, window title) is just **Music**.

It is written in Rust with GTK 4, on the same footing as Elavo Files, System Settings and Archive Utility: windows, title bars and window buttons come from the GTK theme, the window buttons sit over the sidebar, and the app follows the system light and dark setting. Dark is the look it is drawn for.

## What works (version 0.1.0, a first shell)

- A sidebar with Library (Recently Added, Artists, Albums, Songs), Playlists and Devices. Playlists and Devices are placeholders with their own empty pages.
- A header for each view with a filter button (filter by genre) and a "Find in …" field. Ctrl+F goes to Find.
- A scan of the Music folder (the XDG music folder, else `~/Music`) on start and from the empty page. It reads tags and embedded artwork with lofty, falls back to `cover.jpg` or `folder.png` beside the songs, and stores everything in SQLite under `~/.local/share/elavo-music/library.db`. A rescan reads only files whose size or time changed and drops songs whose files are gone. Artwork is cached under `~/.cache/elavo-music/art`.
- Songs: a sortable table (title, time, artist, album, genre, plays). Double-click plays from that song.
- Albums and Recently Added: an artwork grid that opens into an album page (artwork, details, Play and Shuffle, the track list).
- Artists: a split view, the artist list on the left and that artist's albums with their songs on the right.
- A floating pill player bar: shuffle, previous, play or pause, next, repeat (off, all, one), artwork, title and artist, a progress bar you can seek with, and volume. Songs are counted as played after half their length or 30 seconds.
- Empty states for an empty library and for a search or filter with no results.

## Not built yet

MPRIS (media keys and Folio's media controls), gapless playback, ReplayGain, the equalizer, the queue panel, the MiniPlayer, full-screen Now Playing, star ratings, Get Info and tag editing, playlists, a settings window, watching the folder for changes, opening files from the file manager, importing a Music or iTunes library, disc import, and device sync.

## Sound

Audio output uses GStreamer's `playbin3` and is on by default (the `playback` feature). Building it needs the GStreamer development files:

```
sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel
cargo build --release
```

`cargo build --no-default-features` builds a silent backend that keeps the queue, the bar and the clock working, and the speaker icon says so in its tooltip. At run time, playback needs the GStreamer plugins for your formats. `gstreamer1-plugins-good` covers MP3. AAC (`.m4a`) needs `gstreamer1-plugin-libav`. FLAC and WAV are already in the base plugins.

## Accent

Under System Settings' coming Automatic accent, an app can declare its own default accent and have it respected, and any color the person picks overrides it. Music's default is a pinkish red, set in `src/accent.rs` and declared for the system as `X-Elavo-DefaultAccent` in the `.desktop` file, with separate light and dark values. **The current values are placeholders**, to be replaced with colors sampled from the Apple Music screenshots.

## Settings

The settings model (`elavo-music-core/src/settings.rs`, stored in `~/.config/elavo/music.conf`) holds the music folder and the disc-import format. The default is AAC at 256 kbps, variable bitrate, with MP3, Apple Lossless and FLAC as options. There is no window for it yet.

## Build and test

```
cargo build
cargo test
scripts/make-test-library.sh          # a small library of generated tones (needs ffmpeg)
scripts/nested-session.sh start       # a headless labwc, never your real session
scripts/nested-session.sh run
scripts/nested-session.sh shot NAME
scripts/nested-session.sh stop
```

`scripts/install` installs a release build (`DESTDIR` and `PREFIX` are honored), the way the image build does for the other apps.

## Layout

- `crates/elavo-music-core`: the database, scanner, tags, artwork cache, play queue and settings. No GTK.
- `crates/elavo-music`: the GTK app.
- `data/`: the `.desktop` file, metainfo, icon, stylesheet.

## License

GPL-3.0-or-later, so code can be shared with other GPL players. See `LICENSE`. The other Elavo apps are MIT; Music's code must not be copied into them.
