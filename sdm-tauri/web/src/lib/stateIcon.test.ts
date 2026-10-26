import { describe, expect, it } from "vitest";
import type { ItemStateDto } from "../bindings";
import { stateIcon } from "./stateIcon";

const st = (kind: ItemStateDto["kind"]): ItemStateDto =>
  kind === "failed" ? { kind, reason: "x" } : ({ kind } as ItemStateDto);

describe("stateIcon (ItemRowBody.stateIconName / stateIconColor)", () => {
  it("maps each state to its glyph, tone and whether it draws a progress ring", () => {
    expect(stateIcon(st("running"), false, false)).toEqual({
      glyph: "download",
      tone: "accent",
      ring: true,
    });
    expect(stateIcon(st("queued"), false, false)).toEqual({
      glyph: "clock",
      tone: "secondary",
      ring: true,
    });
    expect(stateIcon(st("stopped"), false, false)).toEqual({
      glyph: "pause",
      tone: "secondary",
      ring: true,
    });
    expect(stateIcon(st("completed"), false, false)).toEqual({
      glyph: "done",
      tone: "online",
      ring: false,
    });
    expect(stateIcon(st("failed"), false, false)).toEqual({
      glyph: "retry",
      tone: "failed",
      ring: false,
    });
  });

  it("shows a retrying item with the retry glyph in the faulty tone, ring off", () => {
    expect(stateIcon(st("queued"), true, false)).toEqual({
      glyph: "retry",
      tone: "faulty",
      ring: false,
    });
  });

  it("lets assembling win the glyph but keeps the state's tone", () => {
    expect(stateIcon(st("running"), false, true)).toEqual({
      glyph: "assembling",
      tone: "accent",
      ring: true,
    });
  });
});
