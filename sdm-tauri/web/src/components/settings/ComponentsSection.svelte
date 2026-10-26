<script lang="ts">
  import type { BinariesStatusDto } from "../../bindings";
  import type { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import SettingsPopup from "./SettingsPopup.svelte";
  import SettingsSection from "./SettingsSection.svelte";

  let {
    draft,
    status,
    onChannel,
    onCheck,
  }: {
    draft: SettingsDraft;
    status: BinariesStatusDto | null;
    onChannel: (channel: string) => void;
    onCheck: () => void;
  } = $props();

  const channels = [
    { value: "stable", label: "Stable" },
    { value: "nightly", label: "Nightly" },
  ];

  /** The text after a tool's name: the version and where it came from. */
  function toolText(version: string | null | undefined, prefix = ""): string {
    if (status?.bundled) return `${prefix}${version ?? "—"}  (bundled)`;
    if (version) return `${prefix}${version}`;
    return status?.downloading ? "downloading…" : "not installed";
  }
</script>

<SettingsSection title="Components">
  <div class="line">
    <span>yt-dlp</span>
    <span class:muted={!status?.ytDlpVersion}>
      {status?.ytDlpVersion ?? "not installed"}
    </span>
  </div>
  {#if status?.latestKnown && status.latestKnown !== status.ytDlpVersion}
    <p class="caption muted">latest: {status.latestKnown}</p>
  {/if}
  <SettingsPopup
    label="Update channel"
    value={draft.values.ytDlpChannel}
    options={channels}
    width={120}
    onchange={onChannel}
  />
  <div class="line">
    <button type="button" onclick={onCheck}>Check now</button>
  </div>
  {#if status?.lastError}
    <p class="caption failed">{status.lastError}</p>
  {/if}
  <hr />
  <div class="line">
    <span>ffmpeg</span><span class="muted"
      >{toolText(status?.ffmpegVersion)}</span
    >
  </div>
  <div class="line">
    <span>ffprobe</span><span class="muted"
      >{toolText(status?.ffmpegVersion)}</span
    >
  </div>
  <div class="line">
    <span>JS runtime</span>
    <span class="muted">{toolText(status?.qjsVersion, "QuickJS-ng ")}</span>
  </div>
</SettingsSection>

<style>
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .muted {
    color: var(--text-secondary);
  }
  p {
    margin: 0;
  }
  .failed {
    color: var(--status-failed);
  }
  hr {
    width: 100%;
    margin: 0;
    border: none;
    border-top: 1px solid var(--border);
  }
</style>
