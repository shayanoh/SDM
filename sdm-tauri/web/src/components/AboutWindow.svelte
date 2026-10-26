<script lang="ts">
  import { onMount } from "svelte";
  import type {
    AboutInfo,
    BinariesStatusDto,
    ChromeExtensionDto,
  } from "../bindings";
  import { componentRows } from "../lib/about";
  import { errorMessage, useBridge } from "../lib/context";
  import { afterPaint } from "../lib/paint";
  import { firstThemePaint } from "../lib/theme";
  import { closeWindow, fitWindow } from "../lib/windowControl";

  /** The About window: what SDM is, which version and helpers it runs, and the licenses of what it is built from. */
  let {
    onClose = closeWindow,
    themeReady = firstThemePaint,
  }: {
    onClose?: () => void | Promise<void>;
    /** Settles when the theme is painted; the window is shown after it. */
    themeReady?: Promise<void>;
  } = $props();

  const WIDTH = 420;
  const HEIGHT = 520;
  const bridge = useBridge();

  let info = $state<AboutInfo | null>(null);
  let binaries = $state<BinariesStatusDto | null>(null);
  let chrome = $state<ChromeExtensionDto | null>(null);
  let licenses = $state<string | null>(null);
  let showLicenses = $state(false);
  let error = $state<string | null>(null);

  const rows = $derived(componentRows(binaries, chrome));

  async function act(call: () => Promise<unknown>) {
    try {
      await call();
      error = null;
    } catch (failure) {
      error = errorMessage(failure);
    }
  }

  async function refresh() {
    await act(async () => {
      [binaries, chrome] = await Promise.all([
        bridge.binariesStatus(),
        bridge.chromeExtensionStatus(),
      ]);
    });
  }

  // The licenses are large, so they are fetched when first asked for.
  async function openLicenses() {
    showLicenses = true;
    if (licenses === null) {
      await act(async () => {
        licenses = await bridge.openSourceLicenses();
      });
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    if (showLicenses) showLicenses = false;
    else void onClose();
  }

  onMount(() => {
    void act(async () => {
      info = await bridge.aboutInfo();
    });
    void refresh();
    // A tool that is still being fetched fills in as it lands.
    const timer = setInterval(refresh, 2000);
    return () => clearInterval(timer);
  });

  // The window starts hidden: shown once its text is in, the theme painted
  // and the window sized.
  let revealed = false;
  $effect(() => {
    if (revealed || !info) return;
    revealed = true;
    void (async () => {
      await themeReady;
      await afterPaint();
      await fitWindow(WIDTH, HEIGHT);
      await afterPaint();
      await bridge.windowReady();
    })().catch(() => {});
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="window" style:width="{WIDTH}px" style:height="{HEIGHT}px">
  {#if showLicenses}
    <header class="bar">
      <button type="button" onclick={() => (showLicenses = false)}>
        ‹ Back
      </button>
      <h2>Open-source licenses</h2>
    </header>
    {#if licenses === null}
      <p class="muted center">Loading…</p>
    {:else}
      <!-- A scrollable region takes focus so the keyboard can scroll it. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre
        class="licenses selectable"
        role="region"
        aria-label="Licenses"
        tabindex="0">{licenses}</pre>
    {/if}
  {:else}
    <img class="logo" src="/app-icon.png" alt="" width="96" height="96" />
    <h1>Shayan's Download Manager</h1>
    <p class="version muted">Version {info?.version ?? ""}</p>
    <p class="blurb">
      Fast, resumable downloads over several connections, with clipboard link
      grabbing, a Chrome extension and media-site support.
    </p>

    <section aria-label="Components">
      <h2>Components</h2>
      {#each rows as row (row.name)}
        <div class="line">
          <span>{row.name}</span>
          <span class="mono selectable" class:muted={row.missing}
            >{row.value}</span
          >
        </div>
      {/each}
    </section>

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="links">
      <button
        type="button"
        onclick={() => void act(() => bridge.openHomepage())}
      >
        {(info?.homepage ?? "").replace(/^https?:\/\//, "")}
      </button>
      <button type="button" onclick={() => void openLicenses()}>
        Open-source licenses ›
      </button>
    </div>
    <p class="caption muted footer">© 2026 Shayan Ostadhassan · MIT License</p>
  {/if}
</div>

<style>
  .window {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 24px 28px 16px;
    overflow: hidden;
    background: var(--surface-primary);
    color: var(--text-primary);
  }
  .logo {
    margin-bottom: 8px;
  }
  h1 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    text-align: center;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  .muted {
    color: var(--text-secondary);
  }
  .version {
    margin: 2px 0 10px;
  }
  .blurb {
    margin: 0 0 14px;
    text-align: center;
    color: var(--text-secondary);
  }
  section {
    align-self: stretch;
    padding: 12px 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 2px 0;
  }
  .links {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    margin-top: auto;
    padding-top: 14px;
  }
  .footer {
    margin: 12px 0 0;
  }
  .error {
    margin: 8px 0 0;
    color: var(--status-failed);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    align-self: stretch;
    margin-bottom: 10px;
  }
  .bar h2 {
    margin: 0;
  }
  .center {
    margin: auto;
  }
  .licenses {
    flex: 1;
    align-self: stretch;
    min-height: 0;
    margin: 0;
    padding: 8px;
    overflow: auto;
    font:
      11px/1.35 ui-monospace,
      monospace;
    white-space: pre-wrap;
    word-break: break-word;
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
</style>
