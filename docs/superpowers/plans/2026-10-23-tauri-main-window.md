# Tauri main window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> Execution method chosen by Shayan: **in-line, in this session, on branch `tauri-ui`** (executing-plans). Commit after every task (blanket consent in CLAUDE.md, fast-forward pushes only, no push unless asked).

**Goal:** Build the Tauri main window so it looks and behaves like the SwiftUI main window (spec 2), in two phases: controller actions in `sdm-app`, then the Svelte window.

**Architecture:** Phase A adds list-based actions, organize actions, deletion preview and open/reveal to `sdm_app::App`, plus three protocol fields, and mirrors every call through the Tauri commands, `Bridge`, `TauriBridge` and `MockBridge`. Phase B adds pure TypeScript modules (format, eligibility, selection, drag math, windowing) first, then Svelte components on top of the existing store and patch stream.

**Tech Stack:** Rust (sdm-app, tokio, specta), Tauri v2, Svelte 5, TypeScript, Vite, pnpm, Vitest, Playwright, Lucide icons.

**Spec:** [docs/superpowers/specs/2026-10-23-tauri-main-window-design.md](../specs/2026-10-23-tauri-main-window-design.md). Reference screenshots: `docs/superpowers/specs/assets/2026-10-23-swiftui-main-window/`.

## Global Constraints

- Every color is `var(--role)`; no color literal in a component or stylesheet. Roles available: `--surface-primary|secondary|tertiary`, `--text-primary|secondary|tertiary`, `--accent`, `--border`, `--status-online|faulty|offline|failed`, `--progress-fill`, `--completed-segment-fill`, `--active-head-tint`, `--graph-stroke`, `--graph-average-stroke`, `--sidebar-background`, `--selection-tint`.
- The frontend holds no business logic (T6): eligibility is one pure module with a fixture table; engine and disk work go through the controller.
- No test touches the network or a real clock (spec 1 rule). Rust tests use `tests/support` (`harness`, `tick_until`, `payload`, `FakeOrigin`).
- Format TypeScript with `prettier -w`, Rust with `cargo fmt`. Add packages only with `pnpm add` / `cargo add` (never hand-edit manifests or lockfiles). Verify any library API against current docs (Context7) before using it.
- `bindings.ts` is generated: `cargo test -p sdm-tauri export_bindings` (it is excluded from prettier). Never edit it by hand.
- Item ids and package ids are lowercase hyphenated UUID strings; byte counts are `f64` (`null` on the wire means non-finite and reads as 0).
- Windows and Linux compile gates must stay green: `scripts/check-windows.sh`, `scripts/check-linux.sh`. Run the Rust tests from `sdm-core/` (`cargo test -p sdm-app`) and the frontend tests from `sdm-tauri/web/` (`pnpm test`, `pnpm check`).
- The SwiftUI app and `sdm-ffi` are not touched. `legacy/SDMKitSwift` is frozen.
- Copy and sizes come from the spec, sections 5.2 and 5.3, and the screenshots.

## Review Focus

1. An action fired with an empty effective set (nothing selected, or a package with no items, or only ineligible items): the menu entry is disabled and **no bridge call** is made. (Task B8.)
2. A drag payload mixing items and packages, or dropping a selection on one of its own members: refused, nothing moves. (Tasks A3, B2, B11.)
3. Removing or moving an item while it is selected and its details panel is open: the selection is pruned on the next structure patch, the panel falls back to its placeholder, and `setDetailItems` is updated. (Task B5.)
4. Zero or unknown sizes: `totalBytes` null or 0 never divides by zero, and renders `Zero KB`, 0.0 % and an empty bar. Very long and right-to-left file names truncate inside the row. (Tasks B1, B4, B7.)
5. A 5,000-item list: only the visible rows exist in the DOM, Ctrl/Cmd+A still selects every item, and scrolling to the end renders the last row. (Tasks B3, B7, B12.)

---

# Phase A: controller (`sdm-core/crates/sdm-app`, `sdm-tauri/src-tauri`, bridges)

### Task A1: Protocol additions

**Files:**
- Modify: `sdm-core/crates/sdm-app/src/protocol.rs`, `convert.rs`, `differ.rs`, `app.rs`
- Test: `sdm-core/crates/sdm-app/src/differ.rs` (unit tests module), `sdm-core/crates/sdm-app/tests/app_tests.rs`

**Interfaces:**
- Produces: `protocol::AssemblyDto { None, Mux }` (`#[serde(rename_all = "camelCase")]`, `Type`); `StructurePackage.folder: String`; `StructureItem.assembly: AssemblyDto`; `ItemFigures.remaining_attempts: Option<u32>`; `Differ::set_download_folder(&mut self, folder: PathBuf) -> bool` (true when it changed); `convert::structure_package(package: &PackageSnapshot, folder: &Path) -> StructurePackage`.

