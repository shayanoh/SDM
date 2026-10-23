import type { MenuId } from "./menu";
import type { Platform } from "./platform";

export interface ShortcutContext {
  platform: Platform;
  modalOpen: boolean;
  menuOpen: boolean;
  /** A text field has focus, so editing keys belong to it. */
  focusInEditable: boolean;
  run(id: MenuId): void;
  selectAll(): void;
  moveSelection(delta: -1 | 1, extend: boolean): void;
  closeMenu(): void;
}

/**
 * The window's keyboard shortcuts: Enter opens the selected finished files,
 * Mod+Enter shows them in the file manager, Delete removes them from the list,
 * Mod+Delete removes them and trashes the files, Mod+A selects everything and
 * the arrows move the selection. Mod is Ctrl (Command on macOS, where the
 * delete key is Backspace). Returns whether the key was handled; a handled key
 * has its default prevented. Nothing is handled behind a modal, inside a text
 * field, or (apart from Escape) while a menu is open.
 */
export function handleKey(event: KeyboardEvent, ctx: ShortcutContext): boolean {
  if (ctx.modalOpen || event.altKey) return false;
  const mac = ctx.platform === "mac";
  const mod = mac
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
  const plain = !event.ctrlKey && !event.metaKey;
  const handled = (action: () => void) => {
    event.preventDefault();
    action();
    return true;
  };

  if (event.key === "Escape") {
    return ctx.menuOpen && plain && !event.shiftKey
      ? handled(ctx.closeMenu)
      : false;
  }
  if (ctx.menuOpen || ctx.focusInEditable) return false;

  const key = event.key;
  if (key === "Enter" && !event.shiftKey) {
    if (plain) return handled(() => ctx.run("open"));
    if (mod) return handled(() => ctx.run("show"));
  }
  const isDelete = key === "Delete" || (mac && key === "Backspace");
  if (isDelete && !event.shiftKey) {
    if (plain) return handled(() => ctx.run("removeFromList"));
    if (mod) return handled(() => ctx.run("removeAndDelete"));
  }
  if ((key === "a" || key === "A") && mod && !event.shiftKey)
    return handled(ctx.selectAll);
  if ((key === "ArrowDown" || key === "ArrowUp") && plain)
    return handled(() =>
      ctx.moveSelection(key === "ArrowDown" ? 1 : -1, event.shiftKey),
    );
  return false;
}
