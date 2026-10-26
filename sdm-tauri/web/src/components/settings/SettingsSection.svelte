<script lang="ts">
  import type { Snippet } from "svelte";

  /** A bold heading over a bordered card, as the SwiftUI Settings sections look. */
  let { title, children }: { title: string; children: Snippet } = $props();
</script>

<section>
  <h3 class="headline">{title}</h3>
  <div class="card">{@render children()}</div>
</section>

<style>
  section {
    min-width: 0;
  }
  h3 {
    margin: 0 0 6px;
    font-size: 15px;
    font-weight: 600;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px;
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
</style>
