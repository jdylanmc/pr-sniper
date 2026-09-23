import { expect, test } from "./fixtures.mjs";

const idle = (accounts = []) => ({ accounts, flow: { state: "idle" } });
const connected = (accountId, login) => ({
  provider: "github",
  state: "connected",
  account_id: accountId,
  login,
});

test("browser OAuth confirms two accounts without exposing credentials", async ({
  page,
}) => {
  let state = idle();
  let pendingIdentity = null;
  let connectingReads = 0;
  const starts = [];
  await page.exposeFunction("__githubAuth", (command, args) => {
    if (command === "start_github_browser_auth") {
      starts.push(args ?? {});
      connectingReads = 0;
      pendingIdentity =
        state.accounts.length === 0
          ? { account_id: "6954990", login: "jdylanmc" }
          : { account_id: "84", login: "hubot" };
      state = {
        accounts: state.accounts,
        flow: {
          state: "connecting",
          expected_account_id: args?.expectedAccountId,
        },
      };
    }
    if (command === "github_auth_state" && state.flow.state === "connecting") {
      connectingReads += 1;
      if (connectingReads > 1)
        state = {
          accounts: state.accounts,
          flow: {
            state: "pending_account_confirmation",
            ...pendingIdentity,
          },
        };
    }
    if (command === "confirm_github_account") {
      state = idle([
        ...state.accounts,
        connected(pendingIdentity.account_id, pendingIdentity.login),
      ]);
      pendingIdentity = null;
    }
    if (command === "cancel_github_auth") state = idle(state.accounts);
    if (command === "disconnect_github_auth")
      state = idle(
        state.accounts.filter(
          (account) => account.account_id !== args.accountId,
        ),
      );
    return state;
  });

  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      [
        "github_auth_state",
        "start_github_browser_auth",
        "confirm_github_account",
        "cancel_github_auth",
        "disconnect_github_auth",
      ].includes(command)
        ? window.__githubAuth(command, args)
        : original(command, args);
  });
  await page.goto("/?view=settings");

  const card = page.locator(".github-auth-card");
  await expect(card.getByRole("status")).toContainText(
    "No GitHub accounts connected",
  );
  await expect(card).toContainText(
    "repo scope grants access to public and private repositories",
  );
  await expect(card).toContainText(
    "never starts monitoring every accessible repository automatically",
  );
  await card.getByRole("button", { name: "Add GitHub account" }).click();
  await expect(card.getByRole("status")).toContainText("default browser");
  await expect(card).not.toContainText("device");
  await expect(card.getByRole("status")).toContainText(
    "Confirm jdylanmc (6954990)",
  );
  await expect(card).toContainText("not saved until you confirm");
  await card.getByRole("button", { name: "Confirm" }).click();
  await expect(card).toContainText("jdylanmc (6954990)");

  await card.getByRole("button", { name: "Add GitHub account" }).click();
  await expect(card.getByRole("status")).toContainText("Confirm hubot (84)");
  await card.getByRole("button", { name: "Use a different account" }).click();
  await expect.poll(() => starts.at(-1)?.selectAccount).toBe(true);
  await expect(card.getByRole("status")).toContainText("Confirm hubot (84)");
  await card.getByRole("button", { name: "Confirm" }).click();
  await expect(card).toContainText("hubot (84)");
  await expect(card.getByRole("status")).toContainText(
    "2 GitHub accounts configured",
  );
  await expect(page.locator("body")).not.toContainText("access_token");
  await expect(page.locator("body")).not.toContainText("refresh_token");

  await card.getByRole("button", { name: "Disconnect jdylanmc" }).click();
  await expect(card).not.toContainText("jdylanmc (6954990)");
  await expect(card).toContainText("hubot (84)");
});

