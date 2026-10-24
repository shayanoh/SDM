<script lang="ts">
  import { onMount } from "svelte";
  import type { BinariesStatusDto } from "../bindings";
  import { errorMessage, useBridge, useUi } from "../lib/context";

  /**
   * The download folder and the tools status: the only way to reach those
   * until Settings (spec 4) exists. TODO(spec 4): remove with the Settings tab.
   */
  const bridge = useBridge();
  const ui = useUi();

  let folder = $state("");
  let binaries = $state<BinariesStatusDto | null>(null);

  async function saveFolder() {
    try {
      await bridge.updateSettings({ downloadFolder: folder });
    } catch (error) {
      ui.report(errorMessage(error));
    }
    // Show what is actually in force (a rejected value leaves the old one).
    folder = (await bridge.getSettings()).downloadFolder;
  }

  onMount(() => {
    void bridge
      .getSettings()
      .then((settings) => (folder = settings.downloadFolder));
    const poll = async () => {
      try {
        binaries = await bridge.binariesStatus();
      } catch {
        // The status line is informational; a failed poll keeps the old text.
      }
    };
    void poll();
    const timer = setInterval(poll, 5000);
    return () => clearInterval(timer);
  });
</script>

<div class="tools">
  <label>
    Download folder
    <input
      type="text"
      class="selectable"
      bind:value={folder}
      onchange={saveFolder}
      aria-label="Download folder"
    />
  </label>
  <p class="muted">
    yt-dlp: {binaries?.ytDlpVersion ?? "not installed"} · ffmpeg: {binaries?.ffmpegVersion ??
      "not installed"} · qjs: {binaries?.qjsVersion ?? "not installed"}
  </p>
</div>

<style>
  .tools {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: min(560px, 100%);
  }
  label {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  input {
    flex: 1;
    padding: 6px 8px;
    color: var(--text-primary);
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .muted {
    margin: 0;
    color: var(--text-secondary);
  }
</style>
