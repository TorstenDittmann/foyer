#!/usr/bin/env bash
# Builds the Linux packages into dist/:
#
#   Foyer-x86_64.AppImage   runs on most distributions and updates itself
#   Foyer-amd64.deb         for Debian and Ubuntu; updated by hand or apt
#
# Usage (from the repository root, on Ubuntu 22.04 or older for wide glibc
# compatibility, with the GPUI build libraries installed):
#   crates/foyer-desktop/packaging/linux/package.sh
#
# Needs cargo-deb (`cargo install cargo-deb`). Downloads linuxdeploy.
# Set FOYER_UPDATE_PUBLIC_KEY so the AppImage can update itself.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"
cd "$root"

dist="$root/dist"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$dist"

cargo build --release --locked -p foyer-desktop

# .deb
cargo deb -p foyer-desktop --no-build --output "$dist/Foyer-amd64.deb"

# AppImage
linuxdeploy="$work/linuxdeploy-x86_64.AppImage"
curl -fsSL -o "$linuxdeploy" \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
chmod +x "$linuxdeploy"
cp "$here/../icon-512.png" "$work/lol.foyer.Foyer.png"
(
  cd "$work"
  APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 OUTPUT=Foyer-x86_64.AppImage "$linuxdeploy" \
    --appdir AppDir \
    --executable "$root/target/release/foyer-desktop" \
    --desktop-file "$here/lol.foyer.Foyer.desktop" \
    --icon-file "$work/lol.foyer.Foyer.png" \
    --output appimage
)
mv "$work/Foyer-x86_64.AppImage" "$dist/"
ls -la "$dist"
