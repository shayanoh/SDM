# Tauri link grabber and platform tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Linkgrabber placeholder with a SwiftUI-identical link grabber in the Tauri app, and provision `ffmpeg`/`ffprobe`/`qjs` on Windows and Linux.

**Architecture:** `sdm-app` gains a `GrabberController` (native `sdm_grabber::GrabberSession`) whose view rides the existing patch stream as `Patch.grabber`; all hand-off logic is in Rust. `sdm-resolve`'s `ManagedBinaries` gains a pinned, checksum-verified platform-tool step. The Svelte frontend renders the view and sends ids.

**Tech Stack:** Rust (tokio, reqwest, `zip`, `tar`, an xz decoder), Tauri v2 + tauri-specta, Svelte 5, TypeScript, Vitest, Playwright, pnpm.

**Spec:** [docs/superpowers/specs/2026-10-24-tauri-link-grabber-design.md](../specs/2026-10-24-tauri-link-grabber-design.md). Read it and [spec 2](../specs/2026-10-23-tauri-main-window-design.md) first; this plan only says what they leave open.

## Global Constraints

- Execution is inline on branch `tauri-ui`, commit after every task (Shayan's blanket consent for commits and fast-forward pushes; no force-push).
- Packages are added only with `cargo add` / `pnpm add` (never by hand-editing manifests or lockfiles). Format Rust with `cargo fmt`, TypeScript/Svelte with `prettier -w`.
- No test touches the network or a real clock. Real-download tests are `#[ignore]`d and run by hand.
- Colors come from `var(--role)` CSS variables, never literals. Icons are Lucide (spec 2 section 6.1).
- All grabber logic lives in `sdm-app` (T6); the frontend holds none. Byte counts are `f64`; ids are lowercase hyphenated UUID strings; every `f64` reaches TypeScript as `number | null`.
- x64 only: Windows `x86_64-pc-windows-msvc`, Linux `x86_64`. Pinned URLs only, never `latest`; SHA-256 verified before extraction.
- Do not touch `legacy/SDMKitSwift`, the SwiftUI app or `sdm-ffi` (beyond keeping them building).
- `bindings.ts` is generated (`cargo test -p sdm-tauri export_bindings`), excluded from prettier, never hand-edited.
- Grabber defaults (spec G8): deep sniff on, auto-clear 30 min, default quality preferences, no cookie source, 50 playlist videos, no extra yt-dlp args, probes 32 global / 8 per host, 4 resolves.

## Review Focus

1. A pasted blob with no links, only duplicates, or thousands of links: no panic, no empty hand-off, view stays responsive (Task 5 tests).
2. Hand-off when the engine rejects or the package name collides with an existing one: rows are removed only after the engine accepted (Task 6).
3. A corrupt, truncated or wrong-checksum tool archive, or a zip/tar member with `../` in its path: nothing is written outside `bin/` (Task 2).
4. A format option id that is stale (row re-resolved or removed between render and click): no-op with a clear error, no panic (Task 6).
5. A removed id, a malformed id or an empty id list in any grabber command: clean `AppError`, view unchanged (Task 5).

---

## Phase T: platform tools (`sdm-core/crates/sdm-resolve`)

### Task 1: Name the tool sources (built as a pinned table, then changed to latest-only on 2026-10-24)

**Files:**
- Create: `sdm-core/crates/sdm-resolve/src/platform_tools.rs` (table and doc comment only in this task)
- Modify: `sdm-core/crates/sdm-resolve/src/lib.rs` (`mod platform_tools;` + re-export)

**Interfaces:**
- Produces:
```rust
pub enum ArchiveKind { Raw, Zip, TarXz }
pub struct PlatformTool { pub name: &'static str, pub version: &'static str,
  pub url: &'static str, pub sha256: &'static str, pub archive: ArchiveKind,
  pub members: &'static [(&'static str /*path in archive*/, &'static str /*file name in bin/*/)] }
/// The tools for the current target; empty on unsupported targets.
pub fn platform_tools() -> &'static [PlatformTool];
```

- [ ] **Step 1:** Find current stable assets. BtbN: `gh release list -R BtbN/FFmpeg-Builds`, pick a **dated autobuild tag** (not `latest`) with `ffmpeg-n*-win64-gpl-*.zip` and `ffmpeg-n*-linux64-gpl-*.tar.xz` and the release's `checksums.sha256`. quickjs-ng: `gh release view -R quickjs-ng/quickjs` for the `qjs` Windows x64 and Linux x86_64 assets (single exe or archive; note which). Use Context7/the release pages; do not rely on memory.
- [ ] **Step 2:** Download each asset into the scratchpad (own empty dir each), compute `shasum -a 256`, compare with the published checksum where one exists, and list the archive (`unzip -l`, `tar -tJf`) to record the member path of `ffmpeg`, `ffprobe`, `qjs`. If BtbN has no usable pinned tag, **stop and ask Shayan** (spec 6.1 fallback).
- [ ] **Step 3:** Write the table with `cfg(all(target_os = "windows", target_arch = "x86_64"))` and `cfg(all(target_os = "linux", target_arch = "x86_64"))` variants; the doc comment records date, tag, URL, size, SHA-256 source, member paths. Other targets return `&[]`.
- [ ] **Step 4:** Test `platform_tools_are_pinned`: every entry's `url` has no `latest` segment, `sha256` is 64 lowercase hex chars, `version` non-empty, member file names are `ffmpeg`/`ffprobe`/`qjs` (+ `.exe` on Windows). Run `cargo test -p sdm-resolve platform_tools`; expect PASS.
- [ ] **Step 5:** Commit "Pin the ffmpeg, ffprobe and qjs sources for Windows and Linux".

### Task 2: Verify, extract and install a tool

**Files:**
- Create: `sdm-core/crates/sdm-resolve/src/tool_install.rs`
- Modify: `sdm-core/crates/sdm-resolve/Cargo.toml` (via `cargo add zip tar` and an xz decoder, pure Rust: try `lzma-rs`; check with Context7 that it decodes `.tar.xz` streams, else `xz2` is not allowed (needs a C lib)), `lib.rs`

**Interfaces:**
- Produces:
```rust
#[derive(Debug, thiserror::Error)] pub enum ToolInstallError { ChecksumMismatch, MemberMissing(String), Archive(String), Io(String) }
/// Verifies `bytes` against `tool.sha256`, extracts each member to `bin_dir/<file name>`
/// (temp file + atomic rename, 0o755 on unix), returns Ok only when all members landed.
pub fn install_tool(tool: &PlatformTool, bytes: &[u8], bin_dir: &Path) -> Result<(), ToolInstallError>;
```

- [ ] **Step 1:** Write failing tests with archives generated in the test (a `zip` and an in-memory `tar.xz`, tiny, with a test `PlatformTool` whose `sha256` is computed from them): `installs_members_from_zip`, `installs_members_from_tar_xz`, `raw_archive_writes_the_bytes`, `checksum_mismatch_writes_nothing` (bin dir stays empty), `missing_member_writes_nothing`, `truncated_archive_leaves_bin_clean`, `path_traversal_member_is_never_written_outside_bin` (a member named `../evil` is not extracted; only the exact configured member paths are read, by name, never by joining archive paths), `executable_bit_is_set_on_unix` (`#[cfg(unix)]`).
- [ ] **Step 2:** Run `cargo test -p sdm-resolve tool_install`; expect FAIL (not defined).
- [ ] **Step 3:** Implement `install_tool`: hash first (`sha2`, already a dependency), then read only the configured members by exact name into memory, write each via temp file in `bin_dir` then `rename`; any failure removes the temp files. Run the tests; expect PASS.
- [ ] **Step 4:** `cargo clippy -p sdm-resolve --all-targets` clean; commit "Verify and extract the pinned platform tools".

### Task 3: Provision the tools in `ManagedBinaries`

**Files:**
- Modify: `sdm-core/crates/sdm-resolve/src/managed_binaries.rs`, `managed_binaries_types.rs`, `sdm-core/crates/sdm-app/src/toolchain.rs`, `sdm-core/crates/sdm-resolve/src/testing.rs` if the fake fetcher needs a body-by-URL helper

**Interfaces:**
- Consumes: `platform_tools()`, `install_tool`, `BinaryFetching`.
- Produces:
```rust
// ManagedBinaries
pub fn with_platform_tools(self, tools: &'static [PlatformTool]) -> Self; // default platform_tools(); tests inject
pub fn tools_activity(&self) -> ToolsActivity;
pub enum ToolsActivity { Idle, Fetching(String /*tool name*/), Failed(String /*message*/) }
```
`perform_check` (and therefore `check_now`/`tick`) ends with `ensure_platform_tools()`: for each tool group (ffmpeg+ffprobe share one archive), if any member file is missing or the manifest version (`ffmpegVersion`/`qjsVersion`) differs from `tool.version`, fetch, `install_tool`, then set the manifest version and call `on_binaries_changed`. A failure sets `lastError` (`format!("fetch {name}: {error}")`) and `ToolsActivity::Failed`; success clears it.

- [ ] **Step 1:** Failing tests with `FakeBinaryFetcher` and test tools: `missing_tools_are_fetched_and_recorded`, `present_tools_with_current_version_are_not_refetched` (request count unchanged), `version_bump_refetches`, `checksum_mismatch_sets_last_error_and_leaves_bin_clean`, `fetch_failure_sets_failed_activity_and_retries_on_the_next_check`, `ffmpeg_and_ffprobe_share_one_download`, `on_binaries_changed_fires_after_a_tool_lands`, `unsupported_target_does_nothing` (empty table).
- [ ] **Step 2:** Run; expect FAIL. Implement as specified (activity kept in `State`; no clock; tick-driven retry already exists through the absent-interval).
- [ ] **Step 3:** In `toolchain.rs`, no change to the call unless the constructor needs it; `production_tests.rs` must still pass. Run `cargo test -p sdm-resolve -p sdm-app`; expect PASS.
- [ ] **Step 4:** Add two `#[ignore]`d tests `real_windows_tools_download` / `real_linux_tools_download` (cfg-gated) that fetch the real pinned artifact and run `ffmpeg -version` / `qjs --help`. Run the one for macOS-irrelevant targets by hand later (Task 16).
- [ ] **Step 5:** `scripts/check-windows.sh` and `scripts/check-linux.sh` pass; commit "Download ffmpeg, ffprobe and qjs on Windows and Linux with checksum verification".

---

## Phase A: controller (`sdm-core/crates/sdm-app`)

### Task 4: Grabber wire types and the view builder

**Files:**
- Modify: `src/protocol.rs`
- Create: `src/grabber_view.rs`, `tests/grabber_view_tests.rs` (or unit tests in the module)

**Interfaces:**
- Produces (all `#[derive(Debug, Clone, PartialEq, Serialize, Type)]`, `camelCase`; tagged enums use `#[serde(tag = "kind")]` like `ItemStateDto`):
```rust
pub struct GrabberView { pub checked_count: u32, pub total_count: u32, pub summary: GrabberSummaryDto,
  pub rows: Vec<GrabberRow>, pub packages: Vec<GrabberPackage>, pub held_back: Option<u32>, pub tools: Option<ToolsNotice> }
pub struct GrabberSummaryDto { pub online: u32, pub faulty: u32, pub offline: u32, pub failed: u32, pub is_checking: bool, pub recheckable: u32 }
pub struct GrabberPackage { pub id: String, pub name: String, pub note: Option<String>, pub row_ids: Vec<String> }
pub enum GrabberRow {
  Link { id, filename, final_url, stage: LinkStageDto, verdict: Option<VerdictDto>, size_bytes: Option<f64>, is_duplicate: bool, removable: bool },
  Media { id, title, source_url, state: MediaStateDto, state_text: Option<String>, summary: Option<String>, options: Vec<FormatOptionDto>, is_duplicate: bool, size_bytes: Option<f64> } }
pub enum LinkStageDto { Queued, Probing, Sniffing, Done }
pub enum VerdictDto { Online, Offline, CheckFailed, Faulty { reason: String } }
pub enum MediaStateDto { Resolving, Resolved, Unselected, Unsupported, NeedsYtDlp, NeedsFfmpeg, Failed }
pub struct FormatOptionDto { pub id: String, pub label: String, pub matches_preferences: bool, pub is_wholesale: bool, pub selected: bool }
pub struct ToolsNotice { pub text: String, pub is_error: bool }
// Patch gains:  pub grabber: Option<GrabberView>
pub(crate) fn build_view(snapshot: &GrabberSnapshot, prefs: &QualityPreferences, held_back: Option<u32>, tools: Option<ToolsNotice>) -> GrabberView;
pub(crate) fn summary_text(row: &MediaRow) -> Option<String>; // "Streaming · 1080p · avc1 · mp4"
```
Option id: `format!("{index}:{label hash}")` is **not** allowed (unstable); derive it from the `FormatChoice` (video format id + audio format id + container, `"v:a:ext"`; wholesale: selector string) and keep a `fn option_id(choice: &FormatChoice) -> String` shared with Task 6's lookup.

- [ ] **Step 1:** Failing tests (fixtures built with `ProbedLink::new(..).with_*` and `MediaRow::new(..).with_*`): `rows_without_a_package_come_first_links_then_media`; `package_row_ids_are_links_then_media`; `link_verdict_and_stage_map`; `removable_only_with_a_verdict`; `size_is_none_for_zero_or_missing_length`; `media_resolved_summary_matches_swiftui_text` (`Streaming · 1080p · avc1 · mp4`, `Direct · …`); `options_only_for_resolved_unselected_needs_ffmpeg`; `selected_option_is_the_rows_choice`; `failed_state_text_is_the_full_reason`; `summary_counts_and_checking_flag`.
- [ ] **Step 2:** Run; expect FAIL. Implement `build_view` using `MediaFormatMenu::options` (partition order: matching first, then non-matching, as the SwiftUI picker). Run; expect PASS.
- [ ] **Step 3:** Update every place that constructs `Patch` (`app.rs`, tests, `simulation.rs`) with `grabber: None`; `cargo test -p sdm-app` PASS; commit "Add the grabber wire types and the view builder".

### Task 5: `GrabberController`, ingest, edits and the patch

**Files:**
- Create: `src/grabber.rs`; Modify: `src/app.rs` (`Dependencies.grabber`, `App.grabber`, `publish`, `tick`), `src/lib.rs`, `src/toolchain.rs`, `tests/support/mod.rs`, create `tests/grabber_tests.rs`

**Interfaces:**
- Consumes: Task 4 types; `sdm_grabber::{GrabberSession, LinkProber, ReqwestProbeTransport, GrabberBudget}`; `ProbeTransport` (tests use `FakeProbeOrigin`).
- Produces:
```rust
pub struct GrabberDefaults;  // consts per Global Constraints
pub struct GrabberController { /* session, ffmpeg flag Arc<AtomicBool>, held_back Mutex<Option<u32>>, last_sent Mutex<Option<GrabberView>> */ }
impl GrabberController {
  pub fn new(prober: LinkProber, resolver: Option<Arc<dyn LinkResolver>>, ffmpeg_available: Arc<AtomicBool>) -> Arc<Self>;
  pub fn production(resolver: Option<Arc<dyn LinkResolver>>, ffmpeg_available: Arc<AtomicBool>) -> Arc<Self>;
  pub(crate) fn view(&self, tools: Option<ToolsNotice>) -> GrabberView;
}
// Dependencies gains: pub grabber: Option<Arc<GrabberController>>  (tests: None; harness gets a with_grabber variant)
// App methods (all async, publish afterward, ids parsed with parse_ids/parse_id):
grabber_ingest_text(&self, text: &str) -> Result<(), AppError>
grabber_remove_links(&self, ids: &[String]) / grabber_remove_package(&self, id: &str) / grabber_clear(&self)
grabber_move_link(&self, id: &str, package_id: &str) / grabber_rename_package(&self, id: &str, name: &str)
grabber_merge_packages(&self, source: &str, destination: &str) / grabber_split_package(&self, id: &str)
grabber_recheck(&self, ids: &[String]) / grabber_recheck_failed(&self) / grabber_cancel_checks(&self)
```
`publish` also builds the grabber view when a controller exists; it sets `Patch.grabber` only if it differs from `last_sent` (always on a full patch) and also sends a patch when only the grabber changed. `tick` additionally: every 60 ticks-seconds prunes with `prune_older_than(30 min)`, and after each engine snapshot with a changed URL set calls `set_known_download_urls`. Errors with no grabber configured: `AppError::failed("the link grabber is not available")`. `grabber_ingest_text` also calls `binaries.check_now(CheckReason::Launch)` in a detached task (SwiftUI `kick()`), never awaited. `ffmpeg_available` is initialised from `bin/ffmpeg` (`.exe` on Windows) being an executable file and updated by the `on_binaries_changed` hook (wired in `toolchain.rs`).

- [ ] **Step 1:** Failing tests in `tests/grabber_tests.rs` using the fake probe origin and a harness that attaches a grabber: `ingest_text_adds_rows_that_probe_to_online`; `a_blob_without_links_adds_nothing_and_does_not_error`; `the_same_link_twice_is_one_row`; `idle_grabber_sends_no_grabber_field` (tick 5 times, no patch carries `grabber`); `a_changed_row_sends_the_view`; `full_patch_always_carries_the_view`; `remove_links_with_a_malformed_id_errors_and_changes_nothing`; `empty_id_list_is_a_no_op`; `rename_merge_split_move_clear_work` (one test each); `cancel_checks_stops_probing`; `recheck_failed_retries_check_failed_rows`; `known_downloads_mark_duplicates` (add the same URL through `add_url`, tick, the grabber row is `isDuplicate`); `prune_removes_rows_older_than_thirty_minutes` (the controller takes an injectable `Clock`, as `GrabberSession::builder().clock` does; advance it past 30 min and tick 60 times); `thousands_of_links_ingest_without_blocking` (2,000 URLs against the fake origin, the tick returns).
- [ ] **Step 2:** Run `cargo test -p sdm-app grabber`; expect FAIL. Implement. Run; expect PASS; `cargo test -p sdm-app` whole crate PASS.
- [ ] **Step 3:** Wire `production_dependencies` (the same resolver, `ReqwestProbeTransport`, availability flag shared with the `on_binaries_changed` hook). `cargo clippy -p sdm-app --all-targets`; commit "Add the grabber controller and publish its view on the patch stream".

### Task 6: Hand-off, format choice, copy URL

**Files:** Modify `src/grabber.rs`, `src/app.rs`, `src/protocol.rs`; Test `tests/grabber_tests.rs`

**Interfaces:**
- Produces:
```rust
#[derive(Deserialize, Type)] #[serde(tag = "kind", rename_all = "camelCase")]
pub enum HandoffTarget { Rows { ids: Vec<String> }, Package { id: String }, All }
App::grabber_add_to_downloads(&self, target: HandoffTarget, start: bool) -> Result<(), AppError>
App::grabber_set_format(&self, id: &str, option_id: &str) -> Result<(), AppError>
App::grabber_copy_url(&self, id: &str) -> Result<String, AppError>
```
`grabber_add_to_downloads`: resolve all groups and handoff ids **first** (`MediaHandoff::build`), then for each group build a `DownloadPackage::new(name).with_note?`-equivalent with items staged `state = if start { Queued } else { Stopped }`, `is_enabled = true` (SwiftUI `addItems`), call `add_package`, and only after it returns remove that group's handed-off rows. Rows target: group addable rows by their package name (standalone rows each under their own filename); non-addable ids are dropped. `held_back` = number of media rows without a usable format among the targeted packages (`Package`/`All`); cleared when zero.

- [ ] **Step 1:** Failing tests: `offline_and_check_failed_links_are_not_handed_off`; `online_and_faulty_links_are_handed_off`; `a_media_row_is_handed_off_only_when_resolved`; `held_back_rows_stay_and_report_the_count`; `held_back_message_clears_on_the_next_clean_handoff`; `add_all_removes_nothing_before_every_package_is_resolved` (two packages, both handed off, none lost to reclustering); `add_and_start_queues_items_and_add_leaves_them_stopped`; `rows_are_removed_only_after_the_engine_accepted` (engine add path forced to fail via a failing test hook or a rejected empty package: rows remain); `standalone_rows_go_under_their_own_filename`; `set_format_applies_the_option` / `set_format_with_a_stale_option_id_errors_and_changes_nothing`; `copy_url_returns_final_url_for_links_and_source_for_media`; `selection_with_nothing_addable_does_nothing`.
- [ ] **Step 2:** Run; expect FAIL. Implement (`option_id` from Task 4 does the lookup). Run; PASS; commit "Hand grabber rows off to the engine and pick formats in the controller".

### Task 7: Tauri commands, bindings, simulation fixture

**Files:** Modify `sdm-tauri/src-tauri/src/commands.rs`, `lib.rs` (`collect_commands!`), `sdm-core/crates/sdm-app/src/simulation.rs`; regenerate `sdm-tauri/web/src/bindings.ts`; `sdm-tauri/web/src/lib/bridge.ts`, `tauriBridge.ts`

**Interfaces:**
- Consumes: Task 5/6 `App` methods. Produces one `#[tauri::command] #[specta::specta]` per method, named `grabber_*` (camelCase in TS: `grabberIngestText`, …), each `app.guarded(...)`.
- Bridge gains (exact): `grabberIngestText(text: string)`, `grabberRemoveLinks(ids: string[])`, `grabberRemovePackage(id)`, `grabberClear()`, `grabberMoveLink(id, packageId)`, `grabberRenamePackage(id, name)`, `grabberMergePackages(source, destination)`, `grabberSplitPackage(id)`, `grabberRecheck(ids)`, `grabberRecheckFailed()`, `grabberCancelChecks()`, `grabberSetFormat(id, optionId)`, `grabberCopyUrl(id): Promise<string>`, `grabberAddToDownloads(target: HandoffTarget, start: boolean)`.

- [ ] **Step 1:** Add the commands and register them. `simulation.rs`: `simulated_dependencies` gets a grabber with a fake probe origin and ~10 pre-ingested links, so `pnpm tauri dev -- --simulate` shows a populated grabber (behind the `simulate` feature only).
- [ ] **Step 2:** `cargo test -p sdm-tauri export_bindings` regenerates `bindings.ts`; add the `Bridge` methods and `TauriBridge` implementations; `cd sdm-tauri/web && pnpm exec tsc --noEmit` fails only on `MockBridge` (next task).
- [ ] **Step 3:** `scripts/check-windows.sh` and `scripts/check-linux.sh`; commit "Expose the grabber to the Tauri frontend".

---

## Phase B: the view (`sdm-tauri/web`)

### Task 8: Store, `MockBridge`, pure row logic

**Files:** Modify `src/lib/store.svelte.ts`, `mockBridge.ts`, `mockBridge.test.ts`, `store.test.ts`; Create `src/lib/grabberRows.ts`, `grabberRows.test.ts`

**Interfaces:**
- Produces (`grabberRows.ts`): `verdictText(row): {text: string, role: "online"|"offline"|"failed"|"faulty"|"secondary"}`, `stageLabel(stage): string`, `mediaBadge(row): {text: string, role: Role, spinner: boolean, title?: string}` (first line of a failure, full text as `title`), `pickerLabel(row): {text, role}`, `sizeText(bytes: number | null): string` (binary units, `""` for null/0), `splitOptions(options): {matching, nonMatching}`, `rowsByPackage(view): {loose: GrabberRow[], packages: {pkg, rows}[]}`, `heldBackText(n)` (`"N item(s) held back — choose a format first"`). Store: `store.grabber: GrabberView` replaced wholesale by `patch.grabber` when present, empty view initially.
- `MockBridge` grabber: ~10 links, two packages, one resolving, one resolved media row with two options, one needs-ffmpeg and one failed; ingesting text adds queued links that advance queued → probing → sniffing → done per simulated tick; follows the Task 6 rules (addable, held back, rows removed after hand-off, `grabberCopyUrl`).

- [ ] **Step 1:** Failing tests for every `grabberRows` function with fixture tables (texts exactly as in the SwiftUI source: `online`, `offline`, `check failed`, faulty reason, `queued|probing|sniffing|done`, `resolving`, `not supported yet`, `requires yt-dlp`, `requires ffmpeg`, `choose a format`, `Direct · 720p · avc1 · mp4`; `1.5 GiB`-style sizes) and for the mock's rules; one store test that a patch with `grabber` replaces it and one without leaves it.
- [ ] **Step 2:** Run `pnpm --dir sdm-tauri/web exec vitest run`; expect FAIL. Implement; PASS; `tsc --noEmit` clean; prettier; commit "Add grabber state, the mock grabber and the row text logic".

### Task 9: Header, stat pills, tools notice

**Files:** Create `src/components/GrabberHeader.svelte` (+ `StatPill.svelte`), test `GrabberHeader.test.ts`

**Interfaces:** `GrabberHeader` props: `view: GrabberView`, callbacks `onAddAll(start: boolean)`, `onCancel()`, `onRecheckAll()`, `onClear()`. Layout per spec 5.1 (progress bar; caption; four pills in `--online/--faulty/--offline/--failed` roles on `--surface-tertiary` capsules; buttons small; Cancel Checks replaces Recheck All while checking; held-back and tools lines below).

- [ ] **Step 1:** Component tests: counts text `3 / 10 checked`; pill labels and counts; Add-all buttons disabled with no packages; Cancel shown only when `isChecking`, Recheck All disabled at `recheckable == 0`; Clear disabled with no packages; held-back line shows `heldBackText`; tools notice shown with the error style when `isError`.
- [ ] **Step 2:** Run; FAIL; implement; PASS; commit "Add the grabber header".

### Task 10: Rows and the format picker

**Files:** Create `src/components/LinkRow.svelte`, `MediaRow.svelte`, `FormatPicker.svelte`, tests

**Interfaces:** `LinkRow` props `{row, onRemove}`; `MediaRow` props `{row, onPick(optionId)}`; `FormatPicker` props `{options, label, role, onPick}` rendering a borderless menu button (uses the existing `ContextMenu`/popup primitive from spec 2) with matching options, a divider, then non-matching prefixed `⚠ `. Tooltips: link → final URL, media → source URL, failure → full text. Spinner for no verdict / resolving.

- [ ] **Step 1:** Component tests for each row state in `grabberRows` (every verdict, every media state), duplicate tag, trash only when `removable`, picker opens, lists matching then divider then `⚠` items, calls `onPick` with the option id, is a plain badge for states without options.
- [ ] **Step 2:** FAIL → implement → PASS; commit "Add the grabber rows and format picker".

### Task 11: Package header, windowed list, selection

**Files:** Create `src/components/GrabberList.svelte`, `GrabberPackageHeader.svelte`, test; reuse `lib/windowing`, `lib/selection`

**Interfaces:** `GrabberList` props `{view, selection, onSelect, onContext(event, ids|packageId), onAddPackage(id, start), onRemovePackage(id), onMoveLink(linkId, packageId)}`. Rows with no package first (links, then media), then each package header (title3 bold name, spacer, Add to downloads, Add and start, trash; `--surface-secondary` background) followed by its rows. Two row heights, windowed; selection via spec 2's pure `selection` module; dragging a row onto a package header calls `onMoveLink` (uses `dragdrop` helpers).

- [ ] **Step 1:** Tests: ordering (loose, then each package); only the visible window renders for 2,000 rows; click / Ctrl-click / Shift-click selection through the shared module; header buttons call their callbacks; drop of a row on a header calls `onMoveLink`.
- [ ] **Step 2:** FAIL → implement → PASS; commit "Add the windowed grabber list".

### Task 12: Menus, Add Links sheet, rename prompt, drop ingest

**Files:** Create `src/lib/grabberMenu.ts` (+ test), `components/AddLinksSheet.svelte`, `RenamePackagePrompt.svelte`; reuse `Modal.svelte`, `ContextMenu.svelte`, `ui.svelte.ts`

**Interfaces:** `grabberRowMenu(view, ids): MenuEntry[]` and `grabberPackageMenu(view, pkg): MenuEntry[]` as pure functions: row menu = Recheck; Copy URL (disabled unless exactly one id); divider; Add to Downloads; Add and Start (both disabled when no id is addable; addable computed from the view: online/faulty link or resolved media row); divider; Remove (destructive). Package menu = Rename…; Merge into ▸ (other packages, or a disabled "No Other Packages"); Split. `AddLinksSheet`: title "Add links", monospaced textarea (min 420 × 200), Cancel, **Add** (default action via Enter-with-modifier as in Modal; disabled for blank text); on Add closes first, then calls `onAdd(text)`.

- [ ] **Step 1:** Failing tests for both menu builders (fixture table: single offline link, mixed selection, one package only, several packages) and component tests for the sheet (Add disabled for whitespace, closes then ingests, Cancel does nothing) and the rename prompt (prefilled with the current name, empty name rejected).
- [ ] **Step 2:** FAIL → implement → PASS; commit "Add the grabber menus and the Add Links sheet".

### Task 13: Wire the Linkgrabber view into the shell

**Files:** Create `src/components/LinkgrabberView.svelte`; Modify `Shell.svelte`, `Toolbar` (Add Links action in this view only), `TemporaryTools.svelte`, `src/lib/shortcuts.ts` if needed; delete `LinkgrabberPlaceholder.svelte`

**Interfaces:** `LinkgrabberView` composes header + list + sheets + menus and talks only to `useBridge()` / `useStore()`. Drop of text or URL anywhere on the view calls `grabberIngestText`. Add-to-downloads and Remove clear removed ids from the selection. Errors go through `ui.report(errorMessage(e))`. `TemporaryTools` loses the Add form, keeps the download-folder field and a tools line (yt-dlp, ffmpeg, qjs versions), is shown only in the empty state, and carries `TODO(spec 4)`.

- [ ] **Step 1:** Component test with the `MockBridge`: select the Linkgrabber sidebar row → view shows; open Add Links, paste two URLs, Add → rows appear and progress; pick a format; Add and Start one package → rows leave the grabber and the item appears in the download list; Remove selection; Recheck All; Cancel Checks.
- [ ] **Step 2:** FAIL → implement → PASS. `pnpm exec vitest run`, `tsc --noEmit`, `prettier --check`.
- [ ] **Step 3:** Commit "Replace the Linkgrabber placeholder with the grabber view".

### Task 14: Playwright flows and parity screenshots

**Files:** Create `sdm-tauri/web/e2e/grabber.spec.ts`, `parity` additions; Create `docs/superpowers/specs/assets/2026-10-24-swiftui-link-grabber/` (references)

- [ ] **Step 1:** Playwright (mock): paste links via the sheet and watch them go queued → done; verdict pills update; pick a format and see the row label change; Add all and start moves rows to the main list; drag a row onto another package header; drop text onto the view. Run `pnpm exec playwright test grabber`; PASS.
- [ ] **Step 2:** Produce SwiftUI references: use the fixture approach from spec 2 section 11 (throttled fake origin; links in all verdicts; three media rows) and screenshot the real SwiftUI Linkgrabber into the assets folder (Playwright can't drive it: use the computer-use or screencapture route as for spec 2). Add `parity` screenshots of ours into `web/test-results/parity/` and compare by eye; fix differences in spacing and colors.
- [ ] **Step 3:** Commit "Check the grabber against the SwiftUI view".

### Task 15: Performance check

- [ ] **Step 1:** With the `simulate` release build, ingest 2,000 links through the fake origin; record idle CPU with the grabber view open and a hundred rows probing, in `docs/tauri-performance.md`. If the patch with the full view is heavy (> few ms to serialize, or visible lag), implement the row-level diff fallback (spec risk table) before closing. Commit the table.

### Task 16: Closing

**Files:** Modify `docs/tauri-roadmap.md`, `docs/superpowers/specs/2026-10-24-tauri-link-grabber-design.md` (status), memory `sdm-tauri-ui.md`, `todo.md` if it lists task 16

- [ ] **Step 1:** Run everything: `./test.sh` (Rust, Swift, web), `scripts/check-windows.sh`, `scripts/check-linux.sh`, `scripts/build-tauri.sh mac`, `windows`, `linux` (Docker must be running; if it is not, say so in the log). Run the `#[ignore]`d real-tool tests that match this machine's target and record the result.
- [ ] **Step 2:** Roadmap: mark spec 3 built/in-progress with exactly what is unverified (Windows VM, Linux, real downloads), mark spec 1 task 16 done, add the log entry, and add the spec 4 deferrals (spec section 9) to spec 4's row. Update the memory file the same way.
- [ ] **Step 3:** Commit "Close spec 3: roadmap, log and notes". Report to Shayan: what ran, what is unverified, and the by-hand checks from spec section 8.
