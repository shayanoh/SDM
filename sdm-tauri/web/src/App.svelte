<script lang="ts">
  import { onMount } from "svelte";
  import type { BinariesStatusDto } from "./bindings";
  import AddUrl from "./components/AddUrl.svelte";
  import Banner from "./components/Banner.svelte";
  import DownloadList from "./components/DownloadList.svelte";
  import TotalSpeed from "./components/TotalSpeed.svelte";
  import { errorMessage, useBridge, useStore, useUi } from "./lib/context";

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();

  let binaries = $state<BinariesStatusDto | null>(null);
  let folder = $state("");

  async function guarded(action: () => Promise<void>) {
    try {
      await action();
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }

  async function saveFolder() {
    await guarded(async () => {
      await bridge.updateSettings({ downloadFolder: folder });
    });
    // Show what is actually in force (a rejected value leaves the old one).
    folder = (await bridge.getSettings()).downloadFolder;
  }

  async function pollBinaries() {
    try {
      binaries = await bridge.binariesStatus();
    } catch {
      // The status line is informational; a failed poll just keeps the old text.
    }
  }

  onMount(() => {
    const onVisibility = () => {
      guarded(() => bridge.setListVisible(!document.hidden));
    };
    document.addEventListener("visibilitychange", onVisibility);
    // The only polling in the UI. The download stream is pushed, not polled.
    const timer = setInterval(pollBinaries, 5000);

    void guarded(async () => {
      const style = document.createElement("style");
      style.id = "theme";
      style.textContent = await bridge.themeCss();
      document.head.append(style);
      folder = (await bridge.getSettings()).downloadFolder;
      await bridge.attach(store.apply.bind(store));
      await pollBinaries();
    });

    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      clearInterval(timer);
    };
  });
</script>

<main>
  <header>
    <h1>SDM</h1>
    <TotalSpeed />
    <span class="buttons">
      <button type="button" onclick={() => guarded(() => bridge.pauseAll())}>
        Pause All
      </button>
      <button type="button" onclick={() => guarded(() => bridge.resumeAll())}>
        Resume All
      </button>
    </span>
  </header>
  <Banner />
  <AddUrl />
  <DownloadList />
  <footer>
    <label>
      Download folder
      <input
        type="text"
        bind:value={folder}
        onchange={saveFolder}
        aria-label="Download folder"
      />
    </label>
    <span class="binaries">
      yt-dlp: {binaries?.ytDlpVersion ?? "not installed"}
    </span>
  </footer>
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }
  h1 {
    margin: 0;
    font-size: 1.1rem;
  }
  .buttons {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 12px;
    color: var(--text-secondary);
    border-top: 1px solid var(--border);
  }
  label {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
  }
  label input {
    flex: 1;
    padding: 4px 6px;
    color: var(--text-primary);
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
</style>
