import { describe, expect, it } from "vitest";
import { SettingsDraft } from "./settingsDraft.svelte";
import { defaultSettings } from "./settingsModel";

describe("SettingsDraft", () => {
  it("starts clean and takes the stored values", () => {
    const draft = new SettingsDraft({ ...defaultSettings(), maxConcurrent: 5 });
    expect(draft.dirty).toBe(false);
    expect(draft.values.maxConcurrent).toBe(5);
  });

  it("becomes dirty on a change and clean again when put back", () => {
    const draft = new SettingsDraft(defaultSettings());
    draft.set("maxConcurrent", 7);
    expect(draft.dirty).toBe(true);
    draft.set("maxConcurrent", 3);
    expect(draft.dirty).toBe(false);
  });

  it("never sends a download folder", () => {
    const draft = new SettingsDraft({
      ...defaultSettings(),
      downloadFolder: "/somewhere",
    });
    draft.set("maxConcurrent", 2);
    expect("downloadFolder" in draft.toDto()).toBe(false);
    expect(draft.toDto().maxConcurrent).toBe(2);
  });

  it("fills missing fields with the defaults", () => {
    const draft = new SettingsDraft({});
    expect(draft.values).toEqual(defaultSettings());
  });
});
