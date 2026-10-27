import { test, expect } from "./fixtures.mjs";
import {
  providerFixture,
  repositoryPage,
} from "./repository-provider-fixture.mjs";
import { openFilters, openRule, backOne } from "./navigation.mjs";

const TENANT = "00000000-0000-0000-0000-00000000000a";
const AVERY = "00000000-0000-0000-0000-000000000001";
const BLAKE = "00000000-0000-0000-0000-000000000002";
const CASEY = "00000000-0000-0000-0000-000000000003";
const PROJECT = "00000000-0000-0000-0000-0000000000b1";
const REPO = "00000000-0000-0000-0000-0000000000c1";
const KEY = `contoso/${PROJECT}/${REPO}`;
const ADO_URL = "https://dev.azure.com/contoso/Web/_git/app";

const avery = {
  id: AVERY,
  display_name: "Avery Example",
  unique_name: "avery@contoso.example",
};
const connected = {
  cli: { state: "ready", version: "2.90.0" },
  minimum: "2.54.0",
  installer: "homebrew",
  organization: { state: "tenant_known", name: "contoso", tenant: TENANT },
  account: { state: "connected", identity: avery },
  checking: false,
};
const DANA = "00000000-0000-0000-0000-000000000004";
const directory = [
  { id: BLAKE, login: "Blake Example", mail: "blake@contoso.example" },
  { id: CASEY, login: "Blake Two", mail: "blake2@contoso.example" },
  { id: DANA, login: "Dana Example", mail: "dana@contoso.example" },
];

/** In-page fake of the native Azure DevOps setup, repository and people
 * commands; GitHub commands and Store saves pass through. */
async function azureBackend(page, setup = connected) {
  await page.addInitScript(
    ({ setup, AVERY, KEY, directory }) => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__ado = { setup, calls: [], fallback: false, hold: {} };
      const reply = (value) =>
        window.__settingsTrack(
          Promise.resolve(JSON.parse(JSON.stringify(value))),
        );
      window.__TAURI_INTERNALS__.invoke = (command, args = {}) => {
        const ado = window.__ado;
        if (command.startsWith("azure_devops_")) return reply(ado.setup);
        if (args.provider !== "azure_devops") return original(command, args);
        ado.calls.push({ command, args });
        const identity = { id: AVERY, login: "Avery Example" };
        switch (command) {
          case "list_provider_repository_owners":
            return reply({
              identity,
              owners: [{ login: "Web", kind: "project" }],
              warnings: [],
            });
          case "list_provider_repositories":
            return reply({
              identity,
              owners: [],
              repositories: [{ id: KEY, name: `contoso/${args.owner}/app` }],
              warnings: [],
            });
          case "resolve_provider_repository":
            return reply({
              identity,
              repository: { id: KEY, name: "contoso/Web/app" },
              account_generation: 0,
            });
          case "search_provider_people": {
            const held = ado.hold[args.query];
            if (held)
              return window.__settingsTrack(
                held.then(() =>
                  JSON.parse(
                    JSON.stringify({
                      source: "directory",
                      people: directory.filter((person) =>
                        person.login.toLowerCase().startsWith(args.query),
                      ),
                    }),
                  ),
                ),
              );
            return reply(
              ado.fallback
                ? {
                    source: "recent",
                    people: [{ id: directory[0].id, login: "Blake Example" }],
                  }
                : {
                    source: "directory",
                    people: directory.filter((person) =>
                      person.login.toLowerCase().startsWith(args.query),
                    ),
                  },
            );
          }
        }
        return Promise.reject(`unexpected ${command}`);
      };
    },
    { setup, AVERY, KEY, directory },
  );
}

const calls = (page, command) =>
  page.evaluate(
    (command) => window.__ado.calls.filter((call) => call.command === command),
    command,
  );

async function addByUrl(page, url) {
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await dialog.getByLabel("Repository URL", { exact: true }).fill(url);
  return dialog;
}

