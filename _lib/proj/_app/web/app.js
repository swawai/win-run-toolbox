import { createCatalog, hasChildren } from "./catalog-model.js";
import { createDataRootClaimView } from "./claim.js";
import { createCommandActivityView } from "./command-activity.js";
import { createCommandRunView } from "./command-run.js";
import { createCommandJournalView } from "./command-journal.js";
import { createDetailView } from "./detail.js";
import { createExplorerView } from "./explorer.js";
import { createHostControlView } from "./host-control.js";
import { createEntryProfileView } from "./entry-profile.js";
import {
  commandAtPath,
  parseCommandView,
  updateCommandPath,
} from "./navigation.js";

const elements = {
  breadcrumb: document.querySelector("#breadcrumb"),
  cliCommand: document.querySelector("#cli-command"),
  claimConfirmation: document.querySelector("#claim-confirmation"),
  claimConfirmationName: document.querySelector("#claim-confirmation-name"),
  claimDataRoot: document.querySelector("#claim-data-root"),
  claimEntryFile: document.querySelector("#claim-entry-file"),
  claimEntryName: document.querySelector("#claim-entry-name"),
  claimFeedback: document.querySelector("#claim-feedback"),
  claimFileId: document.querySelector("#claim-file-id"),
  claimForm: document.querySelector("#claim-form"),
  claimKind: document.querySelector("#claim-kind"),
  claimReason: document.querySelector("#claim-reason"),
  claimSourceDataRoot: document.querySelector("#claim-source-data-root"),
  claimState: document.querySelector("#claim-state"),
  claimSubmit: document.querySelector("#claim-submit"),
  claimVolumeId: document.querySelector("#claim-volume-id"),
  commandDetail: document.querySelector("#command-detail"),
  commandHelpActivity: document.querySelector("#command-help-activity"),
  commandHelpAddress: document.querySelector("#command-help-address"),
  commandJournalActivity: document.querySelector("#command-journal-activity"),
  commandJournalAddress: document.querySelector("#command-journal-address"),
  commandJournalDetail: document.querySelector("#command-journal-detail"),
  commandJournalDetailEmpty: document.querySelector("#command-journal-detail-empty"),
  commandJournalDirectoryFeedback: document.querySelector("#command-journal-directory-feedback"),
  commandJournalEmpty: document.querySelector("#command-journal-empty"),
  commandJournalFeedback: document.querySelector("#command-journal-feedback"),
  commandJournalList: document.querySelector("#command-journal-list"),
  commandJournalMeta: document.querySelector("#command-journal-meta"),
  commandJournalOutput: document.querySelector("#command-journal-output"),
  commandJournalOpen: document.querySelector("#command-journal-open"),
  commandJournalRefresh: document.querySelector("#command-journal-refresh"),
  commandJournalState: document.querySelector("#command-journal-state"),
  commandJournalTruncated: document.querySelector("#command-journal-truncated"),
  commandRunActivity: document.querySelector("#command-run-activity"),
  commandRunAdd: document.querySelector("#command-run-add"),
  commandRunAddress: document.querySelector("#command-run-address"),
  commandRunArguments: document.querySelector("#command-run-arguments"),
  commandRunCancel: document.querySelector("#command-run-cancel"),
  commandRunEmpty: document.querySelector("#command-run-empty"),
  commandRunExitCode: document.querySelector("#command-run-exit-code"),
  commandRunFeedback: document.querySelector("#command-run-feedback"),
  commandRunForm: document.querySelector("#command-run-form"),
  commandRunOutput: document.querySelector("#command-run-output"),
  commandRunResult: document.querySelector("#command-run-result"),
  commandRunSection: document.querySelector("#command-run-section"),
  commandRunState: document.querySelector("#command-run-state"),
  commandRunSubmit: document.querySelector("#command-run-submit"),
  commandRunTruncated: document.querySelector("#command-run-truncated"),
  commandWorkspace: document.querySelector("#command-workspace"),
  copyButton: document.querySelector("#copy-button"),
  copyFeedback: document.querySelector("#copy-feedback"),
  copyLabel: document.querySelector("#copy-label"),
  detailAddress: document.querySelector("#detail-address"),
  detailHelp: document.querySelector("#detail-help"),
  detailIssue: document.querySelector("#detail-issue"),
  detailPanel: document.querySelector("#detail-panel"),
  detailSummary: document.querySelector("#detail-summary"),
  errorMessage: document.querySelector("#error-message"),
  errorState: document.querySelector("#error-state"),
  explorerFrame: document.querySelector("#explorer-frame"),
  explorerFlow: document.querySelector("#explorer-flow"),
  finderColumns: document.querySelector("#finder-columns"),
  hostQuit: document.querySelector("#host-quit"),
  hostIndicator: document.querySelector("#host-indicator"),
  hostStatus: document.querySelector("#host-status"),
  invocationSection: document.querySelector("#invocation-section"),
  issueCard: document.querySelector("#issue-card"),
  loadingState: document.querySelector("#loading-state"),
  propertyAddress: document.querySelector("#property-address"),
  propertyEntry: document.querySelector("#property-entry"),
  propertyEntryRow: document.querySelector("#property-entry-row"),
  profileFeedback: document.querySelector("#profile-feedback"),
  profileForm: document.querySelector("#profile-form"),
  profileSaveButton: document.querySelector("#profile-save-button"),
  profileState: document.querySelector("#profile-state"),
  profileValue: document.querySelector("#profile-value"),
  profileVariableName: document.querySelector("#profile-variable-name"),
  retryButton: document.querySelector("#retry-button"),
  selectionStatus: document.querySelector("#selection-status"),
  entryProfileDetail: document.querySelector("#entry-profile-detail"),
  entryProfileSummary: document.querySelector("#entry-profile-summary"),
  entryProfileTitle: document.querySelector("#entry-profile-title"),
};

