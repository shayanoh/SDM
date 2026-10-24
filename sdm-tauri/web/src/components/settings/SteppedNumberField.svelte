<script lang="ts">
  import Icon from "../Icon.svelte";

  /** "Label  [ 12 ]▲▼ 1..20": a number field with its range spelled out. */
  let {
    label,
    value,
    min,
    max,
    onchange,
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    onchange: (value: number) => void;
  } = $props();

  const id = $props.id();

  const clamp = (n: number) => Math.min(max, Math.max(min, n));

  // While typing, only a number already in range is taken: "1" on the way to
  // "15" in a 10..200 field must not be turned into 10 under the caret.
  function onInput(event: Event & { currentTarget: HTMLInputElement }) {
    const parsed = Number.parseInt(event.currentTarget.value, 10);
    if (Number.isFinite(parsed) && parsed >= min && parsed <= max)
      onchange(parsed);
  }

  /** Takes what is typed, clamped, and shows the number that results. */
  function settle(input: HTMLInputElement) {
    const parsed = Number.parseInt(input.value, 10);
    const next = Number.isFinite(parsed) ? clamp(parsed) : value;
    onchange(next);
    input.value = String(next);
  }

  // Leaving the field, or Enter (which also presses OK), settles it.
  const onBlur = (event: FocusEvent & { currentTarget: HTMLInputElement }) =>
    settle(event.currentTarget);
  function onKeyDown(
    event: KeyboardEvent & { currentTarget: HTMLInputElement },
  ) {
    if (event.key === "Enter") settle(event.currentTarget);
  }
</script>

<div class="line">
  <label for={id}>{label}</label>
  <span class="field">
    <input
      {id}
      type="number"
      {min}
      {max}
      value={String(value)}
      oninput={onInput}
      onblur={onBlur}
      onkeydown={onKeyDown}
    />
    <span class="steppers">
      <button
        type="button"
        tabindex="-1"
        aria-label="Increase {label}"
        disabled={value >= max}
        onclick={() => onchange(clamp(value + 1))}
      >
        <Icon name="chevron-up" size={9} strokeWidth={3} />
      </button>
      <button
        type="button"
        tabindex="-1"
        aria-label="Decrease {label}"
        disabled={value <= min}
        onclick={() => onchange(clamp(value - 1))}
      >
        <Icon name="chevron-down" size={9} strokeWidth={3} />
      </button>
    </span>
    <span class="hint">{min}..{max}</span>
  </span>
</div>

<style>
  .line {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  label {
    flex: none;
    width: 215px;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  input {
    width: 54px;
    padding: 3px 6px;
    text-align: right;
    color: var(--text-primary);
    background: var(--surface-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    appearance: textfield;
    -moz-appearance: textfield;
    user-select: text;
    cursor: text;
  }
  input::-webkit-inner-spin-button,
  input::-webkit-outer-spin-button {
    appearance: none;
    margin: 0;
  }
  .steppers {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .steppers button {
    display: grid;
    place-items: center;
    width: 14px;
    height: 12px;
    padding: 0;
    background: var(--surface-tertiary);
    border: none;
    border-radius: 3px;
  }
  .hint {
    margin-left: 2px;
    color: var(--text-tertiary);
  }
</style>
