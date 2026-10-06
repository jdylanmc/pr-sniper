import { test, expect } from "./fixtures.mjs";
import { providerFixture } from "./repository-provider-fixture.mjs";
import { section } from "./navigation.mjs";

const catalog =
  "/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&page=";
const corporate = {
  provider: "github",
  account_id: "22",
  login: "fixture_corp",
  state: "connected",
};
const personal = { ...corporate, account_id: "44", login: "personal" };
const response = (body, headers = {}, status = 200) => ({
  status,
  headers: { "x-oauth-scopes": "repo", ...headers },
  body,
});
const repository = (id, owner, type = "Organization") => ({
  id,
  full_name: `${owner}/repository-${id}`,
  private: true,
  owner: { login: owner, type, name: null, extra_metadata: "ignored" },
  description: null,
});
const responses = () => ({
  22: {
    "/user": response({ id: 22, login: corporate.login }),
    "/repos/fixture_corp/repository-101": response({
      ...repository(101, corporate.login, "User"),
      archived: false,
      disabled: false,
      permissions: { pull: true },
    }),
    "/repos/fixture_corp/repository-101/pulls?state=open&per_page=1": response(
      [],
    ),
    [`${catalog}1`]: response([repository(101, corporate.login, "User")], {
      link: `<https://api.github.com${catalog}2>; rel="next", <https://api.github.com${catalog}2>; rel="last"`,
    }),
    [`${catalog}2`]: response([
      repository(102, "orbit"),
      repository(103, "orbit"),
    ]),
  },
  44: {
    "/user": response({ id: 44, login: personal.login }),
    [`${catalog}1`]: response([repository(201, personal.login, "User")]),
  },
});

async function install(page, store) {
  const state = { responses: responses(), reads: [] };
  const fixture = await providerFixture(page, store, (command, args) => {
    if (
      command === "list_provider_repository_owners" ||
      command === "list_provider_repositories" ||
      command === "resolve_provider_repository"
    ) {
      state.reads.push({ command, ...args });
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
    }
  });
  fixture.accounts = [corporate, personal];
  return { state, fixture };
}

async function open(
  page,
  store,
  account = corporate,
  embedded = false,
  genie = false,
) {
  if (embedded) await store("fixture_show_panel");
  await page.goto(embedded ? "/" : "/?view=settings");
  if (embedded) await openPanelSettings(page);
  return browseFromSettings(page, account, genie);
}

async function openPanelSettings(page, clicked) {
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  clicked?.();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Settings");
  await expect(page.locator(".settings-overview")).toBeVisible();
  await expect(page.locator(".settings-window")).toHaveAttribute(
    "data-settings-section",
    "home",
  );
}

async function browseFromSettings(page, account, genie, entryLookup) {
  if (genie) {
    entryLookup?.();
    await page
      .locator(".settings-window")
      .getByRole("button", { name: "Set up with Genie", exact: true })
      .click();
    const guide = page.locator('[data-panel-view="genie"]');
    await expect(page.locator("[data-panel-heading]")).toHaveText("Genie");
    await expect(guide).toBeVisible();
    await expect(
      guide.locator('[data-genie-edit="repositories"]'),
    ).toBeEnabled();
    await guide.locator('[data-genie-edit="repositories"]').click();
    await expect(page.locator(".settings-window")).toHaveAttribute(
      "data-settings-section",
      "repositories",
    );
  } else {
    await section(page, "Repositories");
  }
  await page
    .getByRole("button", {
      name: `Browse repositories as ${account.login}`,
      exact: true,
    })
    .click();
  return page.getByRole("dialog", {
    name: `Browse repositories as ${account.login}`,
    exact: true,
  });
}

