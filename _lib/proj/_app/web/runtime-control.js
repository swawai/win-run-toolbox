import { t } from "./i18n.js";

const RUNTIME_STATUS_PROTOCOL = "swawkit.runtime-status/v1";
const HOST_STATUS_PROTOCOL = "swawkit.host-status/v1";
const RUNTIME_CLEANUP_PROTOCOL = "swawkit.runtime-cleanup/v1";
const SHA256 = /^[a-f0-9]{64}$/;
const LOOPBACK_URL = /^http:\/\/127\.0\.0\.1:([1-9][0-9]{0,4})\/$/;
const CLEANUP_STATES = {
  preview: new Set(["selected", "inUse", "removable", "retained"]),
  apply: new Set(["selected", "inUse", "removed", "retained"]),
};
const CLEANUP_SUMMARY_FIELDS = [
  "inUse",
  "removable",
  "removed",
  "retained",
  "selected",
];
const RUNTIME_HANDLERS = new Set([
  "runtime.status",
  "host.exit",
  "host.restart",
  "runtime.cleanup",
]);

export class RuntimeControlError extends Error {}

function validHostStatus(document) {
  const port = typeof document?.url === "string"
    ? Number(LOOPBACK_URL.exec(document.url)?.[1])
    : 0;
  return document?.protocol === HOST_STATUS_PROTOCOL
    && typeof document.entryKeySha256 === "string"
    && SHA256.test(document.entryKeySha256)
    && Number.isInteger(document.pid)
    && document.pid > 0
    && typeof document.bootId === "string"
    && document.bootId.length > 0
    && document.bootId.length <= 160
    && Number.isInteger(port)
    && port <= 65535
    && typeof document.runningReleaseId === "string"
    && SHA256.test(document.runningReleaseId)
    && typeof document.selectedReleaseId === "string"
    && SHA256.test(document.selectedReleaseId)
    && typeof document.updateAvailable === "boolean"
    && document.updateAvailable
      === (document.runningReleaseId !== document.selectedReleaseId);
}

export async function readRuntimeStatus(fetchImpl = fetch) {
  const response = await fetchImpl("/api/v2/runtime", {
    cache: "no-store",
    headers: { Accept: "application/json" },
  });
  if (!response.ok) {
    throw new RuntimeControlError(t(
      `Runtime 状态返回 HTTP ${response.status}`,
      `Runtime status returned HTTP ${response.status}`,
    ));
  }
  const document = await response.json();
  if (
    document?.protocol !== RUNTIME_STATUS_PROTOCOL
    || typeof document.selectedReleaseId !== "string"
    || !SHA256.test(document.selectedReleaseId)
    || !Number.isInteger(document.releaseCount)
    || document.releaseCount < 0
    || !(document.host === null || validHostStatus(document.host))
    || (document.host !== null
      && document.host?.selectedReleaseId !== document.selectedReleaseId)
  ) {
    throw new RuntimeControlError(t(
      "Host 返回了无效的 Runtime 状态。",
      "Host returned invalid Runtime status.",
    ));
  }
  return document;
}

async function requestHostControl(path, command, label, fetchImpl) {
  const response = await fetchImpl(path, {
    method: "POST",
    headers: { "X-SwawKit-Control": command },
  });
  if (response.status !== 202 && response.status !== 204) {
    throw new RuntimeControlError(t(
      `${label}请求被拒绝：HTTP ${response.status}`,
      `${label} request was rejected: HTTP ${response.status}`,
    ));
  }
}

export function requestHostShutdown(fetchImpl = fetch) {
  return requestHostControl(
    "/api/v2/host/shutdown",
    "shutdown",
    t("Host 退出", "Host shutdown"),
    fetchImpl,
  );
}

export function requestHostRestart(fetchImpl = fetch) {
  return requestHostControl(
    "/api/v2/host/restart",
    "restart",
    t("Host 重启", "Host restart"),
    fetchImpl,
  );
}

