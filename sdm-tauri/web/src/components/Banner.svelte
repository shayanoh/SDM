<script lang="ts">
  import { useUi } from "../lib/context";

  const ui = useUi();
</script>

{#if ui.error}
  <div class="banner" role="alert">
    <span>{ui.error}</span>
    <button type="button" aria-label="Dismiss" onclick={() => ui.dismiss()}>
      ×
    </button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 12px;
    color: var(--status-failed);
    background: var(--surface-secondary);
    border-bottom: 1px solid var(--border);
  }
  button {
    border: none;
    background: none;
    color: inherit;
    cursor: pointer;
    font-size: 1.2em;
  }
</style>
