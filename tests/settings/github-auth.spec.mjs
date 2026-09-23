import { expect, test } from "./fixtures.mjs";

const idle = (accounts = []) => ({ accounts, flow: { state: "idle" } });
const connected = (accountId, login) => ({
  provider: "github",
  state: "connected",
  account_id: accountId,
  login,
});

test("GitHub App accounts connect concurrently without exposing credentials", async ({
  page,
}) => {
  let state = idle();
  await page.exposeFunction("__githubAuth", (command, args) => {
    if (command === "begin_github_auth")
      state = {
        accounts: state.accounts,
        flow: {
          state: "connecting",
          user_code: "ABCD-EFGH",
          verification_uri: "https://github.com/login/device",
          expires_in_seconds: 900,
          interval_seconds: 5,
          expected_account_id: args?.expectedAccountId,
        },
      };
    if (command === "poll_github_auth")
      state = idle([
        ...state.accounts,
        connected(
          state.accounts.length ? "84" : "6954990",
          state.accounts.length ? "hubot" : "jdylanmc",
        ),
      ]);
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
        "begin_github_auth",
        "poll_github_auth",
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
  await card.getByRole("button", { name: "Add GitHub account" }).click();
  await expect(card.getByRole("status")).toContainText("ABCD-EFGH");
  await expect(card.getByRole("status")).toContainText("at least 5 seconds");
  await card.getByRole("button", { name: "Check authorization" }).click();
  await expect(card).toContainText("jdylanmc (6954990)");

  await card.getByRole("button", { name: "Add GitHub account" }).click();
  await card.getByRole("button", { name: "Check authorization" }).click();
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

for (const [reason, message] of [
  ["denied", "authorization was denied"],
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
              installation_id: args.accountId === "84" ? "9002" : "9001",
              repository: {
                id: "1376547672",
                name: "jdylanmc/pr-sniper",
              },
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
        installation_id: "9001",
        provider_repository_id: "1376547672",
      }),
      expect.objectContaining({
        name: "jdylanmc/pr-sniper",
        provider_account_id: "84",
        installation_id: "9002",
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
