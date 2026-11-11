import { test, expect } from "./fixtures.mjs";
import { backFromEditor } from "./navigation.mjs";
import {
  providerFixture,
  repositoryPage,
  reviewer,
} from "./repository-provider-fixture.mjs";

test.use({ viewport: { width: 400, height: 680 } });

const one = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const two = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const off = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
const assignment = {
  id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
  agent_id: reviewer.id,
  schedule: { kind: "interval", minutes: 5, timezone: "UTC" },
  comment: false,
  approve: false,
};
const repositories = [
  [one, "fixture/one", "22", "100", true],
  [two, "fixture/two", "44", "200", true],
  [off, "fixture/off", "22", "300", false],
];
const AVERY = "00000000-0000-0000-0000-000000000001";
const azure = "abababab-abab-4bab-8bab-abababababab";
const azureRepositories = [
  [
    azure,
    "contoso/Web/app",
    AVERY,
    "contoso/00000000-0000-0000-0000-0000000000b1/00000000-0000-0000-0000-0000000000c1",
    true,
    "azure_devops",
  ],
];

// Enabled, saved repositories with matching monitoring authorizations, plus
// idle health as the scheduler leaves it between occurrences.
async function seed(store, configured = repositories) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  settings.repositories = configured.map(
    ([id, name, account, repository, enabled, provider = "github"]) => ({
      id,
      provider,
      name,
      enabled,
      provider_account_id: account,
      provider_repository_id: repository,
      assignments: [{ ...assignment, id: assignment.id.replace("c", id[0]) }],
    }),
  );
  settings.repository_authorizations = Object.fromEntries(
    configured
      .filter(([, , , , enabled]) => enabled)
      .map(([id, name, account, repository]) => [
        id,
        {
          version: `v-${id}`,
          name,
          account_id: account,
          repository_id: repository,
        },
      ]),
  );
  await store("seed_settings", settings);
  await store("seed_queue_state", {
    jobs: [],
    reviews: [],
    publications: [],
    follow_ups: [],
    monitoring: {
      health: Object.fromEntries(
        configured.map(([id, name, account, repository, enabled]) => [
          id,
          {
            repository_id: id,
            name,
            schedule_key: "fixture",
            provider_account_id: account,
            provider_repository_id: repository,
            enabled,
            last_attempt: null,
            last_success: null,
            next_run: 1_900_000_000,
            schedule_available: enabled,
            last_failure: null,
            in_flight: false,
          },
        ]),
      ),
    },
  });
}

// Records scan requests while the real Store bridge serves them.
async function recordScans(page) {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__scans = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (["check_now", "scan_repository"].includes(command))
        window.__scans.push([command, args?.repositoryId ?? null]);
      return original(command, args);
    };
  });
}
const scans = (page) => page.evaluate(() => window.__scans);
const stateChanged = (page) =>
  page.evaluate(() =>
    window.__appEvent("pr-sniper:state-changed", { revision: 99 }),
  );

test("Running Scan queues every enabled repository once and keeps focus while scans run", async ({
  page,
  store,
}) => {
  await seed(store);
  await recordScans(page);
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Running", exact: true })
    .click();
  const scan = page.getByRole("button", {
    name: "Scan all repositories",
    exact: true,
  });
  await expect(scan).toBeVisible();
  await expect(scan).toBeEnabled();
  await expect(scan).toHaveAttribute("data-scanning", "false");
  await scan.focus();
  await page.keyboard.press("Enter");
  await expect(scan).toBeDisabled();
  await expect(scan).toHaveAttribute("data-scanning", "true");
  await page.evaluate(() => window.__settingsIdle());
  await stateChanged(page);
  expect(await store("repository_scans")).toEqual([one, two].sort());
  // In flight: another press coalesces in the UI and the pill keeps focus.
  await page.keyboard.press("Enter");
  await expect(scan).toBeDisabled();
  await expect(scan).toBeFocused();
  await scan.click({ force: true });
  expect(await scans(page)).toEqual([["check_now", null]]);
  const settings = (await store("snapshot")).settings;
  expect(settings.repositories.find((r) => r.id === off).enabled).toBe(false);
});

