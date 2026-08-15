import assert from "node:assert/strict";
import test from "node:test";

import {
  EntryProfileConflictError,
  createEntryProfileView,
  putEntryProfileSetting,
} from "./entry-profile.js";

function profileDocument(revision, settings) {
  return {
    protocol: "swawkit.entry-profile-state/v5",
    revision,
    status: "ready",
    requiredComplete: true,
    settings,
    profile: { language: settings["..entry.language"] },
  };
}

function profileElements() {
  const text = () => ({ textContent: "" });
  return {
    entryProfileSummary: text(),
    entryProfileTitle: text(),
    profileFeedback: { dataset: {}, textContent: "" },
    profileSaveButton: { disabled: false },
    profileState: { dataset: {}, textContent: "" },
    profileValue: { disabled: false, value: "" },
    profileSettingAddress: text(),
    selectionStatus: text(),
  };
}

test("setting updates use the typed command address and loaded revision", async () => {
  let request;
  const document = {
    protocol: "swawkit.entry-profile-state/v5",
    revision: "sha256-next",
    settings: { "..entry.language": "en" },
    profile: { language: "en" },
  };
  const result = await putEntryProfileSetting(
    "..entry.language",
    "en",
    "sha256-loaded",
    async (url, options) => {
      request = { url, options };
      return {
        ok: true,
        status: 200,
        async json() {
          return document;
        },
      };
    },
  );

  assert.equal(
    request.url,
    "/api/v2/profile/settings/..entry.language",
  );
  assert.equal(request.options.headers["If-Match"], '"sha256-loaded"');
  assert.deepEqual(JSON.parse(request.options.body), { value: "en" });
  assert.deepEqual(result, document);
});

test("profile conflicts are distinguishable from validation failures", async () => {
  await assert.rejects(
    putEntryProfileSetting("..entry.language", "en", "stale", async () => ({
      ok: false,
      status: 409,
      async json() {
        return { error: "changed" };
      },
    })),
    EntryProfileConflictError,
  );
});

test("a completed save does not overwrite a newer Profile command selection", async () => {
  const firstAddress = "..entry.git.name";
  const secondAddress = "..entry.git.email";
  const initial = profileDocument("sha256-loaded", {
    [firstAddress]: "Old Name",
    [secondAddress]: "mail@example.test",
    "..entry.language": "zh-CN",
  });
  const updated = profileDocument("sha256-next", {
    [firstAddress]: "New Name",
    [secondAddress]: "mail@example.test",
    "..entry.language": "zh-CN",
  });
  let resolveUpdate;
  const changed = [];
  const elements = profileElements();
  const view = createEntryProfileView(elements, {
    async fetchImpl(url) {
      if (url === "/api/v2/profile") {
        return { ok: true, async json() { return initial; } };
      }
      return new Promise((resolve) => {
        resolveUpdate = () => resolve({
          ok: true,
          status: 200,
          async json() { return updated; },
        });
      });
    },
    async onProfileChanged(...arguments_) {
      changed.push(arguments_);
    },
  });
  const command = (address) => ({
    address,
    handler: "entry.profile.set",
    summary: address,
  });

  await view.loadProfile();
  view.render(command(firstAddress));
  elements.profileValue.value = "New Name";
  const saving = view.saveProfile();
  view.render(command(secondAddress));
  assert.equal(elements.profileSaveButton.disabled, true);
  resolveUpdate();
  await saving;

  assert.equal(elements.entryProfileTitle.textContent, "email");
  assert.equal(elements.profileValue.value, "mail@example.test");
  assert.equal(elements.profileSaveButton.disabled, false);
  assert.equal(elements.profileFeedback.textContent, "");
  assert.deepEqual(changed, [[updated]]);
});
