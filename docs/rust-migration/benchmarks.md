# Benchmarks: Swift core and Rust core

- Date: 2026-10-13
- Machine: MacBookPro14,3, Intel(R) Core(TM) i7-7820HQ CPU @ 2.90GHz, 8 logical cores, 16 GB memory
- macOS: 15.7.7 (24G720)
- Commit: d181a54
- Release builds at commit d181a54, 5 runs per cell, the two cores alternating run by run; medians below.
- Wall, CPU (user + system) and peak resident memory are measured from outside with `/usr/bin/time -l`.

| Scenario | Core | Wall (s) | CPU user+sys (s) | Peak RSS (MB) | Wall spread | Items | Bytes | Ticks |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| transfer | Rust | 11.08 | 7.17 | 265 | 11.2% | 8 | 2147483648 | 3 |
| transfer | Swift | 15.47 | 11.67 | 519 | 26.9% (over 15%) | 8 | 2147483648 | 18 |
| ticks | Rust | 2.45 | 2.41 | 30 | 4.1% | 1000 | 0 | 600 |
| ticks | Swift | 2.70 | 2.65 | 13 | 2.6% | 1000 | 0 | 600 |
| ffi-snapshot | Rust via bindings | 15.05 | 15.02 | 23 | 2.2% | 1000 | 0 | 600 |
| ffi-snapshot-if-changed | Rust via bindings | 1.17 | 1.18 | 23 | 11.1% | 1000 | 0 | 600 |

Wall spread is (max - min) / median of the wall times over the runs.

The transfer wall times varied between runs by 11.2% (Rust) and 26.9% (Swift) in the recorded run, and by about 25% and 27% in the run before it. Read the wall-time medians for `transfer` with that in mind; the CPU time and memory were steadier.

## Every run

| Scenario | Core | Run | Wall (s) | CPU user+sys (s) | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|
| transfer | Rust | 1 | 11.08 | 7.17 | 265 |
| transfer | Rust | 2 | 10.76 | 5.77 | 265 |
| transfer | Rust | 3 | 10.69 | 5.75 | 265 |
| transfer | Rust | 4 | 11.93 | 9.17 | 265 |
| transfer | Rust | 5 | 11.80 | 9.68 | 265 |
| transfer | Swift | 1 | 15.47 | 13.44 | 519 |
| transfer | Swift | 2 | 15.93 | 14.54 | 519 |
| transfer | Swift | 3 | 13.89 | 11.67 | 519 |
| transfer | Swift | 4 | 11.86 | 10.57 | 519 |
| transfer | Swift | 5 | 16.02 | 10.91 | 519 |
| ticks | Rust | 1 | 2.45 | 2.41 | 31 |
| ticks | Rust | 2 | 2.44 | 2.39 | 30 |
| ticks | Rust | 3 | 2.54 | 2.45 | 30 |
| ticks | Rust | 4 | 2.50 | 2.42 | 31 |
| ticks | Rust | 5 | 2.45 | 2.40 | 29 |
| ticks | Swift | 1 | 2.67 | 2.63 | 13 |
| ticks | Swift | 2 | 2.74 | 2.66 | 13 |
| ticks | Swift | 3 | 2.68 | 2.64 | 13 |
| ticks | Swift | 4 | 2.70 | 2.65 | 13 |
| ticks | Swift | 5 | 2.70 | 2.66 | 13 |
| ffi-snapshot | Rust via bindings | 1 | 15.10 | 15.01 | 23 |
| ffi-snapshot | Rust via bindings | 2 | 15.04 | 15.02 | 23 |
| ffi-snapshot | Rust via bindings | 3 | 15.01 | 14.99 | 23 |
| ffi-snapshot | Rust via bindings | 4 | 15.34 | 15.31 | 24 |
| ffi-snapshot | Rust via bindings | 5 | 15.05 | 15.03 | 23 |
| ffi-snapshot-if-changed | Rust via bindings | 1 | 1.17 | 1.17 | 22 |
| ffi-snapshot-if-changed | Rust via bindings | 2 | 1.17 | 1.18 | 22 |
| ffi-snapshot-if-changed | Rust via bindings | 3 | 1.29 | 1.29 | 23 |
| ffi-snapshot-if-changed | Rust via bindings | 4 | 1.18 | 1.18 | 23 |
| ffi-snapshot-if-changed | Rust via bindings | 5 | 1.16 | 1.18 | 23 |

