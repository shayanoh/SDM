<script lang="ts">
  import type { AutoClearInterval } from "../../bindings";
  import type { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import { AUTO_CLEAR_LABELS, AUTO_CLEAR_ORDER } from "../../lib/settingsModel";
  import SettingsPopup from "./SettingsPopup.svelte";
  import SettingsSection from "./SettingsSection.svelte";

  let { draft }: { draft: SettingsDraft } = $props();

  const intervals = AUTO_CLEAR_ORDER.map((value) => ({
    value,
    label: AUTO_CLEAR_LABELS[value],
  }));
</script>

<div class="tab">
  <SettingsSection title="Linkgrabber">
    <label class="check">
      <input
        type="checkbox"
        checked={draft.values.clipboardWatchingEnabled}
        onchange={(e) =>
          draft.set("clipboardWatchingEnabled", e.currentTarget.checked)}
      />
      Watch clipboard for links
    </label>
    <label class="check">
      <input
        type="checkbox"
        checked={draft.values.autoAddAndStartOnGrab}
        onchange={(e) =>
          draft.set("autoAddAndStartOnGrab", e.currentTarget.checked)}
      />
      Auto-add and start on grab
    </label>
    <label class="check">
      <input
        type="checkbox"
        checked={draft.values.deepSniffEnabled}
        onchange={(e) => draft.set("deepSniffEnabled", e.currentTarget.checked)}
      />
      Deep sniff (stage 2)
    </label>
    <SettingsPopup
      label="Auto-clear grabbed links"
      value={draft.values.autoClearInterval}
      options={intervals}
      width={170}
      onchange={(v) => draft.set("autoClearInterval", v as AutoClearInterval)}
    />
  </SettingsSection>
</div>
