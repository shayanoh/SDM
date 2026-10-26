// jsdom has no layout, no ResizeObserver and no canvas. Components that
// measure themselves or draw must still mount, so give them stand-ins: every
// observed element reports once, the list's scroller is 600 px tall (a
// realistic viewport), and canvases have no context.
class ResizeObserverStub {
  #callback: ResizeObserverCallback;
  constructor(callback: ResizeObserverCallback) {
    this.#callback = callback;
  }
  observe(target: Element) {
    queueMicrotask(() =>
      this.#callback(
        [{ target } as ResizeObserverEntry],
        this as unknown as ResizeObserver,
      ),
    );
  }
  unobserve() {}
  disconnect() {}
}

globalThis.ResizeObserver =
  ResizeObserverStub as unknown as typeof ResizeObserver;

Object.defineProperty(HTMLElement.prototype, "clientHeight", {
  configurable: true,
  get(this: HTMLElement) {
    return this.classList?.contains("scroller") ? 600 : 0;
  },
});

HTMLCanvasElement.prototype.getContext = (() =>
  null) as typeof HTMLCanvasElement.prototype.getContext;