test("Running Scan is disabled while automation is paused", async ({
  page,
  store,
}) => {
  await seed(store);
  await store("set_automation_paused", { paused: true });
  await recordScans(page);
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Running", exact: true })
    .click();
  const scan = page.getByRole("button", {
    name: "Scan all repositories",
    exact: true,
  });
  await expect(scan).toBeVisible();
  await expect(scan).toBeDisabled();
  await scan.click({ force: true });
  expect(await scans(page)).toEqual([]);
  expect(await store("repository_scans")).toEqual([]);
});

test("repository Scan queues only that saved repository and never a draft or disabled one", async ({
  page,
  store,
}) => {
  await seed(store);
  await recordScans(page);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  const open = async (id, name) => {
    await page.locator(`[data-repository="${id}"]`).click();
    const modal = page.getByRole("dialog", { name, exact: true });
    return [
      modal,
      modal.getByRole("button", { name: `Scan ${name}`, exact: true }),
    ];
  };
  const back = (modal) => backFromEditor(page, modal);

  const [disabled, offScan] = await open(off, "fixture/off");
  await expect(offScan).toBeDisabled();
  await offScan.click({ force: true });
  await back(disabled);

  const [draft, draftScan] = await open(two, "fixture/two");
  await expect(draftScan).toBeEnabled();
  await draft.getByRole("switch", { name: "Review drafts" }).check();
  await expect(draftScan).toBeDisabled();
  await draftScan.click({ force: true });
  await draft.getByRole("switch", { name: "Review drafts" }).uncheck();
  await expect(draftScan).toBeEnabled();
  await draft
    .getByRole("button", { name: "Cancel repository changes", exact: true })
    .click();

  const [modal, scan] = await open(one, "fixture/one");
  await expect(scan).toBeEnabled();
  await scan.focus();
  await page.keyboard.press("Enter");
  await expect(scan).toHaveAttribute("data-scanning", "true");
  await expect(scan).toBeDisabled();
  await expect(scan).toBeFocused();
  await page.evaluate(() => window.__settingsIdle());
  expect(await scans(page)).toEqual([["scan_repository", one]]);
  expect(await store("repository_scans")).toEqual([one]);
  await back(modal);
  await expect(page.locator(`[data-repository="${one}"]`)).toBeFocused();
});

test("repository Scan queues a connected Azure DevOps repository", async ({
  page,
  store,
}) => {
  await seed(store, azureRepositories);
  await recordScans(page);
  await page.addInitScript((AVERY) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    const setup = {
      cli: { state: "ready", version: "2.90.0" },
      minimum: "2.54.0",
      installer: "homebrew",
      organization: {
        state: "tenant_known",
        name: "contoso",
        tenant: "00000000-0000-0000-0000-00000000000a",
      },
      account: {
        state: "connected",
        identity: {
          id: AVERY,
          display_name: "Avery Example",
          unique_name: "avery@contoso.example",
        },
      },
      checking: false,
    };
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command.startsWith("azure_devops_")
        ? window.__settingsTrack(Promise.resolve(structuredClone(setup)))
        : original(command, args);
  }, AVERY);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  await page.locator(`[data-repository="${azure}"]`).click();
  const modal = page.getByRole("dialog", {
    name: "contoso/Web/app",
    exact: true,
  });
  const scan = modal.getByRole("button", {
    name: "Scan contoso/Web/app",
    exact: true,
  });
  await expect(scan).toBeEnabled();
  await scan.click();
  await page.evaluate(() => window.__settingsIdle());
  expect(await scans(page)).toEqual([["scan_repository", azure]]);
  expect(await store("repository_scans")).toEqual([azure]);
});
