import { expect, test } from "./fixtures.mjs";
import {
  closeDialog,
  repositorySettings,
  saveChanges,
  section,
  seedAgent,
  setAgentPrompt,
  editAgent,
  repositoryConnectionStatus,
} from "./navigation.mjs";

async function connection(page, name = "jdylanmc/pr-sniper") {
  const modal = await repositorySettings(page, name);
  await modal.getByText("Repository and connection", { exact: true }).click();
  return modal;
}

async function boundConnection(page, repository, accountCaption) {
  await section(page, "Integrations");
  const row = page.locator(`[data-repository="${repository.id}"]`);
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAccessibleName(
    `${repository.name} as ${accountCaption}`,
  );
  await expect(row).toContainText(`GitHub / ${accountCaption}`);
  await row.click();
  const modal = page.getByRole("dialog", {
    name: `Settings for ${repository.name}`,
    exact: true,
  });
  await modal.getByText("Repository and connection", { exact: true }).click();
  await expect(modal.locator(".repository-binding-details dd")).toHaveText([
    repository.provider_account_id,
    repository.provider_repository_id,
  ]);
  return modal;
}

const verified = {
  identity: { id: "6954990", login: "jdylanmc" },
  repository: { id: "1376547672", name: "jdylanmc/pr-sniper" },
  capabilities: { read: true, comment: "available" },
};

async function connectedGithubAccount(page) {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "github_auth_state"
        ? Promise.resolve({
            accounts: [
              {
                provider: "github",
                state: "connected",
                account_id: "6954990",
                login: "jdylanmc",
              },
            ],
            flow: { state: "idle" },
          })
        : original(command, args);
  });
}

async function saveBoundRepository(
  store,
  name = "jdylanmc/pr-sniper",
  accountId = "6954990",
  repositoryId = "1376547672",
) {
  await store("save_repository", { repository: name });
  const settings = (await store("snapshot")).settings;
  Object.assign(
    settings.repositories.find((repository) => repository.name === name),
    {
      enabled: false,
      provider_account_id: accountId,
      provider_repository_id: repositoryId,
    },
  );
  await store("seed_settings", settings);
  return settings;
}

async function githubFixture(page, store, handler) {
  await saveBoundRepository(store);
  await connectedGithubAccount(page);
  await page.exposeFunction("__githubResponse", handler);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (
        !["verify_provider_connection", "read_provider_metadata"].includes(
          command,
        )
      )
        return original(command, args);
      const response = await window.__githubResponse(command, args);
      if ("error" in response) throw response.error;
      return response.ok;
    };
  });
  await page.goto("/?view=settings");
  return connection(page);
}

test("verifying GitHub preserves drafts and does not enable automation", async ({
  page,
  store,
}) => {
  await saveBoundRepository(store);
  const before = await seedAgent(store);
  await connectedGithubAccount(page);
  const calls = [];
  await page.exposeFunction("__githubRead", (command, args) => {
    calls.push({ command, args });
    return {
      identity: { id: "6954990", login: "jdylanmc" },
      repository: { id: "1376547672", name: "jdylanmc/pr-sniper" },
      capabilities: { read: true, comment: "available" },
    };
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "verify_provider_connection"
        ? window.__githubRead(command, args)
        : original(command, args);
  });
  await page.goto("/?view=settings");
  await setAgentPrompt(page, "Keep this unsaved review prompt.");
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("6");
  const card = await connection(page);
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(repositoryConnectionStatus(card)).toContainText(
    "jdylanmc (6954990)",
  );
  await expect(repositoryConnectionStatus(card)).toContainText(
    "not publication authorization",
  );
  await closeDialog(page);
  const modal = await editAgent(page);
  const prompt = modal.getByRole("textbox", { name: "Prompt", exact: true });
  await expect(prompt).toHaveValue("Keep this unsaved review prompt.");
  expect(calls).toEqual([
    {
      command: "verify_provider_connection",
      args: { id: before.repositories[0].id },
    },
  ]);
  expect((await store("snapshot")).settings).toEqual({
    ...before,
    agents: [
      { ...before.agents[0], prompt: "Keep this unsaved review prompt." },
    ],
  });
  await closeDialog(page);
  await section(page, "Preferences");
  await expect(page.locator("#global-capacity")).toHaveValue("6");
});

