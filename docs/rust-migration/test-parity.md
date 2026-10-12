# Swift to Rust test parity ledger

One table per Swift test target, one row per file under `SDMKit/Tests/<target>/` (helpers and fixture files included, with 0 tests). Swift counts come from `grep -c '@Test'` per file. The Rust columns are filled in as each file is ported (see the global constraints).

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
| **Total SDMCoreTests** | **79** |  |  |  |

## SDMEngineTests

| Swift file | Swift tests | Rust file | Rust tests | Notes |
|---|---|---|---|---|
| `CheckpointTickTests.swift` | 9 | `checkpoint_tick_tests.rs` | 8 | The 8 `DownloadTask`-only tests. The ninth (`checkpointFailureReachesTheEngineSnapshot`) drives a `DownloadEngine` and is `checkpoint_tick_engine_tests.rs` (1). |
| `ConnectionBudgetEngineTests.swift` | 2 | `connection_budget_engine_tests.rs` | 2 |  |
| `DownloadEngineTests.swift` | 15 | `download_engine_tests.rs` | 15 |  |
| `DownloadTaskTests.swift` | 8 | `download_task_tests.rs` | 8 | `start()` returns `DownloadError`, which wraps the transport error (`DownloadError::Transport`). |
| `DurableResumeTests.swift` | 8 | `durable_resume_tests.rs` | 8 | 5 sidecar-only tests plus 3 `DownloadTask` tests. |
| `DurableStateTests.swift` | 5 |  |  |  |
| `EngineRetryTests.swift` | 6 | `engine_retry_tests.rs` | 6 |  |
| `FakeOriginTests.swift` | 8 | `fake_origin_tests.rs` | 10 | 8 ported. +2 for the switches the Swift suite never exercises (`truncate_after_bytes`, `chunk_size`). |
| `FakeWholesaleDownloader.swift` | 0 | `support/fake_wholesale_downloader.rs` | 0 | Test support; no tests. |
| `HostileServerTests.swift` | 5 | `hostile_server_tests.rs` | 5 |  |
| `HysteresisTests.swift` | 1 | `hysteresis_tests.rs` | 1 |  |
| `LaunchStateTests.swift` | 3 |  |  |  |
| `ManualRetryTests.swift` | 4 | `manual_retry_tests.rs` | 4 |  |
| `MetadataSnapshotTests.swift` | 1 | `metadata_snapshot_tests.rs` | 1 |  |
| `MinSegmentSizeTests.swift` | 3 | `min_segment_size_tests.rs` | 3 |  |
| `MultiComponentDownloadTests.swift` | 7 | `multi_component_download_tests.rs` | 7 | The 5-seed `@Test(arguments:)` churn test is one Rust test looping over the seeds. |
| `MuxAssemblyTests.swift` | 4 | `mux_assembly_tests.rs` | 4 | `FakeMuxer` is in `support/fake_muxer.rs`. |
| `MuxerTests.swift` | 6 | `muxer_tests.rs` | 6 | All 6 ported; `real_ffmpeg_muxes_two_tiny_streams` is `#[ignore]` (the Rust suite never runs a real `ffmpeg`; Swift ran it when one was installed), so 5 run. |
| `NonResumableRestartTests.swift` | 2 | `non_resumable_restart_tests.rs` | 2 |  |
| `PackageHistoryTests.swift` | 2 | `package_history_tests.rs` | 2 |  |
| `ProcessRunnerSupport.swift` | 0 | `testing.rs` (`FakeProcessRunner`, `ok`, `fail`) | 0 | Shared with the `SDMResolveTests` copy; one Rust double behind the `testing` feature. Not tests. |
| `RemovalTests.swift` | 12 | `removal_tests.rs` | 16 | 12 ported; 4 added: removing / resetting an item whose runner is still unwinding, `move_items` ignoring a repeated id, `move_items_to_new_package`. Files go to a scratch `Trash` double, not the real Trash. |
| `ReorderingTests.swift` | 10 | `reordering_tests.rs` | 10 |  |
| `ResegmentationTests.swift` | 6 | `resegmentation_tests.rs` | 6 | The `@Test(arguments:)` seed table is one test looping over the same 6 seeds. `multipleWorkersNeverWriteOverlappingBytes` is `#[cfg(debug_assertions)]` like Swift's `#if DEBUG` `writeLog` (so it is absent from a `--release` run). The two raise tests hold their bodies at a `WorkerGate` (see the report) instead of racing the transfer. |
| `ResumeSidecarTests.swift` | 8 | `resume_sidecar_tests.rs` | 11 | +3 Rust-only: Swift-fixture round-trip (`loads_sidecar_written_by_swift`), `resumable_ranges` boot-session rule, `current_boot_session_id` (macOS). `matches` takes `Option<&str>`. |
| `ResumeTests.swift` | 9 | `resume_tests.rs` | 9 | `pausingMidFlightNeverRecordsUnwrittenBytes` stalls the origin after a few chunks (`StallingOrigin`) so "mid-flight" is deterministic. |
| `RetryPolicyTests.swift` | 10 | `retry_policy_tests.rs` | 10 | `classify` takes `&dyn Error` and downcasts, like Swift's `as?` casts. |
| `RoutingTransport.swift` | 0 |  |  |  |
| `SchedulerTests.swift` | 12 | `scheduler_tests.rs` | 12 | Fixture table ported entry for entry. |
| `SmokeTests.swift` | 1 | `smoke_tests.rs` | 1 |  |
| `SnapshotAggregatesTests.swift` | 17 | `snapshot_aggregates_tests.rs` | 17 |  |
| (Rust-only) | 0 | `churn_tests.rs` | 1 | `byte_identity_under_randomized_churn`: 40 seeds x 8 MB under worker-count changes, pauses, drops of the task and a dropping/truncating origin (Review Focus 3). |
| (Rust-only) | 0 | `wholesale_component_task_tests.rs` | 5 | `WholesaleComponentTask` is reached only through the engine in Swift: progress synthesis, `pause` waiting for the downloader, failure sweep, success sweep, missing output file. |
| (Rust-only) | 0 | `engine_snapshot_tests.rs` | 3 | Pins `PartialEq` / `content_eq` for the added `aggregate`, `revision`, `figures`, `generation` fields. |
| (Rust-only) | 0 | `engine_interruption_tests.rs` | 5 | Paths Swift's tests do not reach: a mux cut short by a stop or a shutdown is not recorded as a failure, a wholesale item that is stopped or caught by a shutdown lands stopped without charging a retry attempt, and the snapshot `generation` / `revision` counters stay put when nothing changed. |
| `SnapshotReuseTests.swift` | 12 |  |  |  |
| `SparseFileTests.swift` | 5 | `sparse_file_tests.rs` | 5 |  |
| `SpeedSamplerTests.swift` | 14 | `speed_sampler_tests.rs` | 14 |  |
| `StateStoreTests.swift` | 14 | `state_store_tests.rs` | 16 | +2 Rust-only: `loads_state_written_by_swift` (rich, minimal, legacy flat), `wrong_format_version_loads_as_empty_and_is_left_in_place`. Literal-JSON tests use r-strings; encoder-based corruption tests substitute in `serde_json` output. |
| `TestSupport.swift` | 0 |  |  |  |
| `URLRefreshTests.swift` | 3 | `url_refresh_tests.rs` | 5 | +2 Rust-only: stopping an item during its refresh ends the refresh (resolver honours the token) and a refresh that returns after a stop installs no new task (resolver ignores it); both leave the stale component URL in place. `FakeResolver` is in `support/fake_resolver.rs`. |
| `URLSessionTransportTests.swift` | 12 | `reqwest_transport_tests.rs` | 22 | 12 ported (the `HTTPURLResponse` helpers become `HeaderMap` helpers; `defaultConfigurationNeverTouchesTheSharedCache` becomes a loopback test that a cacheable response is refetched, since reqwest has no cache to configure). +4 Review Focus 4 (redirect and final URL, body cut short, lying `Content-Length`, origin ignoring `Range`). +1 cookie replay across a redirect hop (Gate C). +5 loopback: 206/`Content-Range`/validator, error status returned not thrown, refused connection, dropping the body stream closes the connection, dropping the fetch before headers closes the connection. |
| `UpdateSettingsTests.swift` | 1 | `update_settings_tests.rs` | 1 |  |
| `WholesaleDownloadTests.swift` | 8 | `wholesale_download_tests.rs` | 8 | The preemption test waits for the preempted wholesale item to land `Queued` (the cancelled downloader call unwinds at once in Rust), where Swift asserts it straight after the ticks. No test asserted the swept-scratch behaviour, so nothing was changed to the wholesale-resume spec. |
| `YtDlpArtifactsTests.swift` | 3 | `yt_dlp_artifacts_tests.rs` | 3 |  |
| **Total SDMEngineTests** | **256** |  |  |  |

