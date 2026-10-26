import { fireEvent, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { fullPatchOf, type PackageSpec } from "../test/fixtures";
import { makeApp, renderIn } from "../test/render";
import ModalHost from "./ModalHost.svelte";

const specs: PackageSpec[] = [
  {
    id: "p",
    name: "Pack",
    items: [
      { id: "a", state: { kind: "running" } },
      { id: "b", state: { kind: "stopped" } },
    ],
  },
];

const setup = () => makeApp([fullPatchOf(1, specs)]);
const filesInfo = {
  title: "Remove From List And Delete Files?",
  totalBytes: 5 * 1024 * 1024,
  names: ["very-long-name-for-a-file-that-keeps-going.iso.incomplete", "b.iso"],
};

describe("ModalHost", () => {
  it("renders nothing without a modal", () => {
    renderIn(ModalHost, setup());
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("Escape closes any modal without calling the bridge", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "items", ids: ["a"] },
      info: filesInfo,
    };
    const dialog = await screen.findByRole("dialog");
    await fireEvent.keyDown(dialog, { key: "Escape" });
    expect(app.ui.modal).toBeNull();
    expect(app.bridge.calls).toEqual([]);
  });
});

describe("delete confirmation", () => {
  it("shows the title, the count and size and the file names", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "items", ids: ["a", "b"] },
      info: filesInfo,
    };
    await screen.findByRole("dialog");
    expect(screen.getByText("Remove From List And Delete Files?")).toBeTruthy();
    expect(
      screen.getByText(
        /2 files · 5 MB will be moved to the (Trash|Recycle Bin)\./,
      ),
    ).toBeTruthy();
    expect(screen.getByText(/very-long-name/)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: /^Move to (Trash|Recycle Bin)$/ }),
    ).toBeTruthy();
  });

  it("says no files will be moved when there are none, and offers Remove from List", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "items", ids: ["a"] },
      info: { title: "Remove From List?", totalBytes: 0, names: [] },
    };
    await screen.findByRole("dialog");
    expect(
      screen.getByText(/No files will be moved to the (Trash|Recycle Bin)/),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Remove from List" }),
    ).toBeTruthy();
  });

  it("uses the singular for one file", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "items", ids: ["a"] },
      info: {
        title: "Remove From List And Delete Files?",
        totalBytes: 10,
        names: ["a.bin"],
      },
    };
    await screen.findByRole("dialog");
    expect(screen.getByText(/1 file · 10 bytes will be moved/)).toBeTruthy();
  });

  it("Enter confirms an items deletion with the files", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "items", ids: ["a", "b"] },
      info: filesInfo,
    };
    const dialog = await screen.findByRole("dialog");
    await fireEvent.keyDown(dialog, { key: "Enter" });
    expect(app.bridge.calls).toEqual([["removeItems", ["a", "b"], true]]);
    expect(app.ui.modal).toBeNull();
  });

  it("confirms a package deletion through removePackage", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "package", id: "p" },
      info: filesInfo,
    };
    await fireEvent.click(
      await screen.findByRole("button", { name: /^Move to / }),
    );
    expect(app.bridge.calls).toEqual([["removePackage", "p", true]]);
  });

  it("Cancel does nothing", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "items", ids: ["a"] },
      info: filesInfo,
    };
    await fireEvent.click(
      await screen.findByRole("button", { name: "Cancel" }),
    );
    expect(app.bridge.calls).toEqual([]);
    expect(app.ui.modal).toBeNull();
  });

  it("reports a bridge failure and still closes", async () => {
    const app = setup();
    app.bridge.failures.removeItems = new Error("disk said no");
    renderIn(ModalHost, app);
    app.ui.modal = {
      kind: "delete",
      target: { kind: "items", ids: ["a"] },
      info: filesInfo,
    };
    await fireEvent.click(
      await screen.findByRole("button", { name: /^Move to / }),
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(app.ui.error).toBe("disk said no");
    expect(app.ui.modal).toBeNull();
  });
});

describe("pause confirmation", () => {
  it("warns that progress will be lost and pauses on confirm", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = { kind: "pause", ids: ["a", "b"] };
    await screen.findByRole("dialog");
    expect(screen.getByText("Pause non-resumable download?")).toBeTruthy();
    expect(
      screen.getByText(
        "One or more downloads cannot be resumed. Pausing them now will lose their progress permanently.",
      ),
    ).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    // Only "a" is running; "b" is already stopped and no longer applies.
    expect(app.bridge.calls).toEqual([["stopItems", ["a"]]]);
    expect(app.ui.modal).toBeNull();
  });

  it("Cancel leaves everything running", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = { kind: "pause", ids: ["a"] };
    await fireEvent.click(
      await screen.findByRole("button", { name: "Cancel" }),
    );
    expect(app.bridge.calls).toEqual([]);
  });

  it("calls nothing when no item can be paused any more", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = { kind: "pause", ids: ["b"] };
    await fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    expect(app.bridge.calls).toEqual([]);
    expect(app.ui.modal).toBeNull();
  });
});

describe("new package prompt", () => {
  it("disables the button for blank and whitespace names", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = { kind: "newPackage", ids: ["a", "b"] };
    const input = (await screen.findByRole("textbox")) as HTMLInputElement;
    const button = screen.getByRole("button", {
      name: "Move to New Package",
    }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    await fireEvent.input(input, { target: { value: "   " } });
    expect(button.disabled).toBe(true);
    await fireEvent.input(input, { target: { value: "x" } });
    expect(button.disabled).toBe(false);
  });

  it("sends the trimmed name with the items and closes", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = { kind: "newPackage", ids: ["a", "b"] };
    const input = (await screen.findByRole("textbox")) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "  Fresh  " } });
    await fireEvent.keyDown(input, { key: "Enter" });
    expect(app.bridge.calls).toEqual([
      ["moveItemsToNewPackage", ["a", "b"], "Fresh"],
    ]);
    expect(app.ui.modal).toBeNull();
  });

  it("Enter with a blank name does nothing", async () => {
    const app = setup();
    renderIn(ModalHost, app);
    app.ui.modal = { kind: "newPackage", ids: ["a"] };
    const input = await screen.findByRole("textbox");
    await fireEvent.keyDown(input, { key: "Enter" });
    expect(app.bridge.calls).toEqual([]);
    expect(app.ui.modal).not.toBeNull();
  });
});
