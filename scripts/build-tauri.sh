#!/bin/bash
# Builds the Tauri app: ./scripts/build-tauri.sh mac | windows | linux
set -euo pipefail
cd "$(dirname "$0")/../sdm-tauri"
export PATH="$HOME/.cargo/bin:$(brew --prefix llvm 2>/dev/null)/bin:$PATH"
pnpm --dir web install --frozen-lockfile
case "${1:-}" in
  mac)
    # Universal (arm64 + x86_64); needs both Apple Rust targets installed.
    pnpm tauri build --target universal-apple-darwin
    echo "App: ../sdm-core/target/universal-apple-darwin/release/bundle/macos/SDM.app"
    ;;
  windows)
    pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
    echo "Installer: $(ls ../sdm-core/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe)"
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
    ls -1 ../sdm-core/target/linux/release/bundle/{deb/*.deb,rpm/*.rpm,appimage/*.AppImage}
    ;;
  *)
    echo "usage: $0 mac|windows|linux" >&2
    exit 2
    ;;
esac
