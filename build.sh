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
PROJECT="SDM.xcodeproj"
PBXPROJ="$PROJECT/project.pbxproj"

usage() {
  cat <<EOF
usage: $0 [options]

Builds a universal Release build of SDM and installs it into $DEST_DIR.

Options:
  -h              Show this help and exit.
  -v <version>    Set MARKETING_VERSION in $PBXPROJ to the given
                  literal string (e.g. "-v 2.2.1") before building.
                  Replaces every app-target MARKETING_VERSION line
                  (usually two).
  --no-bump       Skip the build-number bump (agvtool next-version).

Without --no-bump the build number is bumped. With -v, the marketing
version is changed first and the build number is still bumped unless
--no-bump is also given.
EOF
}

NO_BUMP=false
NEW_MARKETING_VERSION=""
MV_UPDATED=false
MV_FROM=""

# Echoes the app target's marketing version (the non-1.0 value shared by the
# app build configs). Empty if it can't be determined.
read_marketing_version() {
  [[ -f "$PBXPROJ" ]] || return 0
  grep -E '^[[:space:]]*MARKETING_VERSION = ' "$PBXPROJ" \
    | sed -E 's/.*MARKETING_VERSION = (.*);/\1/' \
    | sort -u \
    | grep -vx '1.0' \
    | head -1 || true
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    -v)
      if [[ $# -lt 2 || -z "${2:-}" ]]; then
        echo "error: -v requires a version string" >&2
        exit 1
      fi
      NEW_MARKETING_VERSION="$2"
      shift 2
      ;;
    --no-bump)
      NO_BUMP=true
      shift
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -n "$NEW_MARKETING_VERSION" ]]; then
  if [[ ! -f "$PBXPROJ" ]]; then
    echo "error: $PBXPROJ not found" >&2
    exit 1
  fi

  CURRENT_MV="$(read_marketing_version)"

  if [[ -z "$CURRENT_MV" ]]; then
    echo "error: could not determine current MARKETING_VERSION in $PBXPROJ" >&2
    exit 1
  fi

  if [[ "$CURRENT_MV" == "$NEW_MARKETING_VERSION" ]]; then
    echo "MARKETING_VERSION already $NEW_MARKETING_VERSION; nothing to change."
  else
    COUNT="$(grep -c "MARKETING_VERSION = $CURRENT_MV;" "$PBXPROJ" || true)"
    sed -i '' "s/MARKETING_VERSION = ${CURRENT_MV};/MARKETING_VERSION = ${NEW_MARKETING_VERSION};/g" "$PBXPROJ"
    MV_UPDATED=true
    MV_FROM="$CURRENT_MV"
    echo "Set MARKETING_VERSION: $CURRENT_MV -> $NEW_MARKETING_VERSION ($COUNT line(s))."
  fi
fi

if [[ "$NO_BUMP" == false ]]; then
  agvtool next-version
else
  echo "Skipping version bump."
fi

#rm -rf build

xcodebuild \
  -project "$PROJECT" \
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

#rm -rf build

FINAL_MV="$(read_marketing_version)"
if [[ "$MV_UPDATED" == true ]]; then
  echo "Marketing version: ${FINAL_MV:-unknown} (updated from $MV_FROM)"
else
  echo "Marketing version: ${FINAL_MV:-unknown} (unchanged)"
fi

echo "Done."
