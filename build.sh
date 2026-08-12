#!/bin/bash
# Builds a universal (arm64 + x86_64) Release build of SDM and installs it
# into /Applications, replacing any existing copy.
set -euo pipefail

cd "$(dirname "$0")"

DERIVED_DATA_PATH="./build"
SCHEME="SDM"
CONFIGURATION="Release"
APP_NAME="SDM.app"
DEST_DIR="/Applications"

NO_BUMP=false

if [[ "${1:-}" == "--no-bump" ]]; then
  NO_BUMP=true
elif [[ $# -gt 0 ]]; then
  echo "error: unknown argument: $1" >&2
  echo "usage: $0 [--no-bump]" >&2
  exit 1
fi

if [[ "$NO_BUMP" == false ]]; then
  agvtool next-version
else
  echo "Skipping version bump."
fi

rm -rf build

xcodebuild \
  -project SDM.xcodeproj \
  -scheme "$SCHEME" \
  -configuration "$CONFIGURATION" \
  -destination "generic/platform=macOS" \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  build

BUILT_APP="$DERIVED_DATA_PATH/Build/Products/$CONFIGURATION/$APP_NAME"

if [ ! -d "$BUILT_APP" ]; then
  echo "error: build succeeded but $BUILT_APP not found" >&2
  exit 1
fi

echo "Build succeeded. Architectures: $(lipo -info "$BUILT_APP/Contents/MacOS/SDM" | sed 's/^.*: //')"

if [ -d "$DEST_DIR/$APP_NAME" ]; then
  echo "Removing existing $DEST_DIR/$APP_NAME"
  rm -rf "$DEST_DIR/$APP_NAME"
fi

echo "Installing to $DEST_DIR/$APP_NAME"
cp -R "$BUILT_APP" "$DEST_DIR/$APP_NAME"

rm -rf build

echo "Done."
