<script lang="ts">
  import type { ItemStateDto } from "../bindings";
  import { stateIcon } from "../lib/stateIcon";
  import Icon from "./Icon.svelte";

  /**
   * The row's state glyph. Running, queued and stopped items draw a ring whose
   * arc is the completed fraction (SwiftUI's variable-value symbol); the
   * others use a plain glyph.
   */
  let {
    state,
    fraction,
    isRetrying,
    isAssembling,
  }: {
    state: ItemStateDto;
    fraction: number;
    isRetrying: boolean;
    isAssembling: boolean;
  } = $props();

  const RADIUS = 9.5;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

  const spec = $derived(stateIcon(state, isRetrying, isAssembling));
  const arc = $derived(
    CIRCUMFERENCE *
      Math.min(1, Math.max(0, Number.isFinite(fraction) ? fraction : 0)),
  );
</script>

<span class="icon tone-{spec.tone}">
  {#if spec.ring}
    <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
      <g
        data-ring
        fill="none"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-width="1.7"
      >
        <circle cx="12" cy="12" r={RADIUS} opacity="0.3" />
        {#if arc > 0}
          <circle
            cx="12"
            cy="12"
            r={RADIUS}
            stroke-dasharray="{arc} {CIRCUMFERENCE}"
            transform="rotate(-90 12 12)"
          />
        {/if}
        {#if spec.glyph === "download"}
          <path d="M12 7.5v8M8.5 12.2 12 15.7l3.5-3.5" />
        {:else if spec.glyph === "pause"}
          <path d="M10 9v6M14 9v6" />
        {:else if spec.glyph === "clock"}
          <path d="M12 7.5V12l3 1.8" />
        {:else}
          <path d="M12 7.5v1.5M12 11v5" />
        {/if}
      </g>
    </svg>
  {:else if spec.glyph === "done"}
    <Icon name="circle-check" size={22} strokeWidth={1.7} />
  {:else if spec.glyph === "assembling"}
    <Icon name="wand-sparkles" size={22} strokeWidth={1.7} />
  {:else}
    <Icon name="rotate-ccw" size={22} strokeWidth={1.7} />
  {/if}
</span>

<style>
  .icon {
    display: inline-flex;
    width: 22px;
    height: 22px;
    flex: none;
  }
  .tone-accent {
    color: var(--accent);
  }
  .tone-online {
    color: var(--status-online);
  }
  .tone-failed {
    color: var(--status-failed);
  }
  .tone-faulty {
    color: var(--status-faulty);
  }
  .tone-secondary {
    color: var(--text-secondary);
  }
</style>
