import { t } from "./i18n.js";

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
      await readApiError(
        response,
        t(`Host 返回 HTTP ${response.status}`, `Host returned HTTP ${response.status}`),
      ),
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
      t(
        `请输入完整名称“${pending.claim.entryName}”后再确认。`,
        `Enter the full name “${pending.claim.entryName}” to confirm.`,
      ),
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
    const message = await readApiError(
      response,
      t(`Host 返回 HTTP ${response.status}`, `Host returned HTTP ${response.status}`),
    );
    if (response.status === 409) {
      throw new DataRootClaimConflictError(message);
    }
    throw new DataRootClaimError(message, response.status);
  }
  if (response.status !== 204) {
    throw new DataRootClaimError(
      t(
        "Host 返回了无效的 DataRoot 认领结果。",
        "Host returned an invalid DataRoot claim result.",
      ),
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
    setFeedback(t(
      "认领信息已经变化，请重新核对并输入名称。",
      "Claim details changed. Review them and enter the name again.",
    ), "error");
    elements.claimConfirmation.focus();
  }

  async function submit() {
    if (!pending || submitting) {
      return;
    }
    const confirmation = elements.claimConfirmation.value;
    if (!matchesClaimConfirmation(pending.claim, confirmation)) {
      setFeedback(t(
        `请输入完整名称“${pending.claim.entryName}”后再确认。`,
        `Enter the full name “${pending.claim.entryName}” to confirm.`,
      ), "error");
      updateSubmitState();
      return;
    }

    submitting = true;
    updateSubmitState();
    setFeedback(t("正在重新核对并认领 DataRoot…", "Rechecking and claiming DataRoot…"));
    try {
      await confirmDataRootClaim(pending, confirmation, fetchClaim);
      pending = null;
      await onReady();
    } catch (error) {
      if (error instanceof DataRootClaimConflictError) {
        setFeedback(t("认领信息已经变化，正在刷新…", "Claim details changed; refreshing…"));
        try {
          await refreshAfterConflict();
        } catch (refreshError) {
          const message = refreshError instanceof Error
            ? refreshError.message
            : t(
              "刷新认领信息时发生未知错误。",
              "An unknown error occurred while refreshing claim details.",
            );
          setFeedback(t(`认领信息刷新失败：${message}`, `Failed to refresh claim details: ${message}`), "error");
        }
      } else {
        setFeedback(
          error instanceof Error
            ? error.message
            : t(
              "认领 DataRoot 时发生未知错误。",
              "An unknown error occurred while claiming DataRoot.",
            ),
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
    throw new DataRootClaimError(t(
      "Host 返回了无效的 DataRoot 认领状态。",
      "Host returned invalid DataRoot claim state.",
    ));
  }
  if (document.protocol !== CLAIM_PROTOCOL) {
    throw new DataRootClaimError(t(
      `Host 不支持 DataRoot 认领协议 ${CLAIM_PROTOCOL}。`,
      `Host does not support DataRoot claim protocol ${CLAIM_PROTOCOL}.`,
    ));
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
      throw new DataRootClaimError(t(
        `DataRoot 认领状态缺少 ${field}。`,
        `DataRoot claim state is missing ${field}.`,
      ));
    }
  }
  if (
    document.claim.sourceDataRoot !== null
    && document.claim.sourceDataRoot !== undefined
    && typeof document.claim.sourceDataRoot !== "string"
  ) {
    throw new DataRootClaimError(t(
      "DataRoot 认领状态包含无效的 sourceDataRoot。",
      "DataRoot claim state contains an invalid sourceDataRoot.",
    ));
  }
  if (typeof revision !== "string" || !revision) {
    throw new DataRootClaimError(t(
      "Host 未提供 DataRoot 认领版本，无法安全确认。",
      "Host did not provide a DataRoot claim revision, so the claim cannot be confirmed safely.",
    ));
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