for (const [embedded, genie] of [
  [false, false],
  [true, false],
  [true, true],
]) {
  test(`managed account browses personal/org paged results through native provider (${genie ? "Genie" : embedded ? "panel" : "standalone"})`, async ({
    page,
    store,
  }, info) => {
    const { state } = await install(page, store);
    const browser = await open(page, store, corporate, embedded, genie);
    await expect(browser.getByLabel("Repository owner")).toBeEnabled();
    await expect(
      browser.getByLabel("Repository owner").locator("option"),
    ).toHaveText([
      "Choose an owner",
      "fixture_corp (personal)",
      "orbit (organization)",
    ]);
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    await expect(browser.getByRole("alert")).toBeHidden();
    await browser.getByLabel("Repository owner").selectOption("orbit");
    await expect(browser.locator("[data-pick]")).toHaveCount(2);
    await expect(browser.getByRole("status")).toHaveText(
      "2 accessible repositories loaded.",
    );
    await browser.getByLabel("Find a repository").fill("103");
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    expect(state.reads.every((read) => read.accountId === "22")).toBe(true);
    await page.screenshot({
      path: info.outputPath("managed-account-browser.png"),
    });
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
  });
}

test("Genie repository browsing waits for the actual delayed Settings bootstrap", async ({
  page,
  store,
  ipc,
}) => {
  const { state } = await install(page, store);
  await store("fixture_show_panel");
  await page.goto("/");
  // The panel footer has its own snapshot; hold the subsequent Settings load.
  await expect(page.locator("[data-panel-version]")).toHaveText(/^v/);
  const held = ipc.holdNext("snapshot");
  const clicked = Promise.withResolvers();
  const order = [];
  let released = false;
  const opening = openPanelSettings(page, () => {
    order.push("Settings click completed");
    clicked.resolve();
  }).then(() =>
    browseFromSettings(page, corporate, true, () => {
      order.push("Genie entry lookup");
      expect(
        released,
        "Genie entry lookup preceded native Settings readiness",
      ).toBe(true);
    }),
  );
  const settled = opening.then(
    (browser) => ({ browser }),
    (cause) => ({ cause }),
  );
  try {
    await held.arrived;
    await clicked.promise;
    await expect(page.locator(".settings-window")).toBeVisible();
    await expect(page.locator("#save-status")).toHaveText(
      "Loading settings...",
    );
    await expect(page.locator('[data-panel-view="genie"]')).toBeHidden();
    expect(state.reads).toEqual([]);
    expect(order).toEqual(["Settings click completed"]);
    released = true;
    order.push("Native Settings released");
    held.release();
    const result = await settled;
    if ("cause" in result) throw result.cause;
    const { browser } = result;
    expect(order).toEqual([
      "Settings click completed",
      "Native Settings released",
      "Genie entry lookup",
    ]);
    await expect(browser.getByLabel("Repository owner")).toBeEnabled();
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    expect(state.reads.every((read) => read.accountId === "22")).toBe(true);
  } finally {
    held.release();
    await settled;
  }
});

test("partial organization authorization is visible without disabling accessible results; Retry is fresh", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[22][`${catalog}2`].headers["x-github-sso"] =
    "partial-results; organizations=123,456";
  const browser = await open(page, store);
  await expect(browser.getByRole("alert")).toContainText(
    "Organization access (page 2)",
  );
  await expect(browser.getByRole("alert")).toContainText("single sign-on");
  await browser.getByLabel("Repository owner").selectOption("orbit");
  await expect(browser.locator("[data-pick]")).toHaveCount(2);
  await expect(browser.getByRole("status")).toContainText(
    "discovery is incomplete",
  );
  await expect(browser.getByRole("alert")).not.toContainText("123,456");
  const count = state.reads.length;
  delete state.responses[22][`${catalog}2`].headers["x-github-sso"];
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByRole("alert")).toBeHidden();
  await expect(browser.getByRole("status")).toHaveText(
    "2 accessible repositories loaded.",
  );
  expect(state.reads).toHaveLength(count + 1);
});

