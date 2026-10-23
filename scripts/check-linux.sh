#!/bin/bash
# Type-checks the portable core crates (and, with --shell, the Tauri shell)
# for Linux inside a container. Skips with a message if Docker is not running.
set -euo pipefail
cd "$(dirname "$0")/.."
if ! docker info >/dev/null 2>&1; then
  echo "check-linux: Docker is not running; skipping the Linux check." >&2
  exit 0
fi
IMAGE=sdm-linux-build
docker build -q -t "$IMAGE" -f sdm-tauri/docker/linux-build.Dockerfile sdm-tauri/docker >/dev/null
PACKAGES="-p sdm-core -p sdm-resolve -p sdm-engine -p sdm-grabber"
if [[ "${1:-}" == "--shell" ]]; then PACKAGES="$PACKAGES -p sdm-app -p sdm-tauri"; fi
docker run --rm \
  -v "$PWD":/work \
  -v sdm-linux-target:/work/sdm-core/target-linux \
  -e CARGO_TARGET_DIR=/work/sdm-core/target-linux \
  "$IMAGE" bash -c "cd sdm-core && cargo check $PACKAGES"
