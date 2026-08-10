import { describe, expect, test } from "bun:test";

import {
  availableCommand,
  captureColumnScrollOffsets,
  childrenColumnWidth,
  commandHasChoices,
  commandDisabledDuringSetup,
  controlledColumnId,
  restoreColumnScrollOffsets,
} from "./explorer.js";

describe("Explorer control-plane behavior", () => {
  test("keeps Control commands available during first setup", () => {
    expect(commandDisabledDuringSetup(true, { source: "control" })).toBe(false);
    expect(commandDisabledDuringSetup(true, { source: "kernel" })).toBe(true);
    expect(commandDisabledDuringSetup(true, { source: "action" })).toBe(true);
    expect(commandDisabledDuringSetup(false, { source: "action" })).toBe(false);
  });

  test("falls back instead of selecting a disabled routed command", () => {
    const control = { address: "..entry", source: "control" };
    const action = { address: "proj", source: "action" };
    const catalog = {
      commandByAddress: new Map([
        [control.address, control],
        [action.address, action],
      ]),
    };

    expect(availableCommand(catalog, true, action.address)).toBeNull();
    expect(availableCommand(catalog, true, control.address)).toBe(control);
    expect(availableCommand(catalog, false, action.address)).toBe(action);
    expect(availableCommand(catalog, false, "missing")).toBeNull();
  });

  test("connects command rows to the column they actually reveal", () => {
    expect(controlledColumnId({ handler: "entry.profile" }, 0))
      .toBe("finder-column-1");
    expect(controlledColumnId({ handler: "host.start" }, 0))
      .toBe("finder-column-1");
  });

  test("uses the parent command's declared child column width", () => {
    expect(childrenColumnWidth({ childrenColumnWidth: "wide" })).toBe("wide");
    expect(childrenColumnWidth({})).toBe("normal");
  });

  test("reveals one next-choice column for activities or children", () => {
    const parent = { address: "proj" };
    const leaf = { address: "proj.build" };
    const catalog = {
      childrenByParent: new Map([[parent.address, [leaf]]]),
    };

    expect(commandHasChoices(catalog, parent, [])).toBe(true);
    expect(commandHasChoices(catalog, leaf, [{ name: "overview" }])).toBe(true);
    expect(commandHasChoices(catalog, leaf, [])).toBe(false);
  });

  test("restores vertical offsets only for columns representing the same parent", () => {
    let rendered = [
      { dataset: { scrollKey: "root" }, scrollTop: 17 },
      { dataset: { scrollKey: "choices:..entry.env" }, scrollTop: 559 },
    ];
    const columns = {
      querySelectorAll() {
        return rendered;
      },
    };
    const offsets = captureColumnScrollOffsets(columns);

    rendered = [
      { dataset: { scrollKey: "root" }, scrollTop: 0 },
      { dataset: { scrollKey: "choices:..entry.env" }, scrollTop: 0 },
      { dataset: { scrollKey: "choices:proj" }, scrollTop: 0 },
    ];
    restoreColumnScrollOffsets(columns, offsets);

    expect(rendered.map((column) => column.scrollTop)).toEqual([17, 559, 0]);
  });
});