## What each scenario measures

All four scenarios are measured from outside (`/usr/bin/time -l`), so the figures include process start-up and the one-off generation of the payload buffer (256 MB for `transfer`, 64 KB for `ticks`), which is the same code and cost on both sides.

- **transfer** (`sdm-bench` / `SDMBench`): 8 items of 256 MB (2 GiB) in one package, 8 segments each, `maxConcurrent = 8`, global and per-host connection limits 64, default checkpoint interval (8 MiB) and minimum segment size (10 MiB), run to completion. `tick()` is called five times a second (`AppTiming.ticksPerSecond`) from the wall clock, and `snapshot()` is polled every 20 ms to see when the items have finished; the result line reports `ticks=<n>`, the number of ticks taken. Each output file's size is verified, then the directory is deleted. It exercises the hot path: stream, claim, write at offset, checkpoint, finalize. It measures the engine plus the filesystem: real files are written to the system temp volume.
- **ticks** (`sdm-bench` / `SDMBench`): 1,000 items of 64 KB in 20 packages, `maxConcurrent = 8`; the fake origin's body fetches are parked forever so the 8 running items stay running (the probe still passes); 600 iterations of `tick()` + `snapshot()`, reading `packages.count`. It exercises scheduling, telemetry and snapshot building at list scale. No bytes are transferred (`bytes=0`).
- **ffi-snapshot** (`SDMFFIBench`, Rust core only): Swift calling the generated bindings. 1,000 disabled items in 20 packages (so nothing is scheduled, downloaded or trashed), then 600 x (`tick()` + `snapshot()`), summing `items.count` over each snapshot so the Swift values are materialized. It measures the cost of copying the snapshot across the boundary. There is no Swift-core counterpart: the all-Swift core has no boundary. Its figures are not comparable to `ticks` (different item states, and the snapshots here are fully converted into Swift values).
- **ffi-snapshot-if-changed** (`SDMFFIBench`, Rust core only): the same 1,000 idle items and 600 ticks, but each tick calls `snapshotIfChanged(sinceGeneration:)` with the generation already held, as the app's heartbeat does. The first call returns the snapshot; the 599 after it return nothing. It measures what an idle engine costs per tick once the snapshot no longer crosses the boundary.

## What the `transfer` tick count and wall time mean

`Ticks` is the median number of `tick()` calls the program made. For `ticks` and `ffi-snapshot*` it is the fixed 600. For `transfer` it is the number of ticks made while the program was waiting for the eight items to finish: 3 for the Rust core and 18 for the Swift core (second run; 3 and 19 in the first). At 5 ticks per second that is far fewer than the 55 to 75 ticks a wall time of 11 to 15 seconds would hold, because most of the wall time is not spent in that wait. The program does these things in order, all inside the measured process:

1. builds the 256 MB payload byte by byte (a loop over 268 million elements, then, on the Swift side, a copy into `Data`);
2. adds the items, which starts the downloads (the first tick is made straight after this);
3. loops: tick if 200 ms have passed since the last tick, look at the snapshot, sleep 20 ms, until all items are `Completed`;
4. checks the size of each output file;
5. deletes the 2 GiB of output (the temp directory) before the process exits.

Only step 3 is the transfer. The tick count bounds its length: with a tick at the start and then at most one per 200 ms (the loop looks every 20 ms, so a tick is at most about 220 ms apart), 3 ticks means the wait lasted at least about 0.4 seconds for the Rust core, and 18 or 19 ticks at least about 3.4 seconds for the Swift core. These are lower bounds, not durations: a tick or the completion check can itself wait on the engine while a file is being flushed, so the real wait can be longer by an amount the tick count does not show. The programs do not print the duration of step 3 on its own, and the benchmarks were not re-run to measure it. The remaining wall time is the payload build, the file check and the deletion of the output (plus process start-up), none of which is separated either. The `transfer` wall and CPU figures therefore mix the transfer with that fixed work, which is the same code on both sides apart from the payload build and the fake origin's copying (see "Making the work identical"). They do not isolate the cost of the transfer path, and the difference between the cores in the table should not be read as the difference in transfer speed.

