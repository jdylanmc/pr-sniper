import { expect, test } from "./fixtures.mjs";
import { closeDialog, repositorySettings, saveChanges } from "./navigation.mjs";

const repositoryName = "fixture/activation";
const repositoryAccount = {
  provider: "github",
  state: "connected",
  account_id: "101",
  login: "fixture-owner",
};

async function seedBoundRepository(store) {
  await store("save_repository", { repository: repositoryName });
  const settings = (await store("snapshot")).settings;
  Object.assign(settings.repositories[0], {
    provider_account_id: repositoryAccount.account_id,
    provider_repository_id: "900",
  });
  await store("seed_settings", settings);
}

function candidates(count) {
  return Array.from({ length: count }, (_, index) => {
    const number = index + 1;
    return {
      pull_request_id: String(number),
      number,
      title: `Pull request ${number}`,
      head_sha: String(number).padStart(40, "a"),
      author_id: String(10_000 + number),
      author_login: `author-${number}`,
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
      trust_confirmation_required: false,
    };
  });
}

async function activationFixture(page, handler) {
  await page.exposeFunction("__activationFixture", handler);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "github_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "github",
              state: "connected",
              account_id: "101",
              login: "fixture-owner",
            },
          ],
          flow: { state: "idle" },
        });
      if (
        [
          "monitoring_activation_status",
          "preview_monitoring_activation",
          "apply_monitoring_activation",
          "cancel_monitoring_activation",
          "resolve_provider_person",
        ].includes(command)
      )
        return window.__activationFixture(command, args ?? {});
      return original(command, args);
    };
  });
}

for (const embedded of [true, false]) {
  test(`${embedded ? "panel" : "legacy"} async scope dialog returns to its invoker rather than later focus`, async ({
    page,
    store,
  }) => {
    await seedBoundRepository(store);
    const preview = Promise.withResolvers();
    await activationFixture(page, (command, args) => {
      if (command === "monitoring_activation_status")
        return { active: false, selected_existing: 0 };
      if (command === "preview_monitoring_activation")
        return preview.promise.then(() => ({
          preview_id: "held-preview",
          repository_id: args.repositoryId,
          name: repositoryName,
          account_id: "101",
          account_login: "fixture-owner",
          candidates: [],
        }));
      return null;
    });
    await page.goto(embedded ? "/" : "/?view=settings");
    if (embedded)
      await page
        .getByRole("navigation", { name: "Application destinations" })
        .getByRole("button", { name: "Settings", exact: true })
        .click();
    const repository = await repositorySettings(page, repositoryName);
    const opener = repository.getByRole("button", { name: "Configure scope" });
    try {
      await opener.click();
      await expect(opener).toBeDisabled();
      await repository.getByLabel("Review start", { exact: true }).focus();
      preview.resolve();
      const scope = page.getByRole("dialog", {
        name: `Monitoring scope for ${repositoryName}`,
        exact: true,
      });
      await scope.getByRole("button", { name: "Cancel", exact: true }).click();
      await expect(opener).toBeFocused();
      await expect(repository).toBeVisible();
    } finally {
      preview.resolve();
    }
  });
}

