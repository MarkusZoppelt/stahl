#!/usr/bin/env bash
# Builds Stahl for a connected iPhone or iPad, signs it with your team, installs and launches it.
# Needs Xcode signed in to that team, and the device trusted and in Developer Mode:
#
#   ios/install-device.sh <device name or UDID> <team id> [bundle id]
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
device="${1:?usage: install-device.sh <device> <team id> [bundle id]}"
team="${2:?usage: install-device.sh <device> <team id> [bundle id]}"
# A bundle id belongs to one team, so another team needs its own.
export STAHL_BUNDLE_ID="${3:-net.zoppelt.stahl}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/stahl-ios-target}"
target=aarch64-apple-ios
export STAHL_RUST_LIB="$CARGO_TARGET_DIR/$target/release"
derived="${DERIVED_DATA:-/tmp/stahl-ios-derived}"
# Xcode finds a device by its UDID or by its name.
if [[ "$device" =~ ^[0-9A-Fa-f-]{25,}$ ]]; then by=id; else by=name; fi

# Rust in the flake's shell; Xcode outside it, where Nix's compiler settings can't reach it.
(cd "$here/.." && IPHONEOS_DEPLOYMENT_TARGET=17.0 nix develop .#ios -c \
  sh -c "cd ios/rust && cargo build --release --target $target")
(cd "$here" && nix shell nixpkgs#xcodegen -c xcodegen generate --spec project.yml --quiet)

xcodebuild -project "$here/Stahl.xcodeproj" -scheme Stahl -configuration Release \
  -destination "platform=iOS,$by=$device" -derivedDataPath "$derived" \
  -allowProvisioningUpdates -allowProvisioningDeviceRegistration build \
  DEVELOPMENT_TEAM="$team" STAHL_BUNDLE_ID="$STAHL_BUNDLE_ID" STAHL_RUST_LIB="$STAHL_RUST_LIB"

app="$derived/Build/Products/Release-iphoneos/Stahl.app"
xcrun devicectl device install app --device "$device" "$app"
xcrun devicectl device process launch --device "$device" "$STAHL_BUNDLE_ID"
