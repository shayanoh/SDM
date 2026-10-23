import { Channel } from "@tauri-apps/api/core";
import { commands, type Patch } from "../bindings";
import { BridgeError, type Bridge } from "./bridge";

type Result<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

async function unwrap<T>(call: Promise<Result<T, any>>): Promise<T> {
  const result = await call;
  if (result.status === "error") throw new BridgeError(result.error);
  return result.data;
}

/** `unwrap` for commands whose result is `()` (serialized as `null`). */
async function unwrapVoid(call: Promise<Result<unknown, any>>): Promise<void> {
  await unwrap(call);
}

export class TauriBridge implements Bridge {
  attach(onPatch: (patch: Patch) => void) {
    const channel = new Channel<Patch>();
    channel.onmessage = onPatch;
    return unwrapVoid(commands.attach(channel));
  }
  fullState() {
    return unwrapVoid(commands.fullState());
  }
  addUrl(url: string, packageName?: string) {
    return unwrapVoid(commands.addUrl(url, packageName ?? null));
  }
  startItems(ids: string[]) {
    return unwrapVoid(commands.startItems(ids));
  }
  stopItems(ids: string[]) {
    return unwrapVoid(commands.stopItems(ids));
  }
  retryItems(ids: string[]) {
    return unwrapVoid(commands.retryItems(ids));
  }
  retryMuxItems(ids: string[]) {
    return unwrapVoid(commands.retryMuxItems(ids));
  }
  resetItems(ids: string[]) {
    return unwrapVoid(commands.resetItems(ids));
  }
  setItemsEnabled(ids: string[], enabled: boolean) {
    return unwrapVoid(commands.setItemsEnabled(ids, enabled));
  }
  removeItems(ids: string[], deleteFiles: boolean) {
    return unwrapVoid(commands.removeItems(ids, deleteFiles));
  }
  removePackage(id: string, deleteFiles: boolean) {
    return unwrapVoid(commands.removePackage(id, deleteFiles));
  }
  moveItems(ids: string[], packageId: string, atIndex?: number) {
    return unwrapVoid(commands.moveItems(ids, packageId, atIndex ?? null));
  }
  moveItemsToNewPackage(ids: string[], name: string) {
    return unwrapVoid(commands.moveItemsToNewPackage(ids, name));
  }
  reorderItems(ids: string[], packageId: string) {
    return unwrapVoid(commands.reorderItems(ids, packageId));
  }
  reorderPackages(ids: string[]) {
    return unwrapVoid(commands.reorderPackages(ids));
  }
  describeDeletion(target: import("../bindings").DeletionTarget) {
    return unwrap(commands.describeDeletion(target));
  }
  openItem(id: string) {
    return unwrapVoid(commands.openItem(id));
  }
  revealItem(id: string) {
    return unwrapVoid(commands.revealItem(id));
  }
  revealPackage(id: string) {
    return unwrapVoid(commands.revealPackage(id));
  }
  pauseAll() {
    return unwrapVoid(commands.pauseAll());
  }
  resumeAll() {
    return unwrapVoid(commands.resumeAll());
  }
  setListVisible(visible: boolean) {
    return unwrapVoid(commands.setListVisible(visible));
  }
  setDetailItems(owner: string, ids: string[]) {
    return unwrapVoid(commands.setDetailItems(owner, ids));
  }
  getSettings() {
    return unwrap(commands.getSettings());
  }
  updateSettings(settings: import("../bindings").SettingsDto) {
    return unwrapVoid(commands.updateSettings(settings));
  }
  binariesStatus() {
    return unwrap(commands.binariesStatus());
  }
  themeCss(themeId?: string) {
    return unwrap(commands.themeCss(themeId ?? null));
  }
}
