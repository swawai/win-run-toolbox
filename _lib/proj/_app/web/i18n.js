const SUPPORTED_LANGUAGES = new Set(["zh-CN", "en"]);

let currentLanguage = "zh-CN";

export function language() {
  return currentLanguage;
}

export function setLanguage(value) {
  if (!SUPPORTED_LANGUAGES.has(value)) {
    throw new Error(`Unsupported interface language: ${value}`);
  }
  currentLanguage = value;
  if (typeof document !== "undefined") {
    document.documentElement.lang = value;
    translateDocument(document);
  }
}

export function t(zhCn, en) {
  return currentLanguage === "en" ? en : zhCn;
}

export function translateDocument(root) {
  for (const element of root.querySelectorAll("[data-i18n-zh][data-i18n-en]")) {
    element.textContent = t(element.dataset.i18nZh, element.dataset.i18nEn);
  }
  for (const element of root.querySelectorAll(
    "[data-i18n-aria-label-zh][data-i18n-aria-label-en]",
  )) {
    element.setAttribute(
      "aria-label",
      t(element.dataset.i18nAriaLabelZh, element.dataset.i18nAriaLabelEn),
    );
  }
}
