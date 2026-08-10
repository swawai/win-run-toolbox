const PROFILE_PROTOCOL = "swawkit.entry-profile-state/v3";
const SETTER_HANDLER = "entry.profile.set";

export class EntryProfileConflictError extends Error {}

function normalizeProfileDocument(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Entry Profile 协议无效：响应必须是对象。");
  }
  if (value.protocol !== PROFILE_PROTOCOL) {
    throw new Error(`Entry Profile 协议无效：protocol 必须是 ${PROFILE_PROTOCOL}。`);
  }
  if (typeof value.revision !== "string" || value.revision.length === 0) {
    throw new Error("Entry Profile 协议无效：revision 必须是非空字符串。");
  }
  if (!value.variables || typeof value.variables !== "object" || Array.isArray(value.variables)) {
    throw new Error("Entry Profile 协议无效：variables 必须是对象。");
  }
  for (const [name, current] of Object.entries(value.variables)) {
    if (!name.startsWith("SWAWKIT_PROJ_") || typeof current !== "string") {
      throw new Error("Entry Profile 协议无效：variables 必须映射变量名到字符串值。");
    }
  }
  return value;
}

export async function putEntryProfileVariable(
  name,
  value,
  revision,
  fetchProfile = fetch,
) {
  const response = await fetchProfile(
    `/api/v2/profile/variables/${encodeURIComponent(name)}`,
    {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "If-Match": `"${revision}"`,
      },
      body: JSON.stringify({ value }),
    },
  );
  const document = await response.json();
  if (response.status === 409) {
    throw new EntryProfileConflictError(document.error);
  }
  if (!response.ok) {
    throw new Error(document.error || `Host 返回 HTTP ${response.status}`);
  }
  return normalizeProfileDocument(document);
}

export function createEntryProfileView(elements, { onProfileChanged }) {
  let currentDocument = null;
  let currentCommand = null;

  function variableName(command) {
    return command.address.slice(command.address.lastIndexOf(".") + 1);
  }

  function renderState() {
    if (!currentDocument) {
      elements.profileState.dataset.state = "loading";
      elements.profileState.textContent = "正在读取 Entry Profile…";
      return;
    }
    elements.profileState.dataset.state = currentDocument.status;
    if (currentDocument.status === "ready") {
      elements.profileState.textContent = "Entry Profile 已生效";
    } else if (currentDocument.status === "invalid") {
      elements.profileState.textContent = currentDocument.error || "Entry Profile 无效";
    } else {
      elements.profileState.textContent = "保存任一有效变量即可发布默认 Profile";
    }
  }

  function render(command) {
    if (command.handler !== SETTER_HANDLER) {
      return false;
    }
    currentCommand = command;
    const name = variableName(command);
    const known = currentDocument && Object.hasOwn(currentDocument.variables, name);
    elements.commandWorkspace.hidden = true;
    elements.entryProfileDetail.hidden = false;
    elements.entryProfileTitle.textContent = name;
    elements.entryProfileSummary.textContent = command.summary
      || "原子修改这个 Entry Profile 变量。";
    elements.profileVariableName.textContent = name;
    elements.profileValue.value = known ? currentDocument.variables[name] : "";
    elements.profileValue.disabled = !known;
    elements.profileSaveButton.disabled = !known;
    elements.profileFeedback.textContent = known || !currentDocument
      ? ""
      : "Catalog 声明了 Profile 中不存在的变量。";
    elements.profileFeedback.dataset.state = known || !currentDocument ? "" : "error";
    renderState();
    elements.selectionStatus.textContent = `已选择命令 ${command.address}`;
    return true;
  }

  function acceptDocument(document) {
    currentDocument = normalizeProfileDocument(document);
    if (currentCommand) {
      render(currentCommand);
    }
    return currentDocument;
  }

  async function loadProfile() {
    currentDocument = null;
    renderState();
    const response = await fetch("/api/v2/profile", {
      cache: "no-store",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`Host 返回 HTTP ${response.status}`);
    }
    return acceptDocument(await response.json());
  }

  async function saveProfile() {
    if (!currentDocument || !currentCommand) {
      return;
    }
    const name = variableName(currentCommand);
    elements.profileSaveButton.disabled = true;
    elements.profileFeedback.dataset.state = "";
    elements.profileFeedback.textContent = "正在保存…";
    try {
      const document = await putEntryProfileVariable(
        name,
        elements.profileValue.value,
        currentDocument.revision,
      );
      acceptDocument(document);
      elements.profileFeedback.textContent = "变量已保存";
      await onProfileChanged(document, currentCommand.address);
    } catch (error) {
      elements.profileFeedback.dataset.state = "error";
      if (error instanceof EntryProfileConflictError) {
        try {
          const latest = await loadProfile();
          await onProfileChanged(latest, currentCommand.address);
          elements.profileFeedback.textContent = "Profile 已被其他进程修改，已重新载入最新值。";
        } catch {
          elements.profileFeedback.textContent = "Profile 已变化，请重新加载页面后再保存。";
        }
      } else {
        elements.profileFeedback.textContent = error instanceof Error
          ? error.message
          : "保存变量时发生未知错误。";
      }
    } finally {
      const known = currentDocument
        && currentCommand
        && Object.hasOwn(currentDocument.variables, variableName(currentCommand));
      elements.profileSaveButton.disabled = !known;
    }
  }

  return {
    loadProfile,
    render,
    saveProfile,
  };
}
