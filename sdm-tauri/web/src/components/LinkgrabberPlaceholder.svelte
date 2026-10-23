<script lang="ts">
  import TemporaryTools from "./TemporaryTools.svelte";
</script>

<div class="placeholder">
  <p class="message">The link grabber arrives in the next release.</p>
  <p class="muted">Until then, paste a link here to download it directly.</p>
  <TemporaryTools />
</div>

<style>
  .placeholder {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
  }
  .message {
    margin: 0;
    font-size: 15px;
  }
  .muted {
    margin: 0 0 12px;
    color: var(--text-secondary);
  }
</style>
