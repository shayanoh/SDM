import { render, type RenderResult } from "@testing-library/svelte";
import type { Component } from "svelte";
import type { Patch } from "../bindings";
import type { Bridge } from "../lib/bridge";
import { createContext } from "../lib/context";
import { DownloadStore } from "../lib/store.svelte";
import { UiState } from "../lib/ui.svelte";
import { wireSelectionPruning } from "../lib/wiring";

export type Call = [method: string, ...args: unknown[]];

export interface StubBridge extends Bridge {
  /** Every call made, in order: method name then arguments. */
  calls: Call[];
  /** Answers for methods that return something; everything else resolves to nothing. */
  answers: Record<string, unknown>;
  /** Methods that reject with the given error instead of resolving. */
  failures: Record<string, Error>;
}

/** A bridge that records every call and resolves it. */
export function stubBridge(): StubBridge {
  const calls: Call[] = [];
  const failures: Record<string, Error> = {};
  const answers: Record<string, unknown> = {
    describeDeletion: { title: "Remove From List?", totalBytes: 0, names: [] },
    getSettings: { downloadFolder: "/dl" },
    binariesStatus: {
      ytDlpVersion: null,
      latestKnown: null,
      ffmpegVersion: null,
      qjsVersion: null,
      lastError: null,
    },
    themeCss: "",
  };
  return new Proxy({} as StubBridge, {
    get(_target, property) {
      if (property === "calls") return calls;
      if (property === "answers") return answers;
      if (property === "failures") return failures;
      if (property === "then") return undefined;
      return async (...args: unknown[]) => {
        calls.push([String(property), ...args]);
        const failure = failures[String(property)];
        if (failure) throw failure;
        return answers[String(property)];
      };
    },
  });
}

export interface App {
  bridge: StubBridge;
  store: DownloadStore;
  ui: UiState;
}

/** A store and UI state with the selection pruning wired, and a recording bridge. */
export function makeApp(patches: Patch[] = []): App {
  const bridge = stubBridge();
  const ui = new UiState();
  const store = new DownloadStore(() => {});
  wireSelectionPruning(store, ui);
  for (const patch of patches) store.apply(patch);
  return { bridge, store, ui };
}

/** Mounts `component` inside the app's context. */
export function renderIn<P extends Record<string, unknown>>(
  component: Component<P>,
  app: App,
  props?: P,
): RenderResult<Component<P>> & App {
  // testing-library's types do not mention the `context` mount option.
  const options = {
    props: props as P,
    context: createContext(app.bridge, app.store, app.ui),
  };
  const result = render(component, options as never);
  return { ...result, ...app };
}
