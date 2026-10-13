# Benchmarks: Swift core and Rust core

- Date: 2026-10-13
- Machine: MacBookPro14,3, Intel(R) Core(TM) i7-7820HQ CPU @ 2.90GHz, 8 logical cores, 16 GB memory
- macOS: 15.7.7 (24G720)
- Commit: 67cc4a1
- Release builds, 5 runs per cell, the two cores alternating run by run; medians below.
- Wall, CPU (user + system) and peak resident memory are measured from outside with `/usr/bin/time -l`.

| Scenario | Core | Wall (s) | CPU user+sys (s) | Peak RSS (MB) | Wall spread | Items | Bytes |
|---|---|---:|---:|---:|---:|---:|---:|
| transfer | Rust | 10.25 | 4.38 | 265 | 39.6% (over 15%) | 8 | 2147483648 |
| transfer | Swift | 11.98 | 12.93 | 563 | 29.5% (over 15%) | 8 | 2147483648 |
| ticks | Rust | 2.51 | 2.42 | 29 | 2.0% | 1000 | 0 |
| ticks | Swift | 2.74 | 2.64 | 13 | 5.1% | 1000 | 0 |
| ffi-snapshot | Rust via bindings | 15.10 | 15.07 | 23 | 4.5% | 1000 | 0 |

Wall spread is (max - min) / median of the wall times over the runs.

## Every run

| Scenario | Core | Run | Wall (s) | CPU user+sys (s) | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|
| transfer | Rust | 1 | 11.96 | 4.42 | 265 |
| transfer | Rust | 2 | 10.25 | 4.20 | 265 |
| transfer | Rust | 3 | 7.90 | 4.38 | 265 |
| transfer | Rust | 4 | 10.73 | 9.05 | 265 |
| transfer | Rust | 5 | 9.36 | 4.37 | 265 |
| transfer | Swift | 1 | 13.71 | 12.93 | 639 |
| transfer | Swift | 2 | 12.56 | 14.42 | 563 |
| transfer | Swift | 3 | 10.71 | 11.24 | 519 |
| transfer | Swift | 4 | 10.17 | 11.77 | 519 |
| transfer | Swift | 5 | 11.98 | 13.98 | 632 |
| ticks | Rust | 1 | 2.51 | 2.42 | 29 |
| ticks | Rust | 2 | 2.50 | 2.42 | 31 |
| ticks | Rust | 3 | 2.53 | 2.43 | 28 |
| ticks | Rust | 4 | 2.48 | 2.40 | 29 |
| ticks | Rust | 5 | 2.52 | 2.44 | 29 |
| ticks | Swift | 1 | 2.77 | 2.65 | 13 |
| ticks | Swift | 2 | 2.74 | 2.64 | 13 |
| ticks | Swift | 3 | 2.67 | 2.61 | 13 |
| ticks | Swift | 4 | 2.81 | 2.66 | 13 |
| ticks | Swift | 5 | 2.71 | 2.60 | 13 |
| ffi-snapshot | Rust via bindings | 1 | 15.60 | 15.54 | 23 |
| ffi-snapshot | Rust via bindings | 2 | 14.92 | 14.90 | 23 |
| ffi-snapshot | Rust via bindings | 3 | 15.15 | 15.12 | 23 |
| ffi-snapshot | Rust via bindings | 4 | 14.99 | 14.98 | 23 |
| ffi-snapshot | Rust via bindings | 5 | 15.10 | 15.07 | 23 |

## What each scenario measures

All three programs are measured from outside (`/usr/bin/time -l`), so the figures include process start-up and the one-off generation of the payload buffer (256 MB for `transfer`, 64 KB for `ticks`), which is the same code and cost on both sides.

- **transfer** (`sdm-bench` / `SDMBench`): 8 items of 256 MB (2 GiB) in one package, 8 segments each, `maxConcurrent = 8`, global and per-host connection limits 64, default checkpoint interval (8 MiB) and minimum segment size (10 MiB), run to completion by looping `tick()`, `snapshot()` (to read item states), yield. Each output file's size is verified, then the directory is deleted. It exercises the hot path: stream, claim, write at offset, checkpoint, finalize. It measures the engine plus the filesystem: real files are written to the system temp volume.
- **ticks** (`sdm-bench` / `SDMBench`): 1,000 items of 64 KB in 20 packages, `maxConcurrent = 8`; the fake origin's body fetches are parked forever so the 8 running items stay running (the probe still passes); 600 iterations of `tick()` + `snapshot()`, reading `packages.count`. It exercises scheduling, telemetry and snapshot building at list scale. No bytes are transferred (`bytes=0`).
- **ffi-snapshot** (`SDMFFIBench`, Rust core only): Swift calling the generated bindings. 1,000 disabled items in 20 packages (so nothing is scheduled, downloaded or trashed), then 600 x (`tick()` + `snapshot()`), summing `items.count` over each snapshot so the Swift values are materialized. It measures the cost of copying the snapshot across the boundary. There is no Swift-core counterpart: the all-Swift core has no boundary. Its figures are not comparable to `ticks` (different item states, and the snapshots here are fully converted into Swift values).

## What they do not measure

No network (both cores read from an in-process fake origin; neither exercises `URLSession` or `reqwest`), no UI, no yt-dlp, muxing or persistence to disk (both use an in-memory state store; `SDMFFIBench` uses a real JSON state file in a temp directory but its items never change state), and nothing about launch time or the app as a whole.

## Making the work identical

- Both programs use their own core's public `FakeOrigin` (serving slices of one buffer built once with the `test_payload` formula `(i*31 + 7) % 251`) with the chunk size set to 64 KiB on both sides, instead of the fake's default of 64 bytes. Both fakes build a response eagerly, chunk by chunk. They are not byte-for-byte the same work: the Swift fake copies every chunk (`Data.subdata`) while the Rust fake slices the shared `Bytes` without copying. That is a property of each core's own fake and is part of what the `transfer` figures include.
- For `ticks`, both programs wrap the core's `FakeOrigin` in the same small transport (written inside each bench program, since the test-only gates are not public) that lets the one-byte probe through and parks every other fetch.
- Both use an in-memory state store and the default retry policy. Rust's release profile has `overflow-checks = true` (the workspace setting); Swift's release build has its default checks.
- `transfer` completion is detected through `snapshot()` once per loop iteration on both sides, so the number of ticks taken depends on how fast each side runs; the loop has no clock.

## Caveats

- **Run-to-run spread is over 15% for `transfer` on both cores.** It was over 15% in two complete runs of the script (the second is the one recorded above; the first gave Rust 11.12 s median / 36% spread and Swift 13.63 s / 42% spread, with CPU 5.03 s and 11.00 s). The transfer writes 2 GiB through the page cache on a laptop with 16 GB of memory, and the wall time and system CPU vary with how the kernel flushes it; the user-mode CPU is steady. The median wall times of the two cores are therefore close to the noise and should not be read as a difference. Peak RSS and the `ticks` figures are stable (spread under 6%).
- The machine is an Intel MacBook Pro (x86_64); the Apple Silicon slice of the Rust core was not run.
- The runs were on AC power; a code-review agent was working in the same repository and no build was seen running when the runs started.
- Both cores moved exactly 2,147,483,648 bytes in `transfer`.

## Release app sizes (previous task, for reference)

| App | Bundle | Main binary (universal) |
|---|---:|---:|
| Rust-core app | 171 MB | 53,818,256 bytes |
| Installed all-Swift app | 133 MB | 14,751,824 bytes |
