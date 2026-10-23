#!/bin/bash
# Builds the Tauri app: ./scripts/build-tauri.sh mac | windows | linux
set -euo pipefail
cd "$(dirname "$0")/../sdm-tauri"
export PATH="$HOME/.cargo/bin:$(brew --prefix llvm 2>/dev/null)/bin:$PATH"
pnpm --dir web install --frozen-lockfile
case "${1:-}" in
  mac)
    pnpm tauri build
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
    # The cargo registry is a named volume so crates download once.
    docker run --rm \
      -v "$PWD/..":/work -w /work/sdm-tauri \
      -v /work/sdm-tauri/node_modules \
      -v /work/sdm-tauri/web/node_modules \
      -v sdm-linux-cargo-registry:/root/.cargo/registry \
      -e CARGO_TARGET_DIR=/work/sdm-core/target/linux \
      sdm-linux-build bash -c \
      "pnpm install --frozen-lockfile && pnpm tauri build --bundles deb"
    echo "Package: $(ls ../sdm-core/target/linux/release/bundle/deb/*.deb)"
    ;;
  *)
    echo "usage: $0 mac|windows|linux" >&2
    exit 2
    ;;
esac
