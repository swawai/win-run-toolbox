const HOST_RUNTIME_PROTOCOL = "swawkit.host-runtime/v1";
const SHA256 = /^[a-f0-9]{64}$/;
const LOOPBACK_URL = /^http:\/\/127\.0\.0\.1:([1-9][0-9]{0,4})\/$/;

export class HostControlError extends Error {}

export async function readHostStatus(fetchImpl = fetch) {
  const response = await fetchImpl("/api/v2/host", {
    cache: "no-store",
    headers: { Accept: "application/json" },
  });
  if (!response.ok) {
    throw new HostControlError(`Host 状态返回 HTTP ${response.status}`);
  }
  const document = await response.json();
  const port = typeof document?.url === "string"
    ? Number(LOOPBACK_URL.exec(document.url)?.[1])
    : 0;
  if (
    document?.protocol !== HOST_RUNTIME_PROTOCOL
    || typeof document.entryKeySha256 !== "string"
    || !SHA256.test(document.entryKeySha256)
    || !Number.isInteger(document.pid)
    || document.pid <= 0
    || typeof document.bootId !== "string"
    || document.bootId.length === 0
    || document.bootId.length > 160
    || !Number.isInteger(port)
    || port > 65535
  ) {
    throw new HostControlError("Host 返回了无效的运行状态。");
  }
  return document;
}

export async function requestHostShutdown(fetchImpl = fetch) {
  const response = await fetchImpl("/api/v2/host/shutdown", {
    method: "POST",
    headers: { "X-SwawKit-Control": "shutdown" },
  });
  if (response.status !== 202 && response.status !== 204) {
    throw new HostControlError(`Host 拒绝退出请求：HTTP ${response.status}`);
  }
}

export function createHostControlView(
  elements,
  {
    fetchImpl = fetch,
    confirmShutdown = (message) => window.confirm(message),
  } = {},
) {
  async function load() {
    try {
      const status = await readHostStatus(fetchImpl);
      elements.hostIndicator.dataset.state = "online";
      elements.hostStatus.textContent = `Host 在线 · PID ${status.pid}`;
    } catch (error) {
      elements.hostIndicator.dataset.state = "error";
      elements.hostStatus.textContent = error instanceof Error
        ? error.message
        : "Host 状态不可用";
    }
  }

  async function shutdown() {
    if (!confirmShutdown("退出 Host？正在运行的 Web 命令也会被终止。")) {
      return;
    }
    elements.hostQuit.disabled = true;
    elements.hostIndicator.dataset.state = "stopping";
    elements.hostStatus.textContent = "Host 正在退出…";
    try {
      await requestHostShutdown(fetchImpl);
    } catch (error) {
      elements.hostQuit.disabled = false;
      elements.hostIndicator.dataset.state = "error";
      elements.hostStatus.textContent = error instanceof Error
        ? error.message
        : "Host 退出失败";
    }
  }

  elements.hostQuit.addEventListener("click", shutdown);
  return { load, shutdown };
}
