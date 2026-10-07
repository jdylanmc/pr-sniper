import { expect } from "./fixtures.mjs";
import { section, fixtureAgent } from "./navigation.mjs";

export const actor = {
  provider: "github",
  account_id: "22",
  login: "fixture",
  state: "connected",
};
export const actor2 = { ...actor, account_id: "44", login: "other" };
export const reviewer = {
  ...fixtureAgent,
  ai_account: { provider: "copilot", account_id: "33" },
};
export async function providerFixture(page, store, handler) {
  const state = { accounts: [actor, actor2], calls: [], handler };
  await page.exposeFunction("__repositoryFixture", async (command, args) => {
    state.calls.push({ command, args });
    if (state.handler) {
      const result = await state.handler(command, args);
      if (result !== undefined) return result;
    }
    if (command === "github_auth_state")
      return { accounts: state.accounts, flow: { state: "idle" } };
    if (command === "copilot_auth_state")
      return {
        accounts: [
          {
            provider: "copilot",
            account_id: "33",
            login: "ai-only",
            state: "connected",
          },
        ],
        flow: { state: "idle" },
      };
    if (command === "list_copilot_models")
      return [{ id: reviewer.model, name: reviewer.model }];
    if (command === "list_provider_repository_owners")
      return {
        identity: { id: args.accountId },
        owners: [
          { login: "fixture", kind: "personal" },
          { login: "orbit", kind: "organization" },
        ],
      };
    if (command === "list_provider_repositories")
      return {
        identity: { id: args.accountId },
        owners: [
          { login: "fixture", kind: "personal" },
          { login: "orbit", kind: "organization" },
        ],
        repositories: [
          { id: "100", name: `${args.owner}/one` },
          { id: "200", name: `${args.owner}/two` },
        ],
      };
    if (command === "resolve_provider_repository")
      return {
        identity: { id: args.accountId },
        repository: {
          id: args.repository.includes("two") ? "200" : "100",
          name: await store("canonical_repository_name", {
            repository: args.repository,
          }),
        },
      };
    if (command === "monitoring_setup_review")
      return store("fixture_setup_review", {
        repositoryAccounts: Object.fromEntries(
          state.accounts.map((a) => [
            a.account_id,
            { login: a.login, connected: a.state === "connected" },
          ]),
        ),
        aiAccounts: { 33: { login: "ai-only", connected: true } },
      });
    throw new Error(`Unexpected fixture command: ${command}`);
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args = {}) =>
      [
        "github_auth_state",
        "start_github_browser_auth",
        "confirm_github_account",
        "cancel_github_auth",
        "copilot_auth_state",
        "list_copilot_models",
        "list_provider_repository_owners",
        "list_provider_repositories",
        "resolve_provider_repository",
        "monitoring_setup_review",
      ].includes(command)
        ? window
            .__repositoryFixture(command, args)
            .catch((error) =>
              Promise.reject(error instanceof Error ? error.message : error),
            )
        : original(command, args);
  });
  return state;
}
export async function repositoryPage(page, store, embedded = true) {
  if (embedded) await store("fixture_show_panel");
  await page.goto(embedded ? "/" : "/?view=settings");
  if (embedded)
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
  await section(page, "Repositories");
  await expect(
    page.getByRole("button", {
      name: "Browse repositories as fixture",
      exact: true,
    }),
  ).toBeVisible();
}
export async function addByUrl(page, name = "fixture/one", accountId = "22") {
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await dialog.getByLabel("Repository URL", { exact: true }).fill(name);
  await dialog
    .getByLabel("Acting GitHub account", { exact: true })
    .selectOption(accountId);
  await dialog
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  return page.getByRole("dialog", {
    name: `Settings for ${name.toLowerCase().replace("https://github.com/", "")}`,
    exact: true,
  });
}
