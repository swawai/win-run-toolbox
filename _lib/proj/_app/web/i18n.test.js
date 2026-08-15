import { expect, test } from "bun:test";

import { language, setLanguage, t } from "./i18n.js";

test("Entry language is one exact shared interface choice", () => {
  setLanguage("en");
  expect(language()).toBe("en");
  expect(t("中文", "English")).toBe("English");

  setLanguage("zh-CN");
  expect(t("中文", "English")).toBe("中文");
  expect(() => setLanguage("auto")).toThrow("Unsupported interface language");
});
