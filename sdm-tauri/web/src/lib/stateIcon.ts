import type { ItemStateDto } from "../bindings";

export type StateGlyph =
  "download" | "clock" | "pause" | "done" | "retry" | "assembling";
export type StateTone = "accent" | "online" | "failed" | "faulty" | "secondary";

export interface StateIconSpec {
  glyph: StateGlyph;
  tone: StateTone;
  /** Draw a progress ring around the glyph (the SwiftUI symbol's variable value). */
  ring: boolean;
}

/**
 * Port of `ItemRowBody.stateIconName` / `stateIconColor` /
 * `stateIconVariableValue`: an assembling item shows the wand, a retrying one
 * the retry glyph in the faulty tone; otherwise the state decides.
 */
export function stateIcon(
  state: ItemStateDto,
  isRetrying: boolean,
  isAssembling: boolean,
): StateIconSpec {
  const ring =
    !isRetrying && ["running", "queued", "stopped"].includes(state.kind);
  const tone: StateTone = isRetrying
    ? "faulty"
    : state.kind === "running"
      ? "accent"
      : state.kind === "completed"
        ? "online"
        : state.kind === "failed"
          ? "failed"
          : "secondary";
  let glyph: StateGlyph;
  if (isAssembling) glyph = "assembling";
  else if (isRetrying) glyph = "retry";
  else
    glyph = {
      running: "download",
      completed: "done",
      failed: "retry",
      queued: "clock",
      stopped: "pause",
    }[state.kind] as StateGlyph;
  return { glyph, tone, ring };
}
