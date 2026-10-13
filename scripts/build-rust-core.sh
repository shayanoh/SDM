#!/usr/bin/env bash
# Builds the Rust core (sdm-core/) into SDMKit/Artifacts/SDMRust.xcframework and
# generates the Swift bindings into SDMKit/Sources/SDMRust/Generated/.
#
# Usage: scripts/build-rust-core.sh [debug|release]
#   debug   (default) host triple only, dev profile
#   release x86_64-apple-darwin + aarch64-apple-darwin, lipo'd into one library
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=15.0

usage() {
  echo "usage: $(basename "$0") [debug|release]" >&2
  exit 2
}

[[ $# -le 1 ]] || usage
MODE="${1:-debug}"
case "$MODE" in
  debug | release) ;;
  *) usage ;;
esac

stage() { echo "==> $*"; }
die() {
  echo "error: $*" >&2
  exit 1
}

for tool in cargo rustc xcodebuild lipo; do
  command -v "$tool" > /dev/null 2>&1 || die "'$tool' not found on PATH"
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE="$ROOT/sdm-core"
TARGET_DIR="$WORKSPACE/target"
GENERATED="$ROOT/SDMKit/Sources/SDMRust/Generated"
XCFRAMEWORK="$ROOT/SDMKit/Artifacts/SDMRust.xcframework"
STAGING="$TARGET_DIR/rust-core-staging/$MODE"
STAMP="$TARGET_DIR/build-rust-core.stamp"
LIB_NAME="libsdm_ffi.a"

HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
[[ -n "$HOST_TRIPLE" ]] || die "could not determine the host triple from rustc"

if [[ "$MODE" == "release" ]]; then
  TRIPLES=(x86_64-apple-darwin aarch64-apple-darwin)
  CARGO_PROFILE_ARGS=(--release)
  PROFILE_DIR=release
else
  TRIPLES=("$HOST_TRIPLE")
  CARGO_PROFILE_ARGS=()
  PROFILE_DIR=debug
fi
CONFIG="$MODE ${TRIPLES[*]} deployment=$MACOSX_DEPLOYMENT_TARGET"

# The outputs themselves must still be there: a stamp alone proves nothing
# once someone has deleted generated sources or the XCFramework's contents.
outputs_present() {
  [[ -d "$GENERATED" && -d "$XCFRAMEWORK" && -f "$XCFRAMEWORK/Info.plist" ]] || return 1
  [[ -n "$(find "$GENERATED" -name '*.swift' -type f -print -quit 2> /dev/null)" ]] || return 1
  [[ -n "$(find "$XCFRAMEWORK" -mindepth 2 -name "$LIB_NAME" -print -quit 2> /dev/null)" ]] || return 1
}

# Skip when nothing changed since the last successful build of this configuration.
if [[ -f "$STAMP" && "$(cat "$STAMP")" == "$CONFIG" ]] && outputs_present; then
  NEWER="$(find "$WORKSPACE/crates" "$WORKSPACE/Cargo.toml" "$WORKSPACE/Cargo.lock" \
    "$WORKSPACE/rust-toolchain.toml" "${BASH_SOURCE[0]}" \
    -type f -newer "$STAMP" -print -quit 2> /dev/null || true)"
  if [[ -z "$NEWER" ]]; then
    echo "rust core up to date"
    exit 0
  fi
fi
rm -f "$STAMP"

cd "$WORKSPACE"

stage "cargo build sdm-ffi ($MODE: ${TRIPLES[*]})"
for triple in "${TRIPLES[@]}"; do
  cargo build -p sdm-ffi --target "$triple" ${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"}
done

stage "cargo build uniffi-bindgen-swift (host)"
cargo build -p uniffi-bindgen --bin uniffi-bindgen-swift
BINDGEN="$TARGET_DIR/debug/uniffi-bindgen-swift"
[[ -x "$BINDGEN" ]] || die "uniffi-bindgen-swift was not built at $BINDGEN"

rm -rf "$STAGING"
mkdir -p "$STAGING/lib" "$STAGING/Headers"

stage "assemble static library"
if [[ "$MODE" == "release" ]]; then
  LIPO_INPUTS=()
  for triple in "${TRIPLES[@]}"; do
    LIPO_INPUTS+=("$TARGET_DIR/$triple/$PROFILE_DIR/$LIB_NAME")
  done
  lipo -create "${LIPO_INPUTS[@]}" -output "$STAGING/lib/$LIB_NAME"
else
  cp "$TARGET_DIR/${TRIPLES[0]}/$PROFILE_DIR/$LIB_NAME" "$STAGING/lib/$LIB_NAME"
fi

# Bindings are generated from one architecture's slice (UniFFI reads the
# embedded metadata; the architectures carry identical metadata).
METADATA_LIB="$TARGET_DIR/$HOST_TRIPLE/$PROFILE_DIR/$LIB_NAME"
[[ -f "$METADATA_LIB" ]] || METADATA_LIB="$TARGET_DIR/${TRIPLES[0]}/$PROFILE_DIR/$LIB_NAME"

stage "generate Swift sources, headers and modulemap (module SDMRustFFI)"
rm -rf "$GENERATED"
mkdir -p "$GENERATED"
"$BINDGEN" "$METADATA_LIB" "$GENERATED" --swift-sources
"$BINDGEN" "$METADATA_LIB" "$STAGING/Headers" --headers
"$BINDGEN" "$METADATA_LIB" "$STAGING/Headers" --modulemap \
  --module-name SDMRustFFI --modulemap-filename module.modulemap
# Defensive: drop any duplicated header line (UniFFI emits one per crate header).
awk '!(/^[[:space:]]*header / && seen[$0]++)' "$STAGING/Headers/module.modulemap" \
  > "$STAGING/Headers/module.modulemap.tmp"
mv "$STAGING/Headers/module.modulemap.tmp" "$STAGING/Headers/module.modulemap"

stage "xcodebuild -create-xcframework"
rm -rf "$XCFRAMEWORK"
mkdir -p "$(dirname "$XCFRAMEWORK")"
xcodebuild -create-xcframework \
  -library "$STAGING/lib/$LIB_NAME" \
  -headers "$STAGING/Headers" \
  -output "$XCFRAMEWORK"

echo "$CONFIG" > "$STAMP"
stage "done: $XCFRAMEWORK"
