import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import {
  providerFixture,
  repositoryPage,
  addByUrl,
  reviewer,
} from "./repository-provider-fixture.mjs";

const repoId = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const neighborId = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const assignment = {
  id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
  agent_id: reviewer.id,
  schedule: { kind: "interval", minutes: 5, timezone: "UTC" },
  comment: false,
  approve: false,
};

async function seed(store, assigned = true) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  settings.repositories = [
    {
      id: repoId,
      provider: "github",
      name: "fixture/one",
      enabled: false,
      provider_account_id: "22",
      provider_repository_id: "100",
      assignments: assigned ? [assignment] : [],
    },
    {
      id: neighborId,
      provider: "github",
      name: "fixture/two",
      enabled: false,
      provider_account_id: "44",
      provider_repository_id: "200",
      assignments: [assignment],
    },
  ];
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

const toggle = (page) =>
  page.locator(`.repository-list [data-toggle-repository="${repoId}"]`);
const editor = (page) =>
  page.getByRole("dialog", { name: "Settings for fixture/one", exact: true });
const openEditor = async (page) => {
  await page.locator(`[data-repository="${repoId}"]`).click();
  return editor(page);
};

for (const embedded of [true, false]) {
  test(`persistent repository switch keeps global pause separate (${embedded ? "panel" : "standalone"})`, async ({
    page,
    store,
  }, info) => {
    const original = await seed(store);
    await store("set_automation_paused", { paused: true });
    const fixture = await providerFixture(page, store);
    await repositoryPage(page, store, embedded);
    await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
    await toggle(page).focus();
    await page.keyboard.press("Space");
    await expect(toggle(page)).toHaveAttribute("aria-checked", "true");
    await expect(toggle(page)).toBeFocused();
    await expect(page.locator(`[data-repository="${repoId}"]`)).toContainText(
      "Global Monitoring paused",
    );
    let saved = (await store("snapshot")).settings;
    expect(saved.repositories[0].enabled).toBe(true);
    expect(saved.repository_authorizations[repoId]).toMatchObject({
      account_id: "22",
      repository_id: "100",
    });
    expect(saved.repositories[1]).toEqual(original.repositories[1]);
    expect(saved.agents).toEqual(original.agents);
    expect(saved.repositories[0].assignments[0].comment).toBe(false);
    expect((await store("automation_snapshot")).paused).toBe(true);
    const modal = await openEditor(page);
    await expect(
      modal.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Enabled");
    await expect(modal.getByText(/on Save/)).toHaveCount(0);
    await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
    await expect(
      modal.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Disabled");
    saved = (await store("snapshot")).settings;
    expect(saved.repositories[0].enabled).toBe(false);
    expect(saved.repository_authorizations[repoId]).toBeNull();
    await modal
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(modal).toHaveCount(0);
    expect((await store("automation_snapshot")).paused).toBe(true);
    await page.reload();
    if (embedded)
      await page
        .getByRole("navigation", { name: "Application destinations" })
        .getByRole("button", { name: "Settings", exact: true })
        .click();
    await section(page, "Repositories");
    await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    await page.setViewportSize({ width: 320, height: 300 });
    await toggle(page).focus();
    await expect(toggle(page)).toBeFocused();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: info.outputPath("disabled-compact-high-contrast.png"),
    });
    expect(
      fixture.calls.some((call) =>
        /preview|activation|pull_requests/.test(call.command),
      ),
    ).toBe(false);
  });
}

test("quick editor switch commits only saved configuration and retains repository and Preferences drafts", async ({
  page,
  store,
}) => {
  const original = await seed(store);
  await providerFixture(page, store);
  await repositoryPage(page, store, false);
  await section(page, "Preferences");
  await page.locator("#global-cron").fill("invalid draft cron");
  await page.locator("#global-capacity").fill("9");
  await section(page, "Repositories");
  const modal = await openEditor(page);
  await modal.locator("[data-reviewer-trigger]").selectOption("off");
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Enabled",
  );
  await expect(modal.locator("[data-reviewer-trigger]")).toHaveValue("off");
  let saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].overrides?.reviewer_assignment).toBeUndefined();
  expect(saved.defaults).toEqual(original.defaults);
  expect(saved.capacity).toBe(original.capacity);
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  await modal
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].enabled).toBe(false);
  expect(saved.repositories[0].overrides.reviewer_assignment).toBe(false);
  expect(saved.repositories[1]).toEqual(original.repositories[1]);
  await section(page, "Preferences");
  await expect(page.locator("#global-cron")).toHaveValue("invalid draft cron");
  await expect(page.locator("#global-capacity")).toHaveValue("9");
});

test("invalid quick enable reports failure without optimistic enabled state or losing row focus", async ({
  page,
  store,
}) => {
  const original = await seed(store, false);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  await toggle(page).click();
  await expect(page.locator("#error")).toContainText(
    "assign at least one saved Agent",
  );
  await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
  await expect(toggle(page)).toBeFocused();
  expect((await store("snapshot")).settings).toEqual(original);
  const modal = await openEditor(page);
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-resource-error]")).toContainText(
    "assign at least one saved Agent",
  );
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  expect((await store("snapshot")).settings).toEqual(original);
});

test("compare-save conflicts retain the repository draft and never overwrite newer saved state", async ({
  page,
  store,
}) => {
  const original = await seed(store);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  const modal = await openEditor(page);
  await modal.locator("[data-reviewer-trigger]").selectOption("off");
  const newer = structuredClone(original.repositories[0]);
  newer.watched_authors = [{ id: "11", login: "newer-author" }];
  await store("save_resource", {
    edit: {
      kind: "repository",
      id: repoId,
      expected: original.repositories[0],
      value: newer,
    },
  });
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-resource-error]")).toContainText(
    "Resource changed",
  );
  await expect(modal.locator("[data-reviewer-trigger]")).toHaveValue("off");
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  expect((await store("snapshot")).settings.repositories[0]).toEqual(newer);
});

test("account loss stays separate from enablement and allows a durable off switch without provider requests", async ({
  page,
  store,
}) => {
  const original = await seed(store);
  await store("save_resource", {
    edit: {
      kind: "repository",
      id: repoId,
      expected: original.repositories[0],
      value: { ...original.repositories[0], enabled: true },
    },
  });
  const fixture = await providerFixture(page, store);
  fixture.accounts[0] = {
    ...fixture.accounts[0],
    state: "reconnect_required",
    reason: "expired",
  };
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "save_resource" && args.edit?.value?.enabled)
        return Promise.reject({
          stage: "session",
          account_id: "22",
          error: "signed_out",
        });
      return invoke(command, args);
    };
  });
  await repositoryPage(page, store);
  const row = page.locator(`[data-repository="${repoId}"]`);
  await expect(row).toContainText("Enabled");
  await expect(row).toContainText("Reconnect account");
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(true);
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await toggle(page).click();
  await expect(page.locator("#error")).toContainText(
    "Connect the PR Sniper GitHub OAuth App",
  );
  await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  expect(
    fixture.calls.some((call) =>
      /resolve|preview|activation/.test(call.command),
    ),
  ).toBe(false);
});

test("new rows stay disabled until valid Save and an explicit off choice survives assignment saves and reopening", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  await store("seed_settings", settings);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  let modal = await addByUrl(page);
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).toBeChecked();
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).uncheck();
  await modal
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assign = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await assign.getByLabel("Agent", { exact: true }).selectOption(reviewer.id);
  await assign
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await expect(assign).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await modal
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await page.locator("[data-repository]").click();
  modal = editor(page);
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  await modal
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
});
