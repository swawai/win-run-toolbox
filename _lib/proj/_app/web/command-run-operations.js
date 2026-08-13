export function createCommandRunOperations(elements, options) {
  const documentObject = options.document;
  const onExecute = options.onExecute;
  let operations = [];
  let pending = null;
  let renderState = { blocked: false, runnable: false };

  function buttons() {
    return elements.commandRunOperationList.querySelectorAll(".command-run-operation");
  }

  function render(state = renderState) {
    renderState = state;
    const operationMode = operations.length > 0;
    elements.commandRunEditor.hidden = operationMode;
    elements.commandRunOperations.hidden = !operationMode;
    elements.commandRunSubmit.hidden = operationMode;
    for (const button of buttons()) {
      button.disabled = !state.runnable || state.blocked;
    }
    const confirming = operationMode && pending !== null;
    elements.commandRunConfirmation.hidden = !confirming;
    elements.commandRunConfirmationText.textContent = confirming
      ? pending.confirmation
      : "";
    elements.commandRunConfirm.disabled = !state.runnable || state.blocked;
    elements.commandRunConfirmDismiss.disabled = state.blocked;
  }

  function select(command) {
    operations = Array.isArray(command?.runOperations) ? command.runOperations : [];
    pending = null;
    const operationButtons = operations.map((operation) => {
      const button = documentObject.createElement("button");
      button.className = "command-run-operation";
      button.type = "button";
      button.dataset.operation = operation.id;
      button.textContent = operation.label;
      button.addEventListener("click", () => {
        if (operation.confirmation) {
          pending = operation;
          render();
          elements.commandRunConfirm.focus();
        } else {
          onExecute([...operation.arguments]);
        }
      });
      return button;
    });
    elements.commandRunOperationList.replaceChildren(...operationButtons);
    render();
  }

  elements.commandRunConfirm.addEventListener("click", () => {
    const operation = pending;
    if (!operation) {
      return;
    }
    pending = null;
    render();
    onExecute([...operation.arguments]);
  });
  elements.commandRunConfirmDismiss.addEventListener("click", () => {
    pending = null;
    render();
  });

  select(null);
  return {
    render,
    select,
    usesOperations: () => operations.length > 0,
  };
}
