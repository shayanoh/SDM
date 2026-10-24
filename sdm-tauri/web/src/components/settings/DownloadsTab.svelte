<script lang="ts">
  import type { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import SettingsSection from "./SettingsSection.svelte";
  import SteppedNumberField from "./SteppedNumberField.svelte";

  let { draft }: { draft: SettingsDraft } = $props();
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
</div>
