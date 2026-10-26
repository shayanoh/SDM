/**
 * The click reaction every button shares: a ripple that grows from the point
 * of the press (from the center when a key activated the button), like a
 * Material button. The pressed and hover tints are plain CSS in `theme.css`;
 * only the ripple needs script, because its origin is where the pointer went
 * down.
 */

const RIPPLE_MS = 450;
const MAX_RIPPLES_PER_BUTTON = 4;

/** The button a press should react on, or null for anything inert. */
function pressTarget(event: Event): HTMLButtonElement | null {
  const target = event.target;
  if (!(target instanceof Element)) return null;
  const button = target.closest("button");
  if (!button || button.disabled) return null;
  if (button.getAttribute("aria-disabled") === "true") return null;
  // Menu entries close the menu on press; a ripple there would only flash.
  if (button.getAttribute("role") === "menuitem") return null;
  return button;
}

/** Starts a ripple at (`x`, `y`) in the button's own coordinates. */
export function spawnRipple(
  button: HTMLElement,
  x: number,
  y: number,
  width: number,
  height: number,
): HTMLElement | null {
  if (![x, y, width, height].every(Number.isFinite)) return null;
  if (width <= 0 || height <= 0) return null;
  if (
    button.querySelectorAll(":scope > .press-ripple").length >=
    MAX_RIPPLES_PER_BUTTON
  )
    return null;
  // Big enough to reach the farthest corner from the press point.
  const radius = Math.hypot(Math.max(x, width - x), Math.max(y, height - y));
  const ripple = button.ownerDocument.createElement("span");
  ripple.className = "press-ripple";
  ripple.setAttribute("aria-hidden", "true");
  ripple.style.width = ripple.style.height = `${radius * 2}px`;
  ripple.style.left = `${x - radius}px`;
  ripple.style.top = `${y - radius}px`;
  const remove = () => ripple.remove();
  ripple.addEventListener("animationend", remove, { once: true });
  // Where animations do not run (reduced motion, tests) the animation never
  // ends, so the timer is the way out.
  setTimeout(remove, RIPPLE_MS * 2);
  button.append(ripple);
  return ripple;
}

/** Wires the ripple to every button in `doc`, present and future. Returns the undo. */
export function installPressFeedback(doc: Document = document): () => void {
  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0) return;
    const button = pressTarget(event);
    if (!button) return;
    const rect = button.getBoundingClientRect();
    spawnRipple(
      button,
      event.clientX - rect.left,
      event.clientY - rect.top,
      rect.width,
      rect.height,
    );
  };
  // A click with no pointer behind it (Enter or Space on a focused button).
  const onClick = (event: MouseEvent) => {
    if (event.detail !== 0) return;
    const button = pressTarget(event);
    if (!button) return;
    const rect = button.getBoundingClientRect();
    spawnRipple(
      button,
      rect.width / 2,
      rect.height / 2,
      rect.width,
      rect.height,
    );
  };
  doc.addEventListener("pointerdown", onPointerDown, true);
  doc.addEventListener("click", onClick, true);
  return () => {
    doc.removeEventListener("pointerdown", onPointerDown, true);
    doc.removeEventListener("click", onClick, true);
  };
}
