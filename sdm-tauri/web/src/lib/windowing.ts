export interface WindowInput {
  scrollTop: number;
  viewport: number;
  rowHeight: number;
  count: number;
  overscan: number;
}

/** The half-open range of rows to render. Fixed row height. */
export function visibleRange({
  scrollTop,
  viewport,
  rowHeight,
  count,
  overscan,
}: WindowInput) {
  if (!(rowHeight > 0) || count <= 0) return { start: 0, end: 0 };
  const top = Number.isFinite(scrollTop) ? Math.max(0, scrollTop) : 0;
  const height = Number.isFinite(viewport) ? Math.max(0, viewport) : 0;
  // Clamp to the list before padding, so an overscrolled position still
  // renders the last rows instead of an empty range.
  const first = Math.min(count, Math.floor(top / rowHeight));
  const last = Math.min(count, Math.ceil((top + height) / rowHeight));
  const pad = Math.max(0, overscan);
  return {
    start: Math.max(0, first - pad),
    end: Math.min(count, last + pad),
  };
}
