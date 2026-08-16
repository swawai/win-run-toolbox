const RUNTIME_STATUS_HANDLER = "runtime.status";

export function subjectFacets(subject) {
  return subject?.facets ?? [];
}

export function subjectFacetItems(subject) {
  return subjectFacets(subject).map((facet) => ({
    ...facet,
    name: facet.id,
  }));
}

export function defaultSubjectFacet(subject) {
  const facets = subjectFacetItems(subject);
  if (subject?.handler === RUNTIME_STATUS_HANDLER) {
    return facets.find((facet) => facet.renderer === "overview")?.name ?? null;
  }
  return facets[0]?.name ?? null;
}

export function defaultCommandFacet(command) {
  const facets = subjectFacetItems(command);
  if (command?.handler === RUNTIME_STATUS_HANDLER) {
    return null;
  }
  return facets.find((facet) => facet.renderer === "edit")?.name
    ?? facets.find((facet) => facet.kind === "collection" && facet.name !== "runs")?.name
    ?? null;
}

export function createSubjectFacetView(elements = null, options = {}) {
  const panes = elements
    ? new Map([
      ["edit", elements.entryProfileDetail],
      ["overview", elements.commandDetail],
      ["help", elements.commandHelpPane],
      ["run", elements.commandRunPane],
    ])
    : new Map();
  const workspaceRenderers = new Set(["overview", "help", "run"]);
  let available = [];
  let selected = null;
  let selectedRef = null;
  let hasSubject = false;
  const chooseDefault = options.defaultFacet ?? defaultSubjectFacet;
  const fallbackRenderer = options.fallbackRenderer ?? null;

  function selectedFacet() {
    return available.find((facet) => facet.name === selected) ?? null;
  }

  function render() {
    if (!elements) {
      return;
    }
    const renderer = selectedFacet()?.renderer ?? (hasSubject ? fallbackRenderer : null);
    elements.commandWorkspace.hidden = !workspaceRenderers.has(renderer);
    for (const [candidate, pane] of panes) {
      pane.hidden = candidate !== renderer;
    }
  }

  function select(subject, { facet = null } = {}) {
    available = subjectFacetItems(subject);
    const defaultFacet = chooseDefault(subject);
    const selectable = new Set(available.map((candidate) => candidate.name));
    selected = selectable.has(facet) ? facet : defaultFacet;
    selectedRef = subject?.canonicalRef ?? subject?.address ?? null;
    hasSubject = subject !== null && subject !== undefined;
    render();
    return {
      defaultFacet,
      facet: selectedFacet(),
      selectedFacet: selected,
    };
  }

  function items(subject) {
    const reference = subject?.canonicalRef ?? subject?.address ?? null;
    return subjectFacetItems(subject).map((facet) => ({
      ...facet,
      selected: reference === selectedRef && facet.name === selected,
    }));
  }

  render();
  return { items, select };
}