- [ ] **Step 1: Write failing tests.** In `differ.rs` tests: `structure_carries_folder_and_assembly` (one package named `Pack`, folder `/dl`, item with `Assembly::Mux`; assert `structure.packages[0].folder == "/dl/Pack"` (use `Path::join` for the expectation) and `items[0].assembly == AssemblyDto::Mux`); `changing_the_download_folder_forces_a_structure_patch`. In `app_tests.rs`: `package_folder_follows_the_download_folder_setting` (add a URL, read the first structure, call `update_settings` with a new absolute folder, tick, assert the new structure's `folder` starts with the new path).
- [ ] **Step 2: Run** `cargo test -p sdm-app` from `sdm-core/`; expect compile failures for the missing items.
- [ ] **Step 3: Implement** the fields in `protocol.rs`; fill them in `convert.rs` (`assembly` from `ItemSnapshot.assembly`, `remaining_attempts` mapped through `count`). `Differ` stores `download_folder: PathBuf`; `App::publish` calls `state.differ.set_download_folder(settings.download_folder())` before diffing and, when it returns true, treats the publish as a full one (`need_full = true`).
- [ ] **Step 4: Run** `cargo test -p sdm-app`; expect all pass. Run `cargo test -p sdm-tauri export_bindings` to regenerate `sdm-tauri/web/src/bindings.ts`; run `pnpm check` in `sdm-tauri/web` (the mock bridge and store will fail to type-check until A6/B-tasks add the new fields; fix `mockBridge.ts`'s `Structure`/`ItemFigures` literals here with the new fields set to `"none"`, `/dl/<name>` and `null`).
- [ ] **Step 5: Commit** `git add -A sdm-core sdm-tauri && git commit -m "Add package folder, item assembly and remaining attempts to the protocol"`.

### Task A2: List actions on `App`

**Files:**
- Modify: `sdm-core/crates/sdm-app/src/app.rs`, `sdm-core/crates/sdm-app/tests/app_tests.rs` (migrate the existing calls)
- Create: `sdm-core/crates/sdm-app/tests/actions_tests.rs`

**Interfaces:**
- Produces (all `pub async fn … -> Result<(), AppError>` on `App`): `start_items(&self, ids: &[String])`, `stop_items`, `retry_items`, `retry_mux_items`, `reset_items`, `set_items_enabled(&self, ids: &[String], enabled: bool)`, `remove_items(&self, ids: &[String], delete_files: bool)`. A private `parse_ids(&[String]) -> Result<Vec<Uuid>, AppError>` rejects the whole call on the first bad id and de-duplicates while keeping order. `start_item`, `stop_item`, `retry_item`, `remove_item` are deleted.

- [ ] **Step 1: Write failing tests** in `actions_tests.rs` (each `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`, using `harness`): `stop_items_then_start_items_round_trips_state` (add two URLs, `stop_items` both, assert both `Stopped` in the last structure, `start_items`, tick until `Running` or `Completed`); `a_bad_id_rejects_the_whole_batch` (valid id + `"nope"` → `Err(AppError::InvalidArgument{..})` and the valid item unchanged); `set_items_enabled_false_marks_items_disabled` (structure `is_enabled == false`); `remove_items_without_files_keeps_the_file_on_disk` and `remove_items_with_files_trashes_it` (complete a 3000-byte download, remove, assert file presence/absence in `h.downloads`); `reset_items_skips_a_running_item` (running item untouched, a stopped one reset, call returns `Ok`); `retry_items_restarts_a_failed_item` (FakeOrigin that fails: use `FakeOrigin::new(..).with_status(404)` if it exists, else make the origin return an error via its existing "misbehave" switches; check `sdm-engine/src/fake_origin.rs`); `retry_mux_items_is_a_no_op_for_a_non_mux_item`; `empty_id_list_is_ok_and_publishes_nothing`.
- [ ] **Step 2: Run** `cargo test -p sdm-app --test actions_tests`; expect compile failure.
- [ ] **Step 3: Implement** the methods: parse, loop over the engine call, remove from `detail_items` for removals, one `publish(false)` at the end. `reset_items` filters out ids whose snapshot state is `Running` using `self.engine.snapshot().await` first.
- [ ] **Step 4: Migrate** `app_tests.rs`, `tests/production_tests.rs`, `simulation.rs` and any other caller of the removed methods (`grep -rn "start_item\|stop_item\|retry_item\|remove_item" sdm-core sdm-tauri`), keeping tests meaningful. Run `cargo test -p sdm-app`; expect all pass.
- [ ] **Step 5: Commit** `git commit -am "Replace single-item actions with list-based ones on App"` (plus new files).

### Task A3: Organize actions

**Files:** Modify `app.rs`; Test `tests/actions_tests.rs`.

**Interfaces:**
- Produces: `remove_package(&self, id: &str, delete_files: bool)`, `move_items(&self, ids: &[String], package_id: &str, at_index: Option<u32>)`, `move_items_to_new_package(&self, ids: &[String], name: &str)` (trims; empty after trim → `AppError::invalid("name", "the package name is empty")`), `reorder_items(&self, ids: &[String], package_id: &str)`, `reorder_packages(&self, ids: &[String])`. Engine calls: `remove_package`, `move_items_to_package(ids, package, at_index)` (check its exact signature in `sdm-engine/src/engine/organize.rs:435`), `move_items_to_new_package`, `reorder_items`, `reorder_packages`.

- [ ] **Step 1: Write failing tests**: `move_items_to_new_package_creates_it_and_moves_the_items`; `move_items_into_another_package_at_an_index` (two packages of queued/stopped items; stop everything first because the engine only moves non-running items; assert the resulting order in the structure); `a_blank_package_name_is_rejected`; `reorder_items_sets_the_order_in_the_structure`; `reorder_packages_sets_the_order`; `remove_package_removes_all_its_items`; `moving_a_running_item_is_refused_without_error` (the engine ignores it; structure unchanged).
- [ ] **Step 2: Run** `cargo test -p sdm-app --test actions_tests`; expect failures.
- [ ] **Step 3: Implement** with the same parse/publish shape as A2.
- [ ] **Step 4: Run** all `sdm-app` tests; expect pass.
- [ ] **Step 5: Commit** `git commit -am "Add package and ordering actions to App"`.

### Task A4: `describe_deletion`

**Files:** Create `sdm-core/crates/sdm-app/src/deletion.rs`; modify `lib.rs`, `app.rs`, `protocol.rs`; Test `tests/deletion_tests.rs`.

**Interfaces:**
- Produces: `protocol::DeletionTarget` (`#[serde(tag = "kind", rename_all = "camelCase")]`, `Deserialize`, `Type`): `Items { ids: Vec<String> }` | `Package { id: String }`; `protocol::DeletionInfo { title: String, total_bytes: f64, names: Vec<String> }` (`Serialize`, `Type`); `App::describe_deletion(&self, target: &DeletionTarget) -> Result<DeletionInfo, AppError>`; `deletion::describe(folder: &Path, files: &[(String, Vec<String>)]) -> (f64, Vec<String>)`, a pure helper over `(output_filename, part_filenames)` per item that sums on-disk sizes of each name and its `.incomplete` sibling inside `folder`, returning sorted names.

- [ ] **Step 1: Write failing tests** (`deletion_tests.rs`, temp dir): `sums_actual_sizes_of_output_and_incomplete_files` (a 100-byte `a.bin` and a 40-byte `a.bin.incomplete` → total 140, names sorted `["a.bin", "a.bin.incomplete"]`); `part_files_are_included`; `missing_files_are_skipped`; `no_files_means_remove_from_list_title` (via the `App`, item never downloaded → title `"Remove From List?"`, names empty); `with_files_the_title_names_the_deletion` (`"Remove From List And Delete Files?"`); `unknown_package_returns_the_package_title` (`"Delete Package From Disk?"`, 0 bytes); `a_bad_id_is_an_error`.
- [ ] **Step 2: Run** `cargo test -p sdm-app --test deletion_tests`; expect failure.
- [ ] **Step 3: Implement** `describe` and `App::describe_deletion` (uses `engine.snapshot()` for `filename` and `part_filenames`, and `settings.download_folder().join(package.name)`).
- [ ] **Step 4: Run** the tests; expect pass.
- [ ] **Step 5: Commit** `git commit -am "Add describe_deletion to the controller"`.

### Task A5: Open and reveal

**Files:** Modify `host.rs`, `app.rs`; Test `tests/actions_tests.rs`; Modify `sdm-tauri/src-tauri/src/host.rs`.

**Interfaces:**
- Produces: `Host::open_path(&self, path: &Path) -> Result<(), HostError>` (trait method; `FakeHost` keeps `opened_paths()` and `revealed_paths()` getters, and `reveal_in_file_manager` now records too); `App::open_item(&self, id: &str)`, `reveal_item(&self, id: &str)`, `reveal_package(&self, id: &str)`. Item file path = `download_folder/<package name>/<output filename>`; package folder = `download_folder/<package name>`.

- [ ] **Step 1: Write failing tests**: `open_item_opens_a_completed_file` (complete a download, `open_item`, `h.host.opened_paths()` equals the file path); `open_item_is_a_no_op_for_an_unfinished_item`; `open_item_is_a_no_op_when_the_file_is_missing` (delete the file, tick so `file_missing` becomes true, then call); `reveal_item_reveals_the_file`; `reveal_package_reveals_the_folder`; `unknown_ids_are_errors`.
- [ ] **Step 2: Run**; expect failure.
- [ ] **Step 3: Implement.** `TauriHost::open_path` uses `tauri_plugin_opener::open_path(path, None::<&str>)` (confirm the signature in the plugin docs before coding; add `opener:allow-open-path` to `capabilities/default.json` if the plugin needs it for Rust-side calls, else leave it).
- [ ] **Step 4: Run** `cargo test -p sdm-app`, `scripts/check-windows.sh` and `scripts/check-linux.sh`; expect pass.
- [ ] **Step 5: Commit** `git commit -am "Add open and reveal actions and Host::open_path"`.

### Task A6: Commands, bridges, mock, skeleton migration

**Files:** Modify `sdm-tauri/src-tauri/src/commands.rs`, `lib.rs` (command list and the specta builder), `web/src/lib/bridge.ts`, `tauriBridge.ts`, `mockBridge.ts`, `mockBridge.test.ts`, `components/ItemRow.svelte` (skeleton), `web/e2e/mock.spec.ts`; regenerate `bindings.ts`.

**Interfaces:**
- Produces (Rust commands, snake_case, each through `app.guarded`): `start_items`, `stop_items`, `retry_items`, `retry_mux_items`, `reset_items`, `set_items_enabled(ids, enabled)`, `remove_items(ids, delete_files)`, `remove_package(id, delete_files)`, `move_items(ids, package_id, at_index: Option<u32>)`, `move_items_to_new_package(ids, name)`, `reorder_items(ids, package_id)`, `reorder_packages(ids)`, `describe_deletion(target) -> DeletionInfo`, `open_item(id)`, `reveal_item(id)`, `reveal_package(id)`. The four removed singular commands are deleted.
- Produces (TypeScript, on `Bridge`): `startItems(ids: string[])`, `stopItems`, `retryItems`, `retryMuxItems`, `resetItems`, `setItemsEnabled(ids: string[], enabled: boolean)`, `removeItems(ids: string[], deleteFiles: boolean)`, `removePackage(id: string, deleteFiles: boolean)`, `moveItems(ids: string[], packageId: string, atIndex?: number)`, `moveItemsToNewPackage(ids: string[], name: string)`, `reorderItems(ids: string[], packageId: string)`, `reorderPackages(ids: string[])`, `describeDeletion(target: DeletionTarget): Promise<DeletionInfo>`, `openItem(id)`, `revealItem(id)`, `revealPackage(id)`.
- `MockBridge` keeps packages as an ordered array of `{id, name, items[]}` (spec-1 skeleton had one package per item; the mock now seeds the five-package reference fixture: Linux ISOs, Conference Talks, Old Archive Dump, Season 2, Holiday Photos, with running/queued/stopped/disabled/failed/non-resumable/completed items and `folder` values) and implements every call above with the engine's semantics (moves refuse running items, reset skips running, remove-package, rename trimmed, `describeDeletion` returns names for completed/partial items in the fixture). It also supports `?theme=deep-purple` via `themeCss`.

- [ ] **Step 1: Write failing tests** in `mockBridge.test.ts`: one per action asserting the emitted patches (`stopItems` → `stopped`; `moveItemsToNewPackage` creates a package; `moveItems` honors `atIndex`; `reorderPackages`; `removePackage`; `describeDeletion` for a package returns the completed items' names; blank new-package name throws `BridgeError`; unknown id throws).
- [ ] **Step 2: Run** `pnpm test` in `sdm-tauri/web`; expect failure.
- [ ] **Step 3: Implement** the commands, register them, regenerate bindings (`cargo test -p sdm-tauri export_bindings`), then `Bridge`/`TauriBridge`/`MockBridge`. Migrate the skeleton `ItemRow.svelte` and `e2e/mock.spec.ts` to the new calls (the skeleton UI is replaced later, it only has to keep working).
- [ ] **Step 4: Verify**: `pnpm test`, `pnpm check`, `pnpm exec playwright test` (all in `sdm-tauri/web`), `cargo test --workspace` in `sdm-core`, `cargo build` in `sdm-tauri/src-tauri`, `scripts/check-windows.sh`, `scripts/check-linux.sh`; expect all green. This ends Phase A.
- [ ] **Step 5: Commit** `git commit -am "Expose the new controller actions through the Tauri commands and bridges"`.

---

# Phase B: window (`sdm-tauri/web`)

### Task B1: Formatting

**Files:** Modify `src/lib/format.ts`, `format.test.ts`.

**Interfaces:**
- Produces: `formatBytes(bytes: number): string`, `formatSpeed(bytesPerSecond: number): string`, `formatEta(seconds: number): string` (replacing the spec-1 versions; every caller updated).

- [ ] **Step 1: Write failing tests** with the screenshot values: `formatBytes(0) === "Zero KB"`, `512 → "512 bytes"`, `1 → "1 byte"`, `1024 → "1 KB"`, `1536 → "2 KB"` (KB has 0 digits; round half away from zero like `ByteCountFormatter`), `4 * 1024**2 → "4 MB"`, `629.4 * 1024**2 → "629.4 MB"`, `5.59 * 1024**3 → "5.59 GB"`, `12.07 * 1024**3 → "12.07 GB"`, `1.3 * 1024**3 → "1.3 GB"`; non-finite and negative → `"Zero KB"`; `formatSpeed(0) === "Zero KB/s"`, `formatSpeed(3.7 * 1024**2) === "3.7 MB/s"`. `formatEta` table from `SDM/Helpers.swift`: `0 → "Soon"` is **not** produced for ≤ 0 (the row hides the ETA when `eta <= 0`; `formatEta(0)` returns `""`), `5 → "A few seconds"`, `30 → "Less than a minute"`, `60 → "About 1 Minute"`, `125 → "About 2 Minutes"` (seconds 5 ≤ 15 dropped), `205 → "About 3 Minutes and 25 Seconds"`, `1200 → "About 20 Minutes"`, `3600 → "About 1 Hour"`, `3600+60*17 → "About 1 Hour and 15 Minutes"`, `86400 → "About 1 Day"`, `86400+3600*3 → "About 1 Day and 3 Hours"`.
- [ ] **Step 2: Run** `pnpm test`; expect failure.
- [ ] **Step 3: Implement** the three functions by porting `formatTimeIntervalForEta` and the rules in spec 5.3 "Formatting".
- [ ] **Step 4: Run** `pnpm test`; expect pass.
- [ ] **Step 5: Commit** `git commit -am "Match the SwiftUI byte, speed and ETA formatting"`.

### Task B2: Eligibility, selection and drag math

**Files:** Create `src/lib/eligibility.ts`, `selection.ts`, `dnd.ts` and their `*.test.ts`.

**Interfaces:**
- Produces in `eligibility.ts`: `type RowItem = { state: ItemStateDto; isEnabled: boolean; isResumable: boolean | null; fileMissing: boolean; assembly: "none" | "mux" }`; `canStart|canStop|canEnable|canDisable|canRetry|canRetryMux|canOpen|canReset(item: RowItem): boolean`; `needsPauseConfirmation(items: RowItem[]): boolean` (any item running with `isResumable === false`); `allDownloadableStopped(items: RowItem[]): boolean` and `downloadable(items)` (queued, running, stopped).
- Produces in `selection.ts`: `class SelectionModel` (plain TS, no Svelte) over an ordered list of row keys supplied by `setOrder(keys: string[])`: `selected: Set<string>`, `click(key)`, `toggle(key)`, `range(key)`, `selectAll(itemKeys: string[])`, `clear()`, `prune(liveKeys: Set<string>)`, `contextTarget(key): string[]` (if `key` is selected returns the whole selection in list order, else selects only `key` and returns `[key]`). Keys are the raw ids (item or package id).
- Produces in `dnd.ts`: `type DragPayload = { kind: "item" | "package"; id: string }[]`; `orderedItemIds(payload, itemOrder: string[]): string[] | null` and `orderedPackageIds(payload, packageOrder: string[]): string[] | null` (null for a mixed or empty payload); `dropIndex(rowIndex: number, offsetY: number, rowHeight: number): number` (bottom half → `rowIndex + 1`); `reorderedPackageIds(order: string[], dragged: string[], targetId: string, preferAfter: boolean): string[] | null` (null when the target is itself dragged).

- [ ] **Step 1: Write failing tests**: eligibility fixture table (every state × enabled combination for each predicate, from `PackagesListView.swift` 1219–1273, e.g. disabled+stopped cannot start, completed cannot be disabled, running cannot be reset, failed+mux can retry mux, `needsPauseConfirmation` false for `isResumable === null`); selection (`click` replaces, `toggle` adds/removes, `range` from anchor in both directions, `selectAll` ignores package keys, `prune` drops dead keys, `contextTarget` on an unselected row selects just it, on a selected row keeps the multi-selection); dnd (mixed payload → null for both, item ids returned in list order not drag order, `dropIndex(3, 10, 40) === 3`, `dropIndex(3, 30, 40) === 4`, `reorderedPackageIds(["a","b","c"], ["c"], "a", false) → ["c","a","b"]`, `preferAfter` → `["a","c","b"]`, target in dragged → null).
- [ ] **Step 2: Run** `pnpm test`; expect failure.
- [ ] **Step 3: Implement** the three modules (pure, no DOM).
- [ ] **Step 4: Run** `pnpm test`; expect pass.
- [ ] **Step 5: Commit** `git commit -am "Add eligibility, selection and drag modules"`.

### Task B3: Two-height windowing

**Files:** Modify `src/lib/windowing.ts`, `windowing.test.ts`, `layout.ts`.

**Interfaces:**
- Produces: `layout.ts` exports `ITEM_HEIGHT = 61`, `HEADER_HEIGHT = 53`, `OVERSCAN = 6` (replacing `ROW_HEIGHT`); `windowing.ts` exports `buildOffsets(heights: number[]): number[]` (prefix sums, length `n + 1`) and `visibleRange({ offsets, scrollTop, viewport, overscan }): { start: number; end: number }` (binary search; clamps an overscrolled position to the last rows; empty list → `{0,0}`; non-finite inputs → treated as 0).

- [ ] **Step 1: Write failing tests**: offsets of `[53,61,61]` are `[0,53,114,175]`; range at scrollTop 0 with viewport 100 and overscan 0 covers rows 0–1; scrollTop beyond the end returns the last rows; 5,000 rows resolve in under 5 ms (loose bound); NaN scrollTop treated as 0; overscan pads and clamps.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run**; expect pass (update the skeleton `DownloadList` call site to compile, it is replaced in B7).
- [ ] **Step 5: Commit** `git commit -am "Window the list over two row heights"`.

### Task B4: Foundations (typography, icons, graphics)

**Files:** Modify `src/theme.css`; create `src/components/Icon.svelte`, `StateIcon.svelte`, `SegmentedBar.svelte` (replaces `ProgressCanvas.svelte`), `Sparkline.svelte` (rewrite), `BandwidthGraph.svelte`, `src/lib/graphs.ts` and `graphs.test.ts`.

**Interfaces:**
- Produces in `graphs.ts`: `windowed(samples: number[], width: number): number[]` (last `width` samples, left-padded with zeros), `sparklinePoints(samples, width, height): string`, `runningAverage(values: number[], window: number): number[]` (port of `BandwidthGraph.runningAverage`, window = `TICKS_PER_SECOND * 2` = 10 samples), `unionRanges(parts: {ranges: {start:number;end:number}[]; offset: number}[]): {start:number;end:number}[]`.
- Produces components: `<Icon name={IconName} size={number} />` over the Lucide set named in spec 6.1 (choose the package after reading current docs: `@lucide/svelte` for Svelte 5; add with `pnpm add`; import icons individually so unused ones tree-shake); `<StateIcon state fraction isRetrying isAssembling />` (24 px ring: a track circle plus an arc of `fraction` for running/queued/stopped, glyph inside, color by state per `stateIconColor`, a CSS breathing animation while running, respects `prefers-reduced-motion`); `<SegmentedBar completed total />` (canvas, 6 px, fully rounded, `--surface-tertiary` at 0.6 opacity track, `--progress-fill`, min range width 0.5 px, redraws on change, handles `total` null/0); `<Sparkline samples width height />`; `<BandwidthGraph history />` (area `--graph-stroke` at 25 %, 1.5 px `--graph-average-stroke` line).
- `theme.css`: font stack `system-ui, -apple-system, "Segoe UI", sans-serif`; body 13 px; utility classes `.caption` (10 px), `.title3` (15 px), `.headline` (13 px bold), `.mono` (tabular-nums); global `user-select: none` except `.selectable`; a `[data-vibrancy]` hook left for task B14.

- [ ] **Step 1: Write failing tests** for `graphs.ts` (windowed pads and truncates; `sparklinePoints` is empty for fewer than 2 samples or a max of 0; `runningAverage` starts with zeros until the window fills then averages; `unionRanges` merges overlapping and adjacent ranges after offsetting) and a component test (`@testing-library/svelte`) that `SegmentedBar` and `Sparkline` render without throwing for `total = null`, `total = 0`, empty samples.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run** `pnpm test` and `pnpm check`; expect pass.
- [ ] **Step 5: Commit** `git commit -am "Add typography, Lucide icons and the graph components"`.

### Task B5: UI state and derived views

**Files:** Modify `src/lib/ui.svelte.ts`, `store.svelte.ts`; create `src/lib/views.ts`, `views.test.ts`, `src/lib/actions.ts` (later tasks add to it).

**Interfaces:**
- Produces in `ui.svelte.ts` (`UiState`): `tab = $state<"downloads" | "completed" | "linkgrabber">("downloads")`, `selection = new SelectionModel()` wrapped so `selection.selected` is reactive, `collapsed = $state.raw<Record<"downloads" | "completed", Set<string>>>`, `panelVisible = $state(true)`, `panelHeight = $state(150)` (clamped 100 to 600 by `setPanelHeight`), `modal = $state.raw<ModalState | null>(null)` (`{ kind: "delete"; target: DeletionTarget; info: DeletionInfo } | { kind: "pause"; ids: string[] } | { kind: "newPackage"; ids: string[] }`), `menu = $state.raw<{ x: number; y: number; targetId: string } | null>(null)`, `setTab(tab)` (clears the selection), plus the existing `error`/`report`/`dismiss`.
- Produces in `views.ts`: `type ListView = { packages: { id: string; itemIds: string[] }[] }`; `completedView(order: PackageOrder[], items: ReadonlyMap<string, ItemModel>): ListView` (items whose state is `completed`, packages without any omitted); `completedAggregate(itemIds, items): { completedBytes: number; totalBytes: number }`; `flatRows(view: ListView, collapsed: Set<string>): Row[]` where `Row = { kind: "package" | "item"; id: string; height: number }`; `selectedItemIds(selection: Set<string>, view: ListView): string[]` (package ids expand to their items, list order, de-duplicated).
- `DownloadStore` gains `pruneSelection` hook: after `#applyStructure`, `UiState.selection.prune(liveKeys)` is called by `main.ts` through a `store.onStructure` callback; the details owner id is a constant `"main-details"`.

- [ ] **Step 1: Write failing tests**: `completedView` drops packages with no completed items and non-completed items; `completedAggregate` sums only completed items; `flatRows` omits the items of a collapsed package and uses the two heights; `selectedItemIds` expands a package id and de-duplicates an item selected both ways; `setTab` clears the selection; panel height clamps at 100 and 600; pruning removes a deleted id from the selection (Review Focus 3).
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run**; expect pass.
- [ ] **Step 5: Commit** `git commit -am "Add UI state and the derived Completed view"`.

### Task B6: Shell, sidebar, overview, bottom bar

**Files:** Create `src/components/Shell.svelte`, `Sidebar.svelte`, `OverviewBlock.svelte`, `BottomBar.svelte`; modify `App.svelte` (becomes: load theme CSS, attach, `setListVisible` wiring, render `<Shell/>`), `main.ts`; delete `AddUrl.svelte`, `TotalSpeed.svelte` after moving "add URL" to a temporary dev-only entry (kept: a `?dev` query shows the old add field so the mock can still add items; remove in B15 if unused).

**Interfaces:**
- Consumes: B1 formatting, B4 components, B5 `UiState`.
- Produces: `Sidebar` (200 min / 220 ideal width, resizable divider with `mousedown` drag between 200 and 360, `--sidebar-background`, rows Downloads / Completed / Linkgrabber; Downloads shows badge `running/(running+queued)` (`--status-online` capsule, caption bold) and a 16 px ring of `overallFraction` only while `runningCount > 0`; Linkgrabber shows count `0` and selecting it renders the placeholder text "The link grabber arrives in the next release." in the detail area; the selected row has `--selection-tint` background); `OverviewBlock` per spec 5.3; `BottomBar` (`Pause All`/`Resume All` toggled by `allDownloadableStopped`, disabled when `downloadable` is empty, divider, speed `.title3 .mono`, `N packages, M items`; on Completed the button is absent and the speed is `Zero KB/s`).

- [ ] **Step 1: Write failing component tests** (`@testing-library/svelte` with a stub bridge and a populated store): sidebar badge text `"3/14"` for 3 running + 11 queued and hidden when none run; Overview shows `No running downloads` / `0 active` with no runners and the speed text with runners; bottom bar label flips to `Resume All` when every downloadable item is stopped and clicking calls `resumeAll`; disabled with no downloadable items; Completed tab hides the button.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run** `pnpm test`, `pnpm check`; then `pnpm --dir web dev` and look at the mock in a Playwright screenshot of the shell against `01-downloads-running.png` (record that you looked).
- [ ] **Step 5: Commit** `git commit -am "Add the shell, sidebar, overview block and bottom bar"`.

### Task B7: Package list, package headers and item rows

**Files:** Create `src/components/PackageList.svelte`, `PackageHeader.svelte`, `ItemRow.svelte` (rewrite), `RowSlot.svelte`; delete `DownloadList.svelte` and the old `ItemRow`.

**Interfaces:**
- Consumes: B2 `SelectionModel`, B3 windowing, B5 `flatRows`/`selectedItemIds`, B4 graphics.
- Produces: `<PackageList view allowsReordering showsPauseResume showsLiveAggregates tab />`. One windowed scroller; each visible row is a `RowSlot` (`position: absolute`, top from offsets) that owns `onclick`/`ctrl|meta`/`shift` selection, `oncontextmenu` (sets `ui.menu`), `draggable` and the drop handlers (wired in B11), `data-row-id`, and the zebra/selection background computed from the row's index within its package (items) or the package index (headers) per spec 5.2; the item `ItemRow` renders: `StateIcon`, filename (1 line, ellipsis, strikethrough when disabled, `--text-primary` or `--text-secondary`), `not resumable` badge when `isResumable === false`, `file missing` label with a warning icon when `fileMissing`, spacer, `A/C seg`, speed, `Sparkline 48×16`; progress bar 6 px; status line (`describe` ported from `ItemRowBody.describe`: first line of failure reasons, `Retrying in Ns · N attempts left — reason`, `(disabled)` suffix, failed in `--status-failed`, retrying in `--status-faulty`) with ETA (`ETA: …`, shown only while running or queued and `eta > 0`; a queued item with no speed of its own uses the engine's recent global speed: use `stats.bytesPerSecond`), `checkpointFailure` line in `--status-faulty`, bytes text `DONE / TOTAL`; whole row at 0.55 opacity when disabled. `PackageHeader`: disclosure chevron (collapses via `ui.collapsed`), name `.title3` bold, `DONE / TOTAL` caption with `ETA: …` when the package ETA > 0, right side speed and `Sparkline 60×20`; on Completed the header figures come from `completedAggregate` and speed is `Zero KB/s`.
- The rows read model fields inside leaf components so a tick re-renders only the leaves (spec-1 rule); the list component must not read per-tick fields.

- [ ] **Step 1: Write failing tests**: `describe` table (running, queued, stopped, completed, failed with a two-line reason shows the first line, retrying with hold and attempts, assembling, disabled suffix) in `rowText.test.ts` over a new pure `src/lib/rowText.ts` (`describeItem(item, retryHoldSeconds)`, `statusColor`, `stateIconKind`); a component test that a list of 5,000 items renders fewer than 60 row elements (Review Focus 5); click, ctrl-click and shift-click change `ui.selection`; clicking a header toggles the chevron only when the chevron is hit and selects the package otherwise; zero `totalBytes` renders `Zero KB / Zero KB`-style text without NaN.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run** tests and `pnpm check`; Playwright-screenshot the mock Downloads and Completed views and compare with `01-downloads-running.png` and `08-completed-tab.png`; adjust paddings and heights until the row rhythm matches (record the numbers you settle on in `layout.ts`).
- [ ] **Step 5: Commit** `git commit -am "Add the windowed package list with item rows and package headers"`.

### Task B8: Toolbar, context menus and the actions module

**Files:** Create `src/components/Toolbar.svelte`, `ContextMenu.svelte`, `src/lib/menu.ts`, `menu.test.ts`; extend `src/lib/actions.ts`.

**Interfaces:**
- Consumes: B2 eligibility, B5 `UiState`, `Bridge`.
- Produces in `menu.ts`: `type MenuEntry = { id: MenuId; label: string; icon: IconName; shortcut?: string; disabled: boolean } | { id: "divider" }` and `itemMenu(items: RowItem[], opts: { platform: Platform }): MenuEntry[]` / `packageMenu(): MenuEntry[]` / `toolbarEntries(items, opts): MenuEntry[]` (the item menu without dividers plus `info`), where labels pluralize (`Copy URLs to Clipboard`, `Start Downloads`, `Enable All`, `Retry Failed Items`, `Remove and Delete Files`) and the file-manager label follows `Platform` (`"Show in Explorer"`, `"Show in File Manager"`, `"Show in Finder"`); shortcut text uses `Ctrl+`/`⌘`. `type Platform = "windows" | "linux" | "mac"` from `navigator.userAgent` in `platform.ts`.
- Produces in `actions.ts`: `runMenu(id: MenuId, ctx: ActionContext): Promise<void>` where `ActionContext = { ids: string[] /* selected item ids */; items: Map<string, ItemModel>; bridge: Bridge; ui: UiState; clipboard: { writeText(text: string): Promise<void> } }`. Each action filters `ids` by its predicate and does nothing (no bridge call) when the result is empty (Review Focus 1); `pause` opens the pause modal when `needsPauseConfirmation`; `moveToNewPackage` opens the new-package modal; `removeFromList` calls `removeItems(ids, false)` and clears the selection; `removeAndDelete` calls `describeDeletion` then opens the delete modal; `copyUrls` joins URLs with newlines; `open` calls `openItem` per completed id; `show` calls `revealItem(firstId)`; `info` toggles `ui.panelVisible`; package actions `sortByName` (natural, case-insensitive: `localeCompare(b, undefined, { numeric: true, sensitivity: "base" })`), `removePackage`, `removePackageAndDelete`.
- `ContextMenu`: positioned at the pointer, flipped to stay inside the viewport, closes on Escape, outside click, scroll or a structure patch that removes its target, keyboard arrows and Enter; shows icon, label, shortcut; disabled entries are not clickable.

- [ ] **Step 1: Write failing tests**: `menu.test.ts` (entry order and dividers equal the spec; labels singular vs plural; every entry disabled for an empty selection; each entry's `disabled` flag for a fixture selection of one running + one disabled + one failed-mux item; platform labels; toolbar has 13 buttons and no dividers); `actions.test.ts` with a recording stub bridge (empty set → zero calls; start only calls ids that `canStart`; pause with a non-resumable running item opens the pause modal and calls nothing; remove-and-delete calls `describeDeletion` with `{kind:"items",ids}`; copy joins URLs; sort-by-name sends `reorderItems` with `["file2","file10"]` ordered naturally).
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement** modules then components; wire `RowSlot` `oncontextmenu` to `selection.contextTarget`.
- [ ] **Step 4: Run** tests, `pnpm check`, and a Playwright script that right-clicks a row in the mock and screenshots against `04-context-menu-item.png` / `05-context-menu-package.png`.
- [ ] **Step 5: Commit** `git commit -am "Add the toolbar, context menus and menu actions"`.

### Task B9: Details panel

**Files:** Create `src/components/DetailsPanel.svelte`, `PanelHandle.svelte`, `src/lib/details.ts`, `details.test.ts`; modify `PackageList` host layout in `Shell`.

**Interfaces:**
- Produces in `details.ts`: `summarize(items: ItemModel[]): { states: { completed; running; retrying; queued; stopped; failed: number }; speedHistory: number[]; bytesPerSecond: number; completed: Range[]; totalBytes: number; downloadedBytes: number; averageFraction: number; activeSegments: number; configuredSegments: number }` (port of `multipleItemData`: skip items with `totalBytes` ≤ 0, offset each item's ranges by the sizes before it, sum equal-length or left-padded speed histories only for running items) and `statusText(item): { text: string; tone: "normal" | "failed" | "faulty" }` (port of `statusDescription`, including `Completed — file missing on disk`, `Failed — reason`, `Retrying — attempt X of Y, retrying in Ns` + `Last failure: …`, `Assembling…`, `(disabled)`).
- `DetailsPanel` props: `itemIds: string[]`. It calls `bridge.setDetailItems("main-details", ids)` whenever the id list changes and with `[]` on destroy or when hidden. Layouts per spec 5.3 with buttons `Open` (disabled unless `canOpen`), `Show in …`, `Copy URL`. Uses `item.detailHistory ?? item.speedHistory`. `PanelHandle`: 40 × 5 capsule, `cursor: ns-resize`, pointer-capture drag, height = clamp(start − dy, 100, 600).

- [ ] **Step 1: Write failing tests**: `summarize` (two items with histories of different lengths, only running items add, ranges offset, zero-size item skipped, percent average); `statusText` cases from the SwiftUI source; component test that the panel calls `setDetailItems` with exactly the shown ids, then `[]` after unmount; removing the shown item flips to the placeholder (Review Focus 3); multi-selection with a shared folder shows `Destination Folder`, differing folders hide it.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run** tests; Playwright screenshots against `02`, `03`, `06`, `09`.
- [ ] **Step 5: Commit** `git commit -am "Add the details panel and its resize handle"`.

### Task B10: Modals

**Files:** Create `src/components/Modal.svelte`, `DeleteConfirm.svelte`, `PauseConfirm.svelte`, `NewPackagePrompt.svelte`, `ModalHost.svelte`.

**Interfaces:**
- Produces: `ModalHost` renders `ui.modal` (one at a time, centered, backdrop, focus trap, Esc cancels, Enter confirms, restores focus on close). `DeleteConfirm` per spec 5.3 (trash icon in `--status-failed`, title, count line `N file(s) · SIZE will be moved to the Trash.` or `No files will be moved to the Trash`, scroll list max 180 px with middle truncation, `Cancel` / `Move to Trash` or `Remove from List`; confirm calls `removeItems(ids, true)` or `removePackage(id, true)` then closes). `PauseConfirm`: title `Pause non-resumable download?`, the body text in spec 5.3, `Pause` (calls `stopItems(ids filtered by canStop)`) / `Cancel`. `NewPackagePrompt`: title `Enter new package name:`, autofocused field, `Move to New Package` disabled while the trimmed name is empty, confirm calls `moveItemsToNewPackage`.

- [ ] **Step 1: Write failing component tests**: Esc closes without a bridge call; Enter on the delete modal confirms with the right call for items and for a package; the zero-file wording; the new-package button is disabled for blank and whitespace names and the name is trimmed before the call; a bridge error is shown through `ui.report` and the modal closes.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run**; Playwright screenshot of the delete modal against `07-delete-sheet.png`.
- [ ] **Step 5: Commit** `git commit -am "Add the delete, pause and new-package modals"`.

### Task B11: Drag and drop

**Files:** Modify `RowSlot.svelte`, `PackageList.svelte`, `src/lib/dnd.ts`; create `src/lib/dragState.svelte.ts`; Test `src/components/PackageList.dnd.test.ts` and `web/e2e/dnd.spec.ts`.

**Interfaces:**
- Consumes: B2 `dnd.ts`. Produces: `dragState` (`$state`): `payload: DragPayload | null`, `targetId: string | null` (drives the 2 px `--accent` outline on the target slot). Start: if the dragged row is selected drag the whole selection (item keys and package keys as `kind`-tagged entries), else just that row; `dataTransfer.setData("application/x-sdm-rows", JSON.stringify(payload))` and `effectAllowed = "move"`. Drop on an item row → `bridge.moveItems(orderedItemIds, packageId, dropIndex(...))`; on a header with an item payload → `moveItems(ids, packageId)` (append); on a header with a package payload → `reorderPackages(reorderedPackageIds(...))`; anything else is ignored. Only `allowsReordering` lists accept or start drags.

- [ ] **Step 1: Write failing tests**: component tests dispatching `dragstart`/`dragover`/`drop` with a fake `DataTransfer` for each case above plus the refusals (mixed payload, drop on own member, Completed tab); the e2e test drags the first item below the third item in the mock and asserts the new order.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run** unit and e2e tests.
- [ ] **Step 5: Commit** `git commit -am "Add drag and drop reordering and moving"`.

### Task B12: Keyboard shortcuts

**Files:** Create `src/lib/shortcuts.ts`, `shortcuts.test.ts`; modify `Shell.svelte`.

**Interfaces:**
- Produces: `handleKey(event: KeyboardEvent, ctx: ShortcutContext): boolean` (true when handled and `preventDefault` was called). `ShortcutContext = { platform: Platform; modalOpen: boolean; menuOpen: boolean; focusInEditable: boolean; rowKeys: string[]; itemKeys: string[]; run(id: MenuId): void; selectAll(): void; moveSelection(delta: -1 | 1, extend: boolean): void; closeMenu(): void }`. Bindings: Enter → open, Mod+Enter → show, Delete → removeFromList, Mod+Delete → removeAndDelete, Mod+A → selectAll, ArrowUp/Down → moveSelection (Shift extends), Escape → close the menu. Mod is Ctrl except on macOS (Cmd). Ignored when `modalOpen`, or `focusInEditable`.
- Scroll-into-view for keyboard moves uses the offsets from B3, so rows that are not rendered still become visible.

- [ ] **Step 1: Write failing tests**: each binding per platform; ignored with a modal open and in an input; Ctrl+A selects all item ids of a 5,000-row view including ones never rendered (Review Focus 5); Delete with an empty selection does nothing.
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement** and attach one `keydown` listener on `window` in `Shell`. **Step 4: Run** tests.
- [ ] **Step 5: Commit** `git commit -am "Add the keyboard shortcuts"`.

### Task B13: Window configuration and persistence

**Files:** Modify `sdm-tauri/src-tauri/tauri.conf.json`, `Cargo.toml` (via `cargo add`), `src/lib.rs`, `capabilities/default.json`; modify `scripts/check-*.sh` only if needed.

**Interfaces:**
- Config: window `title` `"Shayan's Download Manager"`, `width` 1000, `height` 640, `minWidth` 760, `minHeight` 480, `dragDropEnabled` `false`. Plugin `tauri-plugin-window-state` registered in the builder with its capability permission. Verify plugin name, registration call and permission string against current docs (Context7) before coding.

- [ ] **Step 1: Write failing test**: a Rust unit test in `sdm-tauri` (`src-tauri/src/lib.rs` tests module) that parses `tauri.conf.json` and asserts the title, minimum size and `dragDropEnabled == false` (the config is JSON; use `include_str!`).
- [ ] **Step 2: Run** `cargo test -p sdm-tauri`; expect failure.
- [ ] **Step 3: Implement** the config, add the plugin with `cargo add tauri-plugin-window-state` (in `sdm-tauri/src-tauri`), register it, add the permission.
- [ ] **Step 4: Run** `cargo test -p sdm-tauri`, `scripts/check-windows.sh`, `scripts/check-linux.sh`; then `pnpm tauri dev` once to confirm the window opens (record what you saw).
- [ ] **Step 5: Commit** `git commit -am "Configure the main window and remember its size and position"`.

### Task B14: Mica and Acrylic on Windows

**Files:** Modify `sdm-tauri/src-tauri/Cargo.toml` (`cargo add window-vibrancy`), `src/lib.rs`, `src/commands.rs`, `tauri.conf.json` (per-platform transparent window), `web/src/App.svelte`, `theme.css`; Test: `src-tauri` unit test for the pure decision function.

**Interfaces:**
- Produces: `fn vibrancy_effect(os: Os, build: Option<u32>) -> Vibrancy` in `src-tauri/src/vibrancy.rs` (`Mica` for Windows build ≥ 22000, `Acrylic` for Windows 10 builds ≥ 17763, `None` otherwise) so the choice is testable; the Windows-only setup calls `window_vibrancy::apply_mica` / `apply_acrylic` (confirm names and the `cfg(windows)` requirement in the crate docs) and a command `vibrancy_active() -> bool` the frontend reads at start to set `document.documentElement.dataset.vibrancy = "on"`. CSS: with `[data-vibrancy="on"]`, `.sidebar` and `.toolbar` backgrounds become `color-mix(in srgb, var(--sidebar-background) 80%, transparent)`; otherwise they stay opaque. Failure of the effect is logged and ignored.

- [ ] **Step 1: Write failing test** for `vibrancy_effect` (builds 22621 → Mica, 19045 → Acrylic, 17000 → None, macOS/Linux → None).
- [ ] **Step 2: Run**; expect failure. **Step 3: Implement.** **Step 4: Run** `cargo test -p sdm-tauri`, `scripts/check-windows.sh`, `scripts/check-linux.sh`, `scripts/build-tauri.sh windows` (produces the installer for the user's VM). The visual check is the user's.
- [ ] **Step 5: Commit** `git commit -am "Apply Mica or Acrylic to the window on Windows"`.

### Task B15: Parity pass, performance, docs

**Files:** Modify component CSS as needed; create `web/e2e/parity.spec.ts`; modify `test.sh`, `docs/tauri-roadmap.md`, `CLAUDE.md`, `docs/tauri-performance.md`.

- [ ] **Step 1: Write the parity spec**: with the mock (`?theme=deep-purple`, fixed window 1300 × 960, the reference fixture) it reproduces the 11 states and saves screenshots to `web/test-results/parity/NN-*.png`; assertions that matter structurally (the toolbar has 13 buttons, the menus list the entries in order, panel height clamps).
- [ ] **Step 2: Run** `pnpm exec playwright test parity`; open each screenshot next to its reference (`Read` both) and list every difference you see (spacing, weights, colors, sizes). Fix them in CSS/components and repeat until the remaining differences are only the intended ones (Lucide vs SF Symbols, system font, window chrome). Record the checked list in the commit message.
- [ ] **Step 3: Re-measure performance** on macOS with `--simulate` as in `docs/tauri-performance.md` (idle CPU, 50 active items, memory) and append the new numbers; they must still meet the spec-1 thresholds. If they do not, fix the cause before continuing (typical suspects: a row reading a per-tick field outside a leaf, the windowing offsets rebuilt per tick).
- [ ] **Step 4: Run the full gate**: `./test.sh` (Rust, Swift where it applies, Svelte unit tests, Playwright), `pnpm check`, both compile checks, `scripts/build-tauri.sh mac`.
- [ ] **Step 5: Update docs**: `docs/tauri-roadmap.md` (spec 2 row to `built`, a log line with date and what is unverified: the Windows VM visuals for Mica, drag and drop, shortcuts), `CLAUDE.md` Tauri paragraph (main window exists), spec status line; commit `git commit -am "Finish the Tauri main window: parity pass, performance and docs"`.

---

## Self-review (run against the spec)

- Spec 4.1/4.2/4.3 → A1–A5; commands, bindings, bridges, mock → A6. Spec 5.1 structure → B6–B10; 5.2 measurements → B3, B4, B7, B15; 5.3 behaviors: selection → B2/B5/B7, eligibility → B2, context menu and toolbar → B8, pause confirm → B8/B10, delete flow → A4/B8/B10, drag and drop → B2/B11, keyboard → B12, sidebar → B6, details panel → B9, row text → B7, formatting → B1, segmented bar and sparkline → B4. 5.4 persistence and vibrancy → B13, B14. Testing section → every task plus B15. M8 Completed derivation → B5/B7. M12 list ids → A2/A6.
- Types: `RowItem`, `SelectionModel`, `DragPayload`, `MenuId`, `ActionContext`, `Platform`, `ModalState`, `DeletionTarget`/`DeletionInfo`, owner id `"main-details"`, and the bridge method names are used identically in the tasks that define and consume them.
- Proportion: Phase A is Rust-heavy but compact; each task lists tests and signatures, no bodies.
