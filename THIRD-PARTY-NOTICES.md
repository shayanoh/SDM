# Third-party software

SDM itself is released under the MIT License (see `LICENSE`). It does not ship
the tools below. It downloads them to its own data folder the first time it
needs them, straight from their publishers, and runs them as separate programs.
Each keeps its own license.

| Tool | Used for | License | Source |
|---|---|---|---|
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Reading media-site pages and the HLS/DASH downloads | The Unlicense | https://github.com/yt-dlp/yt-dlp |
| [FFmpeg](https://ffmpeg.org) (`ffmpeg`, `ffprobe`) | Joining a video and an audio stream | GNU GPL v3 (the static builds SDM downloads are GPL builds) | https://ffmpeg.org/download.html#get-sources |
| [QuickJS-ng](https://github.com/quickjs-ng/quickjs) (`qjs`) | The JavaScript runtime yt-dlp needs for some sites | MIT | https://github.com/quickjs-ng/quickjs |

The Rust crates and npm packages SDM is built from carry their own licenses,
all permissive or weak-copyleft, recorded in their own packages.
