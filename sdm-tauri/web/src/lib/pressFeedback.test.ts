// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { installPressFeedback, spawnRipple } from "./pressFeedback";

function press(target: Element, init: PointerEventInit = {}) {
  target.dispatchEvent(
    new PointerEvent("pointerdown", {
      bubbles: true,
      button: 0,
      clientX: 10,
      clientY: 5,
      ...init,
    }),
  );
}

function sized<T extends HTMLElement>(button: T): T {
  button.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width: 100, height: 30 }) as DOMRect;
  return button;
}

describe("press feedback", () => {
  let undo: () => void;
  beforeEach(() => {
    document.body.innerHTML = "";
    undo = installPressFeedback(document);
  });
  afterEach(() => undo());

  it("starts a ripple from the press point, sized to reach the far corner", () => {
    const button = sized(
      document.body.appendChild(document.createElement("button")),
    );
    press(button);
    const ripple = button.querySelector<HTMLElement>(".press-ripple")!;
    expect(ripple).toBeTruthy();
    const radius = Math.hypot(90, 25);
    expect(parseFloat(ripple.style.width)).toBeCloseTo(radius * 2);
    expect(parseFloat(ripple.style.left)).toBeCloseTo(10 - radius);
    expect(parseFloat(ripple.style.top)).toBeCloseTo(5 - radius);
  });

  it("reacts when the press lands on something inside the button", () => {
    const button = sized(
      document.body.appendChild(document.createElement("button")),
    );
    const icon = button.appendChild(document.createElement("span"));
    press(icon);
    expect(button.querySelector(".press-ripple")).toBeTruthy();
  });

  it("ignores disabled buttons, menu entries and other mouse buttons", () => {
    const disabled = sized(
      document.body.appendChild(document.createElement("button")),
    );
    disabled.disabled = true;
    const entry = sized(
      document.body.appendChild(document.createElement("button")),
    );
    entry.setAttribute("role", "menuitem");
    const plain = sized(
      document.body.appendChild(document.createElement("button")),
    );
    press(disabled);
    press(entry);
    press(plain, { button: 2 });
    expect(document.querySelector(".press-ripple")).toBeNull();
  });

  it("ripples from the center when a key activates the button", () => {
    const button = sized(
      document.body.appendChild(document.createElement("button")),
    );
    button.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 0 }));
    const ripple = button.querySelector<HTMLElement>(".press-ripple")!;
    expect(
      parseFloat(ripple.style.left) + parseFloat(ripple.style.width) / 2,
    ).toBeCloseTo(50);
  });

  it("does not stack unbounded ripples and rejects bad geometry", () => {
    const button = document.body.appendChild(document.createElement("button"));
    for (let i = 0; i < 10; i++) spawnRipple(button, 1, 1, 100, 30);
    expect(button.querySelectorAll(".press-ripple").length).toBe(4);
    expect(spawnRipple(button, NaN, 1, 100, 30)).toBeNull();
    expect(spawnRipple(button, 1, 1, 0, 30)).toBeNull();
  });
});
