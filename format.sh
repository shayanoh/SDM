#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
# The generated UniFFI bindings (git-ignored) are rewritten by the build, not formatted.
find SDM SDMKit -name '*.swift' -not -path '*/.*' -not -path 'SDMKit/Sources/SDMRust/Generated/*' -print0 \
  | xargs -0 swift-format format -i
prettier -w SDMChromeExtension
(cd sdm-core && cargo fmt --all)
