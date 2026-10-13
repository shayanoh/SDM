#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
swift-format lint -r SDM SDMKit
(cd sdm-core && cargo clippy --workspace --all-targets -- -D warnings)
