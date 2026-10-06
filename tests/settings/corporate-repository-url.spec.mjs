import { test, expect } from "./fixtures.mjs";
import { providerFixture } from "./repository-provider-fixture.mjs";
import { section } from "./navigation.mjs";

const account = {
  provider: "github",
  account_id: "120949562",
  login: "fixture_corp",
  state: "connected",
};
const neighbor = { ...account, account_id: "44", login: "neighbor" };
const url = "https://github.com/gaming-microsoft/xgang-harness/";
const endpoint = "/repos/gaming-microsoft/xgang-harness";
const pulls = `${endpoint}/pulls?state=open&per_page=1`;
const response = (status, body, headers = { "x-oauth-scopes": "repo" }) => ({
  status,
  body,
  headers,
});
const metadata = {
  id: 100,
  full_name: "gaming-microsoft/xgang-harness",
  private: true,
  archived: false,
  disabled: false,
  description: null,
  permissions: null,
  owner: { login: "gaming-microsoft", type: "Organization" },
};

async function install(page, store) {
  const state = {
    responses: {
      [account.account_id]: {
        "/user": response(200, { id: 120949562, login: account.login }),
        [endpoint]: response(200, metadata),
        [pulls]: response(200, []),
      },
      [neighbor.account_id]: {},
    },
  };
  const fixture = await providerFixture(page, store, (command, args) => {
    if (command === "resolve_provider_repository")
      return store("fixture_repository_browser", {
        ...args,
        operation: "resolve",
        responses: state.responses,
        sessionFailure: state.sessionFailure,
      });
  });
  fixture.accounts = [account, neighbor];
  return { state, fixture };
}

async function open(page, store, genie = false) {
  if (genie) await store("fixture_show_panel");
  await page.goto(genie ? "/" : "/?view=settings");
  if (genie) {
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    await expect(page.locator(".settings-overview")).toBeVisible();
    await page
      .locator(".settings-window")
      .getByRole("button", { name: "Set up with Genie", exact: true })
      .click();
    await page
      .locator('[data-panel-view="genie"] [data-genie-edit="repositories"]')
      .click();
  } else {
    await section(page, "Repositories");
  }
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await dialog.getByLabel("Repository URL", { exact: true }).fill(url);
  await dialog
    .getByLabel("Acting GitHub account", { exact: true })
    .selectOption(account.account_id);
  return dialog;
}

const submit = (dialog) =>
  dialog.getByRole("button", { name: "Add & configure", exact: true }).click();
const editor = (page) =>
  page.getByRole("dialog", {
    name: "Settings for gaming-microsoft/xgang-harness",
    exact: true,
  });

