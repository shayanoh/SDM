#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
(cd sdm-core && cargo test --workspace --exclude sdm-tauri)
(cd sdm-core && cargo test -p sdm-tauri)
(cd sdm-tauri/web && pnpm test)
node --test SDMChromeExtension/cookies.test.js
python3 -m unittest scripts/test_version.py scripts/test_bundle_resources.py
./scripts/check-windows.sh
./scripts/check-linux.sh
