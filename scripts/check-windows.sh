#!/bin/bash
# Type-checks the portable core crates for 64-bit Windows from macOS.
# Needs: rustup target x86_64-pc-windows-msvc, cargo-xwin, Homebrew llvm.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$(brew --prefix llvm 2>/dev/null)/bin:$PATH"
cd sdm-core
cargo xwin check --target x86_64-pc-windows-msvc \
  -p sdm-core -p sdm-resolve -p sdm-engine -p sdm-grabber "$@"
