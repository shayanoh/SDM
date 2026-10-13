#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
swift-format format -ri SDM SDMKit
prettier -w SDMChromeExtension
(cd sdm-core && cargo fmt --all)
