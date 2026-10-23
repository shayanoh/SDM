import { describe, expect, it } from "vitest";
import type { MenuId } from "./menu";
import type { Platform } from "./platform";
import { handleKey, type ShortcutContext } from "./shortcuts";

function context(over: Partial<ShortcutContext> = {}) {
  const log: string[] = [];
  const ctx: ShortcutContext = {
    platform: "windows",
    modalOpen: false,
    menuOpen: false,
    focusInEditable: false,
    run: (id: MenuId) => log.push(`run:${id}`),
    selectAll: () => log.push("selectAll"),
    moveSelection: (delta, extend) => log.push(`move:${delta}:${extend}`),
    closeMenu: () => log.push("closeMenu"),
    ...over,
  };
  return { ctx, log };
}

function key(init: KeyboardEventInit & { key: string }) {
  return new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    ...init,
  });
}

const press = (
  init: KeyboardEventInit & { key: string },
  over: Partial<ShortcutContext> = {},
) => {
  const { ctx, log } = context(over);
  const event = key(init);
  const handled = handleKey(event, ctx);
  return { handled, log, prevented: event.defaultPrevented };
};

describe("handleKey on Windows and Linux", () => {
  it.each(["windows", "linux"] as Platform[])(
    "binds the keys on %s",
    (platform) => {
      expect(press({ key: "Enter" }, { platform }).log).toEqual(["run:open"]);
      expect(press({ key: "Enter", ctrlKey: true }, { platform }).log).toEqual([
        "run:show",
      ]);
      expect(press({ key: "Delete" }, { platform }).log).toEqual([
        "run:removeFromList",
      ]);
      expect(press({ key: "Delete", ctrlKey: true }, { platform }).log).toEqual(
        ["run:removeAndDelete"],
      );
      expect(press({ key: "a", ctrlKey: true }, { platform }).log).toEqual([
        "selectAll",
      ]);
      expect(press({ key: "A", ctrlKey: true }, { platform }).log).toEqual([
        "selectAll",
      ]);
      expect(press({ key: "ArrowDown" }, { platform }).log).toEqual([
        "move:1:false",
      ]);
      expect(
        press({ key: "ArrowUp", shiftKey: true }, { platform }).log,
      ).toEqual(["move:-1:true"]);
    },
  );

  it("treats the command key as nothing special", () => {
    expect(press({ key: "a", metaKey: true }).handled).toBe(false);
    expect(press({ key: "Backspace" }).handled).toBe(false);
  });

  it("marks handled keys as handled and prevents their default", () => {
    const result = press({ key: "Enter" });
    expect(result.handled).toBe(true);
    expect(result.prevented).toBe(true);
  });

  it("leaves other keys and other modifier mixes alone", () => {
    for (const init of [
      { key: "x" },
      { key: "Enter", altKey: true },
      { key: "Enter", shiftKey: true },
      { key: "a", ctrlKey: true, shiftKey: true },
      { key: "Delete", shiftKey: true },
    ]) {
      const result = press(init);
      expect(result.handled).toBe(false);
      expect(result.log).toEqual([]);
      expect(result.prevented).toBe(false);
    }
  });
});

describe("handleKey on macOS", () => {
  const mac = { platform: "mac" as Platform };

  it("uses Command and the delete key", () => {
    expect(press({ key: "Enter", metaKey: true }, mac).log).toEqual([
      "run:show",
    ]);
    expect(press({ key: "Backspace" }, mac).log).toEqual([
      "run:removeFromList",
    ]);
    expect(press({ key: "Backspace", metaKey: true }, mac).log).toEqual([
      "run:removeAndDelete",
    ]);
    expect(press({ key: "a", metaKey: true }, mac).log).toEqual(["selectAll"]);
  });

  it("does not take Control as the modifier", () => {
    expect(press({ key: "a", ctrlKey: true }, mac).handled).toBe(false);
  });
});

describe("when shortcuts must stay out of the way", () => {
  it("ignores everything while a modal is open", () => {
    for (const k of ["Enter", "Delete", "ArrowDown", "Escape"]) {
      expect(press({ key: k }, { modalOpen: true }).handled).toBe(false);
    }
    expect(
      press({ key: "a", ctrlKey: true }, { modalOpen: true }).handled,
    ).toBe(false);
  });

  it("ignores editing keys while a text field has focus", () => {
    for (const init of [
      { key: "Enter" },
      { key: "Delete" },
      { key: "a", ctrlKey: true },
      { key: "ArrowUp" },
    ]) {
      expect(press(init, { focusInEditable: true }).handled).toBe(false);
    }
  });

  it("Escape closes an open menu and does nothing otherwise", () => {
    expect(press({ key: "Escape" }, { menuOpen: true }).log).toEqual([
      "closeMenu",
    ]);
    expect(press({ key: "Escape" }).handled).toBe(false);
  });

  it("leaves the arrows and Enter to an open menu", () => {
    for (const k of ["ArrowDown", "ArrowUp", "Enter"]) {
      expect(press({ key: k }, { menuOpen: true }).handled).toBe(false);
    }
  });
});
