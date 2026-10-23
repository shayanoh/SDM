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
  startItem(id: string) {
    return unwrapVoid(commands.startItem(id));
  }
  stopItem(id: string) {
    return unwrapVoid(commands.stopItem(id));
  }
  retryItem(id: string) {
    return unwrapVoid(commands.retryItem(id));
  }
  removeItem(id: string, deleteFile: boolean) {
    return unwrapVoid(commands.removeItem(id, deleteFile));
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