test("malformed records and denied later pages retain good owners, not a complete empty org backlog", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[22][`${catalog}1`].body.push({
    id: 999,
    full_name: "unavailable/bad",
    private: null,
    owner: null,
  });
  state.responses[22][`${catalog}2`] = response(
    { message: "SAML authorization required" },
    { "x-github-sso": "required; url=https://github.com/orgs/private/sso" },
    403,
  );
  const browser = await open(page, store);
  await expect(browser.getByLabel("Repository owner")).toBeEnabled();
  await expect(browser.getByRole("alert")).toContainText(
    "Repository metadata (page 1)",
  );
  await expect(browser.getByRole("alert")).toContainText(
    "Repository page (page 2)",
  );
  await browser.getByLabel("Repository owner").selectOption(corporate.login);
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  await expect(browser.getByRole("status")).toContainText(
    "discovery is incomplete",
  );
  await expect(browser.getByRole("status")).not.toContainText("No accessible");
  state.responses = responses();
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByRole("alert")).toBeHidden();
});

for (const [name, failedResponse, diagnostic] of [
  [
    "schema",
    { ...response([]), rawBody: "not-json" },
    "malformed or unsupported data",
  ],
  ["transport", { error: "network" }, "Cannot reach GitHub"],
  ["provider", response({}, {}, 503), "GitHub lookup failed"],
  ["provider-rejection", response({}, {}, 422), "GitHub rejected this lookup"],
  [
    "rate-limit",
    response({}, { "retry-after": "12" }, 429),
    "Retry after 12 seconds",
  ],
  [
    "provider-delay",
    response({}, { "retry-after": "15" }, 503),
    "Retry after 15 seconds",
  ],
  [
    "permission",
    response({}, {}, 403),
    "organization authorization with your administrator",
  ],
  [
    "scope",
    response([], { "x-oauth-scopes": "read:user" }),
    "Reconnect the GitHub account",
  ],
  [
    "SSO",
    response({}, { "x-github-sso": "required" }, 403),
    "Authorize this app",
  ],
]) {
  test(`${name} failure remains distinct and failed Retry does not pretend success`, async ({
    page,
    store,
  }) => {
    const { state } = await install(page, store);
    state.responses[22][`${catalog}1`] = failedResponse;
    const browser = await open(page, store);
    await expect(browser.getByRole("alert")).toContainText(diagnostic);
    await expect(browser.getByLabel("Find a repository")).toBeDisabled();
    const count = state.reads.length;
    await browser.getByRole("button", { name: "Retry", exact: true }).click();
    await expect.poll(() => state.reads.length).toBe(count + 1);
    await expect(browser.getByRole("alert")).toContainText(diagnostic);
    await expect(browser.getByRole("status")).toHaveText(
      "Repository browsing unavailable.",
    );
    state.responses = responses();
    await browser.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(browser.getByRole("alert")).toBeHidden();
    await expect(browser.getByLabel("Repository owner")).toBeEnabled();
  });
}

test("failed refresh retains previous selected-account results visibly; later Retry replaces them", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[22][`${catalog}2`].headers["x-github-sso"] =
    "partial-results; organizations=123";
  const browser = await open(page, store);
  await browser.getByLabel("Repository owner").selectOption("orbit");
  await expect(browser.locator("[data-pick]")).toHaveCount(2);
  state.responses[22][`${catalog}1`] = { error: "network" };
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByRole("status")).toContainText("Lookup failed");
  await expect(browser.locator("[data-pick]")).toHaveCount(2);
  await browser.getByLabel("Find a repository").fill("missing");
  await expect(browser.getByRole("status")).toContainText(
    "discovery is incomplete",
  );
  state.responses = responses();
  state.responses[22][`${catalog}2`].body = [repository(104, "orbit")];
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByRole("alert")).toBeHidden();
  await browser.getByLabel("Find a repository").fill("");
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  await expect(browser.locator("[data-pick]")).toContainText("repository-104");
});

test("selected identity mismatch never returns another account's repositories", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[22]["/user"] = response({ id: 44, login: personal.login });
  const browser = await open(page, store);
  await expect(browser.getByRole("alert")).toContainText("different account");
  await expect(browser.locator("[data-pick]")).toHaveCount(0);
  await expect(browser.getByLabel("Repository owner")).toBeDisabled();
  expect(state.reads.every((read) => read.accountId === "22")).toBe(true);
  await browser
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await page
    .getByRole("button", {
      name: "Browse repositories as personal",
      exact: true,
    })
    .click();
  const other = page.getByRole("dialog", {
    name: "Browse repositories as personal",
    exact: true,
  });

  await other.getByLabel("Repository owner").selectOption("personal");
  await expect(other.locator("[data-pick]")).toContainText(
    "personal/repository-201",
  );
  await expect(other.locator("[data-owner-results]")).not.toContainText(
    "fixture_corp",
  );
});