test("Keychain confirmation failure stays visible and retryable", async ({
  page,
}) => {
  let state = {
    accounts: [],
    flow: {
      state: "pending_account_confirmation",
      account_id: "6954990",
      login: "jdylanmc",
    },
  };
  let confirmations = 0;
  await page.exposeFunction("__githubAuth", (command) => {
    if (command === "confirm_github_account") {
      confirmations += 1;
      if (confirmations === 1)
        throw new Error("GitHub credentials could not be saved securely.");
      state = idle([connected("6954990", "jdylanmc")]);
    }
    return state;
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      ["github_auth_state", "confirm_github_account"].includes(command)
        ? window.__githubAuth(command, args)
        : original(command, args);
  });
  await page.goto("/?view=settings");

  const card = page.locator(".github-auth-card");
  await card.getByRole("button", { name: "Confirm" }).click();
  await expect(card.getByRole("status")).toContainText(
    "could not be saved securely",
  );
  await expect(card).toContainText("jdylanmc (6954990)");
  await expect(card).not.toContainText("Connected through");
  await card.getByRole("button", { name: "Confirm" }).click();
  await expect(card).toContainText("Connected through");
  expect(confirmations).toBe(2);
});

for (const [reason, message] of [
  ["expired", "authorization expired"],
  ["network", "network request failed"],
  ["provider", "GitHub rejected"],
]) {
  test(`reconnect-required ${reason} state preserves its account and reason`, async ({
    page,
  }) => {
    await page.addInitScript((failureReason) => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        command === "github_auth_state"
          ? Promise.resolve({
              accounts: [
                {
                  provider: "github",
                  state: "reconnect_required",
                  account_id: "6954990",
                  login: "jdylanmc",
                  reason: failureReason,
                },
                {
                  provider: "github",
                  state: "connected",
                  account_id: "84",
                  login: "hubot",
                },
              ],
              flow: { state: "idle" },
            })
          : original(command, args);
    }, reason);
    await page.goto("/?view=settings");
    const card = page.locator(".github-auth-card");
    await expect(card).toContainText(message);
    await expect(card).toContainText("jdylanmc (6954990)");
    await expect(card).toContainText("hubot (84)");
    await expect(
      card.getByRole("button", { name: "Reconnect jdylanmc" }),
    ).toBeVisible();
  });
}

test("overlapping repository access requires an explicit acting account choice", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "github_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "github",
              state: "connected",
              account_id: "6954990",
              login: "jdylanmc",
            },
            {
              provider: "github",
              state: "connected",
              account_id: "84",
              login: "hubot",
            },
          ],
          flow: { state: "idle" },
        });
      if (command === "list_provider_repositories")
        return Promise.resolve({
          identity:
            args.accountId === "84"
              ? { id: "84", login: "hubot" }
              : { id: "6954990", login: "jdylanmc" },
          repositories: [
            {
              id: "1376547672",
              name: "jdylanmc/pr-sniper",
            },
          ],
        });
      return original(command, args);
    };
  });
  await page.goto("/?view=settings");

  const card = page.locator(".github-auth-card");
  await card
    .getByRole("button", { name: "Load repositories for jdylanmc" })
    .click();
  await card
    .getByRole("button", {
      name: "Use jdylanmc/pr-sniper as jdylanmc",
    })
    .click();
  await card
    .getByRole("button", { name: "Load repositories for hubot" })
    .click();
  await card
    .getByRole("button", {
      name: "Use jdylanmc/pr-sniper as hubot",
    })
    .click();

  await expect(
    page.getByRole("article", {
      name: "jdylanmc/pr-sniper as jdylanmc",
    }),
  ).toContainText("GitHub as jdylanmc");
  await expect(
    page.getByRole("article", { name: "jdylanmc/pr-sniper as hubot" }),
  ).toContainText("GitHub as hubot");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await page.evaluate(() => window.__settingsIdle());
  const snapshot = await store("snapshot");
  expect(snapshot.settings.repositories).toHaveLength(2);
  expect(snapshot.settings.repositories).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        name: "jdylanmc/pr-sniper",
        provider_account_id: "6954990",
        provider_repository_id: "1376547672",
      }),
      expect.objectContaining({
        name: "jdylanmc/pr-sniper",
        provider_account_id: "84",
        provider_repository_id: "1376547672",
      }),
    ]),
  );

  await page.reload();
  await expect(
    page.getByRole("article", {
      name: "jdylanmc/pr-sniper as jdylanmc",
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("article", { name: "jdylanmc/pr-sniper as hubot" }),
  ).toBeVisible();
});