function validCleanupDocument(document, apply) {
  const action = apply ? "apply" : "preview";
  if (
    document?.protocol !== RUNTIME_CLEANUP_PROTOCOL
    || document.action !== action
    || !Array.isArray(document.items)
    || typeof document.summary !== "object"
    || document.summary === null
    || Object.keys(document.summary).sort().join("\0")
      !== CLEANUP_SUMMARY_FIELDS.join("\0")
  ) {
    return false;
  }
  const counts = {
    selected: 0,
    inUse: 0,
    removable: 0,
    removed: 0,
    retained: 0,
  };
  const releaseIds = new Set();
  for (const item of document.items) {
    if (
      typeof item?.releaseId !== "string"
      || !SHA256.test(item.releaseId)
      || releaseIds.has(item.releaseId)
      || !CLEANUP_STATES[action].has(item.state)
      || !Array.isArray(item.pids)
      || item.pids.some((pid) => !Number.isInteger(pid) || pid <= 0)
      || !(item.reason === null || typeof item.reason === "string")
      || (item.state === "inUse") !== (item.pids.length > 0)
      || (item.state === "retained")
        !== (typeof item.reason === "string" && item.reason.length > 0)
    ) {
      return false;
    }
    releaseIds.add(item.releaseId);
    counts[item.state] += 1;
  }
  return counts.selected === 1
    && Object.entries(counts).every(([name, count]) => (
    Number.isInteger(document.summary[name])
    && document.summary[name] === count
    ));
}

export async function requestRuntimeCleanup(apply, fetchImpl = fetch) {
  const response = await fetchImpl("/api/v2/runtime/cleanup", {
    method: "POST",
    headers: {
      Accept: "application/json",
      "X-SwawKit-Control": apply
        ? "runtime-cleanup-apply"
        : "runtime-cleanup-preview",
    },
  });
  if (!response.ok) {
    throw new RuntimeControlError(t(
      `Runtime 清理返回 HTTP ${response.status}`,
      `Runtime cleanup returned HTTP ${response.status}`,
    ));
  }
  const document = await response.json();
  if (!validCleanupDocument(document, apply)) {
    throw new RuntimeControlError(t(
      "Host 返回了无效的 Runtime 清理结果。",
      "Host returned an invalid Runtime cleanup result.",
    ));
  }
  return document;
}

export function runtimeRootPresentation(document) {
  if (!document.host) {
    return { icon: "●", summary: t("Host 离线", "Host offline"), tone: "error" };
  }
  if (document.host.updateAvailable) {
    return { icon: "●", summary: t("新版本待重启", "New release pending restart"), tone: "warning" };
  }
  return { icon: "●", summary: t("Host 在线 · 当前版本", "Host online · current release"), tone: "online" };
}

function handlerPresentation(handler) {
  return {
    "runtime.status": ["..runtime", t(
      "查看 Runtime 与 Host 状态，并执行明确的生命周期控制。",
      "Inspect Runtime and Host state and perform explicit lifecycle operations.",
    )],
    "host.exit": ["..runtime.host.exit", t(
      "退出当前 Entry 的 Host；正在运行的 Web 命令也会终止。",
      "Exit this Entry's Host; running Web commands will also terminate.",
    )],
    "host.restart": ["..runtime.host.restart", t(
      "重启 Host，并切换到已经发布且选中的 Runtime Release。",
      "Restart Host and switch to the published selected Runtime release.",
    )],
    "runtime.cleanup": ["..runtime.cleanup", t(
      "预览或清理未被选中、也未被进程占用的旧 Runtime Release。",
      "Preview or remove old Runtime releases that are neither selected nor in use.",
    )],
  }[handler];
}

function cleanupLabel(state) {
  return {
    selected: t("当前选中", "Selected"),
    inUse: t("正在使用", "In use"),
    removable: t("可删除", "Removable"),
    removed: t("已删除", "Removed"),
    retained: t("已保留", "Retained"),
  }[state];
}

