#!/bin/sh
# Installs the Foyer CLI (`foyer`) on Linux and macOS.
#
#   curl -fsSL https://foyer.lol/install.sh | sh
#   curl -fsSL https://foyer.lol/install.sh | sh -s -- --version 0.2.0
#
# Settings (flags win over environment variables):
#   --version X / FOYER_VERSION        CLI version, default the newest release
#   --dir PATH  / FOYER_INSTALL_DIR    where `foyer` goes, default ~/.foyer/bin
#   FOYER_REPO                         GitHub repository, default TorstenDittmann/foyer
#   GITHUB_TOKEN                       optional, avoids GitHub API rate limits in CI
#
# The archive is checked against the release's SHA256SUMS before anything is
# installed. In GitHub Actions the install directory is added to GITHUB_PATH.
# On Windows use install.ps1 instead.

set -eu

repo="${FOYER_REPO:-TorstenDittmann/foyer}"
version="${FOYER_VERSION:-}"
dir="${FOYER_INSTALL_DIR:-$HOME/.foyer/bin}"

say() { printf '%s\n' "$*" >&2; }
fail() { say "error: $*"; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a value"; version="$2"; shift 2 ;;
    --dir) [ $# -ge 2 ] || fail "--dir needs a value"; dir="$2"; shift 2 ;;
    -h|--help) say "usage: install.sh [--version X] [--dir PATH]"; exit 0 ;;
    *) fail "unknown argument $1" ;;
  esac
done

# fetch URL OUTPUT ("-" for stdout). Sends GITHUB_TOKEN only to the GitHub API.
fetch() {
  auth=""
  case "$1" in https://api.github.com/*) [ -z "${GITHUB_TOKEN:-}" ] || auth="Authorization: Bearer $GITHUB_TOKEN" ;; esac
  if command -v curl >/dev/null 2>&1; then
    if [ -n "$auth" ]; then
      curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -H "$auth" -o "$2" "$1"
    else
      curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [ -n "$auth" ]; then
      wget -q --header "$auth" -O "$2" "$1"
    else
      wget -q -O "$2" "$1"
    fi
  else
    fail "curl or wget is needed"
  fi
}

case "$(uname -s)" in
  Linux) os=unknown-linux-musl ;;
  Darwin) os=apple-darwin ;;
  MINGW*|MSYS*|CYGWIN*) fail "on Windows run: irm https://foyer.lol/install.ps1 | iex" ;;
  *) fail "unsupported OS $(uname -s); build from source with: cargo install --git https://github.com/$repo foyer-cli" ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) fail "unsupported CPU $(uname -m)" ;;
esac
# Rosetta reports x86_64 on Apple silicon; prefer the native build.
if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] \
  && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
  arch=aarch64
fi
target="$arch-$os"

if [ -z "$version" ]; then
  # Releases come newest first; desktop releases share the repository, so
  # pick the first CLI tag rather than "latest".
  tag=$(fetch "https://api.github.com/repos/$repo/releases?per_page=50" - \
    | grep -o '"tag_name": *"cli-v[^"]*"' | head -n 1 | sed 's/.*"\(cli-v[^"]*\)"/\1/') \
    || true
  [ -n "$tag" ] || fail "could not find a CLI release of $repo"
else
  tag="cli-v${version#v}"
fi

base="https://github.com/$repo/releases/download/$tag"
archive="foyer-$target.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading foyer ${tag#cli-v} for $target"
fetch "$base/$archive" "$tmp/$archive" || fail "no download at $base/$archive"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fail "no SHA256SUMS in release $tag"

expected=$(grep " $archive\$" "$tmp/SHA256SUMS" | cut -d ' ' -f 1)
[ -n "$expected" ] || fail "$archive is not listed in SHA256SUMS"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$archive" | cut -d ' ' -f 1)
else
  actual=$(shasum -a 256 "$tmp/$archive" | cut -d ' ' -f 1)
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive"

tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$dir"
# Move into place in one step so a running `foyer` is never half-written.
mv -f "$tmp/foyer" "$dir/foyer.new"
chmod 755 "$dir/foyer.new"
mv -f "$dir/foyer.new" "$dir/foyer"
say "Installed $("$dir/foyer" --version) to $dir/foyer"

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$dir" >> "$GITHUB_PATH"
fi
case ":$PATH:" in
  *":$dir:"*) ;;
  *) [ -n "${GITHUB_PATH:-}" ] || say "Add it to your PATH: export PATH=\"$dir:\$PATH\"" ;;
esac