let catalog = null;
const detail = createDetailView(elements);
const commandRun = createCommandRunView(elements);
const commandJournal = createCommandJournalView(elements);
const commandActivity = createCommandActivityView(elements);
const entryProfile = createEntryProfileView(elements, {
  async onProfileChanged(document) {
    explorer.setSetupRequired(!document.requiredComplete);
    await loadCatalog();
  },
});
const explorer = createExplorerView({
  breadcrumb: elements.breadcrumb,
  columns: elements.finderColumns,
  detailPanel: elements.detailPanel,
  getCommandViews(command) {
    return commandActivity.items(command, {
      hasChildren: hasChildren(catalog, command),
    });
  },
  onSelectCommand(command, options = {}) {
    entryProfile.render(command);
    detail.render(catalog, command);
    commandRun.select(command);
    const selection = commandActivity.selectCommand(command, {
      hasChildren: hasChildren(catalog, command),
      view: options.view,
    });
    commandJournal.select(command, { active: selection.view === "logs" });
    elements.detailPanel.dataset.view = selection.view ?? "";
    elements.detailPanel.hidden = selection.view === "children";
    updateCommandPath(
      window.history,
      window.location,
      command,
      {
        ...selection,
        mode: options.history ?? "none",
      },
    );
  },
});
const dataRootClaim = createDataRootClaimView(elements, {
  onClaimRequired() {
    setLoadState("claim");
  },
  onReady: loadApplication,
});
const hostControl = createHostControlView(elements);

function setLoadState(status, message = "") {
  const loading = status === "loading";
  const failed = status === "error";

  elements.loadingState.hidden = !loading;
  elements.errorState.hidden = !failed;
  elements.claimState.hidden = status !== "claim";
  elements.explorerFlow.hidden = status !== "ready";
  elements.explorerFrame.setAttribute("aria-busy", String(loading));

  if (failed) {
    elements.errorMessage.textContent = message || "无法连接 Host。";
  }
}

async function startApplication() {
  setLoadState("loading");
  try {
    await dataRootClaim.ensureReady();
  } catch (error) {
    const message = error instanceof Error
      ? error.message
      : "读取 DataRoot 状态时发生未知错误。";
    setLoadState("error", message);
  }
}

async function loadCatalog() {
  setLoadState("loading");
  try {
    const response = await fetch("/api/v2/catalog", {
      cache: "no-store",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`Host 返回 HTTP ${response.status}`);
    }

    catalog = createCatalog(await response.json());
    explorer.setCatalog(catalog, { history: "replace" });
    setLoadState("ready");
  } catch (error) {
    const message = error instanceof Error
      ? error.message
      : "读取 Catalog 时发生未知错误。";
    setLoadState("error", message);
  }
}

async function loadApplication() {
  setLoadState("loading");
  try {
    const document = await entryProfile.loadProfile();
    explorer.setSetupRequired(!document.requiredComplete);
    const response = await fetch("/api/v2/catalog", {
      cache: "no-store",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`Host 返回 HTTP ${response.status}`);
    }
    catalog = createCatalog(await response.json());
    const routed = commandAtPath(catalog, window.location.pathname, {
      allowMissing: !document.requiredComplete,
    });
    explorer.setCatalog(catalog, {
      address: routed?.address,
      history: "replace",
      view: parseCommandView(window.location.search),
    });
    setLoadState("ready");
  } catch (error) {
    const message = error instanceof Error
      ? error.message
      : "读取控制台状态时发生未知错误。";
    setLoadState("error", message);
  }
}

elements.copyButton.addEventListener("click", detail.copyInvocation);
elements.profileForm.addEventListener("submit", (event) => {
  event.preventDefault();
  entryProfile.saveProfile();
});
elements.finderColumns.addEventListener("keydown", explorer.handleKeyboard);
elements.retryButton.addEventListener("click", startApplication);
window.addEventListener("popstate", () => {
  if (!catalog) {
    return;
  }
  try {
    const routed = commandAtPath(catalog, window.location.pathname);
    if (
      !routed
      || !explorer.selectAddress(routed.address, {
        history: "none",
        view: parseCommandView(window.location.search),
      })
    ) {
      explorer.setCatalog(catalog, { history: "replace" });
    }
    setLoadState("ready");
  } catch (error) {
    setLoadState(
      "error",
      error instanceof Error ? error.message : "当前命令 URL 无效。",
    );
  }
});

void commandRun.restore();
void hostControl.load();
startApplication();
