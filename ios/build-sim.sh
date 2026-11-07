#!/usr/bin/env bash
# Builds Stahl.app for the iOS simulator, and with --run installs and launches it on a booted
# (or the named) simulator. Needs Xcode, and a Rust toolchain with the iOS simulator target,
# which the flake's `ios` shell provides. Run from anywhere:
#
#   nix develop .#ios -c ios/build-sim.sh [--release] [--run] [--device "iPhone 18 Pro"]
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
profile=debug
cargo_profile=()
run=0
device="iPhone 18 Pro"
while [ $# -gt 0 ]; do
  case "$1" in
    --release) profile=release; cargo_profile=(--release) ;;
    --run) run=1 ;;
    --device) device="$2"; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

target=aarch64-apple-ios-sim
export IPHONEOS_DEPLOYMENT_TARGET=17.0
target_dir="${CARGO_TARGET_DIR:-$here/rust/target}"
out="$target_dir/$target/$profile/Stahl.app"

(cd "$here/rust" && cargo build --target "$target" "${cargo_profile[@]}")

rm -rf "$out"
mkdir -p "$out"
cp "$here/Shell/Info.plist" "$out/Info.plist"
# The door the app switcher's cover shows: the same as the app's mark.
cp "$here/../assets/door.png" "$out/door.png"
# The icon and the launch screen's colour, compiled as Xcode would, with the icon's keys merged in.
partial="$(mktemp -d)/assets.plist"
xcrun actool "$here/Shell/Assets.xcassets" --compile "$out" --platform iphonesimulator \
  --minimum-deployment-target "$IPHONEOS_DEPLOYMENT_TARGET" --target-device iphone --target-device ipad \
  --app-icon AppIcon --output-partial-info-plist "$partial" >/dev/null
/usr/libexec/PlistBuddy -c "Merge $partial" "$out/Info.plist" >/dev/null
xcrun -sdk iphonesimulator swiftc \
  -target "arm64-apple-ios$IPHONEOS_DEPLOYMENT_TARGET-simulator" \
  -module-name Stahl -parse-as-library \
  -import-objc-header "$here/Shell/stahl.h" \
  "$here"/Shell/*.swift \
  -L "$target_dir/$target/$profile" -lstahl_ios \
  -framework Security -framework QuartzCore -framework CoreGraphics -framework Metal \
  -framework Foundation -framework CoreFoundation -framework UIKit -liconv -lobjc \
  -o "$out/Stahl"
codesign --force --sign - "$out"
echo "built $out"

if [ "$run" = 1 ]; then
  xcrun simctl boot "$device" 2>/dev/null || true
  # Xcode 27 shows simulators in DeviceHub; older ones in Simulator.
  xcode="$(xcode-select -p)/.."
  open -a "$xcode/Applications/DeviceHub.app" 2>/dev/null ||
    open -a "$xcode/Developer/Applications/Simulator.app" 2>/dev/null || true
  xcrun simctl install "$device" "$out"
  xcrun simctl launch --terminate-running-process "$device" net.zoppelt.stahl
fi
