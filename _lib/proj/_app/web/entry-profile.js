import { t } from "./i18n.js";

const PROFILE_PROTOCOL = "swawkit.entry-profile-state/v5";
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
  if (!value.settings || typeof value.settings !== "object" || Array.isArray(value.settings)) {
    throw new Error("Entry Profile 协议无效：settings 必须是对象。");
  }
  for (const [address, current] of Object.entries(value.settings)) {
    if (!address.startsWith(".") || typeof current !== "string") {
      throw new Error("Entry Profile 协议无效：settings 必须映射命令地址到字符串值。");
    }
  }
  if (!value.profile || typeof value.profile !== "object" || Array.isArray(value.profile)) {
    throw new Error("Entry Profile 协议无效：profile 必须是对象。");
  }
  if (!new Set(["zh-CN", "en"]).has(value.profile.language)) {
    throw new Error("Entry Profile 协议无效：profile.language 只能是 zh-CN 或 en。");
  }
  if (value.settings["..entry.language"] !== value.profile.language) {
    throw new Error("Entry Profile 协议无效：语言设置与 profile.language 不一致。");
  }
  return value;
}

export async function putEntryProfileSetting(
  address,
  value,
  revision,
  fetchProfile = fetch,
) {
  const response = await fetchProfile(
    `/api/v2/profile/settings/${encodeURIComponent(address)}`,
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
    throw new Error(document.error || t(
      `Host 返回 HTTP ${response.status}`,
      `Host returned HTTP ${response.status}`,
    ));
  }
  return normalizeProfileDocument(document);
}

export function createEntryProfileView(
  elements,
  { fetchImpl = fetch, onProfileChanged },
) {
  let currentDocument = null;
  let currentCommand = null;
  let saveInFlight = false;

  function renderState() {
    if (!currentDocument) {
      elements.profileState.dataset.state = "loading";
      elements.profileState.textContent = t(
        "正在读取 Entry Profile…",
        "Loading Entry Profile…",
      );
      return;
    }
    elements.profileState.dataset.state = currentDocument.status;
    if (currentDocument.status === "ready") {
      elements.profileState.textContent = t("Entry Profile 已生效", "Entry Profile is active");
    } else if (currentDocument.status === "invalid") {
      elements.profileState.textContent = currentDocument.error
        || t("Entry Profile 无效", "Entry Profile is invalid");
    } else {
      elements.profileState.textContent = t(
        "保存任一有效设置即可发布默认 Profile",
        "Save any valid setting to publish the default Profile",
      );
    }
  }

  function render(command) {
    if (command?.handler !== SETTER_HANDLER) {
      currentCommand = null;
      return false;
    }
    currentCommand = command;
    const address = command.address;
    const label = address.slice(address.lastIndexOf(".") + 1);
    const known = currentDocument && Object.hasOwn(currentDocument.settings, address);
    elements.entryProfileTitle.textContent = label;
    elements.entryProfileSummary.textContent = command.summary
      || t(
        "原子修改这个 Entry Profile 设置。",
        "Atomically update this Entry Profile setting.",
      );
    elements.profileSettingAddress.textContent = address;
    elements.profileValue.value = known ? currentDocument.settings[address] : "";
    elements.profileValue.disabled = !known;
    elements.profileSaveButton.disabled = !known || saveInFlight;
    elements.profileFeedback.textContent = known || !currentDocument
      ? ""
      : t(
        "Catalog 声明了 Profile 中不存在的设置。",
        "The Catalog declares a setting that is absent from the Profile.",
      );
    elements.profileFeedback.dataset.state = known || !currentDocument ? "" : "error";
    renderState();
    elements.selectionStatus.textContent = t(
      `已选择命令 ${command.address}`,
      `Selected command ${command.address}`,
    );
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
    const response = await fetchImpl("/api/v2/profile", {
      cache: "no-store",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(t(
        `Host 返回 HTTP ${response.status}`,
        `Host returned HTTP ${response.status}`,
      ));
    }
    return acceptDocument(await response.json());
  }

  async function saveProfile() {
    if (!currentDocument || !currentCommand || saveInFlight) {
      return;
    }
    const command = currentCommand;
    const address = command.address;
    const operationIsCurrent = () => currentCommand?.address === command.address;
    saveInFlight = true;
    elements.profileSaveButton.disabled = true;
    elements.profileFeedback.dataset.state = "";
    elements.profileFeedback.textContent = t("正在保存…", "Saving…");
    try {
      const document = await putEntryProfileSetting(
        address,
        elements.profileValue.value,
        currentDocument.revision,
        fetchImpl,
      );
      acceptDocument(document);
      await onProfileChanged(document);
      if (operationIsCurrent()) {
        elements.profileFeedback.textContent = t("设置已保存", "Setting saved");
      }
    } catch (error) {
      if (error instanceof EntryProfileConflictError) {
        try {
          const latest = await loadProfile();
          await onProfileChanged(latest);
          if (operationIsCurrent()) {
            elements.profileFeedback.dataset.state = "error";
            elements.profileFeedback.textContent = t(
              "Profile 已被其他进程修改，已重新载入最新值。",
              "Another process changed the Profile; the latest value has been loaded.",
            );
          }
        } catch {
          if (operationIsCurrent()) {
            elements.profileFeedback.dataset.state = "error";
            elements.profileFeedback.textContent = t(
              "Profile 已变化，请重新加载页面后再保存。",
              "The Profile changed. Reload the page before saving again.",
            );
          }
        }
      } else if (operationIsCurrent()) {
        elements.profileFeedback.dataset.state = "error";
        elements.profileFeedback.textContent = error instanceof Error
          ? error.message
          : t("保存设置时发生未知错误。", "An unknown error occurred while saving.");
      }
    } finally {
      saveInFlight = false;
      const known = currentDocument
        && currentCommand
        && Object.hasOwn(currentDocument.settings, currentCommand.address);
      elements.profileSaveButton.disabled = !known;
    }
  }

  return {
    loadProfile,
    render,
    saveProfile,
  };
}
