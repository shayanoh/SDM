<script lang="ts">
  import type { BinariesStatusDto, ChromeExtensionDto } from "../../bindings";
  import { statusLine } from "../../lib/chromeSetup";
  import type { Platform } from "../../lib/platform";
  import type { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import {
    RESOLUTION_CHOICES,
    cookieChoices,
    isLastChecked,
    resolutionLabel,
  } from "../../lib/settingsModel";
  import ComponentsSection from "./ComponentsSection.svelte";
  import SettingsPopup from "./SettingsPopup.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SteppedNumberField from "./SteppedNumberField.svelte";

  let {
    draft,
    platform,
    status,
    chrome,
    onSetupChrome,
    onChannel,
    onCheck,
  }: {
    draft: SettingsDraft;
    platform: Platform;
    status: BinariesStatusDto | null;
    chrome: ChromeExtensionDto | null;
    onSetupChrome: () => void;
    onChannel: (channel: string) => void;
    onCheck: () => void;
  } = $props();

  const heights = RESOLUTION_CHOICES.map((h) => ({
    value: h,
    label: resolutionLabel(h),
  }));
  const cookies = $derived(cookieChoices(platform));

  const video = [
    ["allowAv1", "AV1"],
    ["allowVp9", "VP9"],
    ["allowH264", "H.264"],
  ] as const;
  const containers = [
    ["allowMp4", "MP4"],
    ["allowWebm", "WebM"],
  ] as const;
  const audio = [
    ["allowOpus", "Opus"],
    ["allowAac", "AAC"],
  ] as const;

  const flags = (
    group: readonly (readonly [keyof typeof draft.values, string])[],
  ) => group.map(([key]) => draft.values[key] as boolean);
</script>

{#snippet checks(group: readonly (readonly [string, string])[])}
  {@const on = flags(group as never)}
  {#each group as [key, label], index (key)}
    <label class="check">
      <input
        type="checkbox"
        checked={on[index]}
        disabled={isLastChecked(on, index)}
        onchange={(e) =>
          draft.set(key as never, e.currentTarget.checked as never)}
      />
      {label}
    </label>
  {/each}
{/snippet}

<div class="tab">
  <SettingsSection title="Quality">
    <SettingsPopup
      label="Maximum resolution"
      value={draft.values.ytMaxHeight}
      options={heights}
      width={120}
      onchange={(v) => draft.set("ytMaxHeight", Number(v))}
    />
    <SteppedNumberField
      label="Maximum playlist videos"
      value={draft.values.maxPlaylistVideos}
      min={10}
      max={200}
      onchange={(v) => draft.set("maxPlaylistVideos", v)}
    />
  </SettingsSection>
  <div class="three">
    <SettingsSection title="Video codecs">
      {@render checks(video)}
    </SettingsSection>
    <SettingsSection title="Containers">
      {@render checks(containers)}
    </SettingsSection>
    <SettingsSection title="Audio codecs">
      {@render checks(audio)}
    </SettingsSection>
  </div>
  <SettingsSection title="Authentication">
    <SettingsPopup
      label="Cookies from browser"
      value={draft.values.cookieSource}
      options={cookies}
      width={160}
      onchange={(v) => draft.set("cookieSource", v)}
    />
    {#if draft.values.cookieSource === "chrome"}
      <div class="note" data-testid="chrome-cookies-note">
        <p>Chrome cookies come from the SDM Chrome extension.</p>
        {#if chrome}<p class="muted">{statusLine(chrome)}</p>{/if}
        <button type="button" onclick={onSetupChrome}>
          Set up the extension
        </button>
      </div>
    {/if}
  </SettingsSection>
  <ComponentsSection {draft} {status} {onChannel} {onCheck} />
</div>

<style>
  .note {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }
  .note p {
    margin: 0;
    font-size: 14px;
  }
  .note .muted {
    color: var(--text-secondary);
  }
  .three {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 16px;
    align-items: start;
  }
</style>