## SDMGrabberTests

| Swift file | Swift tests | Rust file | Rust tests | Notes |
|---|---|---|---|---|
| `FakeLinkResolver.swift` | 0 |  |  |  |
| `FakeProbeOriginTests.swift` | 6 |  |  |  |
| `FileSignatureTests.swift` | 8 |  |  |  |
| `GrabberSessionOverrideTests.swift` | 6 |  |  |  |
| `GrabberSessionTests.swift` | 9 |  |  |  |
| `LinkProberStageTwoTests.swift` | 3 |  |  |  |
| `LinkProberTests.swift` | 5 |  |  |  |
| `MediaFormatMenuTests.swift` | 6 |  |  |  |
| `MediaHandoffTests.swift` | 6 |  |  |  |
| `MediaMetadataTests.swift` | 3 |  |  |  |
| `MediaRowRoutingTests.swift` | 12 |  |  |  |
| `PackageClusteringTests.swift` | 10 |  |  |  |
| `PackageOverrideTests.swift` | 5 |  |  |  |
| `PlaylistExpansionTests.swift` | 8 |  |  |  |
| `ReleaseTagsTests.swift` | 4 |  |  |  |
| `SmokeTests.swift` | 1 |  |  |  |
| `URLExtractorTests.swift` | 8 |  |  |  |
| `URLSessionProbeTransportTests.swift` | 6 |  |  |  |
| `VerdictRulesTests.swift` | 9 |  |  |  |
| **Total SDMGrabberTests** | **115** |  |  |  |

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
| `Fixtures/playlist_flat.json` | 0 |  |  |  |
| `Fixtures/playlist_soundcloud.json` | 0 |  |  |  |
| `Fixtures/single_but_type_playlist.json` | 0 |  |  |  |
| `Fixtures/video_direct_vimeo.json` | 0 |  |  |  |
| `Fixtures/video_hls_only.json` | 0 |  |  |  |
| `Fixtures/video_muxed.json` | 0 |  |  |  |
| `Fixtures/video_progressive_only.json` | 0 |  |  |  |
| `Fixtures/video_refresh_format_gone.json` | 0 |  |  |  |
| `Fixtures/video_refresh_ok.json` | 0 |  |  |  |
| `Fixtures/video_tube_no_codecs.json` | 0 |  |  |  |
| **Total SDMResolveTests** | **107** |  |  |  |

## Grand total (four original test targets)

**557** Swift tests.

## SDMRustTests (FFI skeleton, listed separately)

These test the Rust FFI skeleton, not the ported core, and are not part of the grand total above.

| Swift file | Swift tests | Rust file | Rust tests | Notes |
|---|---|---|---|---|
| `SkeletonTests.swift` | 2 |  |  |  |
| **Total SDMRustTests** | **2** |  |  |  |
