import {
  cliInvocation,
  isGroup,
} from "./catalog-model.js";

export function createDetailView(elements) {
  let copyTimer = null;
  let copyVersion = 0;

  function resetCopyFeedback() {
    if (copyTimer !== null) {
      window.clearTimeout(copyTimer);
      copyTimer = null;
    }
    elements.copyLabel.textContent = "复制";
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
      || (group ? "浏览这个命令组下的子命令。" : "该命令尚未提供摘要。");
    elements.issueCard.hidden = !issue;
    elements.detailIssue.textContent = issue;
    elements.invocationSection.hidden = !command.runnable;
    elements.cliCommand.textContent = command.runnable
      ? cliInvocation(catalog, command)
      : "";
    elements.copyButton.disabled = !command.runnable;
    elements.copyButton.title = command.runnable ? "复制 CLI 调用" : "";
    elements.propertyAddress.textContent = command.address;
    elements.propertyEntryRow.hidden = !command.entry;
    elements.propertyEntry.textContent = command.adapter
      ? `${command.entry} · ${command.adapter}`
      : command.entry;
    elements.detailHelp.textContent = command.help || "该命令尚未提供详细帮助。";
    elements.commandHelpAddress.textContent = command.address;
    elements.selectionStatus.textContent = `已选择命令 ${command.address}`;
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
      elements.copyFeedback.textContent = "已复制到剪贴板";
      elements.copyFeedback.dataset.state = "success";
      elements.copyLabel.textContent = "已复制";
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
      elements.copyFeedback.textContent = "复制失败，请手动选择命令文本。";
      elements.copyFeedback.dataset.state = "error";
    }
  }

  return {
    copyInvocation,
    render,
  };
}
