# Tauri skeleton performance (spec 1 gate)

Measured 2026-10-23 on an Intel Mac (Core i7-7820HQ, macOS 15.7.7), **release** build
(`cargo build -p sdm-tauri --release --features simulate,tauri/custom-protocol`),
`--simulate 500 <active>` against the in-process `SlowOrigin` (no network, a scratch folder
under the system temp directory). Each run: 20 s warm-up, then 60 one-second samples of the
app process plus the `com.apple.WebKit.*` helper processes that appeared after launch
(four processes in total). CPU is the sum, as a percentage of one core. Script:
the measure loop sampled `ps -o %cpu,rss` once a second and reported the median and the peak.

| Platform / webview                          | Load              | CPU (% of one core, median of 60 s) | Peak RSS (MB) |
| ------------------------------------------- | ----------------- | ----------------------------------- | ------------- |
| macOS / WKWebView                           | idle (500 paused) | 0.3 (max 0.8)                       | 178           |
| macOS / WKWebView                           | 50 active         | 10.3 (max 17.9)                     | 206           |
| Windows x64 VM / WebView2                   | idle              | not measured yet (Task 15)          | —             |
| Windows x64 VM / WebView2                   | 50 active         | not measured yet (Task 15)          | —             |
| SwiftUI app, same engine load (if drivable) | 50 active         | not measured                        | —             |

Thresholds (spec §10): idle < 1 % of a core; 50 active < 15 % of a core and < 400 MB;
≤ 2× the SwiftUI CPU where measured.

## Result

- macOS idle: **PASS** (0.3 % < 1 %).
- macOS 50 active: **PASS** (10.3 % median < 15 %, 206 MB < 400 MB). The maximum single
  sample reached 17.9 %, so the margin on this Intel machine is not large.
- Windows rows: pending (Task 15). SwiftUI comparison: not measured, because the SwiftUI app
  has no equivalent harness for a fake origin, so the "≤ 2× SwiftUI" clause is unchecked.

## Caveats

- Each simulated item moves about 160 KiB/s (four segments of 8 KiB every 200 ms), not the
  1.6 MB/s the plan suggested: at 1.6 MB/s fifty items would write roughly 5 GB a minute to
  disk. Patch size and UI work depend on how many rows change per tick, not on the byte rate,
  so this should not change the UI cost, but disk write cost is lower than a real 50-item
  download.
- The release binary needs the `simulate` feature for `--simulate` (debug builds always have
  it) and `tauri/custom-protocol` so it serves the embedded `web/dist` instead of the dev URL.
- Idle is `--simulate 500 1` plus `SDM_SIMULATE_IDLE=1`, which pauses everything after loading.

If a row fails later, record the profile and the first optimization tried (coarsened ranges,
spec §4.3) before spec 2 starts.
