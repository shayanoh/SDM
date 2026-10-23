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
    docker run --rm -v "$PWD/..":/work -w /work/sdm-tauri sdm-linux-build bash -c \
      "curl -fsSL https://get.pnpm.io/install.sh | sh - >/dev/null && export PATH=/root/.local/share/pnpm:\$PATH && pnpm install --frozen-lockfile && pnpm --dir web install --frozen-lockfile && pnpm tauri build --bundles deb"
    ;;
  *)
    echo "usage: $0 mac|windows|linux" >&2
    exit 2
    ;;
esac