for (const [error, expected] of [
  ["signed_out", "OAuth authorization is missing, expired, or rejected"],
  ["wrong_identity", "does not match"],
  ["missing_read_permission", "denied repository or pull-request read"],
  ["missing_scope", "no longer grants the required repo scope"],
  ["organization_policy_denied", "organization policy or SAML single sign-on"],
  ["rate_limited", "rate limited"],
  ["network", "Could not reach GitHub securely"],
  ["provider_failure", "could not complete this read"],
  ["incomplete_read", "No partial result was accepted"],
  ["gho_untrusted_secret_error_body", "No raw error details"],
]) {
  test(`connection reports ${error} without enabling metadata or leaking errors`, async ({
    page,
    store,
  }) => {
    const card = await githubFixture(page, store, () => ({ error }));
    await card
      .getByRole("button", { name: "Verify GitHub connection", exact: true })
      .click();
    await expect(repositoryConnectionStatus(card)).toContainText(expected);
    await expect(
      card.getByRole("button", { name: "Read PR metadata", exact: true }),
    ).toBeDisabled();
    await expect(page.locator("body")).not.toContainText(
      "gho_untrusted_secret",
    );
  });
}

for (const [comment, message] of [
  ["unknown", "Comment permission unverified"],
  ["unavailable", "Comment permission unavailable"],
]) {
  test(`read access does not fabricate ${comment} comment permission`, async ({
    page,
    store,
  }) => {
    const card = await githubFixture(page, store, () => ({
      ok: { ...verified, capabilities: { read: true, comment } },
    }));
    await card
      .getByRole("button", { name: "Verify GitHub connection", exact: true })
      .click();
    await expect(repositoryConnectionStatus(card)).toContainText(message);
    await expect(repositoryConnectionStatus(card)).toContainText(
      "not publication authorization",
    );
  });
}

test("metadata pins verified identities, renders all files safely and clears failed results", async ({
  page,
  store,
}) => {
  let reads = 0;
  const calls = [];
  const card = await githubFixture(page, store, (command, args) => {
    calls.push({ command, args });
    if (command === "verify_provider_connection") return { ok: verified };
    if (++reads > 1) return { error: "revision_changed" };
    return {
      ok: {
        connection: verified,
        pull_requests: [
          {
            number: 31,
            title: "<script>bad()</script>",
            state: "open",
            draft: false,
            head_sha: "a".repeat(40),
            author: { id: "42", login: "author" },
            requested_reviewers: [{ id: "6954990", login: "jdylanmc" }],
            files: [
              { path: "first.rs", status: "added" },
              { path: "last.rs", status: "renamed" },
            ],
          },
        ],
      },
    };
  });
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await card
    .getByRole("button", { name: "Read PR metadata", exact: true })
    .click();
  await expect(repositoryConnectionStatus(card)).toContainText(
    "1 PRs, 2 changed files",
  );
  await card.locator(".connection summary").click();
  await expect(card.locator("li")).toHaveText([
    "added: first.rs",
    "renamed: last.rs",
  ]);
  await expect(card.locator("script")).toHaveCount(0);
  expect(calls[1].args.expectedAccountId).toBe("6954990");
  expect(calls[1].args.expectedRepositoryId).toBe("1376547672");
  await card
    .getByRole("button", { name: "Read PR metadata", exact: true })
    .click();
  await expect(repositoryConnectionStatus(card)).toContainText(
    "changed during the read",
  );
  await expect(card.locator(".connection summary")).toHaveCount(0);
  await expect(
    card.getByRole("button", { name: "Read PR metadata", exact: true }),
  ).toBeDisabled();
});

