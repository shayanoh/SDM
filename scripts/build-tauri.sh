#!/bin/bash
# Builds the Tauri app and copies the result to Releases/<version>/<os>/:
#
#   ./scripts/build-tauri.sh mac | windows | linux
#   ./scripts/build-tauri.sh mac | windows | linux --publish-only   (copy an existing build, no build)
#
# The version comes from scripts/version.py (the build stops if the crates and
# tauri.conf.json disagree). Releases/ is not in git. If Releases/<version>/<os>
# already exists it is left alone: a warning is shown and nothing is copied.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/sdm-tauri"
export PATH="$HOME/.cargo/bin:$(brew --prefix llvm 2>/dev/null)/bin:$PATH"

os="${1:-}"
mode="${2:-}"
if [ -n "$mode" ] && [ "$mode" != "--publish-only" ]; then
  echo "usage: $0 mac|windows|linux [--publish-only]" >&2
  exit 2
fi

VERSION="$("$root/scripts/version.py" --short)"
bundle="$root/sdm-core/target"

# publish <os> <path>...: copies each path into Releases/<version>/<os>/, unless
# that folder already exists. A path that does not exist is an error (a missing
# or stale build must not look like a release).
publish() {
  local target="$1"; shift
  local dest="$root/Releases/$VERSION/$target"
  local item
  for item in "$@"; do
    if [ ! -e "$item" ]; then
      echo "error: expected build output not found: $item" >&2
      exit 1
    fi
  done
  if [ -e "$dest" ]; then
    echo "WARNING: $dest already exists, so nothing was copied. Remove it to publish again." >&2
    return 0
  fi
  mkdir -p "$dest"
  for item in "$@"; do
    cp -R "$item" "$dest/"
  done
  echo "Copied to $dest:"
  ls -1 "$dest" | sed 's/^/  /'
}

# The one match of a glob, or an error if there is none or several.
only() {
  local matches=( $1 )
  if [ ! -e "${matches[0]}" ] || [ "${#matches[@]}" -ne 1 ]; then
    echo "error: expected exactly one build output for: $1" >&2
    exit 1
  fi
  echo "${matches[0]}"
}

publish_mac() {
  local app="$bundle/universal-apple-darwin/release/bundle/macos/SDM.app"
  local built
  built="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
  if [ "$built" != "$VERSION" ]; then
    echo "error: SDM.app is version $built, not $VERSION: build it again" >&2
    exit 1
  fi
  local dmg
  dmg="$(only "$bundle/universal-apple-darwin/release/bundle/dmg/*_${VERSION}_*.dmg")"
  publish mac "$app" "$dmg"
}

publish_windows() {
  local release="$bundle/x86_64-pc-windows-msvc/release"
  # The extension ships loose too, for loading unpacked by hand (no tests, no hidden files).
  local extension
  extension="$(mktemp -d)/ChromeExtension"
  rsync -a --exclude='.*' --exclude='*.test.js' "$root/SDMChromeExtension/" "$extension/"
  local installer
  installer="$(only "$release/bundle/nsis/*_${VERSION}_*-setup.exe")"
  publish windows "$release/sdm-tauri.exe" "$installer" "$extension"
  rm -rf "$(dirname "$extension")"
}

publish_linux() {
  local release="$bundle/linux/release/bundle"
  local deb rpm appimage
  deb="$(only "$release/deb/*_${VERSION}_*.deb")"
  rpm="$(only "$release/rpm/*-${VERSION}-*.rpm")"
  appimage="$(only "$release/appimage/*_${VERSION}_*.AppImage")"
  publish linux "$deb" "$rpm" "$appimage"
}

if [ "$mode" = "--publish-only" ]; then
  case "$os" in
    mac) publish_mac ;;
    windows) publish_windows ;;
    linux) publish_linux ;;
    *) echo "usage: $0 mac|windows|linux [--publish-only]" >&2; exit 2 ;;
  esac
  exit 0
fi

case "$os" in
  mac|windows|linux) pnpm --dir web install --frozen-lockfile ;;
esac
case "$os" in
  mac)
    # Universal (arm64 + x86_64); needs both Apple Rust targets installed.
    pnpm tauri build --target universal-apple-darwin
    publish_mac
    ;;
  windows)
    pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
    publish_windows
    ;;
  linux)
    docker build -q -t sdm-linux-build -f docker/linux-build.Dockerfile docker >/dev/null
    # The repo is mounted, so keep the container from touching the Mac's build
    # state: Linux node_modules go in throwaway volumes (the Mac's hold macOS
    # binaries) and Cargo builds into target/linux instead of target/release.
    # APPIMAGE_EXTRACT_AND_RUN lets the AppImage tools run without FUSE, which
    # a container does not have.
    # The cargo registry and pnpm store are named volumes so packages download
    # once and nothing is written into the repo.
    docker run --rm \
      -v "$PWD/..":/work -w /work/sdm-tauri \
      -v /work/sdm-tauri/node_modules \
      -v /work/sdm-tauri/web/node_modules \
      -v sdm-linux-cargo-registry:/root/.cargo/registry \
      -v sdm-linux-pnpm-store:/pnpm-store \
      -e pnpm_config_store_dir=/pnpm-store \
      -e APPIMAGE_EXTRACT_AND_RUN=1 \
      -e CARGO_TARGET_DIR=/work/sdm-core/target/linux \
      sdm-linux-build bash -c \
      "pnpm install --frozen-lockfile && pnpm tauri build --bundles deb,rpm,appimage"
    # .deb for Debian/Ubuntu, .rpm for Fedora/openSUSE, .AppImage for any distro.
    publish_linux
    ;;
  *)
    echo "usage: $0 mac|windows|linux" >&2
    exit 2
    ;;
esac
