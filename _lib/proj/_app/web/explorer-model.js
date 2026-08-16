export function commandDisabledDuringSetup(setupRequired, command) {
  return setupRequired && command.setupAvailable !== true && command.source !== "control";
}

export function availableCommand(catalog, setupRequired, address) {
  const command = catalog.commandByAddress.get(address);
  return command && !commandDisabledDuringSetup(setupRequired, command)
    ? command
    : null;
}

export function childrenColumnWidth(command) {
  return command.childrenColumnWidth || "normal";
}

export function commandHasChoices(_catalog, _command, facets) {
  return facets.length > 0;
}

export function commandMenuExpanded(selectedPath, address, depth) {
  return selectedPath[depth] === address && depth === selectedPath.length - 1;
}

export function selectedCommandFacet(facets) {
  return facets.find((facet) => facet.selected)?.name ?? null;
}

export function choiceColumnModels(
  catalog,
  selectedPath,
  getViews,
  selectedSubjectCollection = null,
) {
  return selectedPath.flatMap((address, index) => {
    const command = catalog.commandByAddress.get(address);
    if (!command) {
      return [];
    }
    const facets = getViews(command);
    const terminal = index === selectedPath.length - 1;
    if (!terminal) {
      const children = facets.find((facet) => (
        facet.kind === "collection"
        && facet.resolver?.type === "catalog"
        && facet.resolver.relation === "children"
      ));
      return children
        ? [{ command, depth: index + 1, mode: children.name }]
        : [];
    }
    if (selectedSubjectCollection?.owner === address) {
      return [{
        command,
        depth: index + 1,
        mode: selectedSubjectCollection.facet,
      }];
    }
    const facet = facets.find(({ selected }) => selected);
    return facet?.kind === "collection"
      ? [{ command, depth: index + 1, mode: facet.name }]
      : [];
  });
}