for (const genie of [false, true]) {
  test(`private organization URL opens disabled configuration with exact account (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture } = await install(page, store);
    const dialog = await open(page, store, genie);
    await submit(dialog);
    await expect(editor(page)).toBeVisible();
    const saved = (await store("snapshot")).settings;
    expect(saved.repositories).toHaveLength(1);
    expect(saved.repositories[0]).toMatchObject({
      name: "gaming-microsoft/xgang-harness",
      provider_account_id: account.account_id,
      provider_repository_id: "100",
      enabled: false,
    });
    expect(saved.repositories[0].assignments).toBeUndefined();
    expect(saved.repository_authorizations).toBeUndefined();
    expect(
      fixture.calls
        .filter((call) => call.command === "resolve_provider_repository")
        .map((call) => call.args),
    ).toEqual([
      { provider: "github", accountId: account.account_id, repository: url },
    ]);
  });
}

for (const [name, failure, message] of [
  [
    "hidden or absent",
    response(404, { message: "synthetic-secret-do-not-publish" }),
    "A 404 does not prove the repository is absent",
  ],
  [
    "read denial",
    response(403, { message: "synthetic-secret-do-not-publish" }),
    "GitHub denied read access for this account",
  ],
  [
    "scope",
    response(403, {}, { "x-oauth-scopes": "read:user" }),
    "Reconnect this account in Accounts",
  ],
  [
    "organization SSO",
    response(
      403,
      {},
      { "x-github-sso": "required; url=https://github.com/sso" },
    ),
    "Reconnecting alone cannot bypass organization policy",
  ],
  ["expired", response(401, {}), "GitHub is disconnected"],
  ["rate limit", response(429, {}, { "retry-after": "60" }), "Retry after 60"],
  ["provider", response(503, {}), "Check provider health"],
  ["schema", response(200, { id: null }), "malformed or unsupported data"],
  ["network", { error: "network" }, "Check your network"],
  ["timeout", { error: "timeout" }, "GitHub lookup timed out"],
]) {
  test(`URL ${name} failure preserves input/account, blocks persistence and retries through native provider`, async ({
    page,
    store,
  }) => {
    const { state, fixture } = await install(page, store);
    state.responses[account.account_id][endpoint] = failure;
    const dialog = await open(page, store);
    await submit(dialog);
    await expect(dialog.getByRole("alert")).toContainText(message);
    await expect(dialog.getByRole("alert")).not.toContainText(
      "synthetic-secret",
    );
    await expect(dialog.getByLabel("Repository URL")).toHaveValue(url);
    await expect(dialog.getByLabel("Acting GitHub account")).toHaveValue(
      account.account_id,
    );
    await expect(
      dialog.getByRole("button", { name: "Add & configure" }),
    ).toBeEnabled();
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
    state.responses[account.account_id][endpoint] = response(200, metadata);
    await submit(dialog);
    await expect(editor(page)).toBeVisible();
    expect(
      fixture.calls
        .filter((call) => call.command === "resolve_provider_repository")
        .every((call) => call.args.accountId === account.account_id),
    ).toBe(true);
    expect((await store("snapshot")).settings.repositories[0]).toMatchObject({
      enabled: false,
      provider_account_id: account.account_id,
    });
  });
}

test("URL mismatched stable account cannot borrow a neighbor's access", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[account.account_id]["/user"] = response(200, {
    id: 44,
    login: account.login,
  });
  const dialog = await open(page, store);
  await submit(dialog);
  await expect(dialog.getByRole("alert")).toContainText(
    "GitHub returned a different account",
  );
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
  state.responses[account.account_id]["/user"] = response(200, {
    id: 120949562,
    login: "renamed_corporate",
  });
  await submit(dialog);
  await expect(editor(page)).toBeVisible();
});

for (const failure of ["configuration", "broken_cli"]) {
  test(`URL unusable selected session (${failure}) preserves input and permits explicit retry`, async ({
    page,
    store,
  }) => {
    const { state } = await install(page, store);
    state.sessionFailure = failure;
    const dialog = await open(page, store);
    await submit(dialog);
    await expect(dialog.getByRole("alert")).toContainText(
      "The selected GitHub session is unavailable",
    );
    await expect(dialog.getByLabel("Repository URL")).toHaveValue(url);
    await expect(dialog.getByLabel("Acting GitHub account")).toHaveValue(
      account.account_id,
    );
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
    state.sessionFailure = undefined;
    await submit(dialog);
    await expect(editor(page)).toBeVisible();
  });
}

test("cancelled URL request cannot persist its late verified repository", async ({
  page,
  store,
}) => {
  const { fixture } = await install(page, store);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__urlReadCompleted = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      const request = original(command, args);
      return command === "resolve_provider_repository"
        ? request.finally(() => {
            window.__urlReadCompleted = true;
          })
        : request;
    };
  });
  const gate = Promise.withResolvers();
  const entered = Promise.withResolvers();
  const original = fixture.handler;
  fixture.handler = async (command, args) => {
    const result = await original(command, args);
    if (command === "resolve_provider_repository") {
      entered.resolve();
      await gate.promise;
    }
    return result;
  };
  const dialog = await open(page, store);
  await submit(dialog);
  await entered.promise;
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  gate.resolve();
  await page.waitForFunction(() => window.__urlReadCompleted);
  await expect(dialog).toHaveCount(0);
  await page.evaluate(() => window.__settingsIdle());
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
});
