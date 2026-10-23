<script lang="ts">
  import { onMount } from "svelte";
  import type { BinariesStatusDto } from "../bindings";
  import { errorMessage, useBridge, useUi } from "../lib/context";

  /**
   * Add a link, the download folder and the yt-dlp status: the only way to
   * reach those until the Linkgrabber (spec 3) and Settings (spec 4) exist.
   */
  const bridge = useBridge();
  const ui = useUi();

  let url = $state("");
  let busy = $state(false);
  let folder = $state("");
  let binaries = $state<BinariesStatusDto | null>(null);

  async function addUrl(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    try {
      await bridge.addUrl(url);
      url = "";
      ui.dismiss();
    } catch (error) {
      ui.report(errorMessage(error));
    } finally {
      busy = false;
    }
  }

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
  <form onsubmit={addUrl}>
    <input
      type="text"
      class="selectable"
      placeholder="Paste a link to download"
      aria-label="Download address"
      bind:value={url}
    />
    <button type="submit" disabled={busy}>Add</button>
  </form>
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
  <p class="muted">yt-dlp: {binaries?.ytDlpVersion ?? "not installed"}</p>
</div>

<style>
  .tools {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: min(560px, 100%);
  }
  form,
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