test("adds an Azure DevOps repository by URL and picks teammates with autocomplete", async ({
  page,
  store,
}) => {
  await providerFixture(page, store);
  await azureBackend(page);
  await repositoryPage(page, store);
  let dialog = await addByUrl(
    page,
    "https://dev.azure.com/fabrikam/Web/_git/app",
  );
  await expect(dialog.getByRole("status")).toHaveText("Other organization");
  const add = dialog.getByRole("button", { name: "Add & configure" });
  await expect(add).toBeDisabled();
  await expect(dialog.getByLabel("Acting GitHub account")).toBeHidden();
  await dialog
    .getByLabel("Repository URL", { exact: true })
    .fill(ADO_URL.replace("dev.azure.com/contoso", "contoso.visualstudio.com"));
  await expect(dialog.getByRole("status")).toHaveText(
    "Azure DevOps / Avery Example",
  );
  await dialog.getByLabel("Repository URL", { exact: true }).fill(ADO_URL);
  await add.click();
  const editor = page.getByRole("dialog", {
    name: "contoso/Web/app",
    exact: true,
  });
  await expect(editor).toBeVisible();
  await expect(editor.locator(".repository-identity p")).toHaveText(
    "Azure DevOps / Avery Example",
  );
  expect(await calls(page, "resolve_provider_repository")).toEqual([
    {
      command: "resolve_provider_repository",
      args: { provider: "azure_devops", accountId: AVERY, repository: ADO_URL },
    },
  ]);
  let saved = (await store("snapshot")).settings.repositories;
  expect(saved).toHaveLength(1);
  expect(saved[0]).toMatchObject({
    provider: "azure_devops",
    name: "contoso/Web/app",
    enabled: false,
    provider_account_id: AVERY,
    provider_repository_id: KEY,
  });
  // Pull requests are not polled yet, so a new repository starts unmonitored.
  await expect(
    editor.getByRole("switch", { name: "Monitor contoso/Web/app" }),
  ).not.toBeChecked();

  const allPulls = editor.getByRole("switch", { name: "All pull requests" });
  if (await allPulls.isChecked()) await allPulls.setChecked(false);
  await openFilters(page, editor);
  await openRule(page, "include_users");
  await page.locator(".admission-add").click();
  const picker = page.getByRole("dialog", { name: "Add people", exact: true });
  const person = picker.getByLabel("Person", { exact: true });
  await expect(person).toBeFocused();
  await person.pressSequentially("bl", { delay: 20 });
  await expect(picker.locator("[data-person]")).toHaveCount(2);
  // Debounced: one search for the settled prefix.
  expect(
    (await calls(page, "search_provider_people")).map((call) => call.args),
  ).toEqual([
    {
      provider: "azure_devops",
      accountId: AVERY,
      repositoryId: KEY,
      query: "bl",
    },
  ]);
  await person.press("ArrowDown");
  await expect(
    picker.getByRole("button", {
      name: "Blake Example, blake@contoso.example",
    }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(picker).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Remove Blake Example", exact: true }),
  ).toBeVisible();

  // Directory failure: recent pull request people, marked Recent.
  await page.evaluate(() => {
    window.__ado.fallback = true;
  });
  await page.locator(".admission-add").click();
  await picker.getByLabel("Person", { exact: true }).fill("bl");
  await expect(picker.locator("[data-person-status]")).toHaveText("Recent");
  await picker.getByRole("button", { name: "Blake Example" }).click();
  await expect(picker.getByRole("alert")).toHaveText("Already added");
  await picker.getByRole("button", { name: "Cancel", exact: true }).click();

  await backOne(page);
  await backOne(page);
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
  saved = (await store("snapshot")).settings.repositories;
  const include = saved[0].admission.rules.find(
    (rule) => rule.kind === "include_users",
  );
  expect(include).toMatchObject({
    enabled: true,
    users: [{ id: BLAKE, login: "Blake Example" }],
  });
  const row = page.locator(`[data-repository="${saved[0].id}"]`);
  await expect(row).toContainText("Azure DevOps / Avery Example");
});

test("browses Azure DevOps projects and adds a repository", async ({
  page,
  store,
}) => {
  await providerFixture(page, store);
  await azureBackend(page);
  await repositoryPage(page, store);
  await page
    .getByRole("button", {
      name: "Browse repositories as Avery Example",
      exact: true,
    })
    .click();
  const browser = page.getByRole("dialog", {
    name: "Browse repositories as Avery Example",
    exact: true,
  });
  const project = browser.getByLabel("Project", { exact: true });
  await expect(project).toBeEnabled();
  await project.selectOption("Web");
  const row = browser.locator(`[data-pick="${KEY}"]`);
  await expect(row).toContainText("app");
  await expect(row).toContainText("Add");
  await row.click();
  await expect(
    page.getByRole("dialog", { name: "contoso/Web/app", exact: true }),
  ).toBeVisible();
  expect(
    (await calls(page, "resolve_provider_repository"))[0].args.repository,
  ).toBe(KEY);
  expect((await store("snapshot")).settings.repositories[0]).toMatchObject({
    provider: "azure_devops",
    provider_account_id: AVERY,
    provider_repository_id: KEY,
  });
});

test("Azure DevOps URLs wait for a connected account", async ({
  page,
  store,
}) => {
  await providerFixture(page, store);
  await azureBackend(page, {
    ...connected,
    account: { state: "signed_out" },
  });
  await repositoryPage(page, store);
  await expect(
    page.getByRole("button", { name: "Browse repositories as Avery Example" }),
  ).toHaveCount(0);
  const dialog = await addByUrl(page, ADO_URL);
  await expect(dialog.getByRole("status")).toHaveText("Not connected");
  await expect(
    dialog.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
  expect(await calls(page, "resolve_provider_repository")).toEqual([]);
});

test("teammate results of an earlier query are cleared and never picked", async ({
  page,
  store,
}) => {
  await providerFixture(page, store);
  await azureBackend(page);
  await repositoryPage(page, store);
  const dialog = await addByUrl(page, ADO_URL);
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  const editor = page.getByRole("dialog", {
    name: "contoso/Web/app",
    exact: true,
  });
  const allPulls = editor.getByRole("switch", { name: "All pull requests" });
  if (await allPulls.isChecked()) await allPulls.setChecked(false);
  await openFilters(page, editor);
  await openRule(page, "include_users");
  await page.locator(".admission-add").click();
  const picker = page.getByRole("dialog", { name: "Add people", exact: true });
  const person = picker.getByLabel("Person", { exact: true });
  await person.fill("bl");
  await expect(picker.locator("[data-person]")).toHaveCount(2);
  // The next query's reply is held: the old results must not answer it.
  await page.evaluate(() => {
    window.__ado.release = undefined;
    window.__ado.hold.da = new Promise((resolve) => {
      window.__ado.release = resolve;
    });
  });
  await person.fill("da");
  await expect(picker.locator("[data-person]")).toHaveCount(0);
  await person.press("Enter");
  await expect(picker).toBeVisible();
  await page.waitForFunction(() =>
    window.__ado.calls.some(
      (call) =>
        call.command === "search_provider_people" && call.args.query === "da",
    ),
  );
  await page.evaluate(() => window.__ado.release());
  await expect(picker.locator("[data-person]")).toHaveCount(1);
  await expect(picker.locator(`[data-person="${DANA}"]`)).toBeVisible();
  await person.press("Enter");
  await expect(picker).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Remove Dana Example", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Remove Blake Example", exact: true }),
  ).toHaveCount(0);
});
