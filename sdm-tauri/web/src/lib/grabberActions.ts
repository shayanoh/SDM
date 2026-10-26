import type { GrabberPackage, HandoffTarget } from "../bindings";
import type { Bridge } from "./bridge";
import { errorMessage } from "./context";
import type { UiState } from "./ui.svelte";

export interface GrabberContext {
  bridge: Bridge;
  ui: UiState;
  clipboard: Pick<Clipboard, "writeText">;
}

/** Runs `work`, showing any failure in the banner instead of throwing. */
async function reporting(ui: UiState, work: () => Promise<void>) {
  try {
    await work();
  } catch (error) {
    ui.report(errorMessage(error));
  }
}

/** A row menu entry, acting on the rows the menu was opened for. */
export function runRowMenu(
  id: string,
  ids: string[],
  { bridge, ui, clipboard }: GrabberContext,
): Promise<void> {
  return reporting(ui, async () => {
    switch (id) {
      case "recheck":
        await bridge.grabberRecheck(ids);
        break;
      case "copy": {
        if (ids.length !== 1) return;
        await clipboard.writeText(await bridge.grabberCopyUrl(ids[0]));
        break;
      }
      case "add":
      case "addStart":
        await bridge.grabberAddToDownloads(
          { kind: "rows", ids },
          id === "addStart",
        );
        break;
      case "remove":
        await bridge.grabberRemoveLinks(ids);
        break;
    }
  });
}

/**
 * A package menu entry. `rename` needs a name, so it returns `"rename"` for
 * the caller to prompt; everything else is done here.
 */
export async function runPackageMenu(
  id: string,
  pkg: GrabberPackage,
  { bridge, ui }: GrabberContext,
): Promise<"rename" | "done"> {
  if (id === "rename") return "rename";
  await reporting(ui, async () => {
    if (id === "split") await bridge.grabberSplitPackage(pkg.id);
    else if (id.startsWith("merge:"))
      await bridge.grabberMergePackages(pkg.id, id.slice("merge:".length));
  });
  return "done";
}

/** The header's and package headers' buttons. */
export function addToDownloads(
  target: HandoffTarget,
  start: boolean,
  { bridge, ui }: GrabberContext,
): Promise<void> {
  return reporting(ui, () => bridge.grabberAddToDownloads(target, start));
}

/** Text or links dropped on the view. Empty or blank drops do nothing. */
export function ingestDropped(
  text: string,
  { bridge, ui }: GrabberContext,
): Promise<void> {
  if (text.trim() === "") return Promise.resolve();
  return reporting(ui, () => bridge.grabberIngestText(text));
}

export const renamePackage = (
  pkg: GrabberPackage,
  name: string,
  { bridge, ui }: GrabberContext,
) => reporting(ui, () => bridge.grabberRenamePackage(pkg.id, name));

export const simple = (
  action: (bridge: Bridge) => Promise<void>,
  { bridge, ui }: GrabberContext,
) => reporting(ui, () => action(bridge));
