#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
(cd sdm-core && cargo fmt --all)
prettier -w SDMChromeExtension sdm-tauri/web/src
