#!/usr/bin/env bash
# Builds Foyer.app (universal: Apple Silicon and Intel), signs and notarizes
# it when the credentials are set, and writes to dist/:
#
#   Foyer-macos.dmg            what players download
#   Foyer-macos.app.tar.gz     what the client downloads to update itself
#
# Usage (from the repository root, on a Mac with Xcode):
#   crates/foyer-desktop/packaging/macos/package.sh
#
# Optional environment:
#   APPLE_CERTIFICATE           base64 of a "Developer ID Application" .p12
#   APPLE_CERTIFICATE_PASSWORD  its password
#   APPLE_SIGNING_IDENTITY      e.g. "Developer ID Application: Name (TEAMID)"
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD
#                               notarize with an app-specific password
#   FOYER_UPDATE_PUBLIC_KEY     compiled in so the client can update itself
#
# Without a certificate the app is signed ad hoc, which runs but makes
# Gatekeeper ask players to approve it in System Settings.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"
cd "$root"

version="$(cargo metadata --no-deps --format-version 1 |
  python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="foyer-desktop"))')"
dist="$root/dist"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$dist"

export MACOSX_DEPLOYMENT_TARGET=11.0
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target add "$target" >/dev/null
  cargo build --release --locked -p foyer-desktop --target "$target"
done

app="$work/Foyer.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/Foyer" \
  target/aarch64-apple-darwin/release/foyer-desktop \
  target/x86_64-apple-darwin/release/foyer-desktop
sed "s/@VERSION@/$version/g" "$here/Info.plist" >"$app/Contents/Info.plist"
cp "$here/../icon.icns" "$app/Contents/Resources/Foyer.icns"

notarize() {
  xcrun notarytool submit "$1" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_PASSWORD" --wait --timeout 30m
}

if [[ -n "${APPLE_CERTIFICATE:-}" ]]; then
  keychain="$work/signing.keychain-db"
  keychain_password="$(uuidgen)"
  security create-keychain -p "$keychain_password" "$keychain"
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "$keychain_password" "$keychain"
  echo "$APPLE_CERTIFICATE" | base64 --decode >"$work/cert.p12"
  security import "$work/cert.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" \
    -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain"
  security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
  codesign --force --options runtime --timestamp --keychain "$keychain" \
    --sign "$APPLE_SIGNING_IDENTITY" "$app"
  codesign --verify --strict --verbose=2 "$app"

  if [[ -n "${APPLE_ID:-}" ]]; then
    ditto -c -k --keepParent "$app" "$work/Foyer.zip"
    notarize "$work/Foyer.zip"
    xcrun stapler staple "$app"
  else
    echo "APPLE_ID not set: signed but not notarized."
  fi
else
  echo "APPLE_CERTIFICATE not set: signing ad hoc."
  codesign --force --sign - "$app"
fi

tar -czf "$dist/Foyer-macos.app.tar.gz" -C "$work" Foyer.app

mkdir "$work/dmg"
cp -R "$app" "$work/dmg/"
ln -s /Applications "$work/dmg/Applications"
rm -f "$dist/Foyer-macos.dmg"
hdiutil create -volname Foyer -srcfolder "$work/dmg" -ov -format UDZO "$dist/Foyer-macos.dmg"

if [[ -n "${APPLE_CERTIFICATE:-}" ]]; then
  codesign --force --timestamp --keychain "$keychain" --sign "$APPLE_SIGNING_IDENTITY" \
    "$dist/Foyer-macos.dmg"
  if [[ -n "${APPLE_ID:-}" ]]; then
    notarize "$dist/Foyer-macos.dmg"
    xcrun stapler staple "$dist/Foyer-macos.dmg"
  fi
  security delete-keychain "$keychain"
fi

ls -la "$dist"
