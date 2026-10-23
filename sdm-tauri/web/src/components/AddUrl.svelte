<script lang="ts">
  import { errorMessage, useBridge, useUi } from "../lib/context";

  const bridge = useBridge();
  const ui = useUi();

  let url = $state("");
  let busy = $state(false);

  async function submit(event: SubmitEvent) {
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
</script>

<form onsubmit={submit}>
  <input
    type="text"
    placeholder="Paste a link to download"
    aria-label="Download address"
    bind:value={url}
  />
  <button type="submit" disabled={busy}>Add</button>
</form>

<style>
  form {
    display: flex;
    gap: 8px;
    padding: 8px 12px;
  }
  input {
    flex: 1;
    padding: 6px 8px;
    color: var(--text-primary);
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
</style>
