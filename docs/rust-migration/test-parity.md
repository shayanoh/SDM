# Swift to Rust test parity ledger

One table per Swift test target, one row per file under `legacy/SDMKitSwift/Tests/<target>/` (helpers and fixture files included, with 0 tests). Swift counts come from `grep -c '@Test'` per file. Rust-only rows follow each table.

How the counts were taken (2026-10-13, Task 27, on commit `2eb59e1` plus the documentation edits):

- Swift baseline: `grep -r '@Test' legacy/SDMKitSwift/Tests/<target>` per target gives Core 79, Engine 256, Grabber 115 and Resolve 107, 557 in all. The legacy suite was not run again for this task; it was run in Task 24 and passed.
- Rust: `cargo test --workspace` in `sdm-core`, summing every `test result` line, gave 724 passed, 0 failed, 0 ignored. The per-crate sums are Core 98, Engine 308, Grabber 127, Resolve 131 and `sdm-ffi` 60. `sdm-bench`, `uniffi-bindgen` and every doc-test run have no tests.
- Swift tests of the bindings and shims: `swift test` in `SDMKit` ran 37 tests, all passed (`SDMRustTests` 7, `SDMShimTests` 30).

A Rust count that differs from its Swift count has its reason in the Notes column of that row.

## SDMCoreTests

| Swift file | Swift tests | Rust file | Rust tests | Notes |
|---|---|---|---|---|
| `AppTimingTests.swift` | 2 | `app_timing_tests.rs` | 2 |  |
| `ConnectionAllocatorTests.swift` | 6 | `connection_allocator_tests.rs` | 6 |  |
| `ContrastRatioTests.swift` | 6 | `contrast_ratio_tests.rs` | 6 |  |
| `DomainModelTests.swift` | 9 | `domain_model_tests.rs` | 9 |  |
| `DownloadItemTests.swift` | 9 | `download_item_tests.rs` | 9 |  |
| `FileComponentTests.swift` | 7 | `file_component_tests.rs` | 7 |  |
| `FormatChoiceTests.swift` | 3 | `format_choice_tests.rs` | 3 |  |
| `MediaFormatTests.swift` | 9 | `media_format_tests.rs` | 9 |  |
| `RangeSetClaimTests.swift` | 10 | `range_set_claim_tests.rs` | 11 | +1 seeded property test (2,000 claim/complete/release steps) |
| `RangeSetTests.swift` | 11 | `range_set_tests.rs` | 12 | +1 wire-shape test |
| `SmokeTests.swift` | 1 | `smoke_tests.rs` | 1 |  |
| `ThemeCatalogTests.swift` | 5 | `theme_catalog_tests.rs` | 6 | +1 byte-identity test of embedded theme files vs Swift fixtures |
| `ThemeTests.swift` | 1 | `theme_tests.rs` | 1 |  |
| (Rust-only) | 0 | `persistence_compat_tests.rs` | 12 | Swift-written fixtures load and re-serialize (state, packages, media, `SourceUrl` byte-for-byte identity; Review Focus 1 and 2). |
| (Rust-only) | 0 | `priority_tests.rs` | 4 | `Priority` ordering and wire shapes, `ItemState` wire shapes, a Swift-written fixture round trip. |
| **Total SDMCoreTests** | **79** |  | **98** | Rust column sums every row above, Rust-only rows included (82 ported + 16). |

## SDMEngineTests