test("retargeting a repository revalidates its stable binding", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "resolve_provider_repository"
        ? Promise.resolve({
            identity: { id: args.accountId, login: "jdylanmc" },
            repository: { id: "900", name: "other/target" },
          })
        : original(command, args);
  });
  const card = await githubFixture(page, store, () => ({ ok: verified }));
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(repositoryConnectionStatus(card)).toContainText(
    "jdylanmc (6954990)",
  );
  await card
    .getByRole("button", { name: "Edit repository", exact: true })
    .click();
  const editor = page.getByRole("dialog", {
    name: "Edit repository",
    exact: true,
  });
  await editor
    .getByLabel("Repository URL", { exact: true })
    .fill("other/target");
  await editor
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Settings for other/target", exact: true })
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await saveChanges(page);
  expect((await store("snapshot")).settings.repositories).toEqual([
    expect.objectContaining({
      name: "other/target",
      provider_account_id: "6954990",
      provider_repository_id: "900",
    }),
  ]);
  const target = await connection(page, "other/target");
  await expect(repositoryConnectionStatus(target)).toContainText(
    "Not verified. Acting account 6954990",
  );
  await expect(
    target.getByRole("button", { name: "Read PR metadata", exact: true }),
  ).toBeDisabled();
});

test("metadata replaces stale capability evidence for the same account and repository", async ({
  page,
  store,
}) => {
  const capabilities = ["unavailable", "unknown", "available"];
  let reads = 0;
  const card = await githubFixture(page, store, (command) => {
    if (command === "verify_provider_connection") return { ok: verified };
    return {
      ok: {
        connection: {
          ...verified,
          capabilities: { read: true, comment: capabilities[reads++] },
        },
        pull_requests: [],
      },
    };
  });
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(repositoryConnectionStatus(card)).toContainText(
    "Comment scope available",
  );
  for (const message of [
    "Comment permission unavailable",
    "Comment permission unverified",
    "Comment scope available",
  ]) {
    await card
      .getByRole("button", { name: "Read PR metadata", exact: true })
      .click();
    await expect(repositoryConnectionStatus(card)).toContainText(message);
    await expect(repositoryConnectionStatus(card)).toContainText(
      "Complete metadata: 0 PRs",
    );
  }
});