test("managed personal repository selection resolves through provider and persists exact account binding", async ({
  page,
  store,
}) => {
  await install(page, store);
  const browser = await open(page, store);
  await browser.getByLabel("Repository owner").selectOption(corporate.login);
  await browser.locator("[data-pick]").click();
  const editor = page.getByRole("dialog", {
    name: "Settings for fixture_corp/repository-101",
    exact: true,
  });
  await expect(editor).toBeVisible();
  const saved = (await store("snapshot")).settings.repositories;
  expect(saved).toHaveLength(1);
  expect(saved[0]).toMatchObject({
    name: "fixture_corp/repository-101",
    provider: "github",
    provider_account_id: "22",
    provider_repository_id: "101",
    enabled: false,
  });
  await page.reload();
  await section(page, "Repositories");
  await expect(page.locator("[data-repository]")).toContainText(
    "fixture_corp/repository-101",
  );
});

for (const [embedded, genie] of [
  [false, false],
  [true, false],
  [true, true],
]) {
  test(`Retry restores recovered organization owners without changing the selected owner (${genie ? "Genie" : embedded ? "panel" : "standalone"})`, async ({
    page,
    store,
  }) => {
    const { state } = await install(page, store);
    state.responses[22][`${catalog}2`] = { error: "network" };
    const browser = await open(page, store, corporate, embedded, genie);
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    await browser.getByLabel("Find a repository").fill("101");
    await browser.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(browser.getByRole("alert")).toContainText(
      "Cannot reach GitHub",
    );
    await expect(browser.getByLabel("Repository owner")).toHaveValue(
      corporate.login,
    );
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    state.responses = responses();
    await browser.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(browser.getByRole("alert")).toBeHidden();
    await expect(
      browser.getByLabel("Repository owner").locator("option"),
    ).toContainText([
      "Choose an owner",
      "fixture_corp (personal)",
      "orbit (organization)",
    ]);
    await expect(browser.getByLabel("Repository owner")).toHaveValue(
      corporate.login,
    );
    await expect(browser.getByLabel("Find a repository")).toHaveValue("101");
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    await browser.getByLabel("Repository owner").selectOption("orbit");
    await expect(browser.locator("[data-pick]")).toHaveCount(2);
    expect(state.reads.every((read) => read.accountId === "22")).toBe(true);
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
  });
}

for (const failure of [
  "missing_scope",
  "wrong_identity",
  "signed_out",
  "authentication_changed",
]) {
  test(`selection ${failure} clears the populated cache and recovers only through a fresh selected-account read`, async ({
    page,
    store,
  }) => {
    const { state, fixture } = await install(page, store);
    const browser = await open(page, store);
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    const originalHandler = fixture.handler;
    if (failure === "missing_scope") {
      state.responses[22]["/repos/fixture_corp/repository-101"].headers[
        "x-oauth-scopes"
      ] = "read:user";
    } else if (failure === "wrong_identity") {
      state.responses[22]["/user"] = response({
        id: 44,
        login: personal.login,
      });
    } else if (failure === "signed_out") {
      state.responses[22]["/user"] = response({}, {}, 401);
    } else {
      fixture.handler = async (command, args) => {
        const result = await originalHandler(command, args);
        // Native held-generation tests independently exercise this serialized rejection.
        if (command === "resolve_provider_repository")
          throw "authentication_changed";
        return result;
      };
    }
    await browser.locator("[data-pick]").click();
    await expect(browser.getByRole("alert")).toBeVisible();
    await expect(browser.locator("[data-pick]")).toHaveCount(0);
    await expect(browser.getByLabel("Repository owner")).toBeDisabled();
    await expect(browser.getByLabel("Find a repository")).toBeDisabled();
    await expect(browser.getByRole("status")).toHaveText(
      "Repository browsing unavailable.",
    );
    await expect(
      browser.getByRole("button", { name: "Retry", exact: true }),
    ).toBeVisible();
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
    state.responses = responses();
    fixture.handler = originalHandler;
    const reads = state.reads.length;
    await browser.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(browser.getByRole("alert")).toBeHidden();
    await expect(browser.getByLabel("Repository owner")).toBeEnabled();
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toContainText(
      "fixture_corp/repository-101",
    );
    expect(state.reads.length).toBeGreaterThan(reads);
    expect(state.reads.every((read) => read.accountId === "22")).toBe(true);
  });
}

