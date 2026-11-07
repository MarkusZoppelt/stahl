#!/usr/bin/env bash
# Regenerates every icon from the steel door: the app's (assets/icons), the mark it draws inside
# (assets/door.png), the README's logo and the landing page's. Run it after changing site/door.js:
#
#   site/render-icons.sh
#
# Node, Playwright's Chromium and Pillow come from nixpkgs and npm, in a temporary directory.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d /tmp/stahl-icons.XXXX)
trap 'rm -rf "$work"; kill "$server" 2>/dev/null || true' EXIT

# The page's shader, served on loopback as the page would be.
nix shell nixpkgs#python3 --command python3 -m http.server 8199 --bind 127.0.0.1 -d "$root/site" >/dev/null 2>&1 &
server=$!
sleep 1

cp "$root/site/render-icons.mjs" "$work/"
(cd "$work" && nix shell nixpkgs#nodejs --command bash -c \
  'npm init -y >/dev/null && npm install --silent playwright >/dev/null && npx playwright install chromium >/dev/null && node render-icons.mjs http://127.0.0.1:8199/ door.rgba')

nix-shell -p "python3.withPackages (p: [ p.pillow ])" --run "python3 -I - '$work' '$root'" <<'EOF'
import math, sys
from pathlib import Path
from PIL import Image, ImageChops

work, root = Path(sys.argv[1]), Path(sys.argv[2])
size = 1024
door = Image.frombytes("RGBA", (size, size), (work / "door.rgba").read_bytes())

# Nothing outside the rim: the page draws a soft shadow there, which an icon must not carry.
edge = size / 2 - 2
rim = Image.new("L", (size, size))
rim.putdata([
    max(0, min(255, round((edge - math.hypot(x + 0.5 - size / 2, y + 0.5 - size / 2) + 0.5) * 255)))
    for y in range(size) for x in range(size)
])
door.putalpha(ImageChops.multiply(door.getchannel("A"), rim))

def at(px):
    return door.resize((px, px), Image.LANCZOS)

icons = root / "assets/icons"
at(512).save(icons / "icon.png", optimize=True)
at(32).save(icons / "32x32.png", optimize=True)
at(64).save(icons / "64x64.png", optimize=True)
at(128).save(icons / "128x128.png", optimize=True)
at(256).save(icons / "128x128@2x.png", optimize=True)
door.save(icons / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256)])

iconset = work / "icon.iconset"
iconset.mkdir()
for px in (16, 32, 128, 256, 512):
    at(px).save(iconset / f"icon_{px}x{px}.png")
    at(px * 2).save(iconset / f"icon_{px}x{px}@2x.png")

# The app draws the door at up to 168 points, so 2x that with room to spare, mipmapped by Rune.
at(512).save(root / "assets/door.png", optimize=True)
at(256).save(root / "site/icon.png", optimize=True)
at(64).save(root / "site/favicon.png", optimize=True)
at(180).save(root / "site/apple-touch-icon.png", optimize=True)
EOF

if command -v iconutil >/dev/null; then
  iconutil -c icns "$work/icon.iconset" -o "$root/assets/icons/icon.icns"
else
  echo "iconutil is macOS only: assets/icons/icon.icns is unchanged" >&2
fi
