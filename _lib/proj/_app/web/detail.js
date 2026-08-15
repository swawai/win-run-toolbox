import {
  cliInvocation,
  isGroup,
} from "./catalog-model.js";
import { t } from "./i18n.js";

export function createDetailView(elements) {
  let copyTimer = null;
  let copyVersion = 0;

  function resetCopyFeedback() {
    if (copyTimer !== null) {
      window.clearTimeout(copyTimer);
      copyTimer = null;
    }
    elements.copyLabel.textContent = t("复制", "Copy");
    elements.copyFeedback.textContent = "";
    delete elements.copyFeedback.dataset.state;
  }

  function render(catalog, command) {
    copyVersion += 1;
    resetCopyFeedback();
    elements.entryProfileDetail.hidden = true;

    const group = isGroup(catalog, command);
    const issue = command.issue;
    elements.detailAddress.textContent = command.address;
    elements.detailSummary.textContent = command.summary
      || (group
        ? t("浏览这个命令组下的子命令。", "Browse the subcommands in this group.")
        : t("该命令尚未提供摘要。", "This command has no summary yet."));
    elements.issueCard.hidden = !issue;
    elements.detailIssue.textContent = issue;
    elements.invocationSection.hidden = !command.runnable;
    elements.cliCommand.textContent = command.runnable
      ? cliInvocation(catalog, command)
      : "";
    elements.copyButton.disabled = !command.runnable;
    elements.copyButton.title = command.runnable ? t("复制 CLI 调用", "Copy CLI invocation") : "";
    elements.propertyAddress.textContent = command.address;
    elements.propertyEntryRow.hidden = !command.entry;
    elements.propertyEntry.textContent = command.adapter
      ? `${command.entry} · ${command.adapter}`
      : command.entry;
    renderModuleContract(command.module);
    elements.detailHelp.textContent = command.help
      || t("该命令尚未提供详细帮助。", "This command has no detailed help yet.");
    elements.commandHelpAddress.textContent = command.address;
    elements.selectionStatus.textContent = t(
      `已选择命令 ${command.address}`,
      `Selected command ${command.address}`,
    );
  }

  function renderModuleContract(module) {
    elements.moduleContractSection.hidden = !module;
    elements.moduleRequires.replaceChildren();
    elements.moduleProvides.replaceChildren();
    if (!module) {
      return;
    }
    appendItems(
      elements.moduleRequires,
      module.requires.map(({ provider, contract }) => `${provider} · ${contract}`),
      t("无声明依赖", "No declared requirements"),
    );
    appendItems(
      elements.moduleProvides,
      module.provides.map(({ contract }) => contract),
      t("不提供 Export", "No Export provided"),
    );
  }

  function appendItems(list, items, emptyText) {
    const values = items.length > 0 ? items : [emptyText];
    for (const value of values) {
      const item = document.createElement("li");
      item.textContent = value;
      list.append(item);
    }
  }

  async function copyInvocation() {
    const version = ++copyVersion;
    const invocation = elements.cliCommand.textContent;
    try {
      await navigator.clipboard.writeText(invocation);
      if (
        version !== copyVersion
        || invocation !== elements.cliCommand.textContent
      ) {
        return;
      }
      resetCopyFeedback();
      elements.copyFeedback.textContent = t("已复制到剪贴板", "Copied to clipboard");
      elements.copyFeedback.dataset.state = "success";
      elements.copyLabel.textContent = t("已复制", "Copied");
      copyTimer = window.setTimeout(() => {
        if (version === copyVersion) {
          resetCopyFeedback();
        }
      }, 1600);
    } catch {
      if (
        version !== copyVersion
        || invocation !== elements.cliCommand.textContent
      ) {
        return;
      }
      resetCopyFeedback();
      elements.copyFeedback.textContent = t(
        "复制失败，请手动选择命令文本。",
        "Copy failed. Select the command text manually.",
      );
      elements.copyFeedback.dataset.state = "error";
    }
  }

  return {
    copyInvocation,
    render,
  };
}