test("contradictory or empty pagination retains only verified preceding rows with visible Retry", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[22][`${catalog}1`].headers.link =
    `<https://api.github.com${catalog}2>; rel="next", <https://api.github.com${catalog}1>; rel="last"`;
  const browser = await open(page, store);
  await expect(browser.getByRole("alert")).toContainText(
    "Repository pagination (page 1)",
  );
  await browser.getByLabel("Repository owner").selectOption(corporate.login);
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  await expect(browser.getByRole("status")).toContainText(
    "discovery is incomplete",
  );
  await expect(
    browser.getByLabel("Repository owner").locator("option"),
  ).not.toContainText(["orbit (organization)"]);
  state.responses = responses();
  state.responses[22][`${catalog}2`].body = [];
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByRole("alert")).toContainText(
    "Repository pagination (page 2)",
  );
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  state.responses = responses();
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByRole("alert")).toBeHidden();
  await expect(
    browser.getByLabel("Repository owner").locator("option"),
  ).toContainText(["orbit (organization)"]);
});

test("ordinary selection denial preserves authorized rows with a visible failure and Retry", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  const browser = await open(page, store);
  await browser.getByLabel("Repository owner").selectOption(corporate.login);
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  state.responses[22]["/repos/fixture_corp/repository-101"] = response(
    {},
    {},
    403,
  );
  await browser.locator("[data-pick]").click();
  await expect(browser.getByRole("alert")).toContainText("denied read access");
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  await expect(browser.getByLabel("Find a repository")).toBeEnabled();
  await expect(
    browser.getByRole("button", { name: "Retry", exact: true }),
  ).toBeVisible();
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
});

test("refresh reconciles a vanished organization without discarding the selected owner or search", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[22][`${catalog}2`].headers["x-github-sso"] =
    "partial-results; organizations=123";
  const browser = await open(page, store);
  await browser.getByLabel("Repository owner").selectOption("orbit");
  await expect(browser.locator("[data-pick]")).toHaveCount(2);
  await browser.getByLabel("Find a repository").fill("103");
  state.responses[22][`${catalog}1`] = response([
    repository(101, corporate.login, "User"),
  ]);
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.getByRole("alert")).toBeHidden();
  await expect(browser.getByLabel("Repository owner")).toHaveValue("orbit");
  await expect(browser.getByLabel("Find a repository")).toHaveValue("103");
  await expect(
    browser.getByLabel("Repository owner").locator("option:checked"),
  ).toHaveText("orbit (unavailable)");
  await expect(browser.locator("[data-pick]")).toHaveCount(0);
});

