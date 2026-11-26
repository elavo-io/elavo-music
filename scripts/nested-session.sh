#!/bin/bash
# Run Elavo Music in a separate, headless labwc, so the real session is left
# alone. Never touches the real Wayland socket: every launch and screenshot
# first asserts that the socket it is about to use is the nested one.
#
#   scripts/nested-session.sh start [WxH]   headless labwc on a new socket (1280x800)
#   scripts/nested-session.sh run           start Music in it, on a test library
#   scripts/nested-session.sh shot NAME     screenshot to $SHOTS/NAME.png
#   scripts/nested-session.sh keys ARGS     wtype into it (e.g. keys -k space)
#   scripts/nested-session.sh stop          end only what this script started
#
# Music runs inside the elavo-dev toolbox on its own D-Bus session bus, with
# its settings, data and cache under $T, and scans $LIBRARY (made by
# scripts/make-test-library.sh), never your own music. Screenshot hooks are
# passed on: ELAVO_MUSIC_VIEW, ELAVO_MUSIC_OPEN_ALBUM, ELAVO_MUSIC_PLAY,
# ELAVO_MUSIC_PROGRESS_MS, ELAVO_MUSIC_THEN, ELAVO_MUSIC_DEBUG.
# Audio is fakesink by default, so nothing is sent to a speaker.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
T=${ELAVO_MUSIC_TEST:-/tmp/elavo-music-session}
LIBRARY=${LIBRARY:-/tmp/elavo-music-test-library}
SHOTS=${SHOTS:-$HOME/elavo-music-shots}
BOX=${TOOLBOX:-elavo-dev}
RUNTIME=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
mkdir -p "$T"

socket() {
    [ -s "$T/socket" ] || { echo "no nested session; run: $0 start" >&2; exit 1; }
    cat "$T/socket"
}

# Refuse to go on unless the nested socket exists and is not the real
# session's. Called before every launch and every screenshot.
assert_nested() {
    local sock real
    sock=$(socket)
    real=${REAL_WAYLAND_DISPLAY:-wayland-0}
    [ -n "$sock" ] || { echo "empty nested socket name" >&2; exit 1; }
    [ "$sock" != "$real" ] || { echo "refusing: $sock is the real session" >&2; exit 1; }
    [ "$sock" != "wayland-0" ] || { echo "refusing: wayland-0" >&2; exit 1; }
    [ -S "$RUNTIME/$sock" ] || { echo "nested socket $RUNTIME/$sock is missing" >&2; exit 1; }
    [ -s "$T/labwc.pid" ] && kill -0 "$(cat "$T/labwc.pid")" 2>/dev/null \
        || { echo "nested labwc is not running" >&2; exit 1; }
    echo "WAYLAND_DISPLAY=$sock (nested; real session is $real)" >&2
}

# kill_pid FILE MATCH: kill the pid in FILE only if its command line has MATCH.
kill_pid() {
    local file=$1 match=$2 pid
    [ -s "$file" ] || return 0
    pid=$(cat "$file")
    if [ -r "/proc/$pid/cmdline" ] && tr '\0' ' ' <"/proc/$pid/cmdline" | grep -qF -- "$match"; then
        kill "$pid" 2>/dev/null || true
    fi
    rm -f "$file"
}

case "${1:-}" in
start)
    if [ -s "$T/labwc.pid" ] && kill -0 "$(cat "$T/labwc.pid")" 2>/dev/null; then
        echo "nested session already running on $(socket); '$0 stop' first" >&2
        exit 1
    fi
    mode=${2:-1280x800}
    if [ -s "$T/labwc.pid" ] && kill -0 "$(cat "$T/labwc.pid")" 2>/dev/null; then
        echo "a nested session is already running ($(cat "$T/socket")); run: $0 stop" >&2
        exit 1
    fi
    mkdir -p "$T/labwc" "$T/config"
    : >"$T/labwc/autostart"
    # MUSIC_RC_KEYBINDS: <keybind> elements to put in the nested labwc, to
    # try global media keys.
    cat >"$T/labwc/rc.xml" <<XML
<?xml version="1.0"?>
<labwc_config>
  <core><decoration>client</decoration></core>
  <placement><policy>center</policy></placement>
  <keyboard>${MUSIC_RC_KEYBINDS:-}</keyboard>
</labwc_config>
XML
    rm -f "$T/socket"
    env -u WAYLAND_DISPLAY -u DISPLAY WLR_BACKENDS=headless WLR_RENDERER=pixman \
        WLR_HEADLESS_OUTPUTS=1 XDG_CONFIG_HOME="$T/config" \
        setsid labwc -C "$T/labwc" -s "sh -c 'echo \$WAYLAND_DISPLAY >$T/socket'" \
        >"$T/labwc.log" 2>&1 &
    echo $! >"$T/labwc.pid"
    for _ in $(seq 50); do [ -s "$T/socket" ] && break; sleep 0.1; done
    assert_nested
    WAYLAND_DISPLAY=$(socket) wlr-randr --output HEADLESS-1 --custom-mode "$mode" >/dev/null
    echo "nested labwc on $(socket) at $mode (pid $(cat "$T/labwc.pid"))"
    ;;
