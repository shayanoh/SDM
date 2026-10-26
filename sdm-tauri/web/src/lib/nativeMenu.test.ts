// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { installNativeMenuBlock, wantsNativeMenu } from "./nativeMenu";

function rightClick(target: Element): boolean {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
  });
  target.dispatchEvent(event);
  return event.defaultPrevented;
}

describe("the webview's own context menu", () => {
  let undo: () => void;
  beforeEach(() => {
    document.body.innerHTML = `
      <div id="plain"><span id="text">words</span></div>
      <button id="button">Go</button>
      <input id="field" type="text" />
      <input id="box" type="checkbox" />
      <textarea id="area"></textarea>
      <div id="edit" contenteditable="true"><b id="inner">x</b></div>`;
    undo = installNativeMenuBlock(document);
  });
  afterEach(() => undo());

  const at = (id: string) => document.getElementById(id)!;

  it("is blocked on the page, buttons and checkboxes", () => {
    for (const id of ["plain", "text", "button", "box"]) {
      expect(rightClick(at(id)), id).toBe(true);
    }
  });

  it("is left alone where text is typed", () => {
    for (const id of ["field", "area", "edit", "inner"]) {
      expect(rightClick(at(id)), id).toBe(false);
    }
  });

  it("stops being blocked once undone", () => {
    undo();
    expect(rightClick(at("plain"))).toBe(false);
  });

  it("says no to anything that is not an element", () => {
    expect(wantsNativeMenu(null)).toBe(false);
    expect(wantsNativeMenu(document)).toBe(false);
  });
});