for (const [failure, message, invalidates] of [
  [
    "unavailable-account",
    { stage: "session", account_id: "22", error: "signed_out" },
    true,
  ],
  ["changed-generation", "authentication_changed", true],
  [
    "resource-conflict",
    "Resource changed in another window. Your draft has not been written; reload or explicitly repair it before saving.",
    false,
  ],
]) {
  test(`save-time native ${failure} rejection preserves the correct access boundary before persistence`, async ({
    page,
    store,
  }) => {
    const { state } = await install(page, store);
    await page.addInitScript((message) => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__rejectRepositorySave = true;
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (
          command === "save_resource" &&
          args.edit.kind === "repository" &&
          window.__rejectRepositorySave
        ) {
          return Promise.reject(message);
        }
        return original(command, args);
      };
    }, message);
    const browser = await open(page, store);
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    await browser.locator("[data-pick]").click();
    await expect(browser.getByRole("alert")).toBeVisible();
    if (failure === "unavailable-account")
      await expect(browser.getByRole("alert")).toContainText(
        "GitHub is disconnected",
      );
    if (failure === "resource-conflict")
      await expect(browser.getByRole("alert")).toContainText(message);
    await expect(browser.locator("[data-pick]")).toHaveCount(
      invalidates ? 0 : 1,
    );
    if (invalidates) {
      await expect(browser.getByLabel("Repository owner")).toBeDisabled();
      await expect(browser.getByLabel("Find a repository")).toBeDisabled();
      await expect(browser.getByRole("status")).toHaveText(
        "Repository browsing unavailable.",
      );
    } else {
      await expect(browser.getByLabel("Repository owner")).toBeEnabled();
      await expect(browser.getByLabel("Find a repository")).toBeEnabled();
      await expect(browser.getByRole("status")).toContainText(
        "discovery is incomplete",
      );
    }
    await expect(
      browser.getByRole("button", { name: "Retry", exact: true }),
    ).toBeVisible();
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
    expect(state.reads.at(-1).command).toBe("resolve_provider_repository");
    const reads = state.reads.length;
    await page.evaluate(() => {
      window.__rejectRepositorySave = false;
    });
    await browser.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(browser.getByRole("alert")).toBeHidden();
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    expect(state.reads.length).toBeGreaterThan(reads);
    expect(state.reads.every((read) => read.accountId === "22")).toBe(true);
    await browser.locator("[data-pick]").click();
    await expect(
      page.getByRole("dialog", {
        name: "Settings for fixture_corp/repository-101",
        exact: true,
      }),
    ).toBeVisible();
    expect(
      (await store("snapshot")).settings.repositories[0].provider_account_id,
    ).toBe("22");
  });
}

test("200-entry known-final no-Link catalog is complete, while unknown-last full pages probe normally", async ({
  page,
  store,
}) => {
  const { state } = await install(page, store);
  state.responses[22][`${catalog}1`].body = Array.from(
    { length: 100 },
    (_, index) => repository(index + 1, corporate.login, "User"),
  );
  state.responses[22][`${catalog}2`] = response(
    Array.from({ length: 100 }, (_, index) => repository(index + 101, "orbit")),
  );
  state.responses[22][`${catalog}3`] = response([]);
  const browser = await open(page, store);
  await expect(browser.getByLabel("Repository owner")).toBeEnabled();
  await expect(browser.getByRole("alert")).toBeHidden();
  await browser.getByLabel("Repository owner").selectOption(corporate.login);
  await expect(browser.locator("[data-pick]")).toHaveCount(100);
  await expect(browser.getByRole("status")).toHaveText(
    "100 accessible repositories loaded.",
  );
  await browser.getByLabel("Repository owner").selectOption("orbit");
  await expect(browser.locator("[data-pick]")).toHaveCount(100);
  await expect(browser.getByRole("alert")).toBeHidden();
  delete state.responses[22][`${catalog}1`].headers.link;
  await browser.getByLabel("Repository owner").selectOption(corporate.login);
  await expect(browser.locator("[data-pick]")).toHaveCount(100);
  await expect(browser.getByRole("alert")).toBeHidden();
});

