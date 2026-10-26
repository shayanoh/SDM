/**
 * Keeps the webview's own right-click menu (Reload, Share, Inspect…) from
 * appearing. SDM's windows have menus of their own where they want one; a
 * right-click anywhere else does nothing. Text fields keep the system menu,
 * for Cut, Copy and Paste.
 */

/** True for an element whose own context menu is useful: somewhere text is typed. */
export function wantsNativeMenu(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const field = target.closest("input, textarea, [contenteditable]");
  if (!field) return false;
  if (field instanceof HTMLInputElement) {
    // A checkbox or button has no text to cut or paste.
    return ![
      "button",
      "checkbox",
      "radio",
      "range",
      "color",
      "file",
      "submit",
      "reset",
      "image",
    ].includes(field.type);
  }
  return true;
}

/** Blocks the webview's menu in `doc`, except in text fields. Returns the undo. */
export function installNativeMenuBlock(doc: Document = document): () => void {
  const onContextMenu = (event: MouseEvent) => {
    if (!wantsNativeMenu(event.target)) event.preventDefault();
  };
  doc.addEventListener("contextmenu", onContextMenu);
  return () => doc.removeEventListener("contextmenu", onContextMenu);
}
