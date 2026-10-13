#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
(cd sdm-core && cargo test --workspace)
./scripts/build-rust-core.sh debug
(cd SDMKit && swift test)