run)
    assert_nested
    sock=$(socket)
    kill_pid "$T/app.pid" "elavo-music"
    # Settings: dark, the Elavo theme and icons, Elavo's window button layout,
    # and the test library as the music folder.
    mkdir -p "$T/config/gtk-4.0" "$T/config/elavo"
    printf '[Settings]\ngtk-theme-name=%s\ngtk-icon-theme-name=%s\ngtk-interface-color-scheme=dark\ngtk-application-prefer-dark-theme=true\ngtk-decoration-layout=close,minimize,maximize:\n' \
        "${GTK_THEME_NAME:-Elavo}" "${ICON_THEME_NAME:-Elavo}" >"$T/config/gtk-4.0/settings.ini"
    printf 'music-folder=%s\n' "$LIBRARY" >"$T/config/elavo/music.conf"
    # MUSIC_ONLINE=ask|on|off sets the online-lookup choice; unset it stays
    # "ask", which sends nothing and shows the first-run question.
    [ -z "${MUSIC_ONLINE:-}" ] || printf 'online-metadata=%s\n' "$MUSIC_ONLINE" >>"$T/config/elavo/music.conf"
    [ -d "$LIBRARY" ] || { echo "no test library at $LIBRARY; run scripts/make-test-library.sh" >&2; exit 1; }
    [ "${KEEP:-0}" = 1 ] || rm -rf "$T/data/elavo-music" "$T/cache"
    mkdir -p "$T/data" "$T/cache"
    for d in themes icons applications; do
        if [ -e "$HOME/.local/share/$d" ]; then ln -sfn "$HOME/.local/share/$d" "$T/data/$d"; fi
    done
    H="$T/home/$USER"
    mkdir -p "$H"
    rm -f "$T/app.pid"
    toolbox run -c "$BOX" env -u GTK_THEME -u GSETTINGS_SCHEMA_DIR \
        WAYLAND_DISPLAY="$sock" GSK_RENDERER="${GSK_RENDERER:-cairo}" \
        env -u DBUS_SESSION_BUS_ADDRESS GDK_DEBUG=no-portals GTK_USE_PORTAL=0 GTK_A11Y=none \
        HOME="$H" XDG_CONFIG_HOME="$T/config" XDG_DATA_HOME="$T/data" XDG_CACHE_HOME="$T/cache" \
        ELAVO_MUSIC_AUDIO_SINK="${ELAVO_MUSIC_AUDIO_SINK:-fakesink}" \
        ${ELAVO_MUSIC_VIEW:+ELAVO_MUSIC_VIEW="$ELAVO_MUSIC_VIEW"} \
        ${ELAVO_MUSIC_OPEN_ALBUM:+ELAVO_MUSIC_OPEN_ALBUM="$ELAVO_MUSIC_OPEN_ALBUM"} \
        ${ELAVO_MUSIC_PLAY:+ELAVO_MUSIC_PLAY="$ELAVO_MUSIC_PLAY"} \
        ${ELAVO_MUSIC_PROGRESS_MS:+ELAVO_MUSIC_PROGRESS_MS="$ELAVO_MUSIC_PROGRESS_MS"} \
        ${ELAVO_MUSIC_THEN:+ELAVO_MUSIC_THEN="$ELAVO_MUSIC_THEN"} \
        ${ELAVO_MUSIC_DEBUG:+ELAVO_MUSIC_DEBUG="$ELAVO_MUSIC_DEBUG"} \
        ${ELAVO_MUSIC_SHOW_MENU:+ELAVO_MUSIC_SHOW_MENU="$ELAVO_MUSIC_SHOW_MENU"} \
        ${ELAVO_MUSIC_SHOW_PREFS:+ELAVO_MUSIC_SHOW_PREFS="$ELAVO_MUSIC_SHOW_PREFS"} \
        ${MUSIC_HTTPS_PROXY:+HTTPS_PROXY="$MUSIC_HTTPS_PROXY"} \
        dbus-run-session -- sh -c '
            echo $$ >"$1/app.pid"
            gsettings set org.gnome.desktop.interface color-scheme prefer-dark
            gsettings set org.gnome.desktop.interface icon-theme Elavo
            gsettings set org.gnome.desktop.wm.preferences button-layout "close,minimize,maximize:"
            exec "$2"' sh "$T" "$ROOT/${MUSIC_BIN:-target/debug/elavo-music}" >"$T/music.log" 2>&1 &
    echo $! >"$T/toolbox.pid"
    sleep "${WAIT:-5}"
    ;;
shot)
    assert_nested
    mkdir -p "$SHOTS"
    out="$SHOTS/${2:-music}.png"
    WAYLAND_DISPLAY=$(socket) grim -o HEADLESS-1 "$out"
    echo "$out"
    ;;
keys)
    assert_nested
    sock=$(socket)
    toolbox run -c "$BOX" env WAYLAND_DISPLAY="$sock" XDG_RUNTIME_DIR="$RUNTIME"         wlrctl toplevel focus app_id:io.elavo.Music
    toolbox run -c "$BOX" env WAYLAND_DISPLAY="$sock" XDG_RUNTIME_DIR="$RUNTIME"         wtype "${@:2}"
    ;;
stop)
    kill_pid "$T/app.pid" "elavo-music"
    kill_pid "$T/toolbox.pid" "toolbox"
    kill_pid "$T/labwc.pid" "$T/labwc"
    rm -f "$T/socket"
    ;;
*)
    sed -n '2,15p' "$0"
    exit 1
    ;;
esac
