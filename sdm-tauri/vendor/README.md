# Vendored binaries

`ffmpeg-arm64.lzfse`, `ffmpeg-x86_64.lzfse`, `qjs.lzfse` are LZFSE-compressed
binaries copied into `SDM.app/Contents/Resources/vendor/` at build time
(`sdm-tauri/src-tauri/tauri.macos.conf.json`) and inflated into
`~/Library/Application Support/SDM/bin/` on first launch by
`sdm-core/crates/sdm-resolve/src/managed_binaries.rs`. Windows and Linux ship
no blobs; they download the tools instead.

The `.lzfse` blobs are tracked with **Git LFS** (see `.gitattributes`).

Regenerate with `./scripts/vendor-binaries.sh` (edit the pinned versions at
the top of the script first), then commit the new blobs and
`vendor-manifest.json`.

`vendor-manifest.json` with versions `"0"` is the checked-in placeholder for
a fresh clone that has not run the script yet — the app treats missing blobs
as "nothing to inflate" and carries on (yt-dlp still self-downloads).
