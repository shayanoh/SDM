<script lang="ts">
  import { useStore, useUi } from "../lib/context";
  import type { Tab } from "../lib/ui.svelte";
  import Icon from "./Icon.svelte";
  import OverviewBlock from "./OverviewBlock.svelte";

  const store = useStore();
  const ui = useUi();

  const stats = $derived(store.stats);
  const running = $derived(stats?.runningCount ?? 0);
  const queued = $derived(stats?.queuedCount ?? 0);
  const fraction = $derived(
    Math.min(1, Math.max(0, stats?.overallFraction ?? 0)),
  );

  const RING_RADIUS = 6;
  const RING = 2 * Math.PI * RING_RADIUS;

  const rows: {
    tab: Tab;
    label: string;
    icon: "circle-arrow-down" | "circle-check" | "link";
  }[] = [
    { tab: "downloads", label: "Downloads", icon: "circle-arrow-down" },
    { tab: "completed", label: "Completed", icon: "circle-check" },
    { tab: "linkgrabber", label: "Linkgrabber", icon: "link" },
  ];
</script>

<nav class="sidebar" aria-label="Sidebar">
  <ul>
    {#each rows as row (row.tab)}
      <li>
        <button
          type="button"
          class="row"
          class:selected={ui.tab === row.tab}
          aria-current={ui.tab === row.tab ? "page" : undefined}
          onclick={() => ui.setTab(row.tab)}
        >
          <span
            class="icon"
            class:breathing={row.tab === "downloads" && running > 0}
          >
            <Icon name={row.icon} size={16} />
          </span>
          <span class="label">{row.label}</span>
          {#if row.tab === "downloads" && running > 0}
            <span class="badge caption">{running}/{running + queued}</span>
            <svg
              class="ring"
              data-testid="downloads-ring"
              viewBox="0 0 16 16"
              width="16"
              height="16"
              aria-hidden="true"
            >
              <g fill="none" stroke="var(--accent)" stroke-width="2.4">
                <circle cx="8" cy="8" r={RING_RADIUS} opacity="0.25" />
                <circle
                  cx="8"
                  cy="8"
                  r={RING_RADIUS}
                  stroke-dasharray="{RING * fraction} {RING}"
                  transform="rotate(-90 8 8)"
                />
              </g>
            </svg>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
  <section class="overview" aria-label="Overview">
    <h2 class="caption section">Overview</h2>
    <OverviewBlock />
  </section>
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: 100%;
    height: 100%;
    padding-top: 8px;
    overflow: hidden;
    background: var(--sidebar-background);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 0 14px;
    height: 28px;
    text-align: left;
    color: var(--text-primary);
    background: none;
    border: none;
    border-radius: 0;
  }
  .row.selected {
    background: var(--selection-tint);
  }
  .icon {
    display: inline-flex;
    color: var(--accent);
  }
  .breathing {
    animation: breathe 2.4s ease-in-out infinite;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    padding: 3px 8px;
    font-weight: 700;
    color: var(--text-primary);
    background: var(--status-online);
    border-radius: 999px;
  }
  .ring {
    flex: none;
  }
  .overview {
    padding: 0 14px;
  }
  .section {
    margin: 0 0 6px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  @keyframes breathe {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.55;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .breathing {
      animation: none;
    }
  }
</style>
