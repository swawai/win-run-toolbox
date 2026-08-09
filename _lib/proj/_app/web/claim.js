const CLAIM_URL = "/api/v2/data-root/claim";
const CLAIM_PROTOCOL = "swawkit.data-root-claim/v2";

export class DataRootClaimError extends Error {
  constructor(message, status = 0) {
    super(message);
    this.name = "DataRootClaimError";
    this.status = status;
  }
}

export class DataRootClaimConflictError extends DataRootClaimError {
  constructor(message) {
    super(message, 409);
    this.name = "DataRootClaimConflictError";
  }
}

export function matchesClaimConfirmation(claim, confirmation) {
  return typeof confirmation === "string" && confirmation === claim.entryName;
}

export function claimDetailValues(claim) {
  return {
    kind: claim.kind,
    entryName: claim.entryName,
    entryFile: claim.entryFile,
    volumeId: claim.volumeId,
    fileId: claim.fileId,
    dataRoot: claim.dataRoot,
    sourceDataRoot: claim.sourceDataRoot || "—",
    reason: claim.reason,
  };
}

export async function readDataRootClaim(fetchClaim = fetch) {
  const response = await fetchClaim(CLAIM_URL, {
    cache: "no-store",
    headers: { Accept: "application/json" },
  });
  if (response.status === 204) {
    return { status: "ready" };
  }
  if (!response.ok) {
    throw new DataRootClaimError(
      await readApiError(response, `Host 返回 HTTP ${response.status}`),
      response.status,
    );
  }

  const document = await response.json();
  const revision = response.headers.get("etag");
  validateClaimDocument(document, revision);
  return { status: document.status, claim: document.claim, revision };
}

export async function confirmDataRootClaim(pending, confirmation, fetchClaim = fetch) {
  if (!matchesClaimConfirmation(pending.claim, confirmation)) {
    throw new DataRootClaimError(
      `请输入完整名称“${pending.claim.entryName}”后再确认。`,
      422,
    );
  }

  const response = await fetchClaim(CLAIM_URL, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      "If-Match": pending.revision,
    },
    body: JSON.stringify({ confirmation }),
  });
  if (!response.ok) {
    const message = await readApiError(response, `Host 返回 HTTP ${response.status}`);
    if (response.status === 409) {
      throw new DataRootClaimConflictError(message);
    }
    throw new DataRootClaimError(message, response.status);
  }
  if (response.status !== 204) {
    throw new DataRootClaimError(
      "Host 返回了无效的 DataRoot 认领结果。",
      response.status,
    );
  }
}

export function createDataRootClaimView(
  elements,
  { onClaimRequired, onReady, fetchClaim = fetch },
) {
  let pending = null;
  let submitting = false;

  function setFeedback(message = "", state = "") {
    elements.claimFeedback.textContent = message;
    elements.claimFeedback.dataset.state = state;
  }

  function updateSubmitState() {
    elements.claimSubmit.disabled = submitting
      || !pending
      || !matchesClaimConfirmation(pending.claim, elements.claimConfirmation.value);
  }

  function render(next) {
    pending = next;
    const values = claimDetailValues(next.claim);
    elements.claimKind.textContent = values.kind;
    elements.claimEntryName.textContent = values.entryName;
    elements.claimEntryFile.textContent = values.entryFile;
    elements.claimVolumeId.textContent = values.volumeId;
    elements.claimFileId.textContent = values.fileId;
    elements.claimDataRoot.textContent = values.dataRoot;
    elements.claimSourceDataRoot.textContent = values.sourceDataRoot;
    elements.claimReason.textContent = values.reason;
    elements.claimConfirmationName.textContent = values.entryName;
    elements.claimConfirmation.value = "";
    setFeedback();
    updateSubmitState();
  }

  async function ensureReady() {
    const next = await readDataRootClaim(fetchClaim);
    if (next.status === "ready") {
      pending = null;
      await onReady();
      return;
    }
    render(next);
    onClaimRequired();
    elements.claimConfirmation.focus();
    return false;
  }

  async function refreshAfterConflict() {
    const next = await readDataRootClaim(fetchClaim);
    if (next.status === "ready") {
      pending = null;
      await onReady();
      return;
    }
    render(next);
    onClaimRequired();
    setFeedback("认领信息已经变化，请重新核对并输入名称。", "error");
    elements.claimConfirmation.focus();
  }

  async function submit() {
    if (!pending || submitting) {
      return;
    }
    const confirmation = elements.claimConfirmation.value;
    if (!matchesClaimConfirmation(pending.claim, confirmation)) {
      setFeedback(`请输入完整名称“${pending.claim.entryName}”后再确认。`, "error");
      updateSubmitState();
      return;
    }

    submitting = true;
    updateSubmitState();
    setFeedback("正在重新核对并认领 DataRoot…");
    try {
      await confirmDataRootClaim(pending, confirmation, fetchClaim);
      pending = null;
      await onReady();
    } catch (error) {
      if (error instanceof DataRootClaimConflictError) {
        setFeedback("认领信息已经变化，正在刷新…");
        try {
          await refreshAfterConflict();
        } catch (refreshError) {
          const message = refreshError instanceof Error
            ? refreshError.message
            : "刷新认领信息时发生未知错误。";
          setFeedback(`认领信息刷新失败：${message}`, "error");
        }
      } else {
        setFeedback(
          error instanceof Error ? error.message : "认领 DataRoot 时发生未知错误。",
          "error",
        );
      }
    } finally {
      submitting = false;
      updateSubmitState();
    }
  }

  elements.claimConfirmation.addEventListener("input", () => {
    setFeedback();
    updateSubmitState();
  });
  elements.claimForm.addEventListener("submit", (event) => {
    event.preventDefault();
    submit();
  });

  return { ensureReady, submit };
}

function validateClaimDocument(document, revision) {
  if (!document || document.status !== "claimRequired" || !document.claim) {
    throw new DataRootClaimError("Host 返回了无效的 DataRoot 认领状态。");
  }
  if (document.protocol !== CLAIM_PROTOCOL) {
    throw new DataRootClaimError(`Host 不支持 DataRoot 认领协议 ${CLAIM_PROTOCOL}。`);
  }
  for (const field of [
    "kind",
    "entryName",
    "entryFile",
    "volumeId",
    "fileId",
    "dataRoot",
    "reason",
  ]) {
    if (typeof document.claim[field] !== "string" || !document.claim[field]) {
      throw new DataRootClaimError(`DataRoot 认领状态缺少 ${field}。`);
    }
  }
  if (
    document.claim.sourceDataRoot !== null
    && document.claim.sourceDataRoot !== undefined
    && typeof document.claim.sourceDataRoot !== "string"
  ) {
    throw new DataRootClaimError("DataRoot 认领状态包含无效的 sourceDataRoot。");
  }
  if (typeof revision !== "string" || !revision) {
    throw new DataRootClaimError("Host 未提供 DataRoot 认领版本，无法安全确认。");
  }
}

async function readApiError(response, fallback) {
  try {
    const document = await response.json();
    if (typeof document?.error === "string" && document.error) {
      return document.error;
    }
  } catch {
    // The status and fallback remain sufficient when the body is not JSON.
  }
  return fallback;
}
