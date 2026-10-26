<script lang="ts">
  import type { ThemeInfo } from "../../bindings";
  import type { Platform } from "../../lib/platform";
  import type { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import { orderThemes } from "../../lib/settingsModel";
  import SettingsPopup from "./SettingsPopup.svelte";
  import SettingsSection from "./SettingsSection.svelte";

  let {
    draft,
    themes,
    platform,
    onTheme,
  }: {
    draft: SettingsDraft;
    themes: ThemeInfo[];
    platform: Platform;
    onTheme: (id: string) => void;
  } = $props();

  const themeOptions = $derived([
    { value: "system", label: "System" },
    ...orderThemes(themes).map((t) => ({ value: t.id, label: t.name })),
  ]);
  const modes = [
    { value: "menuBarOnly", label: "Menu Bar Only" },
    { value: "dockOnly", label: "Dock Only" },
    { value: "both", label: "Both" },
  ];
</script>

<div class="tab">
  <SettingsSection title="General">
    <label class="check">
      <input
        type="checkbox"
        checked={draft.values.launchAtLogin}
        onchange={(e) => draft.set("launchAtLogin", e.currentTarget.checked)}
      />
      Launch SDM at login
    </label>
  </SettingsSection>
  <SettingsSection title="Theme">
    <SettingsPopup
      label="Theme"
      value={draft.values.themeId}
      options={themeOptions}
      onchange={onTheme}
    />
  </SettingsSection>
  {#if platform === "mac"}
    <SettingsSection title="Dock / Menu Bar">
      <SettingsPopup
        label="Dock / Menu Bar"
        value={draft.values.activationMode}
        options={modes}
        onchange={(v) =>
          draft.set("activationMode", v as "menuBarOnly" | "dockOnly" | "both")}
      />
    </SettingsSection>
  {/if}
</div>
