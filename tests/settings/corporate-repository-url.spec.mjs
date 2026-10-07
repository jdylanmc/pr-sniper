import { test, expect } from "./fixtures.mjs";
import { providerFixture } from "./repository-provider-fixture.mjs";
import { section } from "./navigation.mjs";

const account = {
  provider: "github",
  account_id: "900000002",
  login: "fixture_corp",
  state: "connected",
};
const neighbor = { ...account, account_id: "44", login: "neighbor" };
const url = "https://github.com/example-org/example-repo/";
const endpoint = "/repos/example-org/example-repo";
const pulls = `${endpoint}/pulls?state=open&per_page=1`;
const response = (status, body, headers = { "x-oauth-scopes": "repo" }) => ({
  status,
  body,
  headers,
});
const metadata = {
  id: 100,
  full_name: "example-org/example-repo",
  private: true,
  archived: false,
  disabled: false,
  description: null,
  permissions: null,
  owner: { login: "example-org", type: "Organization" },
};
const policyMessage =
  "Although you appear to have the correct authorization credentials, the fixture-org organization has enabled OAuth App access restrictions, meaning that data access to third-parties is limited. For more information visit https://example.invalid/private";

async function install(page, store) {
  const state = {
    responses: {
      [account.account_id]: {
        "/user": response(200, { id: 900000002, login: account.login }),
        [endpoint]: response(200, metadata),
        [pulls]: response(200, []),
        "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1":
          response(200, [metadata]),
      },
      [neighbor.account_id]: {},
    },
  };
  const fixture = await providerFixture(page, store, (command, args) => {
    if (
      [
        "resolve_provider_repository",
        "list_provider_repository_owners",
        "list_provider_repositories",
      ].includes(command)
    )
      return store("fixture_repository_browser", {
        ...args,
        operation:
          command === "resolve_provider_repository"
            ? "resolve"
            : command === "list_provider_repository_owners"
              ? "owners"
              : "repositories",
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
    await expect(page.locator("[data-panel-heading]")).toHaveText("Welcome");
    await expect(page.locator('[data-panel-view="genie"]')).toBeVisible();
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
    name: "Settings for example-org/example-repo",
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
      name: "example-org/example-repo",
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

for (const genie of [false, true]) {
  for (const path of [endpoint, pulls]) {
    for (const [name, failure, expected, forbidden] of [
      [
        "provider rejection",
        response(422, { message: "synthetic-secret" }),
        "HTTP 422",
        "malformed or unsupported",
      ],
      [
        "policy only",
        response(403, { message: policyMessage }),
        "organization authorization restriction",
        "Reconnect this account",
      ],
      [
        "policy and scope",
        response(
          403,
          { message: policyMessage },
          { "x-oauth-scopes": "read:user" },
        ),
        "authorization missing repo scope",
        "GitHub denied read access",
      ],
      [
        "partial authorization and rate limit",
        response(
          403,
          { message: "API rate limit exceeded" },
          {
            "x-github-sso": "partial-results; organizations=123",
            "x-ratelimit-remaining": "0",
            "x-ratelimit-reset": "1800000060",
            "retry-after": "60",
          },
        ),
        "Retry after 60 seconds",
        "Reconnect this account",
      ],
    ]) {
      test(`URL ${name} at ${path === endpoint ? "metadata" : "pulls"} (${genie ? "Genie" : "Settings"}) retains safe evidence and retries`, async ({
        page,
        store,
      }) => {
        const { state, fixture } = await install(page, store);
        state.responses[account.account_id][path] = failure;
        const dialog = await open(page, store, genie);
        await submit(dialog);
        const alert = dialog.getByRole("alert");
        await expect(alert).toContainText(expected);
        await expect(alert).not.toContainText(forbidden);
        await expect(alert).not.toContainText("synthetic-secret");
        await expect(alert).not.toContainText("https://example.invalid");
        if (name.startsWith("policy")) {
          await expect(alert).toContainText("organization");
          await expect(alert).toContainText(
            "Reconnecting alone cannot bypass organization policy",
          );
        }
        if (name.startsWith("partial")) {
          await expect(alert).toContainText("1800000060");
          await expect(alert).toContainText("restricted or incomplete");
        }
        await expect(dialog.getByLabel("Repository URL")).toHaveValue(url);
        await expect(dialog.getByLabel("Acting GitHub account")).toHaveValue(
          account.account_id,
        );
        expect((await store("snapshot")).settings.repositories).toBeUndefined();
        state.responses[account.account_id][path] = response(
          200,
          path === endpoint ? metadata : [],
        );
        await submit(dialog);
        await expect(editor(page)).toBeVisible();
        expect(
          fixture.calls
            .filter((c) => c.command === "resolve_provider_repository")
            .every((c) => c.args.accountId === account.account_id),
        ).toBe(true);
      });
    }
  }
  test(`URL unknown scope evidence (${genie ? "Genie" : "Settings"}) is not a scope grant or account revocation`, async ({
    page,
    store,
  }) => {
    const { state } = await install(page, store);
    state.responses[account.account_id][endpoint] = response(200, metadata, {});
    const dialog = await open(page, store, genie);
    await submit(dialog);
    await expect(dialog.getByRole("alert")).toContainText(
      "No missing scope or grant is established",
    );
    await expect(dialog.getByRole("alert")).not.toContainText("Reconnect");
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
    state.responses[account.account_id][endpoint] = response(200, metadata);
    await submit(dialog);
    await expect(editor(page)).toBeVisible();
  });
}

test("unknown catalog scope retains the selected connected account and retries without a complete result", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  const catalog =
    "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=1";
  state.responses[account.account_id][catalog] = response(200, [metadata], {});
  await page.goto("/?view=settings");
  await section(page, "Repositories");
  await page
    .getByRole("button", {
      name: `Browse repositories as ${account.login}`,
      exact: true,
    })
    .click();
  const browser = page.getByRole("dialog", {
    name: `Browse repositories as ${account.login}`,
    exact: true,
  });
  await expect(browser.getByRole("alert")).toContainText(
    "No missing scope or grant is established",
  );
  await expect(browser.getByRole("alert")).not.toContainText("Reconnect");
  await expect(browser.locator("[data-pick]")).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
  state.responses[account.account_id][catalog] = response(200, [metadata]);
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByLabel("Repository owner")).toBeEnabled();
  await browser.getByLabel("Repository owner").selectOption("example-org");
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
});

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
    id: 900000002,
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