test("activation saves filter drafts then scopes 1,800 matching pull requests explicitly", async ({
  page,
  store,
}) => {
  await seedBoundRepository(store);
  const previewCandidates = candidates(1_800);
  let previewCalls = 0;
  let applied;
  let status = {
    repository_id: "configuration",
    active: false,
    reason: "scope_confirmation_required",
    mode: null,
    selected_existing: 0,
    creation_watermark: null,
  };
  await activationFixture(page, (command, args) => {
    if (command === "resolve_provider_person")
      return { id: "42", login: "octocat" };
    if (command === "monitoring_activation_status") return status;
    if (command === "preview_monitoring_activation") {
      previewCalls++;
      return {
        preview_id: `preview-${previewCalls}`,
        repository_id: args.repositoryId,
        name: repositoryName,
        account_id: "101",
        account_login: "fixture-owner",
        creation_watermark: 1_800,
        candidates: previewCandidates,
      };
    }
    if (command === "apply_monitoring_activation") {
      applied = args.request;
      status = {
        repository_id: args.request.repositoryId,
        active: true,
        reason: null,
        mode: args.request.mode,
        selected_existing: args.request.selectedPullRequestIds.length,
        creation_watermark: 1_800,
      };
      return status;
    }
    return null;
  });
  await page.goto("/?view=settings");

  let repository = await repositorySettings(page, repositoryName);
  await expect(repository.locator("[data-scope-status]")).toContainText(
    "Scope confirmation required",
  );
  await repository.getByRole("button", { name: "Add people" }).click();
  const picker = page.getByRole("dialog", { name: "Add people", exact: true });
  await picker.getByLabel("GitHub login", { exact: true }).fill("octocat");
  await picker.getByRole("button", { name: "Add person" }).click();
  await repository.getByRole("button", { name: "Configure scope" }).click();
  await expect(repository.locator("[data-scope-status]")).toContainText(
    "Save repository before previewing monitoring scope",
  );
  expect(previewCalls).toBe(0);
  await closeDialog(page);
  await saveChanges(page);
  expect(
    (await store("snapshot")).settings.repositories[0].watched_authors,
  ).toEqual([{ id: "42", login: "octocat" }]);

  repository = await repositorySettings(page, repositoryName);
  await repository.getByRole("button", { name: "Configure scope" }).click();
  const scope = page.getByRole("dialog", {
    name: `Monitoring scope for ${repositoryName}`,
    exact: true,
  });
  await expect(scope.getByText("1800", { exact: true })).toBeVisible();
  await expect(scope.locator(".activation-row")).toHaveCount(1_800);
  await expect(scope.locator(".activation-row input:checked")).toHaveCount(0);
  await expect(scope.locator(".activation-row input:enabled")).toHaveCount(0);

  await scope
    .getByLabel("Selected existing pull requests plus new pull requests", {
      exact: true,
    })
    .check();
  await scope
    .getByLabel("Find matching pull request", { exact: true })
    .fill("Pull request 1800");
  await expect(scope.locator(".activation-row")).toHaveCount(1);
  await scope.getByLabel("Include pull request 1800").check();
  await expect(scope.getByText("1 selected", { exact: true })).toBeVisible();
  await scope
    .getByRole("button", { name: "Confirm monitoring scope", exact: true })
    .click();

  expect(applied).toEqual({
    repositoryId: (await store("snapshot")).settings.repositories[0].id,
    previewId: "preview-1",
    mode: "selected_existing",
    selectedPullRequestIds: ["1800"],
  });
  await expect(repository.locator("[data-scope-status]")).toContainText(
    "Active for new pull requests and 1 selected existing pull request",
  );
});

