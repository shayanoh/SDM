<script lang="ts">
  import { onMount, type Snippet } from "svelte";

  /**
   * A centered dialog over a dimmed window: focus moves in and comes back,
   * Tab stays inside, Escape cancels and Enter presses the default button
   * (`data-default`). Clicking outside does nothing, like a sheet.
   */
  let { onCancel, children }: { onCancel: () => void; children: Snippet } =
    $props();

  let dialog: HTMLDivElement | undefined = $state();

  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const first =
      dialog?.querySelector<HTMLElement>("[data-autofocus]") ??
      dialog?.querySelector<HTMLElement>("button:not(:disabled), input");
    first?.focus();
    return () => previous?.focus?.();
  });

  function focusables(): HTMLElement[] {
    return [
      ...(dialog?.querySelectorAll<HTMLElement>(
        "button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex='-1'])",
      ) ?? []),
    ];
  }

  function onKey(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onCancel();
    } else if (event.key === "Enter") {
      event.preventDefault();
      dialog
        ?.querySelector<HTMLButtonElement>("[data-default]:not(:disabled)")
        ?.click();
    } else if (event.key === "Tab") {
      const items = focusables();
      if (items.length === 0) return;
      const at = items.indexOf(document.activeElement as HTMLElement);
      const next = event.shiftKey
        ? at <= 0
          ? items.length - 1
          : at - 1
        : (at + 1) % items.length;
      event.preventDefault();
      items[next].focus();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="modal-title"
    bind:this={dialog}
  >
    {@render children()}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.35);
  }
  .dialog {
    min-width: 440px;
    max-width: min(640px, calc(100vw - 32px));
    padding: 24px;
    background: var(--surface-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.5);
  }
</style>