| Swift file | Swift tests | Rust file | Rust tests | Notes |
|---|---|---|---|---|
| `CheckpointTickTests.swift` | 9 | `checkpoint_tick_tests.rs` | 8 | The 8 `DownloadTask`-only tests. The ninth (`checkpointFailureReachesTheEngineSnapshot`) drives a `DownloadEngine` and is `checkpoint_tick_engine_tests.rs` (1). |
| `ConnectionBudgetEngineTests.swift` | 2 | `connection_budget_engine_tests.rs` | 2 |  |
| `DownloadEngineTests.swift` | 15 | `download_engine_tests.rs` | 15 |  |
| `DownloadTaskTests.swift` | 8 | `download_task_tests.rs` | 8 | `start()` returns `DownloadError`, which wraps the transport error (`DownloadError::Transport`). |
| `DurableResumeTests.swift` | 8 | `durable_resume_tests.rs` | 8 | 5 sidecar-only tests plus 3 `DownloadTask` tests. |
| `DurableStateTests.swift` | 5 | `durable_state_tests.rs` | 5 | The two JSON-store tests use `JsonStateStore` over a real file; the real-restart test drives two engines over one store and folder. |
| `EngineRetryTests.swift` | 6 | `engine_retry_tests.rs` | 6 |  |
| `FakeOriginTests.swift` | 8 | `fake_origin_tests.rs` | 10 | 8 ported. +2 for the switches the Swift suite never exercises (`truncate_after_bytes`, `chunk_size`). |
| `FakeWholesaleDownloader.swift` | 0 | `support/fake_wholesale_downloader.rs` | 0 | Test support; no tests. |
| `HostileServerTests.swift` | 5 | `hostile_server_tests.rs` | 5 |  |
| `HysteresisTests.swift` | 1 | `hysteresis_tests.rs` | 1 |  |
| `LaunchStateTests.swift` | 3 | `launch_state_tests.rs` | 3 |  |
| `ManualRetryTests.swift` | 4 | `manual_retry_tests.rs` | 4 |  |
| `MetadataSnapshotTests.swift` | 1 | `metadata_snapshot_tests.rs` | 1 |  |
| `MinSegmentSizeTests.swift` | 3 | `min_segment_size_tests.rs` | 3 |  |
| `MultiComponentDownloadTests.swift` | 7 | `multi_component_download_tests.rs` | 7 | The 5-seed `@Test(arguments:)` churn test is one Rust test looping over the seeds. |
| `MuxAssemblyTests.swift` | 4 | `mux_assembly_tests.rs` | 5 | `FakeMuxer` is in `support/fake_muxer.rs`. +1 Rust-only: `a_stop_during_the_mux_does_not_poison_the_next_start` (a stop that interrupts the mux, then a restart: the item goes straight to the mux with no new origin request instead of re-downloading part 0 and failing with "already exists"). Deliberate improvement over Swift, which has the same bug; `context_for` hands a mux item whose finalized parts are all on disk no download contexts. |
| `MuxerTests.swift` | 6 | `muxer_tests.rs` | 6 | All 6 ported. `real_ffmpeg_muxes_two_tiny_streams` is gated on an `ffmpeg` at `/usr/local/bin` or `/opt/homebrew/bin`, as in Swift: with none it returns early and reports as passed (the `cargo test` run counted it as passed; whether an `ffmpeg` was present on this machine was not checked). |
| `NonResumableRestartTests.swift` | 2 | `non_resumable_restart_tests.rs` | 2 |  |
| `PackageHistoryTests.swift` | 2 | `package_history_tests.rs` | 2 |  |
| `ProcessRunnerSupport.swift` | 0 | `testing.rs` (`FakeProcessRunner`, `ok`, `fail`) | 0 | Shared with the `SDMResolveTests` copy; one Rust double behind the `testing` feature. Not tests. |
| `RemovalTests.swift` | 12 | `removal_tests.rs` | 16 | 12 ported; 4 added: removing / resetting an item right after stopping it, `move_items` ignoring a repeated id, `move_items_to_new_package`. Files go to a scratch `Trash` double, not the real Trash. |
| (Rust-only) | 0 | `removal_race_tests.rs` | 3 | `remove_item`, `remove_package` and `reset_download` park at their trash step while a `tick()` runs: nothing restarts the item and nothing reaches the origin (Swift's actor made the sequence atomic; here a transient `removing` set hides the item from the scheduler). The reset test also checks the item restarts and completes afterwards. |
| `ReorderingTests.swift` | 10 | `reordering_tests.rs` | 10 |  |
| `ResegmentationTests.swift` | 6 | `resegmentation_tests.rs` | 6 | The `@Test(arguments:)` seed table is one test looping over the same 6 seeds. `multipleWorkersNeverWriteOverlappingBytes` is `#[cfg(debug_assertions)]` like Swift's `#if DEBUG` `writeLog` (so it is absent from a `--release` run). The two raise tests hold their bodies at a `WorkerGate` (see the report) instead of racing the transfer. |
| `ResumeSidecarTests.swift` | 8 | `resume_sidecar_tests.rs` | 11 | +3 Rust-only: Swift-fixture round-trip (`loads_sidecar_written_by_swift`), `resumable_ranges` boot-session rule, `current_boot_session_id` (macOS). `matches` takes `Option<&str>`. |
| `ResumeTests.swift` | 9 | `resume_tests.rs` | 9 | `pausingMidFlightNeverRecordsUnwrittenBytes` stalls the origin after a few chunks (`StallingOrigin`) so "mid-flight" is deterministic. |
| `RetryPolicyTests.swift` | 10 | `retry_policy_tests.rs` | 10 | `classify` takes `&dyn Error` and downcasts, like Swift's `as?` casts. |
| `RoutingTransport.swift` | 0 | `support/routing_transport.rs` | 0 | Test support; no tests. Also holds `HostTransport` (one `FakeOrigin` per host), used by the engine churn test. |
| `SchedulerTests.swift` | 12 | `scheduler_tests.rs` | 12 | Fixture table ported entry for entry. |
| `SmokeTests.swift` | 1 | `smoke_tests.rs` | 1 |  |
| `SnapshotAggregatesTests.swift` | 17 | `snapshot_aggregates_tests.rs` | 17 |  |
| (Rust-only) | 0 | `churn_tests.rs` | 1 | `byte_identity_under_randomized_churn`: 40 seeds x 8 MB under worker-count changes, pauses, drops of the task and a dropping/truncating origin (Review Focus 3). |
| (Rust-only) | 0 | `wholesale_component_task_tests.rs` | 5 | `WholesaleComponentTask` is reached only through the engine in Swift: progress synthesis, `pause` waiting for the downloader, failure sweep, success sweep, missing output file. |
| (Rust-only) | 0 | `engine_churn_tests.rs` | 1 | `engine_byte_identity_under_churn`: 20 seeds x 6 items of 2-6 MB in 2 packages over a dropping `FakeOrigin` per item; segment-count changes, stop/start, priority changes, reorders and one shutdown + new-engine `restore()` interleaved with `tick()`; every output hashes to its payload and no `.incomplete` / `.sdmpart` is left (Review Focus 3). About 86 s in a debug build. |
| (Rust-only) | 0 | `src/engine/state.rs` (unit tests) | 2 | `every_snapshot_input_bumps_the_generation` (every `&mut` accessor and each helper that writes through one) and its converse for reads. |
| (Rust-only) | 0 | `src/engine/snapshot.rs` (unit tests) | 2 | A package whose aggregate (ETA) changed with equal items gets a new revision; an identical rebuild keeps it. `idle_ticks_do_not_touch_any_snapshot_input`: a stopped plus a finished item, 100 idle ticks leave the internal `snapshot_generation` alone (the published `generation` cannot show an idle tick that rebuilds to equal content, so the ported identity tests no longer pin the fast path). |
| (Rust-only) | 0 | `engine_snapshot_tests.rs` | 3 | Pins `PartialEq` / `content_eq` for the added `aggregate`, `revision`, `figures`, `generation` fields. |
| (Rust-only) | 0 | `engine_interruption_tests.rs` | 6 | Paths Swift's tests do not reach: a mux cut short by a stop or a shutdown is not recorded as a failure, a wholesale item that is stopped or caught by a shutdown lands stopped without charging a retry attempt, and the snapshot `generation` / `revision` counters stay put when nothing changed. +1 in Task 19: a stop that races the end of a transfer must not leave the item stopped beside a finished file (the engine bug the churn test found). |
| `SnapshotReuseTests.swift` | 12 | `snapshot_reuse_tests.rs` | 13 | 12 ported. The two storage-identity tests assert `generation` (engine) and `revision` (package) equality or difference instead of buffer identity. +1 Rust-only: `an_idle_engine_serves_the_same_generation` (100 idle ticks leave `generation` and every `revision` alone). |
| `SparseFileTests.swift` | 5 | `sparse_file_tests.rs` | 5 |  |
| `SpeedSamplerTests.swift` | 14 | `speed_sampler_tests.rs` | 14 |  |
| `StateStoreTests.swift` | 14 | `state_store_tests.rs` | 16 | +2 Rust-only: `loads_state_written_by_swift` (rich, minimal, legacy flat), `wrong_format_version_loads_as_empty_and_is_left_in_place`. Literal-JSON tests use r-strings; encoder-based corruption tests substitute in `serde_json` output. |
| `TestSupport.swift` | 0 | `support/mod.rs`, `support/engine_support.rs` | 0 | Test support; no tests (`Gate`, `GatedOrigin`, `test_payload`, `make_scratch_directory`, engine constructors including `new_engine_with_store`). |
| `URLRefreshTests.swift` | 3 | `url_refresh_tests.rs` | 5 | +2 Rust-only: stopping an item during its refresh ends the refresh (resolver honours the token) and a refresh that returns after a stop installs no new task (resolver ignores it); both leave the stale component URL in place. `FakeResolver` is in `support/fake_resolver.rs`. |
| `URLSessionTransportTests.swift` | 12 | `reqwest_transport_tests.rs` | 24 | 12 ported (the `HTTPURLResponse` helpers become `HeaderMap` helpers; `defaultConfigurationNeverTouchesTheSharedCache` becomes a loopback test that a cacheable response is refetched, since reqwest has no cache to configure). +4 Review Focus 4 (redirect and final URL, body cut short, lying `Content-Length`, origin ignoring `Range`). +1 cookie replay across a redirect hop (Gate C). +5 loopback: 206/`Content-Range`/validator, error status returned not thrown, refused connection, dropping the body stream closes the connection, dropping the fetch before headers closes the connection. +2 (gate E): a validator header with raw UTF-8 bytes is kept (loopback), and one with invalid UTF-8 decodes as Latin-1 instead of being dropped. |
| `UpdateSettingsTests.swift` | 1 | `update_settings_tests.rs` | 1 |  |
| `WholesaleDownloadTests.swift` | 8 | `wholesale_download_tests.rs` | 8 | The preemption test waits for the preempted wholesale item to land `Queued` (the cancelled downloader call unwinds at once in Rust), where Swift asserts it straight after the ticks. No test asserted the swept-scratch behaviour, so nothing was changed to the wholesale-resume spec. |
| `YtDlpArtifactsTests.swift` | 3 | `yt_dlp_artifacts_tests.rs` | 3 |  |
| (Rust-only) | 0 | `src/download_task.rs` (unit tests) | 2 | `the_sentinel_returns_once_the_task_is_cancelled`, `run_workers_after_a_pause_spawns_nothing_and_returns` (private `DownloadTask` items). |
| (Rust-only) | 0 | `checkpoint_tick_engine_tests.rs` | 1 | The ninth `CheckpointTickTests` case (`checkpointFailureReachesTheEngineSnapshot`), which drives a `DownloadEngine` (see the `checkpoint_tick_tests.rs` row). |
| **Total SDMEngineTests** | **256** |  | **308** | Rust column sums every row above, Rust-only rows included (303 + 2 gate E + 3 previously unlisted). |

## SDMGrabberTests

| Swift file | Swift tests | Rust file | Rust tests | Notes |
|---|---|---|---|---|
| `FakeLinkResolver.swift` | 0 | `tests/support/fake_link_resolver.rs` | 0 | Support, not tests. `vf`/`af` live in `tests/support/mod.rs`. |
| `FakeProbeOriginTests.swift` | 6 | `fake_probe_origin_tests.rs` | 6 |  |
| `FileSignatureTests.swift` | 8 | `file_signature_tests.rs` | 8 |  |
| `GrabberSessionOverrideTests.swift` | 6 | `grabber_session_override_tests.rs` | 7 | +1: `a_url_deduplicates_against_its_own_original_spelling` (URL identity is the string as written). |
| `GrabberSessionTests.swift` | 9 | `grabber_session_tests.rs` | 11 | The prune tests drive an injected clock (`ManualClock`) instead of `Task.sleep` and `Date()`. +2 (gate E, E2-F2): dropping the caller of `ingest_urls` / `recheck` mid-probe still finishes the probe and reclusters. |
| `LinkProberStageTwoTests.swift` | 3 | `link_prober_stage_two_tests.rs` | 3 |  |
| `LinkProberTests.swift` | 5 | `link_prober_tests.rs` | 5 |  |
| `MediaFormatMenuTests.swift` | 6 | `media_format_menu_tests.rs` | 6 | Byte counts use a hand-written `ByteCountFormatter` (.binary) twin; this file asserts only the `~` marker and `360p`. The twin's output is pinned against measured Foundation values by a unit test in `src/media_format_menu.rs` (own row below). |
| `MediaHandoffTests.swift` | 6 | `media_handoff_tests.rs` | 6 |  |
| `MediaMetadataTests.swift` | 3 | `media_metadata_tests.rs` | 3 |  |
| `MediaRowRoutingTests.swift` | 12 | `media_row_routing_tests.rs` | 13 | +1 (gate F): `a_panic_under_the_session_lock_does_not_disable_the_session`, the grabber lock recovers from poisoning. |
| `PackageClusteringTests.swift` | 10 | `package_clustering_tests.rs` | 10 |  |
| `PackageOverrideTests.swift` | 5 | `package_override_tests.rs` | 5 |  |
| `PlaylistExpansionTests.swift` | 8 | `playlist_expansion_tests.rs` | 8 | The 30 ms sleeps in the Swift resolvers are gates; the cap test waits for 3 resolves in flight, yields, and asserts no fourth started. |
| `ReleaseTagsTests.swift` | 4 | `release_tags_tests.rs` | 4 | The Swift `arguments:` test is one Rust test looping over the same six cases. |
| `SmokeTests.swift` | 1 | `smoke_tests.rs` | 1 | Constructs a `GrabberSnapshot` instead of `#expect(true)`. |
| `URLExtractorTests.swift` | 8 | `url_extractor_tests.rs` | 9 | `linkify` replaces `NSDataDetector`. One Rust-only test: a link written without a scheme is not picked up (deliberate difference from Swift). |
| `URLSessionProbeTransportTests.swift` | 6 | `reqwest_probe_transport_tests.rs` | 9 | The Swift tests exercise static helpers; the Rust ones drive the transport against a loopback server (HEAD/GET/Range, lowercased headers, timeout via a short idle timeout, DNS via a failing resolver, refused connection). One Rust-only test: the body cap (a GET with no range returns one byte, as in Swift) and the final URL after a redirect. +2 (gate E): a header with raw UTF-8 bytes is kept (loopback), and one with invalid UTF-8 decodes as Latin-1 instead of being dropped. |
| `VerdictRulesTests.swift` | 9 | `verdict_rules_tests.rs` | 9 |  |
| (Rust-only) | 0 | `src/media_format_menu.rs` (unit tests) | 1 | `byte_count_string_matches_byte_count_formatter`: 40 values measured with `ByteCountFormatter(.binary)` on macOS (en_US). |
| (Rust-only) | 0 | `src/media_row.rs` (unit tests) | 1 | `sanitize_strips_format_characters_like_swift` (Cc and Cf, e.g. the Persian ZWNJ). |
| (Rust-only) | 0 | `src/url_text.rs` (unit tests) | 2 | `directory_path_matches_foundation`; `query_only_url_without_a_slash_has_no_path`. |
| **Total SDMGrabberTests** | **115** |  | **127** | Rust column sums every row above, Rust-only rows included (123 + 4). |

## SDMResolveTests

| Swift file | Swift tests | Rust file | Rust tests | Notes |
|---|---|---|---|---|
| `BinaryFetchingTests.swift` | 2 | `binary_fetching_tests.rs` | 6 | 2 ported against `FakeBinaryFetcher`. +4 loopback tests for `ReqwestBinaryFetcher` (body and User-Agent, non-2xx, redirect, connection refused); Swift has none for `URLSessionBinaryFetcher`. |
| `BinaryLocatorTests.swift` | 5 | `binary_locator_tests.rs` | 5 |  |
| `CookieSourceTests.swift` | 3 | `cookie_source_tests.rs` | 3 |  |
| `FormatSelectorTests.swift` | 16 | `format_selector_tests.rs` | 16 |  |
| `LZFSETests.swift` | 3 | `lzfse_tests.rs` | 5 | +2: `decodes_a_file_compressed_by_macos` (fixture made by `compression_tool`), `lzfse_decode_is_independent_of_the_size_hint`. |
| `ManagedBinariesCadenceTests.swift` | 2 | `managed_binaries_cadence_tests.rs` | 2 |  |
| `ManagedBinariesInflationTests.swift` | 4 | `managed_binaries_inflation_tests.rs` | 4 |  |
| `ManagedBinariesTypesTests.swift` | 4 | `managed_binaries_types_tests.rs` | 6 | +2: `manifest_matches_the_swift_fixture_byte_for_byte`, `manifest_json_escapes_like_foundation`. |
| `ManagedBinariesUpdateTests.swift` | 7 | `managed_binaries_update_tests.rs` | 7 |  |
| `SiteRegistryTests.swift` | 4 | `site_registry_tests.rs` | 4 | The `@Test(arguments:)` table is one looping test. |
| `Support.swift` | 0 | `testing.rs` (`FakeProcessRunner`, `ok`, `fail`) | 0 | `FakeBinaryFetcher` now in `testing.rs`; `fixture_data`/`fixture_dump` in `tests/support`; `URL.tmp` is `tempfile::tempdir`. Not tests. |
| `SystemProcessRunnerTests.swift` | 8 | `system_process_runner_tests.rs` | 12 | 8 Swift tests ported (`cancellation_terminates_the_process` cancels a `CancellationToken`, the stand-in for task cancellation). Extra, not ported: 3 in `system_process_runner_tests.rs` (`cancellation_applies_to_run_too`, `already_cancelled_token_returns_cancelled`, `dropping_the_future_still_terminates_the_child`), `child_environment_keeps_non_utf8_variables_untouched` (a Rust-only hazard: `std::env::vars` panics on non-UTF-8) and `process_runner_tests.rs` (3: default `run_streaming`, fake, signal exit code). |
| `WholesaleProgressParserTests.swift` | 8 | `wholesale_progress_parser_tests.rs` | 8 |  |
| `YtDlpJSONTests.swift` | 8 | `yt_dlp_json_tests.rs` | 9 | 8 ported + 1 added test pinning `JSONDecoder` tolerance (float-for-int, null, unknown keys, type mismatches). |
| `YtDlpResolverTests.swift` | 27 | `yt_dlp_resolver_tests.rs` | 32 | +1: `refresh_surfaces_cancellation_from_the_runner` (cancellation token seam). +3 argument-vector tests (single, playlist, refresh: complete ordered vector, upper-case host kept as given) and +1 `a_url_with_no_parsed_form_is_unsupported` (`SourceUrl` can lack a parsed form). |
| `YtDlpSmokeTests.swift` | 1 | `yt_dlp_smoke_tests.rs` | 1 | Skips with `eprintln!` when yt-dlp is not installed. |
| `YtDlpWholesaleDownloaderTests.swift` | 5 | `yt_dlp_wholesale_downloader_tests.rs` | 8 | +1: `runner_cancellation_and_timeout_map_to_wholesale_errors`. +2: `passes_the_complete_argument_vector_with_the_original_url`, `a_url_with_no_parsed_form_still_downloads_with_its_string`. |
| `Fixtures/playlist_flat.json` | 0 | `fixtures/playlist_flat.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/playlist_soundcloud.json` | 0 | `fixtures/playlist_soundcloud.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/single_but_type_playlist.json` | 0 | `fixtures/single_but_type_playlist.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/video_direct_vimeo.json` | 0 | `fixtures/video_direct_vimeo.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/video_hls_only.json` | 0 | `fixtures/video_hls_only.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/video_muxed.json` | 0 | `fixtures/video_muxed.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/video_progressive_only.json` | 0 | `fixtures/video_progressive_only.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/video_refresh_format_gone.json` | 0 | `fixtures/video_refresh_format_gone.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/video_refresh_ok.json` | 0 | `fixtures/video_refresh_ok.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| `Fixtures/video_tube_no_codecs.json` | 0 | `fixtures/video_tube_no_codecs.json` | 0 | Data file, no tests; copied unchanged into the Rust crate. |
| (Rust-only) | 0 | `process_runner_tests.rs` | 3 | Default `run_streaming`, the fake runner, signal exit code (also named in the `SystemProcessRunnerTests` row). |
| **Total SDMResolveTests** | **107** |  | **131** | Rust column sums every row above, Rust-only rows included (128 + 3); the `SystemProcessRunnerTests` row's own count is its file's 12. |

## Grand total

| Crate | Swift tests | Rust tests |
|---|---:|---:|
| `sdm-core` (SDMCoreTests) | 79 | 98 |
| `sdm-engine` (SDMEngineTests) | 256 | 308 |
| `sdm-grabber` (SDMGrabberTests) | 115 | 127 |
| `sdm-resolve` (SDMResolveTests) | 107 | 131 |
| Ported core, subtotal | **557** | **664** |
| `sdm-ffi` (listed below) | 0 | 60 |
| `sdm-bench`, `uniffi-bindgen` | 0 | 0 |
| **`cargo test --workspace`** | **557** | **724** |

The Rust core has 107 more tests than the Swift core has: the extra tests are listed row by row above and in the notes, and are of four kinds. They pin on-disk compatibility with files the Swift app wrote, cover the Rust-only seams (cancellation tokens, the `reqwest` transports, the removal guard, `FakeOrigin` switches the Swift suite never used), pin the deliberate differences from the Swift core, and replace a Swift-only guarantee (the actor, `NSDataDetector`, `ByteCountFormatter`) with an explicit test. Every Swift test has a Rust twin with the same name in snake_case; none was dropped. Where one `@Test(arguments:)` table became one Rust test that loops, the Rust count is lower for that row and the note says so.

## SDMRustTests, SDMShimTests and the FFI crate (listed separately)

These test the UniFFI surface (`sdm-ffi` and the generated Swift) and the thin Swift shims, not the ported core, so they are not part of the grand totals above. The Swift core has no FFI, so every test here is new.

| Swift file / Rust file | Swift tests | Rust tests | Notes |
|---|---:|---:|---|
| `SDMKit/Tests/SDMRustTests/SkeletonTests.swift` | 3 | | `coreVersion()` throws a String, an invalid argument throws `CoreError` instead of trapping, and an engine round-trips `UUID`, a percent-encoded `URL` and a `RangeSet` through `add` / `snapshot`. |
| `SDMKit/Tests/SDMRustTests/UrlRoundTripTests.swift` | 4 | | Strings the core preserves come back byte for byte; Foundation's rewrites of a URL are stable, so the Swift route deduplicates; extracted links carry Foundation's form; the unusable-URL placeholder is refused on the way back in. |
| `SDMKit/Tests/SDMShimTests/ShimTests.swift` | 30 | | One test per shim addition in `SDMKit/Sources/{SDMCore,SDMResolve,SDMEngine,SDMGrabber}`, including `perRowShimsAgreeWithTheCore`, which pins the three per-row figures computed in Swift (`isRetrying`, `fractionCompleted`, `RangeSet.totalBytes`) to the core's answers. |
| `sdm-ffi/tests/engine_ffi_tests.rs` | | 9 | Production `new` over a temp state file; every exported method once with valid arguments and once with an unknown id; invalid arguments are errors; a download completes through the wrapper; state survives shutdown and restore; lifecycle calls change the snapshot; retry requeues a failed item; the settings setters reach the yt-dlp command line. |
| `sdm-ffi/tests/grabber_ffi_tests.rs` | | 7 | Production `new`; every exported method (over `FakeProbeOrigin`); unknown ids; `prune_older_than` with a caller-supplied `now`; a zero budget is refused; setters change what the session does; provider closures read the setters' latest values. |
| `sdm-ffi/tests/binaries_ffi_tests.rs` | | 5 | Production `new`; every exported method over the fake fetcher and process runner; status after a failed check; an empty directory is refused; an update notifies once and invalidates the locators. |
| `sdm-ffi/tests/concurrency_ffi_tests.rs` | | 6 | Review Focus 5: 16 tasks x 2,000 mixed calls while a 17th shuts the engine down at a seeded random point, under a 60 s timeout; a deliberately panicking call is an error, sync and async; a panic inside the engine and inside trashing leaves the engine answering; poisoned engine and settings mutexes are recovered. |
| `sdm-ffi/tests/custom_types_ffi_tests.rs` | | 8 | The custom types through the converters UniFFI generates: `SourceUrl` byte for byte for URLs `url::Url` would rewrite, refusal of unparseable strings and of the unusable-URL placeholder as a throwable `CoreError`, `Uuid`, `PathBuf`, `usize`, `RangeSet` normalization and refusal of inverted ranges. |
| `sdm-ffi/tests/functions_ffi_tests.rs` | | 24 | The free functions: version, timing constants, speed helpers, theme catalog, contrast validation, site registry, format selector and menu, URL extraction, media handoff, range-set helpers, constructors, the grabber summary and names, package aggregates, item snapshot helpers, container extensions, persisted raw values of the settings enums, `Priority` raw values. |
| `sdm-ffi/src/shared.rs` (unit) | | 1 | `Toolchain::configured` starts with the given yt-dlp settings. |
| **Total** | **37** | **60** | `swift test` in `SDMKit`: 37 passed. `cargo test -p sdm-ffi`: 60 passed (sum of the Rust column). |

## Benchmark programs (no tests)

`sdm-core/crates/sdm-bench` (Rust), `legacy/SDMKitSwift/Sources/SDMBench` (Swift core) and `SDMKit/Sources/SDMFFIBench` (Rust core through the bindings) are measurement programs, not tests: they have 0 tests each and are run by `scripts/` and recorded in [benchmarks.md](benchmarks.md).
