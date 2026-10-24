<script lang="ts">
  /** A labelled row with a popup menu on the right. */
  let {
    label,
    value,
    options,
    width = 220,
    onchange,
  }: {
    label: string;
    value: string | number;
    options: { value: string | number; label: string }[];
    width?: number;
    onchange: (value: string) => void;
  } = $props();
</script>

<div class="line">
  <span>{label}</span>
  <select
    aria-label={label}
    style:width="{width}px"
    value={String(value)}
    onchange={(event) => onchange(event.currentTarget.value)}
  >
    {#each options as option (option.value)}
      <option value={String(option.value)}>{option.label}</option>
    {/each}
  </select>
</div>

<style>
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  select {
    font: inherit;
    padding: 3px 24px 3px 8px;
    color: var(--text-primary);
    background-color: var(--surface-tertiary);
    border: 1px solid var(--border);
    border-radius: 6px;
    appearance: none;
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='10' height='14' viewBox='0 0 10 14' fill='none' stroke='white' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'><path d='M2 5l3-3 3 3M2 9l3 3 3-3'/></svg>");
    background-repeat: no-repeat;
    background-position: right 7px center;
  }
</style>