test("disconnecting one account clears only its repository evidence", async ({
  page,
  store,
}) => {
  await store("save_repository", { repository: "jdylanmc/pr-sniper" });
  const settings = (await store("snapshot")).settings;
  Object.assign(settings.repositories[0], {
    provider_account_id: "101",
    provider_repository_id: "1376547672",
    overrides: { automatic_agent_start: true },
  });
  settings.repositories.push({
    ...structuredClone(settings.repositories[0]),
    id: "22222222-2222-4222-8222-222222222222",
    provider_account_id: "202",
    overrides: { automatic_agent_start: false },
  });
  await store("seed_settings", settings);
  let accounts = [
    {
      provider: "github",
      state: "connected",
      account_id: "101",
      login: "account-a",
    },
    {
      provider: "github",
      state: "connected",
      account_id: "202",
      login: "account-b",
    },
  ];
  const calls = [];
  await page.exposeFunction("__githubAccountSwitch", (command, args) => {
    calls.push({ command, args });
    if (command === "disconnect_github_auth") {
      accounts = accounts.filter(
        (account) => account.account_id !== args.accountId,
      );
      return { accounts, flow: { state: "idle" } };
    }
    if (command === "github_auth_state")
      return { accounts, flow: { state: "idle" } };
    const first =
      args.id === settings.repositories[0].id ||
      args.expectedAccountId === "101";
    const identity = first
      ? { id: "101", login: "account-a" }
      : { id: "202", login: "account-b" };
    const repository = {
      id: "1376547672",
      name: "jdylanmc/pr-sniper",
    };
    if (command === "verify_provider_connection") {
      return {
        identity,
        repository,
        capabilities: { read: true, comment: "available" },
      };
    }
    if (command === "read_provider_metadata") {
      return {
        connection: {
          identity,
          repository,
          capabilities: { read: true, comment: "available" },
        },
        pull_requests: [
          {
            number: first ? 31 : 32,
            title: first ? "Account A evidence" : "Account B evidence",
            state: "open",
            draft: false,
            head_sha: (first ? "a" : "b").repeat(40),
            author: { id: "42", login: "author" },
            requested_reviewers: [identity],
            files: [
              {
                path: first ? "account-a.rs" : "account-b.rs",
                status: "modified",
              },
            ],
          },
        ],
      };
    }
    return state;
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      [
        "github_auth_state",
        "disconnect_github_auth",
        "verify_provider_connection",
        "read_provider_metadata",
      ].includes(command)
        ? window.__githubAccountSwitch(command, args)
        : original(command, args);
  });
  await page.goto("/?view=settings");

  let first = await boundConnection(
    page,
    settings.repositories[0],
    "account-a",
  );
  await first
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await first
    .getByRole("button", { name: "Read PR metadata", exact: true })
    .click();
  await expect(repositoryConnectionStatus(first)).toContainText(
    "account-a (101)",
  );
  await expect(repositoryConnectionStatus(first)).toContainText(
    "Complete metadata",
  );
  await expect(first.locator(".connection summary")).toHaveText(
    "#31 Account A evidence - open - 1 files",
  );
  await first.locator(".connection summary").click();
  await expect(first.locator(".connection li")).toHaveText(
    "modified: account-a.rs",
  );
  await closeDialog(page);

  let second = await boundConnection(
    page,
    settings.repositories[1],
    "account-b",
  );
  await second
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await second
    .getByRole("button", { name: "Read PR metadata", exact: true })
    .click();
  await expect(repositoryConnectionStatus(second)).toContainText(
    "account-b (202)",
  );
  await expect(repositoryConnectionStatus(second)).toContainText(
    "Complete metadata",
  );
  await expect(second.locator(".connection summary")).toHaveText(
    "#32 Account B evidence - open - 1 files",
  );
  await second.locator(".connection summary").click();
  await expect(second.locator(".connection li")).toHaveText(
    "modified: account-b.rs",
  );
  await closeDialog(page);

  const auth = page.locator(".github-auth-card");
  await section(page, "Accounts");
  await auth.getByRole("button", { name: "Disconnect account-a" }).click();
  await expect(auth).not.toContainText("account-a (101)");
  await expect(auth).toContainText("account-b (202)");

  first = await boundConnection(
    page,
    settings.repositories[0],
    "Account unavailable",
  );
  await expect(repositoryConnectionStatus(first)).toContainText(
    "Needs attention",
  );
  await expect(first.locator(".connection summary")).toHaveCount(0);
  await expect(
    first.getByRole("button", {
      name: "Verify GitHub connection",
      exact: true,
    }),
  ).toBeDisabled();
  await expect(
    first.getByRole("button", { name: "Read PR metadata", exact: true }),
  ).toBeDisabled();
  await closeDialog(page);

  second = await boundConnection(page, settings.repositories[1], "account-b");
  await expect(repositoryConnectionStatus(second)).toContainText(
    "account-b (202)",
  );
  await expect(repositoryConnectionStatus(second)).toContainText(
    "Complete metadata",
  );
  await expect(second.locator(".connection summary")).toHaveText(
    "#32 Account B evidence - open - 1 files",
  );
  await second.locator(".connection summary").click();
  await expect(second.locator(".connection li")).toHaveText(
    "modified: account-b.rs",
  );
  await expect(
    second.getByRole("button", { name: "Read PR metadata", exact: true }),
  ).toBeEnabled();
  expect(
    calls.filter((call) =>
      ["verify_provider_connection", "read_provider_metadata"].includes(
        call.command,
      ),
    ),
  ).toEqual([
    {
      command: "verify_provider_connection",
      args: { id: settings.repositories[0].id },
    },
    {
      command: "read_provider_metadata",
      args: {
        id: settings.repositories[0].id,
        expectedAccountId: "101",
        expectedRepositoryId: "1376547672",
      },
    },
    {
      command: "verify_provider_connection",
      args: { id: settings.repositories[1].id },
    },
    {
      command: "read_provider_metadata",
      args: {
        id: settings.repositories[1].id,
        expectedAccountId: "202",
        expectedRepositoryId: "1376547672",
      },
    },
  ]);
  expect(
    calls.filter((call) => call.command === "disconnect_github_auth"),
  ).toEqual([
    { command: "disconnect_github_auth", args: { accountId: "101" } },
  ]);
  await expect(page.locator("body")).not.toContainText("gh auth login");
  await expect(page.locator("body")).not.toContainText(
    "current GitHub CLI account",
  );
  const persisted = (await store("snapshot")).settings.repositories;
  expect(persisted).toEqual(settings.repositories);
  expect(
    persisted.find((repository) => repository.provider_account_id === "101")
      .overrides.automatic_agent_start,
  ).toBe(true);
  expect(
    persisted.find((repository) => repository.provider_account_id === "202")
      .overrides.automatic_agent_start,
  ).toBe(false);

  await page.reload();
  const unavailable = await boundConnection(
    page,
    settings.repositories[0],
    "Account unavailable",
  );
  await expect(repositoryConnectionStatus(unavailable)).toContainText(
    "Needs attention",
  );
  await unavailable
    .getByRole("button", { name: "Remove repository", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Remove repository?", exact: true })
    .getByRole("button", { name: "Remove from settings", exact: true })
    .click();
  await saveChanges(page);
  expect((await store("snapshot")).settings.repositories).toEqual([
    expect.objectContaining({ provider_account_id: "202" }),
  ]);
  const available = await connection(page);
  await expect(
    available.getByRole("button", {
      name: "Verify GitHub connection",
      exact: true,
    }),
  ).toBeEnabled();
});

test("missing repo scope disables every binding for one account only", async ({
  page,
  store,
}) => {
  for (const repository of ["octo/one", "octo/two", "octo/three"])
    await store("save_repository", { repository });
  const settings = (await store("snapshot")).settings;
  for (const [index, repository] of settings.repositories.entries()) {
    repository.provider_account_id = index < 2 ? "101" : "202";
    repository.provider_repository_id = String(index + 1);
  }
  await store("seed_settings", settings);
  const firstRepositoryId = settings.repositories[0].id;
  let accounts = [
    {
      provider: "github",
      state: "connected",
      account_id: "101",
      login: "first",
    },
    {
      provider: "github",
      state: "connected",
      account_id: "202",
      login: "second",
    },
  ];
  await page.exposeFunction("__scopeState", (command, args) => {
    if (command === "github_auth_state")
      return { accounts, flow: { state: "idle" } };
    if (
      command === "verify_provider_connection" &&
      args.id === firstRepositoryId
    ) {
      accounts = [
        {
          provider: "github",
          state: "reconnect_required",
          account_id: "101",
          login: "first",
          reason: "missing_scope",
        },
        accounts[1],
      ];
      return { error: "missing_scope" };
    }
    if (command === "verify_provider_connection")
      return {
        identity: { id: "202", login: "second" },
        repository: { id: "3", name: "octo/three" },
        capabilities: { read: true, comment: "unknown" },
      };
    throw new Error(`Unexpected scope command: ${command}`);
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (
        !["github_auth_state", "verify_provider_connection"].includes(command)
      )
        return original(command, args);
      return window
        .__scopeState(command, args)
        .then((result) =>
          result?.error ? Promise.reject(result.error) : result,
        );
    };
  });
  await page.goto("/?view=settings");

  const first = await connection(page, "octo/one");
  await first
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(repositoryConnectionStatus(first)).toContainText(
    "Needs attention",
  );
  await expect(page.locator(".github-auth-card")).toContainText(
    "no longer grants the required repo scope",
  );
  await closeDialog(page);

  await expect(
    page.getByRole("button", { name: "octo/one", exact: true }),
  ).toContainText("Reconnect account");
  await expect(
    page.getByRole("button", { name: "octo/two", exact: true }),
  ).toContainText("Reconnect account");
  await expect(
    page.getByRole("button", { name: "octo/three", exact: true }),
  ).toContainText("GitHub / second");
  await section(page, "Accounts");
  await expect(
    page
      .locator(".github-auth-card")
      .getByRole("button", { name: "Reconnect first" }),
  ).toBeVisible();

  const secondBinding = await connection(page, "octo/two");
  await expect(
    secondBinding.getByRole("button", {
      name: "Verify GitHub connection",
      exact: true,
    }),
  ).toBeDisabled();
  await closeDialog(page);

  const unaffected = await connection(page, "octo/three");
  await expect(
    unaffected.getByRole("button", {
      name: "Verify GitHub connection",
      exact: true,
    }),
  ).toBeEnabled();
});

for (const [commandError, stateReason] of [
  ["signed_out", "expired"],
  ["configuration", "credentials_unavailable"],
  ["wrong_identity", "wrong_identity"],
  ["missing_scope", "missing_scope"],
]) {
  test(`mid-session ${commandError} clears every cache for that account`, async ({
    page,
    store,
  }) => {
    await store("save_repository", { repository: "octo/one" });
    await store("save_repository", { repository: "octo/two" });
    const settings = (await store("snapshot")).settings;
    for (const [index, repository] of settings.repositories.entries())
      Object.assign(repository, {
        provider_account_id: "101",
        provider_repository_id: String(index + 1),
      });
    await store("seed_settings", settings);
    let invalid = false;
    await page.exposeFunction("__githubLifecycle", (command, args) => {
      if (command === "github_auth_state")
        return {
          ok: {
            accounts: [
              {
                provider: "github",
                state: invalid ? "reconnect_required" : "connected",
                account_id: "101",
                login: "account-a",
                ...(invalid ? { reason: stateReason } : {}),
              },
              {
                provider: "github",
                state: "connected",
                account_id: "202",
                login: "account-b",
              },
            ],
            flow: { state: "idle" },
          },
        };
      const first =
        args.id === settings.repositories[0].id ||
        args.expectedRepositoryId === "1";
      const connection = {
        identity: { id: "101", login: "account-a" },
        repository: {
          id: first ? "1" : "2",
          name: first ? "octo/one" : "octo/two",
        },
        capabilities: { read: true, comment: "available" },
      };
      if (command === "read_provider_metadata" && invalid)
        return { error: commandError };
      return command === "read_provider_metadata"
        ? { ok: { connection, pull_requests: [] } }
        : { ok: connection };
    });
    await page.addInitScript(() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        if (
          ![
            "github_auth_state",
            "verify_provider_connection",
            "read_provider_metadata",
          ].includes(command)
        )
          return original(command, args);
        const response = await window.__githubLifecycle(command, args);
        if ("error" in response) throw response.error;
        return response.ok;
      };
    });
    await page.goto("/?view=settings");

    for (const name of ["octo/one", "octo/two"]) {
      const card = await connection(page, name);
      await card
        .getByRole("button", { name: "Verify GitHub connection", exact: true })
        .click();
      await card
        .getByRole("button", { name: "Read PR metadata", exact: true })
        .click();
      await expect(repositoryConnectionStatus(card)).toContainText(
        "Complete metadata",
      );
      await closeDialog(page);
    }

    let card = await connection(page, "octo/one");
    invalid = true;
    await card
      .getByRole("button", { name: "Read PR metadata", exact: true })
      .click();
    await expect(page.locator(".github-auth-card")).toContainText(
      stateReason === "expired"
        ? "authorization expired"
        : stateReason === "credentials_unavailable"
          ? "could not be restored or stored safely"
          : stateReason === "missing_scope"
            ? "no longer grants the required repo scope"
            : "unexpected account",
    );
    await closeDialog(page);

    card = await connection(page, "octo/two");
    await expect(repositoryConnectionStatus(card)).toContainText(
      "Needs attention",
    );
    await expect(card.locator(".connection summary")).toHaveCount(0);
    await expect(
      card.getByRole("button", {
        name: "Verify GitHub connection",
        exact: true,
      }),
    ).toBeDisabled();
    await expect(
      card.getByRole("button", { name: "Read PR metadata", exact: true }),
    ).toBeDisabled();
    await expect(page.locator(".github-auth-card")).toContainText(
      "account-b (202)",
    );
  });
}

