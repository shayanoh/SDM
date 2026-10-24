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

## Spec 2: the main window (2026-10-24)

Same machine, same release build and method, with the full main window (sidebar, windowed package
list, toolbar, details panel, context menus) on screen. `--simulate 500 50` against the in-process
`SlowOrigin`. **The machine was busy** (a virtual machine pinned at about 99 % of a core, Time
Machine and a backup daemon; load average 6.5 falling to 3.4 over the series), so these numbers are
noisier and probably higher than a quiet run.

| Load                       | CPU, % of one core (median of 60 s, one row per run) | Peak RSS (MB) |
| -------------------------- | ---------------------------------------------------- | ------------- |
| idle (500 paused)          | 0.2 (max 0.4)                                        | 227           |
| 50 active, first build     | 23.0 (max 34.3)                                      | 255           |
| 50 active, no animations   | 15.8 (max 28.7)                                      | 234           |
| 50 active, final: run 1    | 12.9 (max 43.5)                                      | 228           |
| 50 active, final: run 2    | 15.8 (max 25.2)                                      | 241           |
| 50 active, final: run 3    | 14.5 (max 27.0)                                      | 232           |

- Idle: **PASS** (0.2 % < 1 %).
- 50 active: **borderline PASS**. The median of the three final runs is 14.5 % (< 15 %), but one run
  was 15.8 %, and the maximum single sample reaches 43.5 %. Memory is 241 MB at most (< 400 MB).
  The spec 1 skeleton measured 10.3 % on a quieter machine; a quiet re-run is still owed.
- What it took: the infinite "breathing" CSS animations on running rows and on the sidebar icon cost
  about 7 points of CPU in WKWebView, so they were **dropped** (the SwiftUI app has them). The canvases
  now resize their backing store only when the size changes, theme colors are read once instead of
  with `getComputedStyle` on every redraw, and the list renders 3 rows beyond each edge instead of 6.
  Profiled in Chromium with the mock at the same load (`?simulate=500:50`): the UI's JavaScript is
  under 3 % of a core, so what remains is WebKit style, layout and paint.
- Windows rows (WebView2) are still pending.

## Link grabber (spec 3, 2026-10-24)

The grabber's view travels whole on the patch stream, and only when it changed (an idle grabber sends
nothing: `an_idle_grabber_sends_no_grabber_field`). The cost of one view for **2,000 links**, measured by
`cargo test --release -p sdm-app --test grabber_tests view_cost -- --ignored --nocapture` (build the
view, convert to JSON and encode it, on the Intel Mac):

| Build   | View size | Per view |
| ------- | --------- | -------- |
| release | 538 KB    | 8 ms     |
| debug   | 538 KB    | 67 ms    |

At the heartbeat rate (5 Hz) while the batch is still being checked that is about 40 ms of work a
second, then nothing once every row has a verdict. A typical batch (tens of links) is 20 to 30 KB.
The list is windowed, so rendering costs the same for 20 rows as for 2,000. Not measured: the
WebView-side parse of a 538 KB patch, and a real 2,000-link probe over the network. If either is
visible, the fallback in the spec's risk table is a row-level diff on the same `grabber` field.
