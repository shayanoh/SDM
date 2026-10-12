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
| `CheckpointTickTests.swift` | 9 |  |  |  |
| `ConnectionBudgetEngineTests.swift` | 2 |  |  |  |
| `DownloadEngineTests.swift` | 15 |  |  |  |
| `DownloadTaskTests.swift` | 8 |  |  |  |
| `DurableResumeTests.swift` | 8 |  |  |  |
| `DurableStateTests.swift` | 5 |  |  |  |
| `EngineRetryTests.swift` | 6 |  |  |  |
| `FakeOriginTests.swift` | 8 |  |  |  |
| `FakeWholesaleDownloader.swift` | 0 |  |  |  |
| `HostileServerTests.swift` | 5 |  |  |  |
| `HysteresisTests.swift` | 1 |  |  |  |
| `LaunchStateTests.swift` | 3 |  |  |  |
| `ManualRetryTests.swift` | 4 |  |  |  |
| `MetadataSnapshotTests.swift` | 1 |  |  |  |
| `MinSegmentSizeTests.swift` | 3 |  |  |  |
| `MultiComponentDownloadTests.swift` | 7 |  |  |  |
| `MuxAssemblyTests.swift` | 4 |  |  |  |
| `MuxerTests.swift` | 6 |  |  |  |
| `NonResumableRestartTests.swift` | 2 |  |  |  |
| `PackageHistoryTests.swift` | 2 |  |  |  |
| `ProcessRunnerSupport.swift` | 0 | `testing.rs` (`FakeProcessRunner`, `ok`, `fail`) | 0 | Shared with the `SDMResolveTests` copy; one Rust double behind the `testing` feature. Not tests. |
| `RemovalTests.swift` | 12 |  |  |  |
| `ReorderingTests.swift` | 10 |  |  |  |
| `ResegmentationTests.swift` | 6 |  |  |  |
| `ResumeSidecarTests.swift` | 8 |  |  |  |
| `ResumeTests.swift` | 9 |  |  |  |
| `RetryPolicyTests.swift` | 10 | `retry_policy_tests.rs` | 10 | `classify` takes `&dyn Error` and downcasts, like Swift's `as?` casts. |
| `RoutingTransport.swift` | 0 |  |  |  |
| `SchedulerTests.swift` | 12 | `scheduler_tests.rs` | 12 | Fixture table ported entry for entry. |
| `SmokeTests.swift` | 1 | `smoke_tests.rs` | 1 |  |
| `SnapshotAggregatesTests.swift` | 17 | `snapshot_aggregates_tests.rs` | 17 |  |
| (Rust-only) | 0 | `engine_snapshot_tests.rs` | 3 | Pins `PartialEq` / `content_eq` for the added `aggregate`, `revision`, `figures`, `generation` fields. |
| `SnapshotReuseTests.swift` | 12 |  |  |  |
| `SparseFileTests.swift` | 5 |  |  |  |
| `SpeedSamplerTests.swift` | 14 | `speed_sampler_tests.rs` | 14 |  |
| `StateStoreTests.swift` | 14 |  |  |  |
| `TestSupport.swift` | 0 |  |  |  |
| `URLRefreshTests.swift` | 3 |  |  |  |
| `URLSessionTransportTests.swift` | 12 |  |  |  |
| `UpdateSettingsTests.swift` | 1 |  |  |  |
| `WholesaleDownloadTests.swift` | 8 |  |  |  |
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
| `LZFSETests.swift` | 3 | `lzfse_tests.rs` | 4 | +1: `decodes_a_file_compressed_by_macos` (fixture made by `compression_tool`). |
| `ManagedBinariesCadenceTests.swift` | 2 | `managed_binaries_cadence_tests.rs` | 2 |  |
| `ManagedBinariesInflationTests.swift` | 4 | `managed_binaries_inflation_tests.rs` | 4 |  |
| `ManagedBinariesTypesTests.swift` | 4 | `managed_binaries_types_tests.rs` | 6 | +2: `manifest_matches_the_swift_fixture_byte_for_byte`, `manifest_json_escapes_like_foundation`. |
| `ManagedBinariesUpdateTests.swift` | 7 | `managed_binaries_update_tests.rs` | 7 |  |
| `SiteRegistryTests.swift` | 4 | `site_registry_tests.rs` | 4 | The `@Test(arguments:)` table is one looping test. |
| `Support.swift` | 0 | `testing.rs` (`FakeProcessRunner`, `ok`, `fail`) | 0 | `FakeBinaryFetcher` now in `testing.rs`; `fixture_data`/`fixture_dump` in `tests/support`; `URL.tmp` is `tempfile::tempdir`. Not tests. |
| `SystemProcessRunnerTests.swift` | 8 | `system_process_runner_tests.rs` | 11 | 8 Swift tests ported (`cancellation_terminates_the_process` cancels a `CancellationToken`, the stand-in for task cancellation). Extra, not ported: 3 in `system_process_runner_tests.rs` (`cancellation_applies_to_run_too`, `already_cancelled_token_returns_cancelled`, `dropping_the_future_still_terminates_the_child`) and `process_runner_tests.rs` (3: default `run_streaming`, fake, signal exit code). |
| `WholesaleProgressParserTests.swift` | 8 | `wholesale_progress_parser_tests.rs` | 8 |  |
| `YtDlpJSONTests.swift` | 8 | `yt_dlp_json_tests.rs` | 9 | 8 ported + 1 added test pinning `JSONDecoder` tolerance (float-for-int, null, unknown keys, type mismatches). |
| `YtDlpResolverTests.swift` | 27 | `yt_dlp_resolver_tests.rs` | 28 | +1: `refresh_surfaces_cancellation_from_the_runner` (cancellation token seam). |
| `YtDlpSmokeTests.swift` | 1 | `yt_dlp_smoke_tests.rs` | 1 | Skips with `eprintln!` when yt-dlp is not installed. |
| `YtDlpWholesaleDownloaderTests.swift` | 5 | `yt_dlp_wholesale_downloader_tests.rs` | 6 | +1: `runner_cancellation_and_timeout_map_to_wholesale_errors`. |
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