At 3 ticks in the Rust run no sidecar rewrite (every 25th tick) or data-file flush (every 300th tick) is reached, and in the Swift run (18 ticks) none is either; the checkpoint work that grew with loop speed in the earlier version of these programs is therefore small in both now.

## What they do not measure

No network (both cores read from an in-process fake origin; neither exercises `URLSession` or `reqwest`), no UI, no yt-dlp, muxing or persistence to disk (both use an in-memory state store; `SDMFFIBench` uses a real JSON state file in a temp directory but its items never change state), and nothing about launch time or the app as a whole.

## Making the work identical

- Both programs use their own core's public `FakeOrigin` (serving slices of one buffer built once with the `test_payload` formula `(i*31 + 7) % 251`) with the chunk size set to 64 KiB on both sides, instead of the fake's default of 64 bytes. Both fakes build a response eagerly, chunk by chunk. They are not byte-for-byte the same work: the Swift fake copies every chunk (`Data.subdata`) while the Rust fake slices the shared `Bytes` without copying. That is a property of each core's own fake. Peak memory and some of the CPU in `transfer` therefore reflect the two fakes, not only the two engines.
- For `ticks`, both programs wrap the core's `FakeOrigin` in the same small transport (written inside each bench program, since the test-only gates are not public) that lets the one-byte probe through and parks every other fetch.
- Both use an in-memory state store and the default retry policy. Rust's release profile has `overflow-checks = true` (the workspace setting); Swift's release build has its default checks.
- `transfer` ticks at the app's rate, 5 per second, on both sides, so the checkpoint and sidecar work per second does not depend on how fast a core's loop spins. Earlier versions of both programs ticked as fast as the loop ran (the Rust one far more often than the app would), which made that work differ between the cores; their figures are not comparable with these. What the `Ticks` column means for `transfer` is explained in the next section.

## Caveats

- **Run-to-run spread for `transfer` is large.** In the recorded run it was 11.2% for the Rust core and 26.9% for the Swift core; in the run before it (same commit, shown under "First full run" below) it was 25.4% and 27.2%. The transfer writes 2 GiB through the page cache on a laptop with 16 GB of memory, and the wall time and system CPU vary with how the kernel flushes it; the user-mode CPU is steady. The median wall times of the two cores are therefore close to the noise and should not be read as a difference. Peak RSS and the `ticks` figures are stable (spread under 6%).
- The machine is an Intel MacBook Pro (x86_64); the Apple Silicon slice of the Rust core was not run.
- The runs were on AC power; a code-review agent was working in the same repository and no build was seen running when the runs started.
- Both cores moved exactly 2,147,483,648 bytes in `transfer`.

## Release app sizes (previous task, for reference)

| App | Bundle | Main binary (universal) |
|---|---:|---:|
| Rust-core app | 171 MB | 53,818,256 bytes |
| Installed all-Swift app | 133 MB | 14,751,824 bytes |

## First full run (same commit, before the repeat)

The first run's wall spread for `transfer` was 25.4 percent (Rust) and 27.2 percent (Swift), over the 15 percent limit, so the whole script was run a second time; the tables above are the second run. The first run's medians:

| Scenario | Core | Wall (s) | CPU user+sys (s) | Peak RSS (MB) | Wall spread | Items | Bytes | Ticks |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| transfer | Rust | 10.59 | 6.04 | 265 | 25.4% (over 15%) | 8 | 2147483648 | 3 |
| transfer | Swift | 14.29 | 12.44 | 519 | 27.2% (over 15%) | 8 | 2147483648 | 19 |
| ticks | Rust | 2.52 | 2.48 | 28 | 3.2% | 1000 | 0 | 600 |
| ticks | Swift | 2.67 | 2.63 | 13 | 23.2% (over 15%) | 1000 | 0 | 600 |
| ffi-snapshot | Rust via bindings | 15.09 | 15.07 | 24 | 1.5% | 1000 | 0 | 600 |
| ffi-snapshot-if-changed | Rust via bindings | 1.18 | 1.19 | 22 | 15.3% (over 15%) | 1000 | 0 | 600 |

