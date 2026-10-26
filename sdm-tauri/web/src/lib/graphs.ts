export interface Range {
  start: number;
  end: number;
}

const round2 = (value: number) => Number(value.toFixed(2));

/**
 * The newest `width` samples (one per pixel), left-padded with zeros when
 * there are fewer, so a graph does not rescale while it is still filling.
 */
export function windowed(samples: number[], width: number): number[] {
  const wanted = Math.round(width);
  if (!(wanted > 0)) return [];
  if (samples.length >= wanted) return samples.slice(samples.length - wanted);
  return [...Array<number>(wanted - samples.length).fill(0), ...samples];
}

/**
 * The `points` of a one-pixel sparkline path, y-scaled to the series' own
 * maximum (top of the box is the maximum). Empty when there is nothing to
 * draw: fewer than two points or no positive sample.
 */
export function sparklinePoints(
  samples: number[],
  width: number,
  height: number,
): string {
  const series = windowed(samples, width).map((v) =>
    Number.isFinite(v) && v > 0 ? v : 0,
  );
  if (series.length < 2) return "";
  const peak = Math.max(...series);
  if (!(peak > 0)) return "";
  const step = width / (series.length - 1);
  return series
    .map(
      (value, index) =>
        `${round2(index * step)},${round2(height - (value / peak) * height)}`,
    )
    .join(" ");
}

/**
 * Port of `BandwidthGraph.runningAverage`: the mean of the last `window`
 * samples, and zero until a full window has been seen.
 */
export function runningAverage(values: number[], window: number): number[] {
  const result: number[] = [];
  let sum = 0;
  let count = 0;
  values.forEach((value, index) => {
    sum += value;
    count += 1;
    if (count >= window) {
      result.push(sum / count);
      count -= 1;
      sum -= values[index - window + 1];
    } else {
      result.push(0);
    }
  });
  return result;
}

/** Every part's ranges shifted by its offset, merged where they overlap or touch. */
export function unionRanges(
  parts: { ranges: Range[]; offset: number }[],
): Range[] {
  const all = parts
    .flatMap(({ ranges, offset }) =>
      ranges.map((r) => ({ start: r.start + offset, end: r.end + offset })),
    )
    .filter(
      (r) =>
        Number.isFinite(r.start) && Number.isFinite(r.end) && r.end > r.start,
    )
    .sort((a, b) => a.start - b.start);
  const merged: Range[] = [];
  for (const range of all) {
    const last = merged.at(-1);
    if (last && range.start <= last.end)
      last.end = Math.max(last.end, range.end);
    else merged.push({ ...range });
  }
  return merged;
}
