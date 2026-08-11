import { describe, expect, test } from "bun:test";

import { createCommandJournalView } from "./command-journal.js";
import {
  FakeElement,
  deferred,
  documentObject,
  response,
  settle,
} from "./command-run-test-support.js";

function elements() {
  const element = () => new FakeElement();
  return {
    commandJournalAddress: element(),
    commandJournalDetail: element(),
    commandJournalDetailEmpty: element(),
    commandJournalEmpty: element(),
    commandJournalFeedback: element(),
    commandJournalList: element(),
    commandJournalMeta: element(),
    commandJournalOutput: element(),
    commandJournalRefresh: element(),
    commandJournalState: element(),
    commandJournalTruncated: element(),
  };
}

const command = {
  address: ".dev.status",
  source: "kernel",
  runnable: true,
};

function history(runs = []) {
  return {
    protocol: "swawkit.command-run-history/v1",
    address: command.address,
    runs,
  };
}

function summary() {
  return {
    id: "run-1",
    source: "cli",
    state: "exited",
    startedAtUnixMs: 1000,
    finishedAtUnixMs: 1200,
    exitCode: 0,
    error: null,
    argumentCount: 0,
    eventCount: 1,
    truncated: false,
  };
}

function journal() {
  return {
    protocol: "swawkit.command-run-journal/v1",
    id: "run-1",
    address: command.address,
    source: "cli",
    state: "exited",
    startedAtUnixMs: 1000,
    finishedAtUnixMs: 1200,
    exitCode: 0,
    error: null,
    argumentCount: 0,
    profileRevision: "sha256-fixture",
    nextCursor: 1,
    events: [{
      sequence: 1,
      timestampUnixMs: 1100,
      phase: "run",
      stream: "stdout",
      text: "ready\n",
    }],
    truncated: false,
  };
}

describe("command journal view", () => {
  test("loads the newest persisted run and renders event metadata", async () => {
    const dom = elements();
    const view = createCommandJournalView(dom, {
      document: documentObject(),
      fetchJournal: async (url) => url.includes("/run-1?")
        ? response(200, journal())
        : response(200, history([summary()])),
    });

    view.select(command, { active: true });
    await settle();
    await settle();

    expect(dom.commandJournalList.children).toHaveLength(1);
    expect(dom.commandJournalDetail.hidden).toBeFalse();
    expect(dom.commandJournalState.textContent).toBe("执行成功");
    expect(dom.commandJournalOutput.children[0].textContent)
      .toContain("[run] ready");
    const eventTime = new Date(1100);
    const two = (part) => String(part).padStart(2, "0");
    const localStamp = `${two(eventTime.getHours())}:${two(eventTime.getMinutes())}:${two(eventTime.getSeconds())}.${String(eventTime.getMilliseconds()).padStart(3, "0")}`;
    expect(dom.commandJournalOutput.children[0].textContent)
      .toStartWith(`[${localStamp}]`);
  });

  test("leaving during a slow load does not block reopening the same command", async () => {
    const first = deferred();
    let requests = 0;
    const view = createCommandJournalView(elements(), {
      document: documentObject(),
      fetchJournal: async () => {
        requests += 1;
        return requests === 1 ? first.promise : response(200, history());
      },
    });

    view.select(command, { active: true });
    view.select(command, { active: false });
    first.resolve(response(200, history()));
    await settle();
    view.select(command, { active: true });
    await settle();

    expect(requests).toBe(2);
  });

  test("history entries stay disabled while one refresh owns the load", async () => {
    const detail = deferred();
    const dom = elements();
    const view = createCommandJournalView(dom, {
      document: documentObject(),
      fetchJournal: async (url) => url.includes("/run-1?")
        ? detail.promise
        : response(200, history([summary()])),
    });

    view.select(command, { active: true });
    await settle();

    expect(dom.commandJournalList.children[0].children[0].disabled).toBeTrue();
    detail.resolve(response(200, journal()));
    await settle();
    await settle();

    expect(dom.commandJournalList.children[0].children[0].disabled).toBeFalse();
  });

  test("opens the validated directory for one persisted run", async () => {
    const dom = elements();
    const requests = [];
    const view = createCommandJournalView(dom, {
      document: documentObject(),
      fetchJournal: async (url, options = {}) => {
        requests.push({ url, options });
        if (options.method === "POST") {
          return response(204);
        }
        return url.includes("/run-1?")
          ? response(200, journal())
          : response(200, history([summary()]));
      },
    });

    view.select(command, { active: true });
    await settle();
    await settle();
    dom.commandJournalList.children[0].children[1].dispatch("click");
    await settle();

    expect(requests.at(-1)).toEqual({
      url: "/api/v2/command-run-journals/run-1/open-directory?command=kernel/.dev.status",
      options: {
        method: "POST",
        headers: {
          Accept: "application/json",
          "X-SwawKit-Control": "open-journal-directory",
        },
      },
    });
    expect(dom.commandJournalFeedback.dataset.state).toBe("success");
  });

  test("reentering logs selects the newest run while an in-view refresh retains selection", async () => {
    const dom = elements();
    const newerSummary = {
      ...summary(),
      id: "run-2",
      source: "web",
      startedAtUnixMs: 2000,
      finishedAtUnixMs: 2200,
    };
    const newerJournal = {
      ...journal(),
      id: "run-2",
      source: "web",
      startedAtUnixMs: 2000,
      finishedAtUnixMs: 2200,
    };
    let historyReads = 0;
    const view = createCommandJournalView(dom, {
      document: documentObject(),
      fetchJournal: async (url) => {
        if (url.includes("/run-2?")) {
          return response(200, newerJournal);
        }
        if (url.includes("/run-1?")) {
          return response(200, journal());
        }
        historyReads += 1;
        return response(200, history(
          historyReads === 1 ? [summary()] : [newerSummary, summary()],
        ));
      },
    });

    view.select(command, { active: true });
    await settle();
    await settle();
    view.select(command, { active: false });
    view.select(command, { active: true });
    await settle();
    await settle();

    const newest = dom.commandJournalList.children[0].children[0];
    expect(newest.dataset.selected).toBe("true");
    expect(dom.commandJournalMeta.textContent).toContain("Web");

    dom.commandJournalList.children[1].children[0].dispatch("click");
    await settle();
    await view.refresh();
    expect(dom.commandJournalList.children[1].children[0].dataset.selected).toBe("true");
  });
});
