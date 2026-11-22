#!/bin/bash
# Make a small music folder for trying Music, with tags, artwork and mixed
# formats, from generated tones. Needs ffmpeg. Nothing of yours is used.
#
#   scripts/make-test-library.sh [DIR]     (default /tmp/elavo-music-test-library)

set -euo pipefail

OUT=${1:-/tmp/elavo-music-test-library}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
rm -rf "$OUT"
mkdir -p "$OUT"

# cover NAME COLOR1 COLOR2: a square gradient picture.
cover() {
    ffmpeg -v error -y -f lavfi -i "gradients=s=500x500:c0=$2:c1=$3:x0=0:y0=0:x1=500:y1=500:d=1:r=1" \
        -frames:v 1 "$TMP/$1.png"
}

# song FILE FREQ SECONDS TITLE ARTIST ALBUM_ARTIST ALBUM GENRE YEAR TRACK DISC COVER
song() {
    local file=$1 freq=$2 secs=$3 title=$4 artist=$5 aartist=$6 album=$7 genre=$8 year=$9 track=${10} disc=${11} art=${12}
    mkdir -p "$(dirname "$file")"
    local codec=() ext=${file##*.}
    case $ext in
        mp3) codec=(-c:a libmp3lame -b:a 128k) ;;
        flac) codec=(-c:a flac) ;;
        m4a) codec=(-c:a aac -b:a 128k) ;;
    esac
    local meta=(-metadata "title=$title" -metadata "artist=$artist" -metadata "album_artist=$aartist"
        -metadata "album=$album" -metadata "genre=$genre" -metadata "date=$year"
        -metadata "track=$track" -metadata "disc=$disc")
    if [ -n "$art" ]; then
        ffmpeg -v error -y -f lavfi -i "sine=frequency=$freq:duration=$secs" -i "$TMP/$art.png" \
            -map 0:a -map 1:v "${codec[@]}" -c:v mjpeg -disposition:v attached_pic \
            "${meta[@]}" -metadata:s:v title="Album cover" -metadata:s:v comment="Cover (front)" "$file"
    else
        ffmpeg -v error -y -f lavfi -i "sine=frequency=$freq:duration=$secs" \
            "${codec[@]}" "${meta[@]}" "$file"
    fi
}

cover dawn "#ff9a8b" "#6a11cb"
cover tide "#00c6ff" "#0072ff"
cover ember "#f12711" "#f5af19"
cover moss "#134e5e" "#71b280"
cover neon "#fc00ff" "#00dbde"
cover dusk "#2c3e50" "#fd746c"

M=$OUT
song "$M/Alpha Drift/First Light/01 Dawn.mp3"        261 28 "Dawn"          "Alpha Drift" "Alpha Drift" "First Light" "Ambient" 2021 1 1 dawn
song "$M/Alpha Drift/First Light/02 Morning Haze.mp3" 294 34 "Morning Haze"  "Alpha Drift" "Alpha Drift" "First Light" "Ambient" 2021 2 1 dawn
song "$M/Alpha Drift/First Light/03 Noon.mp3"         330 41 "Noon"          "Alpha Drift" "Alpha Drift" "First Light" "Ambient" 2021 3 1 dawn
song "$M/Alpha Drift/First Light/04 Dusk.mp3"         349 47 "Dusk"          "Alpha Drift" "Alpha Drift" "First Light" "Ambient" 2021 4 1 dawn
song "$M/Alpha Drift/Low Tide/01 Ebb.mp3"             220 52 "Ebb"           "Alpha Drift" "Alpha Drift" "Low Tide"    "Ambient" 2023 1 1 tide
song "$M/Alpha Drift/Low Tide/02 Flow.mp3"            247 63 "Flow"          "Alpha Drift" "Alpha Drift" "Low Tide"    "Ambient" 2023 2 1 tide
song "$M/Alpha Drift/Low Tide/03 Slack Water.mp3"     277 38 "Slack Water"   "Alpha Drift" "Alpha Drift" "Low Tide"    "Ambient" 2023 3 1 tide
song "$M/The Embers/Burn Rate/01 Kindling.flac"       392 31 "Kindling"      "The Embers"  "The Embers"  "Burn Rate"   "Rock"    2019 1 1 ember
song "$M/The Embers/Burn Rate/02 Flashpoint.flac"     440 44 "Flashpoint"    "The Embers"  "The Embers"  "Burn Rate"   "Rock"    2019 2 1 ember
song "$M/The Embers/Burn Rate/03 Ash.flac"            494 57 "Ash"           "The Embers"  "The Embers"  "Burn Rate"   "Rock"    2019 3 1 ember
song "$M/The Embers/Burn Rate/04 Last Spark.flac"     523 36 "Last Spark"    "The Embers"  "The Embers"  "Burn Rate"   "Rock"    2019 4 1 ember
song "$M/Moss & Fern/Understory/01 Canopy.m4a"        196 49 "Canopy"        "Moss & Fern" "Moss & Fern" "Understory"  "Folk"    2022 1 1 moss
song "$M/Moss & Fern/Understory/02 Lichen.m4a"        233 72 "Lichen"        "Moss & Fern" "Moss & Fern" "Understory"  "Folk"    2022 2 1 moss
song "$M/Moss & Fern/Understory/03 Spore.m4a"         262 33 "Spore"         "Moss & Fern" "Moss & Fern" "Understory"  "Folk"    2022 3 1 moss
song "$M/Neon Harbor/Night Shift/01 Overtime.mp3"     311 29 "Overtime"      "Neon Harbor" "Neon Harbor" "Night Shift" "Electronic" 2024 1 1 neon
song "$M/Neon Harbor/Night Shift/02 Last Train.mp3"   370 66 "Last Train"    "Neon Harbor" "Neon Harbor" "Night Shift" "Electronic" 2024 2 1 neon
song "$M/Neon Harbor/Night Shift/03 Afterglow.mp3"    415 54 "Afterglow"     "Neon Harbor" "Neon Harbor" "Night Shift" "Electronic" 2024 3 1 neon
song "$M/Neon Harbor/Night Shift/04 Closing.mp3"      466 42 "Closing"       "Neon Harbor" "Neon Harbor" "Night Shift" "Electronic" 2024 4 1 neon
# A compilation (several artists, one album artist) and an untagged file.
song "$M/Various/Dusk Sessions/01 Golden Hour.mp3"    247 39 "Golden Hour"   "Alpha Drift"  "Various Artists" "Dusk Sessions" "Ambient" 2020 1 1 dusk
song "$M/Various/Dusk Sessions/02 Lamp Lit.mp3"       294 46 "Lamp Lit"      "Moss & Fern"  "Various Artists" "Dusk Sessions" "Folk"    2020 2 1 dusk
song "$M/Various/Dusk Sessions/03 Embers Fade.mp3"    349 51 "Embers Fade"   "The Embers"   "Various Artists" "Dusk Sessions" "Rock"    2020 3 1 dusk
ffmpeg -v error -y -f lavfi -i "sine=frequency=180:duration=20" -c:a libmp3lame -b:a 96k "$M/loose track.mp3"

echo "$(find "$OUT" -type f | wc -l) files in $OUT"
