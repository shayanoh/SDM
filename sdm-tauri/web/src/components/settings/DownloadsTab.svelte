<script lang="ts">
  import type { ChromeExtensionDto } from "../../bindings";
  import { statusLine } from "../../lib/chromeSetup";
  import type { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SteppedNumberField from "./SteppedNumberField.svelte";

  let {
    draft,
    chrome,
    onSetupChrome,
  }: {
    draft: SettingsDraft;
    chrome: ChromeExtensionDto | null;
    onSetupChrome: () => void;
  } = $props();
</script>

<div class="tab">
  <SettingsSection title="Concurrency">
    <SteppedNumberField
      label="Max concurrent downloads"
      value={draft.values.maxConcurrent}
      min={1}
      max={20}
      onchange={(v) => draft.set("maxConcurrent", v)}
    />
    <SteppedNumberField
      label="Segments per file"
      value={draft.values.segmentsPerItem}
      min={1}
      max={64}
      onchange={(v) => draft.set("segmentsPerItem", v)}
    />
    <SteppedNumberField
      label="Global max connections"
      value={draft.values.globalMaxConnections}
      min={1}
      max={256}
      onchange={(v) => draft.set("globalMaxConnections", v)}
    />
    <SteppedNumberField
      label="Max connections per host"
      value={draft.values.maxConnectionsPerHost}
      min={1}
      max={64}
      onchange={(v) => draft.set("maxConnectionsPerHost", v)}
    />
    <SteppedNumberField
      label="Minimum segment size (MB)"
      value={draft.values.minSegmentSizeMb}
      min={1}
      max={100}
      onchange={(v) => draft.set("minSegmentSizeMb", v)}
    />
  </SettingsSection>
  <SettingsSection title="Startup">
    <label class="check">
      <input
        type="checkbox"
        checked={draft.values.autoStartDownloadsOnLaunch}
        onchange={(e) =>
          draft.set("autoStartDownloadsOnLaunch", e.currentTarget.checked)}
      />
      Resume downloads automatically when SDM opens
    </label>
  </SettingsSection>
  <SettingsSection title="Chrome Extension">
    <p class="about">
      The SDM Chrome extension is a helper: it captures downloads from Chrome
      and shares Chrome's cookies with SDM for media sites.
    </p>
    {#if chrome}<p class="status">{statusLine(chrome)}</p>{/if}
    <div class="action">
      <button type="button" onclick={onSetupChrome}>
        Install Chrome Extension…
      </button>
    </div>
  </SettingsSection>
</div>

<style>
  .about,
  .status {
    margin: 0;
    font-size: 15px;
  }
  .status {
    color: var(--text-secondary);
  }
  .action {
    display: flex;
  }
  .action button {
    padding: 4px 14px;
    font-size: 15px;
  }
</style>