test("activation cancel and failed apply leave scope unchanged and retryable", async ({
  page,
  store,
}) => {
  await seedBoundRepository(store);
  let preview = 0;
  let cancelled = 0;
  let failCancel = false;
  let failApply = true;
  const required = {
    repository_id: "configuration",
    active: false,
    reason: "scope_confirmation_required",
    mode: null,
    selected_existing: 0,
    creation_watermark: null,
  };
  await activationFixture(page, (command, args) => {
    if (command === "monitoring_activation_status") return required;
    if (command === "preview_monitoring_activation")
      return {
        preview_id: `preview-${++preview}`,
        repository_id: args.repositoryId,
        name: repositoryName,
        account_id: "101",
        account_login: "fixture-owner",
        creation_watermark: 1,
        candidates: candidates(1),
      };
    if (command === "cancel_monitoring_activation") {
      if (failCancel) throw "cleanup failed";
      cancelled++;
      return null;
    }
    if (command === "apply_monitoring_activation" && failApply)
      throw "Cannot write monitoring state.";
    return {
      ...required,
      active: true,
      reason: null,
      mode: args.request.mode,
      creation_watermark: 1,
    };
  });
  await page.goto("/?view=settings");
  const repository = await repositorySettings(page, repositoryName);
  await repository.getByRole("button", { name: "Configure scope" }).click();
  let scope = page.getByRole("dialog", {
    name: `Monitoring scope for ${repositoryName}`,
    exact: true,
  });
  await scope.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(
    repository.getByRole("button", { name: "Configure scope" }),
  ).toBeFocused();
  expect(cancelled).toBe(1);
  await expect(repository.locator("[data-scope-status]")).toContainText(
    "Scope confirmation required",
  );

  await repository.getByRole("button", { name: "Configure scope" }).click();
  scope = page.getByRole("dialog", {
    name: `Monitoring scope for ${repositoryName}`,
    exact: true,
  });
  await scope.press("Escape");
  await expect(scope).not.toBeVisible();
  await expect.poll(() => cancelled).toBe(2);

  await repository.getByRole("button", { name: "Configure scope" }).click();
  scope = page.getByRole("dialog", {
    name: `Monitoring scope for ${repositoryName}`,
    exact: true,
  });
  await page
    .locator(`dialog[aria-label="Settings for ${repositoryName}"]`)
    .evaluate((element) => element.close());
  await expect(scope).not.toBeVisible();
  await expect.poll(() => cancelled).toBe(3);

  let reopened = await repositorySettings(page, repositoryName);
  failCancel = true;
  await reopened.getByRole("button", { name: "Configure scope" }).click();
  scope = page.getByRole("dialog", {
    name: `Monitoring scope for ${repositoryName}`,
    exact: true,
  });
  await scope.press("Escape");
  await expect(page.locator("#error")).toContainText(
    "Monitoring scope preview cleanup failed",
  );
  failCancel = false;

  await reopened.getByRole("button", { name: "Configure scope" }).click();
  scope = page.getByRole("dialog", {
    name: `Monitoring scope for ${repositoryName}`,
    exact: true,
  });
  await scope
    .getByRole("button", { name: "Confirm monitoring scope", exact: true })
    .click();
  await expect(scope.getByRole("alert")).toContainText(
    "Cannot write monitoring state",
  );
  await expect(scope).toBeVisible();
  failApply = false;
  await scope
    .getByRole("button", { name: "Confirm monitoring scope", exact: true })
    .click();
  await expect(scope).not.toBeVisible();
});

test("activation apply locks dismissal until the authoritative result returns", async ({
  page,
  store,
}) => {
  await seedBoundRepository(store);
  const apply = Promise.withResolvers();
  let status = {
    repository_id: "configuration",
    active: false,
    reason: "scope_confirmation_required",
    mode: null,
    selected_existing: 0,
    creation_watermark: null,
  };
  await activationFixture(page, (command, args) => {
    if (command === "monitoring_activation_status") return status;
    if (command === "preview_monitoring_activation")
      return {
        preview_id: "preview-held",
        repository_id: args.repositoryId,
        name: repositoryName,
        account_id: "101",
        account_login: "fixture-owner",
        creation_watermark: 0,
        candidates: [],
      };
    if (command === "apply_monitoring_activation") return apply.promise;
    return null;
  });
  await page.goto("/?view=settings");
  const repository = await repositorySettings(page, repositoryName);
  await repository.getByRole("button", { name: "Configure scope" }).click();
  const scope = page.getByRole("dialog", {
    name: `Monitoring scope for ${repositoryName}`,
    exact: true,
  });
  await scope
    .getByRole("button", { name: "Confirm monitoring scope", exact: true })
    .click();
  await expect(
    scope.getByRole("button", { name: "Close dialog", exact: true }),
  ).toBeDisabled();
  await scope.press("Escape");
  await expect(scope).toBeVisible();
  status = {
    repository_id: "configuration",
    active: true,
    reason: null,
    mode: "new_only",
    selected_existing: 0,
    creation_watermark: 0,
  };
  apply.resolve(status);
  await expect(scope).not.toBeVisible();
  await expect(repository.locator("[data-scope-status]")).toContainText(
    "Active for new pull requests only",
  );
});
