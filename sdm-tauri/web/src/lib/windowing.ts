/** Running sums of the row heights: `offsets[i]` is the top of row `i`, the last entry the total. */
export function buildOffsets(heights: number[]): number[] {
  const offsets = [0];
  let total = 0;
  for (const height of heights) {
    total += Number.isFinite(height) && height > 0 ? height : 0;
    offsets.push(total);
  }
  return offsets;
}

export interface WindowInput {
  /** From [`buildOffsets`]. */
  offsets: number[];
  scrollTop: number;
  viewport: number;
  overscan: number;
}

/** The half-open range of rows to render, for rows of any heights. */
export function visibleRange({
  offsets,
  scrollTop,
  viewport,
  overscan,
}: WindowInput) {
  const count = offsets.length - 1;
  if (count <= 0) return { start: 0, end: 0 };
  const top = Number.isFinite(scrollTop) ? Math.max(0, scrollTop) : 0;
  const height = Number.isFinite(viewport) ? Math.max(0, viewport) : 0;

  // The row holding `top`: the first whose bottom edge is below it. Past the
  // end this is `count`, so an overscrolled position still renders the last rows.
  let lo = 0;
  let hi = count;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (offsets[mid + 1] > top) hi = mid;
    else lo = mid + 1;
  }
  const first = lo;

  // One past the last row that starts above the viewport's bottom edge.
  const bottom = top + height;
  lo = 0;
  hi = count;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (offsets[mid] >= bottom) hi = mid;
    else lo = mid + 1;
  }
  const last = lo;

  const pad = Math.max(0, overscan);
  return { start: Math.max(0, first - pad), end: Math.min(count, last + pad) };
}
