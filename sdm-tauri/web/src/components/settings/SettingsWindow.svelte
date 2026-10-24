<script lang="ts">
  import { onMount } from "svelte";
  import type { BinariesStatusDto, ThemeInfo } from "../../bindings";
  import { errorMessage, useBridge } from "../../lib/context";
  import { detectPlatform, type Platform } from "../../lib/platform";
  import { SettingsDraft } from "../../lib/settingsDraft.svelte";
  import {
    closeWindow,
    fitWindow,
    onCloseRequested,
  } from "../../lib/windowControl";
  import AppearanceTab from "./AppearanceTab.svelte";
  import DownloadsTab from "./DownloadsTab.svelte";
  import LinkgrabberTab from "./LinkgrabberTab.svelte";
  import MediaSitesTab from "./MediaSitesTab.svelte";
  import NotificationsTab from "./NotificationsTab.svelte";

  /** The Settings window's content: five tabs and an OK / Cancel bar. */
  let {
    platform = detectPlatform(),
    onClose = closeWindow,
  }: { platform?: Platform; onClose?: () => void | Promise<void> } = $props();

  const WIDTH = 560;
  const TABS = [
    ["appearance", "Appearance"],
    ["downloads", "Downloads"],
    ["linkgrabber", "Linkgrabber"],
    ["media", "Media Sites"],
    ["notifications", "Notifications"],
  ] as const;

  const bridge = useBridge();

  let draft = $state<SettingsDraft | null>(null);
  let themes = $state<ThemeInfo[]>([]);
  let status = $state<BinariesStatusDto | null>(null);
  let tab = $state<(typeof TABS)[number][0]>("appearance");
  let error = $state<string | null>(null);
  let busy = false;
  // The page's natural height: the scrolling part plus the button bar.
  let scrollingHeight = $state(0);
  let barHeight = $state(0);
  const contentHeight = $derived(scrollingHeight + barHeight);

  async function refreshStatus() {
    try {
      status = await bridge.binariesStatus();
    } catch {
      // Informational: a failed poll keeps the old text.
    }
  }

  onMount(() => {
    void (async () => {
      try {
        draft = new SettingsDraft(await bridge.getSettings());
        themes = await bridge.listThemes();
      } catch (failure) {
        error = errorMessage(failure);
      }
    })();
    void refreshStatus();
    const timer = setInterval(refreshStatus, 5000);
    // Closing by the titlebar is Cancel: take a previewed theme back.
    const stopClose = onCloseRequested(() => void bridge.broadcastTheme(null));
    return () => {
      clearInterval(timer);
      stopClose();
    };
  });

  // The window is as tall as the tab it shows.
  $effect(() => {
    if (contentHeight > 0) void fitWindow(WIDTH, contentHeight);
  });

  function previewTheme(id: string) {
    draft?.set("themeId", id);
    void bridge.broadcastTheme(id);
  }

  /** The channel applies as soon as it is picked, and runs a check. */
  async function changeChannel(channel: string) {
    if (!draft) return;
    try {
      // Everything else goes back as last saved: an update sends every field.
      await bridge.updateSettings({ ...draft.saved, ytDlpChannel: channel });
      draft.markSaved({ ytDlpChannel: channel });
      error = null;
      await bridge.checkBinariesNow();
      await refreshStatus();
    } catch (failure) {
      error = errorMessage(failure);
    }
  }

  async function checkNow() {
    try {
      await bridge.checkBinariesNow();
      await refreshStatus();
    } catch (failure) {
      error = errorMessage(failure);
    }
  }

  async function commit() {
    if (!draft || busy) return;
    busy = true;
    try {
      await bridge.updateSettings(draft.toDto());
      // The new theme is the stored one now; every window follows it.
      await bridge.broadcastTheme(null);
      await onClose();
    } catch (failure) {
      error = errorMessage(failure);
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    await bridge.broadcastTheme(null);
    await onClose();
  }

  function onKey(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      void cancel();
    } else if (
      event.key === "Enter" &&
      !(event.target instanceof HTMLButtonElement)
    ) {
      event.preventDefault();
      void commit();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="window" style:width="{WIDTH}px">
  <div class="scroll">
    <div class="content" bind:clientHeight={scrollingHeight}>
      <div class="tabs" role="tablist" aria-label="Settings">
        {#each TABS as [id, label] (id)}
          <button
            type="button"
            role="tab"
            aria-selected={tab === id}
            class:selected={tab === id}
            onclick={() => (tab = id)}
          >
            {label}
          </button>
        {/each}
      </div>

      <div class="body">
        {#if !draft}
          <p data-testid="settings-loading" class="muted">Loading…</p>
        {:else if tab === "appearance"}
          <AppearanceTab {draft} {themes} {platform} onTheme={previewTheme} />
        {:else if tab === "downloads"}
          <DownloadsTab {draft} />
        {:else if tab === "linkgrabber"}
          <LinkgrabberTab {draft} />
        {:else if tab === "media"}
          <MediaSitesTab
            {draft}
            {platform}
            {status}
            onChannel={changeChannel}
            onCheck={checkNow}
          />
        {:else}
          <NotificationsTab {draft} />
        {/if}
      </div>

      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}
    </div>
  </div>
  <div class="bar" bind:clientHeight={barHeight}>
    <button type="button" onclick={cancel}>Cancel</button>
    <button type="button" class="default" onclick={commit}>OK</button>
  </div>
</div>

<style>
  /* The window is as tall as its content, unless the screen is shorter: then
     only the part above the OK / Cancel bar scrolls. */
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--surface-primary);
  }
  .scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .content {
    display: flex;
    flex-direction: column;
  }
  .tabs {
    display: flex;
    align-self: center;
    margin: 10px 0 0;
    padding: 1px;
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .tabs button {
    padding: 4px 14px;
    color: var(--text-secondary);
    background: none;
    border: none;
    border-radius: 7px;
    font-size: 15px;
  }
  .tabs button.selected {
    color: var(--text-primary);
    background: var(--surface-tertiary);
  }
  .body {
    padding: 20px 20px 12px;
  }
  .muted {
    color: var(--text-secondary);
  }
  .error {
    margin: 0;
    padding: 0 20px 8px;
    color: var(--status-failed);
  }
  .bar {
    flex: none;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 16px;
    border-top: 1px solid var(--border);
  }
  .bar button {
    padding: 4px 14px;
    font-size: 15px;
  }
  .bar .default {
    color: #fff;
    background: var(--accent);
    border-color: var(--accent);
  }
  .window :global(.tab) {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .window :global(.check) {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 15px;
  }
  .window :global(.check input) {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
  }
  .window :global(.line) {
    font-size: 15px;
  }
</style>
