import { describe, expect, test } from "bun:test";

import {
  ACTIVE_COMMAND_RUN_KEY,
  createCommandRunView,
} from "./command-run.js";
import {
  deferred,
  documentObject,
  elements,
  outputEvent,
  response,
  scheduler,
  settle,
  snapshot,
  storage,
} from "./command-run-test-support.js";

describe("command run view", () => {
  test("renders declared operations and confirms destructive argv before execution", async () => {
    const ui = elements();
    const bodies = [];
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: storage(),
      async fetchRun(_url, options) {
        bodies.push(JSON.parse(options.body));
        const id = `run-${bodies.length}`;
        return response(201, snapshot({
          id,
          address: ".cache.prune",
          state: "exited",
          exitCode: 0,
        }), `/api/v2/command-runs/${id}`);
      },
    });
    view.select({
      address: ".cache.prune",
      runnable: true,
      source: "kernel",
      runOperations: [
        { id: "preview", label: "预览", arguments: [], confirmation: null },
        {
          id: "apply",
          label: "清理",
          arguments: ["--apply"],
          confirmation: "确认清理？",
        },
      ],
    });

    const buttons = ui.commandRunOperationList
      .querySelectorAll(".command-run-operation");
    expect(buttons.map((button) => button.textContent)).toEqual(["预览", "清理"]);
    expect(ui.commandRunEditor.hidden).toBe(true);
    expect(ui.commandRunOperations.hidden).toBe(false);
    expect(ui.commandRunSubmit.hidden).toBe(true);
    expect(ui.commandRunActions.hidden).toBe(true);

    buttons[0].dispatch("click");
    await settle();
    expect(bodies[0].arguments).toEqual([]);

    buttons[1].dispatch("click");
    expect(bodies).toHaveLength(1);
    expect(ui.commandRunConfirmation.hidden).toBe(false);
    expect(ui.commandRunConfirmationText.textContent).toBe("确认清理？");
    ui.commandRunConfirmDismiss.dispatch("click");
    expect(ui.commandRunConfirmation.hidden).toBe(true);

    buttons[1].dispatch("click");
    ui.commandRunConfirm.dispatch("click");
    await settle();
    expect(bodies[1].arguments).toEqual(["--apply"]);
  });

  test("prefills the exact invocation resolved for a custom Facet", async () => {
    const ui = elements();
    let body;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: storage(),
      async fetchRun(_url, options) {
        body = JSON.parse(options.body);
        return response(201, snapshot({
          address: ".check",
          state: "exited",
          exitCode: 0,
        }), "/api/v2/command-runs/run-1");
      },
    });

    view.select(
      {
        address: ".check",
        runnable: true,
        source: "kernel",
        runOperations: [
          { id: "other", label: "Other", arguments: [], confirmation: null },
        ],
      },
      {
        acceptsTail: false,
        arguments: [".context.list", "--json"],
        key: ".context.list#validate",
        useOperations: false,
      },
    );
    expect(ui.commandRunEditor.hidden).toBe(false);
    expect(
      ui.commandRunArguments
        .querySelectorAll(".command-run-argument")
        .map((input) => input.value),
    ).toEqual([".context.list", "--json"]);
    expect(
      ui.commandRunArguments
        .querySelectorAll(".command-run-argument")
        .every((input) => input.readOnly),
    ).toBe(true);
    expect(ui.commandRunAdd.disabled).toBe(true);

    await view.execute();
    expect(body).toEqual({
      address: ".check",
      arguments: [".context.list", "--json"],
    });
  });

  test("confirms an exact Subject operation with its fixed instance ID", async () => {
    const ui = elements();
    let body;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: storage(),
      async fetchRun(_url, options) {
        body = JSON.parse(options.body);
        return response(201, snapshot({
          address: ".context.delete",
          state: "exited",
          exitCode: 0,
        }), "/api/v2/command-runs/run-1");
      },
    });

    view.select(
      { address: ".context.delete", runnable: true, source: "kernel" },
      {
        acceptsTail: false,
        arguments: ["mycontext01"],
        confirmation: "Delete this Context?",
        key: "::context/mycontext01#delete",
        label: "Delete",
        useOperations: false,
      },
    );
    const [button] = ui.commandRunOperationList
      .querySelectorAll(".command-run-operation");
    expect(button.textContent).toBe("Delete");
    button.dispatch("click");
    expect(ui.commandRunConfirmation.hidden).toBe(false);
    ui.commandRunConfirm.dispatch("click");
    await settle();
    expect(body.arguments).toEqual(["mycontext01"]);
  });

  test("preserves argv rows, polls recursively, and renders both streams", async () => {
    const ui = elements();
    const saved = storage();
    const timers = scheduler();
    const requests = [];
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: saved,
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun(url, options) {
        requests.push({ url, options });
        if (options.method === "POST") {
          return response(201, snapshot({
            nextCursor: 1,
            events: [outputEvent(1, "stdout", "started\n")],
          }), "/api/v2/command-runs/run-1");
        }
        return response(200, snapshot({
          state: "exited",
          exitCode: 0,
          nextCursor: 2,
          events: [outputEvent(2, "stderr", "warning\n")],
        }));
      },
    });

    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    ui.commandRunAdd.dispatch("click");
    ui.commandRunAdd.dispatch("click");
    const inputs = ui.commandRunArguments.querySelectorAll(".command-run-argument");
    inputs[0].value = "-Command";
    inputs[1].value = "";
    await view.execute();

    expect(JSON.parse(requests[0].options.body).arguments).toEqual(["-Command", ""]);
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBe("run-1");
    expect(ui.commandRunOutput.children.map((child) => child.dataset.stream))
      .toEqual(["stdout"]);

    const poll = timers.take();
    expect(poll.delay).toBe(400);
    poll.callback();
    await settle();

    expect(requests[1].url).toBe("/api/v2/command-runs/run-1?after=1");
    expect(ui.commandRunOutput.children.map((child) => child.dataset.stream))
      .toEqual(["stdout", "stderr"]);
    expect(ui.commandRunState.textContent).toBe("执行成功");
    expect(ui.commandRunExitCode.textContent).toBe("退出码 0");
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBeNull();
    expect(timers.take()).toBeUndefined();
  });

  test("restores an active run from session storage at cursor zero", async () => {
    const ui = elements();
    const saved = storage({ [ACTIVE_COMMAND_RUN_KEY]: "run-9" });
    const timers = scheduler();
    let request;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: saved,
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun(url, options) {
        request = { url, options };
        return response(200, snapshot({ id: "run-9" }));
      },
    });
    view.select({ address: ".dev.status", runnable: true, source: "kernel" });
    await view.restore();

    expect(request.url).toBe("/api/v2/command-runs/run-9?after=0");
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBe("run-9");
    expect(timers.take().delay).toBe(400);
  });

  test("a slow restore blocks starting a second run", async () => {
    const ui = elements();
    const pending = deferred();
    let requests = 0;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: storage({ [ACTIVE_COMMAND_RUN_KEY]: "run-9" }),
      setTimer() {
        throw new Error("a terminal restore must not schedule polling");
      },
      clearTimer() {},
      async fetchRun() {
        requests += 1;
        return pending.promise;
      },
    });

    const restoring = view.restore();
    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    expect(ui.commandRunSubmit.disabled).toBe(true);
    expect(ui.commandRunAdd.disabled).toBe(true);
    await view.execute();
    expect(requests).toBe(1);

    pending.resolve(response(200, snapshot({
      id: "run-9",
      state: "canceled",
    })));
    await restoring;

    expect(ui.commandRunState.textContent).toBe("已终止");
    expect(ui.commandRunSubmit.disabled).toBe(false);
    expect(ui.commandRunAdd.disabled).toBe(false);
    expect(requests).toBe(1);
  });

  test("a transient restore failure keeps the previous run identity and retries", async () => {
    const ui = elements();
    const saved = storage({ [ACTIVE_COMMAND_RUN_KEY]: "run-9" });
    const timers = scheduler();
    let requests = 0;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: saved,
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun() {
        requests += 1;
        if (requests === 1) {
          throw new TypeError("fixture network failure");
        }
        return response(200, snapshot({ id: "run-9" }));
      },
    });

    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    await view.restore();
    await view.execute();

    expect(requests).toBe(1);
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBe("run-9");
    expect(ui.commandRunSubmit.disabled).toBe(true);
    const retry = timers.take();
    expect(retry.delay).toBe(1000);
    retry.callback();
    await settle();

    expect(requests).toBe(2);
    expect(timers.take().delay).toBe(400);
  });

  test("a permanent restore error blocks a second run without retrying", async () => {
    const ui = elements();
    const saved = storage({ [ACTIVE_COMMAND_RUN_KEY]: "run-9" });
    const timers = scheduler();
    let requests = 0;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: saved,
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun() {
        requests += 1;
        return response(200, snapshot({
          id: "run-9",
          protocol: "unsupported",
        }));
      },
    });

    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    await view.restore();
    await view.execute();

    expect(requests).toBe(1);
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBe("run-9");
    expect(ui.commandRunSubmit.disabled).toBe(true);
    expect(timers.take()).toBeUndefined();
  });

  test("a permanent poll error stops automatic retries but keeps the run identity", async () => {
    const ui = elements();
    const saved = storage();
    const timers = scheduler();
    let requests = 0;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: saved,
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun(_url, options) {
        requests += 1;
        return options.method === "POST"
          ? response(201, snapshot(), "/api/v2/command-runs/run-1")
          : response(400, { error: "invalid cursor" });
      },
    });

    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    await view.execute();
    timers.take().callback();
    await settle();

    expect(requests).toBe(2);
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBe("run-1");
    expect(timers.take()).toBeUndefined();
  });

  test("a missing run clears arguments retained from a different command", async () => {
    const ui = elements();
    const timers = scheduler();
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: storage(),
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun(_url, options) {
        return options.method === "POST"
          ? response(201, snapshot(), "/api/v2/command-runs/run-1")
          : response(404, { error: "command run not found" });
      },
    });

    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    ui.commandRunAdd.dispatch("click");
    ui.commandRunArguments.querySelectorAll(".command-run-argument")[0].value = "old";
    await view.execute();
    view.select({ address: ".dev.status", runnable: true, source: "kernel" });
    timers.take().callback();
    await settle();

    expect(ui.commandRunArguments.querySelectorAll(".command-run-argument"))
      .toHaveLength(0);
    expect(ui.commandRunSubmit.disabled).toBe(false);
  });

  test("DELETE enters canceling state and schedules an immediate refresh", async () => {
    const ui = elements();
    const timers = scheduler();
    const methods = [];
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: storage(),
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun(_url, options) {
        methods.push(options.method ?? "GET");
        return options.method === "POST"
          ? response(201, snapshot(), "/api/v2/command-runs/run-1")
          : response(204);
      },
    });
    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    await view.execute();
    await view.cancel();

    expect(methods).toEqual(["POST", "DELETE"]);
    expect(ui.commandRunState.textContent).toBe("正在终止…");
    expect(ui.commandRunCancel.disabled).toBe(true);
    expect(timers.take().delay).toBe(0);
  });

  test("cancel invalidates an older in-flight running response", async () => {
    const ui = elements();
    const saved = storage();
    const timers = scheduler();
    const oldPoll = deferred();
    let getCount = 0;
    const view = createCommandRunView(ui, {
      document: documentObject(),
      storage: saved,
      setTimer: timers.setTimer,
      clearTimer: timers.clearTimer,
      async fetchRun(_url, options) {
        if (options.method === "POST") {
          return response(201, snapshot(), "/api/v2/command-runs/run-1");
        }
        if (options.method === "DELETE") {
          return response(204);
        }
        getCount += 1;
        if (getCount === 1) {
          return oldPoll.promise;
        }
        return response(200, snapshot({ state: "canceled" }));
      },
    });

    view.select({ address: ".dev.pwsh", runnable: true, source: "kernel" });
    await view.execute();
    timers.take().callback();
    await view.cancel();
    const currentPoll = timers.take();
    expect(currentPoll.delay).toBe(0);
    currentPoll.callback();
    await settle();

    expect(ui.commandRunState.textContent).toBe("已终止");
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBeNull();
    expect(timers.take()).toBeUndefined();

    oldPoll.resolve(response(200, snapshot({
      nextCursor: 1,
      events: [outputEvent(1, "stdout", "late\n")],
    })));
    await settle();

    expect(ui.commandRunState.textContent).toBe("已终止");
    expect(ui.commandRunOutput.children).toHaveLength(0);
    expect(saved.getItem(ACTIVE_COMMAND_RUN_KEY)).toBeNull();
    expect(timers.take()).toBeUndefined();
  });
});