export function createRuntimeControlView(
  elements,
  {
    fetchImpl = fetch,
    confirmShutdown = (message) => window.confirm(message),
    confirmRestart = (message) => window.confirm(message),
    confirmCleanup = (message) => window.confirm(message),
    onRuntimeState = () => {},
  } = {},
) {
  let selectedHandler = null;
  let statusDocument = null;
  let hostBusy = false;

  function renderStatus() {
    const host = statusDocument?.host ?? null;
    elements.runtimeReleaseCount.textContent = statusDocument
      ? t(
        `${statusDocument.releaseCount} 个 Release`,
        `${statusDocument.releaseCount} releases`,
      )
      : "";
    elements.runtimeHostProperties.hidden = !host;
    elements.runtimeHostExit.disabled = hostBusy || !host;
    elements.runtimeHostRestart.disabled = hostBusy || !host?.updateAvailable;
    elements.runtimeHostRestart.hidden = selectedHandler === "host.exit"
      || (selectedHandler === "runtime.status" && !host?.updateAvailable);
    elements.runtimeHostExit.hidden = selectedHandler === "host.restart";

    if (!statusDocument) {
      elements.runtimeHostConnection.dataset.state = "loading";
      elements.runtimeHostStatus.textContent = t("正在读取状态…", "Loading status…");
      return;
    }
    if (!host) {
      elements.runtimeHostConnection.dataset.state = "error";
      elements.runtimeHostStatus.textContent = t("Host 离线", "Host offline");
      return;
    }
    elements.runtimeHostConnection.dataset.state = host.updateAvailable
      ? "warning"
      : "online";
    elements.runtimeHostStatus.textContent = host.updateAvailable
      ? t("Host 在线 · 新版本待重启", "Host online · new release pending restart")
      : t("Host 在线 · 当前版本", "Host online · current release");
    elements.runtimeHostPid.textContent = String(host.pid);
    elements.runtimeRunningRelease.textContent = host.runningReleaseId.slice(0, 12);
    elements.runtimeSelectedRelease.textContent = host.selectedReleaseId.slice(0, 12);
  }

  function renderStatusError(error) {
    statusDocument = null;
    elements.runtimeHostConnection.dataset.state = "error";
    elements.runtimeHostStatus.textContent = error instanceof Error
      ? error.message
      : t("Runtime 状态不可用", "Runtime status unavailable");
    elements.runtimeHostProperties.hidden = true;
    elements.runtimeHostExit.disabled = true;
    elements.runtimeHostRestart.disabled = true;
    onRuntimeState({
      icon: "●",
      summary: t("Runtime 状态不可用", "Runtime status unavailable"),
      tone: "error",
    });
  }

  async function load() {
    try {
      statusDocument = await readRuntimeStatus(fetchImpl);
      renderStatus();
      onRuntimeState(runtimeRootPresentation(statusDocument));
      return statusDocument;
    } catch (error) {
      renderStatusError(error);
      return null;
    }
  }

  function select(command) {
    selectedHandler = RUNTIME_HANDLERS.has(command?.handler)
      ? command.handler
      : null;
    const active = selectedHandler !== null;
    elements.genericCommandOverview.hidden = active;
    elements.runtimeControl.hidden = !active;
    if (!active) {
      return false;
    }
    const [title, description] = handlerPresentation(selectedHandler);
    elements.runtimeTitle.textContent = title;
    elements.runtimeDescription.textContent = description;
    elements.runtimeHostSection.hidden = selectedHandler === "runtime.cleanup";
    elements.runtimeCleanupSection.hidden = selectedHandler === "host.exit"
      || selectedHandler === "host.restart";
    renderStatus();
    void load();
    return true;
  }

  function setHostBusy(busy) {
    hostBusy = busy;
    renderStatus();
  }

  async function shutdown() {
    if (!confirmShutdown(t(
      "退出 Host？正在运行的 Web 命令也会被终止。",
      "Exit Host? Running Web commands will also terminate.",
    ))) {
      return;
    }
    setHostBusy(true);
    elements.runtimeHostConnection.dataset.state = "warning";
    elements.runtimeHostStatus.textContent = t("Host 正在退出…", "Host is exiting…");
    try {
      await requestHostShutdown(fetchImpl);
      elements.runtimeHostFeedback.textContent = t(
        "退出请求已接受；当前控制台即将断开。",
        "The exit request was accepted; this console will disconnect.",
      );
    } catch (error) {
      setHostBusy(false);
      elements.runtimeHostFeedback.textContent = error instanceof Error
        ? error.message
        : t("Host 退出失败", "Host shutdown failed");
    }
  }

  async function restart() {
    if (!confirmRestart(t(
      "重启 Host 并切换到已发布的新版本？正在运行的 Web 命令会被终止。",
      "Restart Host and switch to the published new release? Running Web commands will terminate.",
    ))) {
      return;
    }
    setHostBusy(true);
    elements.runtimeHostConnection.dataset.state = "warning";
    elements.runtimeHostStatus.textContent = t(
      "Host 正在重启更新…",
      "Host is restarting into the update…",
    );
    try {
      await requestHostRestart(fetchImpl);
      elements.runtimeHostFeedback.textContent = t(
        "重启请求已接受；请稍后重新打开控制台。",
        "The restart request was accepted; reopen the console shortly.",
      );
    } catch (error) {
      setHostBusy(false);
      elements.runtimeHostFeedback.textContent = error instanceof Error
        ? error.message
        : t("Host 重启失败", "Host restart failed");
    }
  }

  function renderCleanup(document) {
    const summary = [
      ["selected", t("选中", "Selected")],
      ["inUse", t("占用", "In use")],
      [
        document.action === "apply" ? "removed" : "removable",
        document.action === "apply" ? t("删除", "Removed") : t("可删", "Removable"),
      ],
      ["retained", t("保留", "Retained")],
    ];
    elements.runtimeCleanupSummary.replaceChildren(...summary.flatMap(([name, label]) => {
      const term = window.document.createElement("dt");
      const value = window.document.createElement("dd");
      term.textContent = label;
      value.textContent = String(document.summary[name]);
      return [term, value];
    }));
    elements.runtimeCleanupList.replaceChildren(...document.items.map((item) => {
      const row = window.document.createElement("li");
      const release = window.document.createElement("code");
      const state = window.document.createElement("span");
      release.textContent = item.releaseId.slice(0, 12);
      state.textContent = cleanupLabel(item.state);
      row.dataset.state = item.state;
      row.append(release, state);
      if (item.pids.length > 0) {
        const pids = window.document.createElement("small");
        pids.textContent = `PID ${item.pids.join(", ")}`;
        row.append(pids);
      } else if (item.reason) {
        const reason = window.document.createElement("small");
        reason.textContent = item.reason;
        row.append(reason);
      }
      return row;
    }));
    elements.runtimeCleanupResult.hidden = false;
  }

  async function cleanup(apply) {
    if (apply && !confirmCleanup(t(
      "删除预览中可清理的旧 Runtime Release？此操作不可撤销。",
      "Remove the old Runtime releases shown as removable? This cannot be undone.",
    ))) {
      return;
    }
    elements.runtimeCleanupPreview.disabled = true;
    elements.runtimeCleanupApply.disabled = true;
    elements.runtimeCleanupFeedback.textContent = apply
      ? t("正在应用清理…", "Applying cleanup…")
      : t("正在生成预览…", "Generating preview…");
    try {
      const document = await requestRuntimeCleanup(apply, fetchImpl);
      renderCleanup(document);
      elements.runtimeCleanupFeedback.textContent = apply
        ? t(
          `清理完成：删除 ${document.summary.removed} 个，保留 ${document.summary.retained} 个。`,
          `Cleanup complete: removed ${document.summary.removed}, retained ${document.summary.retained}.`,
        )
        : t(
          `预览完成：${document.summary.removable} 个 Release 可删除。`,
          `Preview complete: ${document.summary.removable} releases can be removed.`,
        );
      if (apply) {
        await load();
      }
    } catch (error) {
      elements.runtimeCleanupFeedback.textContent = error instanceof Error
        ? error.message
        : t("Runtime 清理失败", "Runtime cleanup failed");
    } finally {
      elements.runtimeCleanupPreview.disabled = false;
      elements.runtimeCleanupApply.disabled = false;
    }
  }

  elements.runtimeHostExit.addEventListener("click", () => void shutdown());
  elements.runtimeHostRestart.addEventListener("click", () => void restart());
  elements.runtimeCleanupPreview.addEventListener("click", () => void cleanup(false));
  elements.runtimeCleanupApply.addEventListener("click", () => void cleanup(true));
  return { cleanup, load, restart, select, shutdown };
}
