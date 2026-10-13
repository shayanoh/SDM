#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
# The generated UniFFI bindings (git-ignored) are not ours to lint.
find SDM SDMKit -name '*.swift' -not -path '*/.*' -not -path 'SDMKit/Sources/SDMRust/Generated/*' -print0 \
  | xargs -0 swift-format lint
(cd sdm-core && cargo clippy --workspace --all-targets -- -D warnings)
