<script lang="ts">
  import type { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import SettingsSection from "./SettingsSection.svelte";

  let { draft }: { draft: SettingsDraft } = $props();

  const toggles = [
    ["notifyDownloadFinished", "Download finished"],
    ["notifyPackageFinished", "Package finished"],
    ["notifyDownloadFailed", "Download failed"],
    ["notifyLinksGrabbed", "Links grabbed"],
  ] as const;
</script>

<div class="tab">
  <SettingsSection title="Notifications">
    {#each toggles as [key, label] (key)}
      <label class="check">
        <input
          type="checkbox"
          checked={draft.values[key]}
          onchange={(e) => draft.set(key, e.currentTarget.checked)}
        />
        {label}
      </label>
    {/each}
  </SettingsSection>
</div>
