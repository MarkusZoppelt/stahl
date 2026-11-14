#!/usr/bin/env bash
# Builds the iPhone and iPad app for the App Store, signed with the Apple Distribution certificate
# and the "Stahl App Store" profile, and with --upload sends it to TestFlight:
#
#   ios/testflight.sh [--upload] <team id> <build number>
#
# .github/workflows/testflight.yml runs it on every push to main, with the certificate and profile
# in a throwaway keychain. On a Mac where they are installed it runs as is; without --upload it
# stops at a signed .ipa in $EXPORT_PATH, which is how to check signing without sending a build.
# Uploading needs an App Store Connect API key: ASC_KEY_ID, ASC_ISSUER_ID and ASC_KEY_PATH (the .p8).
# Run it outside the dev shell, like install-device.sh: Xcode can't have Nix's compiler settings.
set -euo pipefail

upload=false
if [ "${1:-}" = --upload ]; then upload=true; shift; fi
team="${1:?usage: testflight.sh [--upload] <team id> <build number>}"
build="${2:?usage: testflight.sh [--upload] <team id> <build number>}"

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
bundle="${STAHL_BUNDLE_ID:-net.zoppelt.stahl}"
profile="${STAHL_PROFILE:-Stahl App Store}"
work="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/stahl-testflight"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$work/target}"
target=aarch64-apple-ios
lib="$CARGO_TARGET_DIR/$target/release"
EXPORT_PATH="${EXPORT_PATH:-$work/export}"
# The App Store version is the app's own; the build number only has to grow.
version=$(grep -m1 '^version' "$root/Cargo.toml" | cut -d'"' -f2)
rm -rf "$work/Stahl.xcarchive" "$EXPORT_PATH"
mkdir -p "$work"

echo "Stahl $version ($build) for $bundle, team $team"

# A profile file (STAHL_PROFILE_FILE) is installed where Xcode looks, under its UUID.
if [ -n "${STAHL_PROFILE_FILE:-}" ]; then
  uuid=$(security cms -D -i "$STAHL_PROFILE_FILE" | plutil -extract UUID raw -o - -)
  for dir in "$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles" \
    "$HOME/Library/MobileDevice/Provisioning Profiles"; do
    mkdir -p "$dir"
    cp "$STAHL_PROFILE_FILE" "$dir/$uuid.mobileprovision"
  done
fi

if command -v nix >/dev/null && [ -z "${CI:-}" ]; then
  (cd "$root" && IPHONEOS_DEPLOYMENT_TARGET=17.0 nix develop .#ios -c \
    sh -c "cd ios/rust && cargo build --release --target $target")
else
  (cd "$here/rust" && IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build --release --target "$target")
fi

if command -v xcodegen >/dev/null; then
  (cd "$here" && xcodegen generate --spec project.yml --quiet)
else
  (cd "$here" && nix shell nixpkgs#xcodegen -c xcodegen generate --spec project.yml --quiet)
fi

# The Info.plist is written by hand (GENERATE_INFOPLIST_FILE is off), so the version goes in it,
# in a copy, never in the checkout.
plist="$work/Info.plist"
cp "$here/Shell/Info.plist" "$plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build" "$plist"

xcodebuild -project "$here/Stahl.xcodeproj" -scheme Stahl -configuration Release \
  -destination generic/platform=iOS -archivePath "$work/Stahl.xcarchive" archive \
  DEVELOPMENT_TEAM="$team" STAHL_BUNDLE_ID="$bundle" STAHL_RUST_LIB="$lib" \
  INFOPLIST_FILE="$plist" CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY="Apple Distribution" \
  PROVISIONING_PROFILE_SPECIFIER="$profile" | tail -n 5

cat >"$work/ExportOptions.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>$($upload && echo upload || echo export)</string>
  <key>teamID</key><string>$team</string>
  <key>signingStyle</key><string>manual</string>
  <key>signingCertificate</key><string>Apple Distribution</string>
  <key>provisioningProfiles</key>
  <dict><key>$bundle</key><string>$profile</string></dict>
  <key>uploadSymbols</key><true/>
  <key>manageAppVersionAndBuildNumber</key><false/>
</dict>
</plist>
EOF

auth=()
if $upload; then
  : "${ASC_KEY_ID:?ASC_KEY_ID is needed to upload}" "${ASC_ISSUER_ID:?}" "${ASC_KEY_PATH:?}"
  auth=(-authenticationKeyPath "$ASC_KEY_PATH" -authenticationKeyID "$ASC_KEY_ID"
    -authenticationKeyIssuerID "$ASC_ISSUER_ID")
fi
xcodebuild -exportArchive -archivePath "$work/Stahl.xcarchive" -exportPath "$EXPORT_PATH" \
  -exportOptionsPlist "$work/ExportOptions.plist" "${auth[@]}" | tail -n 5

if $upload; then
  echo "uploaded Stahl $version ($build) to App Store Connect; TestFlight lists it once processed"
else
  ls -l "$EXPORT_PATH"
fi