for (const operation of ["retry", "selection", "credential-header"]) {
  test(`unavailable native credential acquisition during ${operation} clears only the stale connected binding`, async ({
    page,
    store,
  }) => {
    const { state, fixture } = await install(page, store);
    state.responses[22][`${catalog}2`].headers["x-github-sso"] =
      "partial-results; organizations=123";
    const browser = await open(page, store);
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    state.sessionFailure =
      operation === "credential-header" ? "broken_cli" : "configuration";
    fixture.accounts = [
      {
        ...corporate,
        state: "reconnect_required",
        reason:
          operation === "credential-header"
            ? "provider"
            : "credentials_unavailable",
      },
      personal,
    ];
    if (operation === "retry") {
      await browser.getByRole("button", { name: "Retry", exact: true }).click();
    } else {
      await browser.locator("[data-pick]").click();
    }
    await expect(browser.locator("[data-pick]")).toHaveCount(0);
    await expect(browser.getByLabel("Repository owner")).toBeDisabled();
    await expect(browser.getByLabel("Find a repository")).toBeDisabled();
    await expect(browser.getByRole("alert")).toContainText("credential access");
    await expect(browser.getByRole("status")).toHaveText(
      "Repository browsing unavailable.",
    );
    await expect(
      browser.getByRole("button", { name: "Retry", exact: true }),
    ).toBeVisible();
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
    await browser
      .getByRole("button", { name: "Close dialog", exact: true })
      .click();
    state.sessionFailure = undefined;
    await page
      .getByRole("button", {
        name: "Browse repositories as personal",
        exact: true,
      })
      .click();
    const neighbor = page.getByRole("dialog", {
      name: "Browse repositories as personal",
      exact: true,
    });
    await neighbor.getByLabel("Repository owner").selectOption(personal.login);
    await expect(neighbor.locator("[data-pick]")).toContainText(
      "personal/repository-201",
    );
    await neighbor
      .getByRole("button", { name: "Close dialog", exact: true })
      .click();
    fixture.accounts = [corporate, personal];
    await page
      .getByRole("button", {
        name: "Browse repositories as fixture_corp",
        exact: true,
      })
      .click();
    const fresh = page.getByRole("dialog", {
      name: "Browse repositories as fixture_corp",
      exact: true,
    });
    await fresh.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(fresh.locator("[data-pick]")).toContainText(
      "fixture_corp/repository-101",
    );
  });
}

for (const failure of [
  "catalog-configuration",
  "partial-configuration",
  "catalog-broken-cli",
  "partial-broken-cli",
  "session-network",
  "session-schema",
]) {
  test(`${failure} retains usable selected-account results rather than inventing auth loss`, async ({
    page,
    store,
  }) => {
    const { state } = await install(page, store);
    state.responses[22][`${catalog}2`].headers["x-github-sso"] =
      "partial-results; organizations=123";
    const browser = await open(page, store);
    await browser.getByLabel("Repository owner").selectOption(corporate.login);
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    if (failure.startsWith("session-"))
      state.sessionFailure =
        failure === "session-network" ? "network" : "invalid_response";
    else
      state.responses[22][
        `${catalog}${failure.startsWith("partial-") ? 2 : 1}`
      ] = {
        error: failure.endsWith("broken-cli") ? "broken_cli" : "configuration",
      };
    await browser.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(browser.getByRole("alert")).toContainText(
      failure === "session-network"
        ? "Cannot reach GitHub"
        : failure === "session-schema"
          ? "malformed"
          : failure.endsWith("broken-cli")
            ? "broken_cli"
            : "lookup configuration",
    );
    await expect(browser.locator("[data-pick]")).toHaveCount(1);
    await expect(browser.getByLabel("Repository owner")).toBeEnabled();
    await expect(browser.getByLabel("Find a repository")).toBeEnabled();
    await expect(browser.getByRole("status")).toContainText("incomplete");
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
  });
}

test("a session failure scoped to another account cannot invalidate this browser's cache", async ({
  page,
  store,
}) => {
  const { fixture } = await install(page, store);
  const browser = await open(page, store);
  await browser.getByLabel("Repository owner").selectOption(corporate.login);
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  const original = fixture.handler;
  fixture.handler = async (command, args) => {
    const result = await original(command, args);
    if (command === "resolve_provider_repository")
      throw { stage: "session", account_id: "44", error: "configuration" };
    return result;
  };
  await browser.locator("[data-pick]").click();
  await expect(browser.getByRole("alert")).toContainText("another account");
  await expect(browser.locator("[data-pick]")).toHaveCount(1);
  await expect(browser.getByLabel("Find a repository")).toBeEnabled();
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
});
