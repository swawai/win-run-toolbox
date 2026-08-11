import { describe, expect, test } from "bun:test";

import {
  commandActivities,
  commandViews,
  createCommandActivityView,
  defaultCommandView,
} from "./command-activity.js";

describe("command activities", () => {
  test("keeps hierarchy separate from command presentation", () => {
    expect(commandActivities({
      source: "action",
      address: "proj.build.app",
      runnable: true,
    })).toEqual(["overview", "help", "run", "logs"]);
    expect(commandActivities({
      source: "control",
      address: "..entry",
      runnable: true,
    })).toEqual(["overview", "help"]);
    expect(commandActivities({
      source: "kernel",
      address: ".group",
      runnable: false,
    })).toEqual(["overview", "help", "logs"]);
    expect(commandActivities(null)).toEqual([]);
  });

  test("presents the dedicated Profile editor through the common activity model", () => {
    expect(commandActivities({
      source: "control",
      address: "..entry.env.default-shell",
      handler: "entry.profile.set",
      runnable: true,
    })).toEqual(["edit", "overview", "help"]);
    expect(defaultCommandView({
      source: "control",
      address: "..entry.env.default-shell",
      handler: "entry.profile.set",
      runnable: true,
    })).toBe("edit");
  });

  test("treats subcommands as a local UI view and defaults groups to it", () => {
    const command = {
      source: "action",
      address: "proj.build",
      runnable: false,
    };
    expect(commandViews(command, { hasChildren: true }).map(({ name }) => name))
      .toEqual(["children", "overview", "help", "logs"]);
    expect(defaultCommandView(command, { hasChildren: true })).toBe("children");
    expect(defaultCommandView(command)).toBe("overview");
    expect(defaultCommandView(null)).toBeNull();
  });

  test("switches a Profile setter between its dedicated and shared panes", () => {
    const pane = () => ({ hidden: false });
    const elements = {
      commandWorkspace: pane(),
      entryProfileDetail: pane(),
      commandDetail: pane(),
      commandHelpActivity: pane(),
      commandRunActivity: pane(),
      commandJournalActivity: pane(),
    };
    const view = createCommandActivityView(elements);
    const command = {
      source: "control",
      address: "..entry.env.git.SWAWKIT_PROJ_GIT_ID_NAME",
      handler: "entry.profile.set",
      runnable: true,
    };

    expect(view.selectCommand(command)).toEqual({
      defaultView: "edit",
      view: "edit",
    });
    expect(elements.entryProfileDetail.hidden).toBeFalse();
    expect(elements.commandWorkspace.hidden).toBeTrue();

    view.selectCommand(command, { view: "help" });
    expect(elements.entryProfileDetail.hidden).toBeTrue();
    expect(elements.commandWorkspace.hidden).toBeFalse();
    expect(elements.commandHelpActivity.hidden).toBeFalse();
  });
});
