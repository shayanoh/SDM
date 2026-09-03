#!/bin/bash
#
# Regenerates the vendored ffmpeg / QuickJS-ng binaries that ship inside
# SDM.app and are inflated into ~/Library/Application Support/SDM/bin/ on
# first launch. Run manually when refreshing a pinned version, then commit
# the new *.lzfse blobs (Git LFS) and vendor-manifest.json.
#
#   ./scripts/vendor-binaries.sh
#
# yt-dlp is NOT handled here — it is downloaded and self-updated at runtime
# by SDMResolve/ManagedBinaries.swift.
#
set -euo pipefail

# ---- pinned versions ------------------------------------------------------
# ffmpeg: build-id path segments from https://ffmpeg.martin-riedl.de/
#         (native per-arch, notarized). Leave empty to auto-pick the current
#         build advertised on the site's homepage.
FFMPEG_BUILD_ARM64="${FFMPEG_BUILD_ARM64:-}"
FFMPEG_BUILD_AMD64="${FFMPEG_BUILD_AMD64:-}"
QJS_TAG="${QJS_TAG:-v0.10.1}"   # github.com/quickjs-ng/quickjs
# ------------------------------------------------------------------------

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/SDM/Resources/vendor"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$OUT"

echo "==> building lzfse-pack"
swiftc -O "$ROOT/scripts/lzfse-pack/main.swift" -o "$TMP/lzfse-pack"

MR="https://ffmpeg.martin-riedl.de"

discover_build() {  # $1 = amd64|arm64
  curl -fsSL "$MR/" \
    | grep -oE "/download/macos/$1/[^/\"]+/ffmpeg\.zip" \
    | head -1 | sed -E "s#/download/macos/$1/([^/]+)/ffmpeg\.zip#\1#"
}

fetch_ffmpeg() {  # $1 = arch(arm64|x86_64)  $2 = site arch(amd64|arm64)  $3 = outfile  $4 = pinned build
  local arch="$1" sitearch="$2" out="$3" build="$4"
  [ -n "$build" ] || build="$(discover_build "$sitearch")"
  [ -n "$build" ] || { echo "!! could not discover ffmpeg build for $sitearch" >&2; exit 1; }
  echo "==> ffmpeg $arch  ($build)"
  local url="$MR/download/macos/$sitearch/$build/ffmpeg.zip"
  curl -fsSL "$url" -o "$TMP/ff-$arch.zip"
  local want got
  want="$(curl -fsSL "$url.sha256" | awk '{print $1}')"
  got="$(shasum -a256 "$TMP/ff-$arch.zip" | awk '{print $1}')"
  [ "$want" = "$got" ] || { echo "!! sha256 mismatch for $url" >&2; exit 1; }
  ( cd "$TMP" && unzip -o "ff-$arch.zip" ffmpeg -d "ff-$arch" >/dev/null )
  chmod +x "$TMP/ff-$arch/ffmpeg"
  "$TMP/lzfse-pack" compress "$TMP/ff-$arch/ffmpeg" "$OUT/$out"
  echo "$build" > "$TMP/ffbuild-$arch"

  # ffprobe from the same build — yt-dlp's HLS/DASH fixup needs it.
  local probeurl="$MR/download/macos/$sitearch/$build/ffprobe.zip"
  curl -fsSL "$probeurl" -o "$TMP/fp-$arch.zip"
  want="$(curl -fsSL "$probeurl.sha256" | awk '{print $1}')"
  got="$(shasum -a256 "$TMP/fp-$arch.zip" | awk '{print $1}')"
  [ "$want" = "$got" ] || { echo "!! sha256 mismatch for $probeurl" >&2; exit 1; }
  ( cd "$TMP" && unzip -o "fp-$arch.zip" ffprobe -d "fp-$arch" >/dev/null )
  chmod +x "$TMP/fp-$arch/ffprobe"
  "$TMP/lzfse-pack" compress "$TMP/fp-$arch/ffprobe" "$OUT/ffprobe-$arch.lzfse"
}

fetch_ffmpeg arm64  arm64 "ffmpeg-arm64.lzfse"  "$FFMPEG_BUILD_ARM64"
fetch_ffmpeg x86_64 amd64 "ffmpeg-x86_64.lzfse" "$FFMPEG_BUILD_AMD64"

echo "==> QuickJS-ng $QJS_TAG"
git clone --depth 1 --branch "$QJS_TAG" https://github.com/quickjs-ng/quickjs "$TMP/qjs" >/dev/null 2>&1
for a in arm64 x86_64; do
  cmake -S "$TMP/qjs" -B "$TMP/qjs/b-$a" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_ARCHITECTURES="$a" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=15.0 >/dev/null
  cmake --build "$TMP/qjs/b-$a" --target qjs_exe -j >/dev/null
done
lipo -create "$TMP/qjs/b-arm64/qjs" "$TMP/qjs/b-x86_64/qjs" -output "$TMP/qjs-universal"
chmod +x "$TMP/qjs-universal"
"$TMP/lzfse-pack" compress "$TMP/qjs-universal" "$OUT/qjs.lzfse"

FF_VER="$(cat "$TMP/ffbuild-arm64" 2>/dev/null || echo unknown)"
cat > "$OUT/vendor-manifest.json" <<JSON
{
  "ffmpegVersion": "$FF_VER",
  "qjsVersion": "${QJS_TAG#v}"
}
JSON

cp "$TMP/qjs/LICENSE" "$OUT/qjs-LICENSE.txt" 2>/dev/null || true
cat > "$OUT/ffmpeg-COPYING.txt" <<TXT
The bundled ffmpeg binaries are the native macOS static builds from
https://ffmpeg.martin-riedl.de/ (build $FF_VER). FFmpeg is free software
licensed under the LGPL v2.1+ / GPL v2+; see https://ffmpeg.org/legal.html.
Corresponding source: https://git.ffmpeg.org/ffmpeg.git and the build
recipe at https://github.com/mZ4h/FFmpeg-Builds-macOS
SDM invokes ffmpeg only as a separate process (mere aggregation).
TXT

echo
echo "==> done. Next:"
echo "    git add SDM/Resources/vendor      # *.lzfse are Git LFS"
echo "    git commit -m 'chore: refresh vendored ffmpeg/qjs'"
echo "    xcodebuild -project SDM.xcodeproj -scheme SDM build"
