import assert from "node:assert/strict";
import test from "node:test";

import {
  EntryProfileConflictError,
  createEntryProfileView,
  putEntryProfileVariable,
} from "./entry-profile.js";

function profileDocument(revision, variables) {
  return {
    protocol: "swawkit.entry-profile-state/v3",
    revision,
    status: "ready",
    requiredComplete: true,
    variables,
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
    profileVariableName: text(),
    selectionStatus: text(),
  };
}

test("variable updates use the command's environment name and loaded revision", async () => {
  let request;
  const document = {
    protocol: "swawkit.entry-profile-state/v3",
    revision: "sha256-next",
    variables: { SWAWKIT_PROJ_DEFAULT_SHELL: "pwsh" },
  };
  const result = await putEntryProfileVariable(
    "SWAWKIT_PROJ_DEFAULT_SHELL",
    "pwsh",
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
    "/api/v2/profile/variables/SWAWKIT_PROJ_DEFAULT_SHELL",
  );
  assert.equal(request.options.headers["If-Match"], '"sha256-loaded"');
  assert.deepEqual(JSON.parse(request.options.body), { value: "pwsh" });
  assert.deepEqual(result, document);
});

test("profile conflicts are distinguishable from validation failures", async () => {
  await assert.rejects(
    putEntryProfileVariable("SWAWKIT_PROJ_DEFAULT_SHELL", "pwsh", "stale", async () => ({
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
  const firstName = "SWAWKIT_PROJ_GIT_ID_NAME";
  const secondName = "SWAWKIT_PROJ_GIT_ID_EMAIL";
  const initial = profileDocument("sha256-loaded", {
    [firstName]: "Old Name",
    [secondName]: "mail@example.test",
  });
  const updated = profileDocument("sha256-next", {
    [firstName]: "New Name",
    [secondName]: "mail@example.test",
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
  const command = (name) => ({
    address: `..entry.env.git.${name}`,
    handler: "entry.profile.set",
    summary: name,
  });

  await view.loadProfile();
  view.render(command(firstName));
  elements.profileValue.value = "New Name";
  const saving = view.saveProfile();
  view.render(command(secondName));
  assert.equal(elements.profileSaveButton.disabled, true);
  resolveUpdate();
  await saving;

  assert.equal(elements.entryProfileTitle.textContent, secondName);
  assert.equal(elements.profileValue.value, "mail@example.test");
  assert.equal(elements.profileSaveButton.disabled, false);
  assert.equal(elements.profileFeedback.textContent, "");
  assert.deepEqual(changed, [[updated]]);
});
