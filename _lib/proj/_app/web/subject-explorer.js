import { t } from "./i18n.js";

function createFacetRow(documentObject, subject, facet, onSelect) {
  const item = documentObject.createElement("li");
  const button = documentObject.createElement("button");
  const icon = documentObject.createElement("span");
  const name = documentObject.createElement("span");

  button.type = "button";
  button.className = "finder-choice command-facet-row subject-facet-row";
  button.dataset.facet = facet.name;
  button.dataset.parentAddress = subject.owner;
  button.dataset.subjectRef = subject.canonicalRef;
  button.dataset.navigationKey = `${subject.canonicalRef}#${facet.name}`;
  button.setAttribute("aria-pressed", String(facet.selected));
  button.title = facet.summary;

  icon.className = "row-icon facet-icon";
  icon.textContent = facet.icon;
  icon.setAttribute("aria-hidden", "true");
  name.className = "row-name";
  name.textContent = facet.label;
  button.append(icon, name);
  button.addEventListener("click", () => onSelect(subject, {
    focusDetail: true,
    history: "push",
    facet: facet.name,
  }));
  item.append(button);
  return item;
}

function createFacetMenu(documentObject, subject, facets, onSelect) {
  const group = documentObject.createElement("div");
  const list = documentObject.createElement("ul");
  group.className = "command-facet-group subject-facet-group";
  list.className = "command-facet-menu";
  list.id = `subject-facet-menu-${subject.ref.kind}-${subject.id}`;
  list.setAttribute("aria-label", `${subject.canonicalRef} facets`);
  for (const facet of facets) {
    list.append(createFacetRow(documentObject, subject, facet, onSelect));
  }
  group.append(list);
  return group;
}

function createSubjectRow(
  documentObject,
  subject,
  selectedSubjectRef,
  getSubjectFacets,
  onSelect,
) {
  const item = documentObject.createElement("li");
  const button = documentObject.createElement("button");
  const icon = documentObject.createElement("span");
  const copy = documentObject.createElement("span");
  const name = documentObject.createElement("span");
  const summary = documentObject.createElement("span");
  const selected = subject.canonicalRef === selectedSubjectRef;
  const facets = getSubjectFacets(subject);

  button.type = "button";
  button.className = "finder-choice subject-row";
  button.dataset.parentAddress = subject.owner;
  button.dataset.subjectRef = subject.canonicalRef;
  button.dataset.navigationKey = subject.canonicalRef;
  button.dataset.selected = String(selected);
  button.setAttribute("aria-expanded", String(selected));
  if (selected) {
    button.setAttribute("aria-current", "page");
    button.setAttribute("aria-controls", `subject-facet-menu-${subject.ref.kind}-${subject.id}`);
  }
  button.title = subject.canonicalRef;

  icon.className = "row-icon subject-icon";
  icon.textContent = "◆";
  icon.setAttribute("aria-hidden", "true");
  copy.className = "row-copy";
  name.className = "row-name";
  name.textContent = subject.label;
  summary.className = "row-summary";
  summary.textContent = subject.summary;
  copy.append(name, summary);
  button.append(icon, copy);
  button.addEventListener("click", (event) => onSelect(subject, {
    focusDetail: event.detail === 0,
    history: "push",
  }));
  item.className = "subject-item";
  item.append(button);
  if (selected && facets.length > 0) {
    item.append(createFacetMenu(documentObject, subject, facets, onSelect));
  }
  return item;
}

export function appendSubjectSection({
  collection,
  column,
  documentObject = document,
  error = null,
  getSubjectFacets,
  label = null,
  onSelect,
  selectedSubjectRef,
}) {
  const section = documentObject.createElement("section");
  const heading = documentObject.createElement("h2");
  section.className = "column-section subject-section";
  heading.className = "column-label";
  heading.textContent = collection?.label ?? label ?? t("正在读取…", "Loading…");
  section.append(heading);
  if (error) {
    const feedback = documentObject.createElement("p");
    feedback.className = "empty-column";
    feedback.dataset.state = "error";
    feedback.textContent = error;
    section.append(feedback);
    column.append(section);
    return;
  }
  if (!collection) {
    const loading = documentObject.createElement("p");
    loading.className = "empty-column";
    loading.textContent = t("正在读取…", "Loading…");
    section.append(loading);
    column.append(section);
    return;
  }
  const list = documentObject.createElement("ul");
  list.className = "column-list";
  for (const subject of collection.subjects) {
    list.append(createSubjectRow(
      documentObject,
      subject,
      selectedSubjectRef,
      getSubjectFacets,
      onSelect,
    ));
  }
  section.append(list);
  if (collection.subjects.length === 0) {
    const empty = documentObject.createElement("p");
    empty.className = "empty-column";
    empty.textContent = t("此集合中尚无对象。", "This collection has no subjects yet.");
    section.append(empty);
  }
  column.append(section);
}
