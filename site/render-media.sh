#!/usr/bin/env bash
# Regenerates site/media from the demo: every image and frame of video on the landing page comes
# from Stahl's own renderer, on the made-up portfolio in src/demo.rs. Run it when the UI changes:
#
#   nix develop -c site/render-media.sh
#
# ffmpeg and cwebp come from nixpkgs, so nothing beyond Nix is needed.
set -euo pipefail

if ! command -v ffmpeg >/dev/null || ! command -v cwebp >/dev/null; then
  exec nix shell nixpkgs#ffmpeg nixpkgs#libwebp --command "$0" "$@"
fi

root=$(cd "$(dirname "$0")/.." && pwd)
media="$root/site/media"
work=$(mktemp -d /tmp/stahl.XXXX)
trap 'rm -rf "$work"' EXIT

(cd "$root" && cargo build --quiet --example demo)
demo="${CARGO_TARGET_DIR:-$root/target}/debug/examples/demo"
export RUNE_PERF_WARN=0
# The demo prints paths under here, and this keeps them short in the footage.
export TMPDIR="$work"

size=1320x860

# A still at 2x, as WebP. Text stays sharp at the size the page shows it.
still() { # name [demo arguments...]
  local name=$1
  shift
  "$demo" --snap "$work/$name.png" --size "$size" "$@" 2>/dev/null
  cwebp -quiet -q 86 -m 6 -resize 1980 0 "$work/$name.png" -o "$media/$name.webp"
}

# The same page with amounts hidden, for the page's own Hide Amounts.
hidden() { # name nav-key
  cat >"$work/$2.json" <<EOF
[
  { "t": 0.2, "type": "tap", "key": "privacy" },
  { "t": 0.4, "type": "tap", "key": "$2" },
  { "t": 3.0, "type": "mark", "name": "end" }
]
EOF
  "$demo" --snap "$work/$1-hidden.png" --size "$size" --script "$work/$2.json" --at 3 2>/dev/null
  cwebp -quiet -q 80 -m 6 -resize 1980 0 "$work/$1-hidden.png" -o "$media/$1-hidden.webp"
}

for scene in overview holdings allocation performance review rebalance policy briefing health documents; do
  still "$scene" --scene "$scene" --at 2.5
done
still palette --scene palette --at 2
still light --scene light --at 2.5

for page in overview holdings allocation performance review rebalance policy briefing health documents; do
  hidden "$page" "nav-$page"
done

# The tour: real input replayed headless, every frame from the renderer. Keycaps and press ripples
# are Rune's own overlays. Encoded twice, because the renderer's encoder aims at quality, not size.
"$demo" --size "$size" --scale 1.5 --fps 60 --script "$media/tour.json" \
  --video "$work/tour.mp4" --duration auto --keys bottom-right --pointer 2>/dev/null
ffmpeg -hide_banner -loglevel error -y -i "$work/tour.mp4" \
  -c:v libx264 -preset veryslow -crf 25 -pix_fmt yuv420p -movflags +faststart -an "$media/tour.mp4"
# Open-source Chromium builds have no H.264.
ffmpeg -hide_banner -loglevel error -y -i "$work/tour.mp4" \
  -c:v libvpx-vp9 -crf 36 -b:v 0 -row-mt 1 -deadline good -cpu-used 2 -pix_fmt yuv420p -an "$media/tour.webm"

du -h "$media"/* | sort -h
