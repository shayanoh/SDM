#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
(cd sdm-core && cargo clippy --workspace --all-targets -- -D warnings)
(cd sdm-tauri/web && pnpm check)
