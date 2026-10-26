<script lang="ts">
  import { onMount } from "svelte";
  import type { ChromeExtensionDto } from "../bindings";
  import { statusLine } from "../lib/chromeSetup";
  import { errorMessage, useBridge } from "../lib/context";
  import { afterPaint } from "../lib/paint";
  import { firstThemePaint } from "../lib/theme";
  import { detectPlatform, type Platform } from "../lib/platform";
  import { closeWindow, fitWindow } from "../lib/windowControl";

  /** The Chrome extension setup window: the Load-unpacked steps, and "all set" once the extension reports in. */
  let {
    onClose = closeWindow,
    clipboard = navigator.clipboard,
    platform = detectPlatform(),
    themeReady = firstThemePaint,
  }: {
    onClose?: () => void | Promise<void>;
    clipboard?: Pick<Clipboard, "writeText">;
    platform?: Platform;
    /** Settles when the theme is painted; the window is shown after it. */
    themeReady?: Promise<void>;
  } = $props();

  const EXTENSIONS_ADDRESS = "chrome://extensions";

  const WIDTH = 560;
  const bridge = useBridge();

  let status = $state<ChromeExtensionDto | null>(null);
  let folder = $state<string | null>(null);
  let error = $state<string | null>(null);
  let height = $state(0);

  const done = $derived(
    status?.state === "connected" &&
      status.installedVersion === status.bundledVersion &&
      status.cookiesSyncedSecondsAgo !== null,
  );

  async function refresh() {
    try {
      status = await bridge.chromeExtensionStatus();
    } catch (failure) {
      error = errorMessage(failure);
    }
  }

  async function act(call: () => Promise<unknown>) {
    try {
      await call();
      error = null;
    } catch (failure) {
      error = errorMessage(failure);
    }
  }

  onMount(() => {
    void act(async () => {
      folder = await bridge.prepareChromeExtension();
    });
    void refresh();
    const timer = setInterval(refresh, 1000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    if (height > 0) void fitWindow(WIDTH, height);
  });

  // The window starts hidden: shown once, with its status in, the theme
  // painted and the window sized.
  let revealed = false;
  $effect(() => {
    if (revealed || !status) return;
    revealed = true;
    void (async () => {
      await themeReady;
      await afterPaint();
      await fitWindow(WIDTH, height);
      await afterPaint();
      await bridge.windowReady();
    })().catch(() => {});
  });
</script>

<div class="window" style:width="{WIDTH}px" bind:clientHeight={height}>
  {#if done && status}
    <div class="done">
      <div class="check" aria-hidden="true">✓</div>
      <h1>You're all set!</h1>
      <p class="muted">
        SDM Chrome Extension {status.installedVersion} is installed.
      </p>
      <button type="button" class="default" onclick={() => onClose()}>
        Done
      </button>
    </div>
  {:else}
    <h1>Install the Chrome Extension</h1>
    <p class="muted">
      Follow these steps to let SDM capture downloads from Google Chrome and use
      its cookies for yt-dlp.
    </p>

    <ol>
      <li>
        <span class="n">1</span>
        <div>
          <strong>Open Chrome's Extensions page</strong>
          {#if platform !== "mac"}
            <!-- On Windows and Linux Chrome ignores a chrome:// address another
                 program hands it (it opens an empty window instead), so the
                 user pastes it. Only the Mac's `open -a` route works. -->
            <p class="muted">
              In Chrome, open a new tab, paste this address and press Enter.
            </p>
            <div class="row">
              <input
                type="text"
                readonly
                aria-label="Extensions page address"
                value={EXTENSIONS_ADDRESS}
                onfocus={(event) => event.currentTarget.select()}
              />
              <button
                type="button"
                onclick={() =>
                  void act(() => clipboard.writeText(EXTENSIONS_ADDRESS))}
              >
                Copy Address
              </button>
            </div>
          {:else}
            <div class="row">
              <button
                type="button"
                disabled={status ? !status.chromeInstalled : false}
                onclick={() => void act(() => bridge.openChromeExtensions())}
              >
                Open Chrome Extensions
              </button>
            </div>
          {/if}
        </div>
      </li>
      <li>
        <span class="n">2</span>
        <div>
          <strong>Enable Developer mode</strong>
          <p class="muted">
            In the top-right corner of the Extensions page, turn on Developer
            mode.
          </p>
          <img src="/chrome-developer-mode.png" alt="Developer mode switch" />
        </div>
      </li>
      <li>
        <span class="n">3</span>
        <div>
          <strong>Choose Load unpacked</strong>
          <p class="muted">
            Click Load unpacked. A folder selection dialog will appear.
          </p>
          <img src="/chrome-load-unpacked.png" alt="Load unpacked button" />
        </div>
      </li>
      <li>
        <span class="n">4</span>
        <div>
          <strong>Select the SDM extension folder</strong>
          <p class="muted">
            Show the folder, or copy its path into the dialog, then click
            Select.
          </p>
          {#if folder}<code>{folder}</code>{/if}
          <div class="row">
            <button
              type="button"
              onclick={() =>
                void act(() => bridge.revealChromeExtensionFolder())}
            >
              Show Folder
            </button>
            <button
              type="button"
              disabled={!folder}
              onclick={() => void act(() => clipboard.writeText(folder ?? ""))}
            >
              Copy Path
            </button>
          </div>
        </div>
      </li>
    </ol>

    {#if status}<p class="status">{statusLine(status)}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="bar">
      <button type="button" class="default" onclick={() => onClose()}>
        I've installed the extension
      </button>
    </div>
  {/if}
</div>

<style>
  .window {
    box-sizing: border-box;
    padding: 24px 28px;
    background: var(--surface-primary);
    color: var(--text-primary);
  }
  h1 {
    margin: 0 0 6px;
    font-size: 20px;
    font-weight: 600;
  }
  .muted {
    margin: 0 0 8px;
    color: var(--text-secondary);
  }
  ol {
    display: flex;
    flex-direction: column;
    gap: 16px;
    margin: 16px 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    gap: 10px;
  }
  .n {
    flex: none;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    font-size: 12px;
    font-weight: 700;
    text-align: center;
    line-height: 22px;
  }
  img {
    display: block;
    max-width: 100%;
    max-height: 100px;
    margin: 4px auto 0;
  }
  code {
    display: block;
    margin: 4px 0;
    padding: 6px 8px;
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 13px;
    word-break: break-all;
  }
  .row {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }
  input {
    flex: 1;
    min-width: 0;
    padding: 4px 8px;
    font: inherit;
    font-family: ui-monospace, monospace;
    color: var(--text-primary);
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .status {
    margin: 0 0 8px;
    color: var(--text-secondary);
  }
  .error {
    margin: 0 0 8px;
    color: var(--status-failed);
  }
  .bar {
    display: flex;
    justify-content: center;
  }
  button {
    padding: 4px 14px;
    font-size: 15px;
  }
  .default {
    color: #fff;
    background: var(--accent);
    border-color: var(--accent);
  }
  .done {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 24px 0;
  }
  .check {
    font-size: 64px;
    color: var(--status-completed, #34c759);
  }
</style>
