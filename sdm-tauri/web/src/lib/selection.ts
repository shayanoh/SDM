/**
 * Which rows are selected. Keys are item ids and package ids (a package header
 * is selectable, as in SwiftUI's list). Pure state: the owner supplies the
 * rows' order and is told after every change.
 */
export class SelectionModel {
  #order: string[] = [];
  #selected = new Set<string>();
  #anchor: string | null = null;
  #onChange: ((selected: ReadonlySet<string>) => void) | undefined;

  constructor(onChange?: (selected: ReadonlySet<string>) => void) {
    this.#onChange = onChange;
  }

  get selected(): ReadonlySet<string> {
    return this.#selected;
  }

  /** The visible rows' keys, top to bottom. */
  setOrder(keys: string[]) {
    this.#order = keys;
  }

  click(key: string) {
    this.#replace(new Set([key]));
    this.#anchor = key;
  }

  toggle(key: string) {
    const next = new Set(this.#selected);
    if (!next.delete(key)) next.add(key);
    this.#replace(next);
    this.#anchor = key;
  }

  /** Selects the rows from the anchor to `key`, both included. */
  range(key: string) {
    const to = this.#order.indexOf(key);
    const from = this.#anchor === null ? -1 : this.#order.indexOf(this.#anchor);
    if (to < 0 || from < 0) {
      this.click(key);
      return;
    }
    const [low, high] = from <= to ? [from, to] : [to, from];
    this.#replace(new Set(this.#order.slice(low, high + 1)));
  }

  selectAll(keys: string[]) {
    this.#replace(new Set(keys));
  }

  clear() {
    this.#replace(new Set());
    this.#anchor = null;
  }

  /** Forgets keys that are no longer rows. */
  prune(live: ReadonlySet<string>) {
    const kept = new Set([...this.#selected].filter((key) => live.has(key)));
    this.#replace(kept);
    if (this.#anchor !== null && !live.has(this.#anchor)) this.#anchor = null;
  }

  /**
   * What a context menu on `key` acts on: the whole selection when it holds
   * the row (in list order), otherwise just that row, which becomes the
   * selection.
   */
  contextTarget(key: string): string[] {
    if (this.#selected.has(key)) {
      const rank = (k: string) => {
        const at = this.#order.indexOf(k);
        return at < 0 ? Number.MAX_SAFE_INTEGER : at;
      };
      return [...this.#selected].sort((a, b) => rank(a) - rank(b));
    }
    this.click(key);
    return [key];
  }

  #replace(next: Set<string>) {
    if (
      next.size === this.#selected.size &&
      [...next].every((k) => this.#selected.has(k))
    )
      return;
    this.#selected = next;
    this.#onChange?.(next);
  }
}