for (const [commandError, warning, message] of [
  ["network", "network", "network request failed"],
  ["rate_limited", "rate_limited", "rate limited the verification request"],
  ["provider_failure", "provider", "GitHub rejected"],
]) {
  test(`mid-session ${commandError} keeps the verified account available for retry`, async ({
    page,
    store,
  }) => {
    await store("save_repository", { repository: "octo/one" });
    await store("save_repository", { repository: "octo/two" });
    const settings = (await store("snapshot")).settings;
    for (const [index, repository] of settings.repositories.entries())
      Object.assign(repository, {
        provider_account_id: "101",
        provider_repository_id: String(index + 1),
      });
    await store("seed_settings", settings);
    let transient = false;
    await page.exposeFunction("__githubRetryable", (command, args) => {
      if (command === "github_auth_state")
        return {
          accounts: [
            {
              provider: "github",
              state: "connected",
              account_id: "101",
              login: "account-a",
              ...(transient ? { warning } : {}),
            },
          ],
          flow: { state: "idle" },
        };
      const first =
        args.id === settings.repositories[0].id ||
        args.expectedRepositoryId === "1";
      const connection = {
        identity: { id: "101", login: "account-a" },
        repository: {
          id: first ? "1" : "2",
          name: first ? "octo/one" : "octo/two",
        },
        capabilities: { read: true, comment: "available" },
      };
      if (command === "read_provider_metadata" && transient) throw commandError;
      return command === "read_provider_metadata"
        ? { connection, pull_requests: [] }
        : connection;
    });
    await page.addInitScript(() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        [
          "github_auth_state",
          "verify_provider_connection",
          "read_provider_metadata",
        ].includes(command)
          ? window.__githubRetryable(command, args)
          : original(command, args);
    });
    await page.goto("/?view=settings");

    for (const name of ["octo/one", "octo/two"]) {
      const card = await connection(page, name);
      await card
        .getByRole("button", { name: "Verify GitHub connection", exact: true })
        .click();
      await card
        .getByRole("button", { name: "Read PR metadata", exact: true })
        .click();
      await expect(repositoryConnectionStatus(card)).toContainText(
        "Complete metadata",
      );
      await closeDialog(page);
    }

    transient = true;
    let card = await connection(page, "octo/one");
    await card
      .getByRole("button", { name: "Read PR metadata", exact: true })
      .click();
    await expect(page.locator(".github-auth-card")).toContainText(message);
    await closeDialog(page);

    card = await connection(page, "octo/two");
    await expect(
      card.getByRole("button", {
        name: "Verify GitHub connection",
        exact: true,
      }),
    ).toBeEnabled();
    await expect(
      card.getByRole("button", { name: "Read PR metadata", exact: true }),
    ).toBeEnabled();
  });
}
