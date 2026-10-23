import type { Bridge } from "./bridge";
import { errorMessage } from "./context";
import {
  canDisable,
  canEnable,
  canOpen,
  canReset,
  canRetry,
  canRetryMux,
  canStart,
  canStop,
  needsPauseConfirmation,
} from "./eligibility";
import type { MenuId } from "./menu";
import type { ItemModel } from "./store.svelte";
import type { UiState } from "./ui.svelte";

export interface ActionContext {
  /** The selected item ids (a selected package already expanded to its items), in list order. */
  ids: string[];
  items: ReadonlyMap<string, ItemModel>;
  bridge: Bridge;
  ui: UiState;
  clipboard: { writeText(text: string): Promise<void> };
  /** Set for the package menu's actions: the package and its items, in order. */
  packageId?: string;
  packageItemIds?: string[];
}

const naturally = (a: string, b: string) =>
  a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });

/**
 * Runs one menu or toolbar action on the selection. An action acts on the
 * items it applies to (each has the same predicate as its menu entry) and does
 * nothing, with no bridge call, when none does. A failure goes to the error
 * banner rather than to the caller.
 */
export async function runMenu(
  id: MenuId,
  context: ActionContext,
): Promise<void> {
  try {
    await perform(id, context);
  } catch (error) {
    context.ui.report(errorMessage(error));
  }
}

async function perform(
  id: MenuId,
  {
    ids,
    items,
    bridge,
    ui,
    clipboard,
    packageId,
    packageItemIds,
  }: ActionContext,
) {
  const chosen = ids.flatMap((itemId) => {
    const item = items.get(itemId);
    return item ? [item] : [];
  });
  const matching = (predicate: (item: ItemModel) => boolean) =>
    chosen.filter(predicate).map((item) => item.id);

  switch (id) {
    case "open":
      for (const itemId of matching(canOpen)) await bridge.openItem(itemId);
      return;
    case "show":
      if (chosen.length > 0) await bridge.revealItem(chosen[0].id);
      return;
    case "copy":
      if (chosen.length > 0)
        await clipboard.writeText(chosen.map((item) => item.url).join("\n"));
      return;
    case "start": {
      const targets = matching(canStart);
      if (targets.length > 0) await bridge.startItems(targets);
      return;
    }
    case "pause": {
      const targets = matching(canStop);
      if (targets.length === 0) return;
      // Losing a non-resumable download's progress is permanent: ask first.
      if (needsPauseConfirmation(chosen))
        ui.modal = { kind: "pause", ids: targets };
      else await bridge.stopItems(targets);
      return;
    }
    case "enable": {
      const targets = matching(canEnable);
      if (targets.length > 0) await bridge.setItemsEnabled(targets, true);
      return;
    }
    case "disable": {
      const targets = matching(canDisable);
      if (targets.length > 0) await bridge.setItemsEnabled(targets, false);
      return;
    }
    case "retry": {
      const targets = matching(canRetry);
      if (targets.length > 0) await bridge.retryItems(targets);
      return;
    }
    case "retryMux": {
      const targets = matching(canRetryMux);
      if (targets.length > 0) await bridge.retryMuxItems(targets);
      return;
    }
    case "reset": {
      const targets = matching(canReset);
      if (targets.length > 0) await bridge.resetItems(targets);
      return;
    }
    case "moveToNew":
      if (chosen.length > 0)
        ui.modal = { kind: "newPackage", ids: chosen.map((item) => item.id) };
      return;
    case "removeFromList":
      if (chosen.length === 0) return;
      ui.selection.clear();
      await bridge.removeItems(
        chosen.map((item) => item.id),
        false,
      );
      return;
    case "removeAndDelete": {
      if (chosen.length === 0) return;
      const target = {
        kind: "items" as const,
        ids: chosen.map((item) => item.id),
      };
      ui.modal = {
        kind: "delete",
        target,
        info: await bridge.describeDeletion(target),
      };
      return;
    }
    case "info":
      ui.panelVisible = !ui.panelVisible;
      return;
    case "sortByName": {
      if (!packageId) return;
      const sorted = (packageItemIds ?? [])
        .flatMap((itemId) => {
          const item = items.get(itemId);
          return item ? [item] : [];
        })
        .sort((a, b) => naturally(a.filename, b.filename))
        .map((item) => item.id);
      if (sorted.length > 0) await bridge.reorderItems(sorted, packageId);
      return;
    }
    case "removePackage":
      if (!packageId) return;
      ui.selection.clear();
      await bridge.removePackage(packageId, false);
      return;
    case "removePackageAndDelete": {
      if (!packageId) return;
      const target = { kind: "package" as const, id: packageId };
      ui.modal = {
        kind: "delete",
        target,
        info: await bridge.describeDeletion(target),
      };
      return;
    }
  }
}
