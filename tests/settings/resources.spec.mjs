import { test, expect } from "./fixtures.mjs";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  section,
  seedAgent,
  editAgent,
  repositorySettings,
  closeDialog,
  assignment,
  saveAssignment,
} from "./navigation.mjs";

for (const [scenario, expression, timezone] of [
  ["invalid cron", "invalid", "UTC"],
  ["invalid timezone", "0 9 * * MON-FRI", "Mars/Olympus_Mons"],
  ["different valid schedule", "0 9 * * MON-FRI", "America/New_York"],
]) {
  test(`new assignment uses saved schedule with ${scenario} in unsaved Preferences`, async ({
    page,
    store,
  }) => {
    await seedAgent(store);
    await store("save_repository", { repository: "fixture/schedules" });
    const settings = (await store("snapshot")).settings;
    settings.defaults.schedule = {
      kind: "cron",
      expression: "5 * * * *",
      timezone: "UTC",
    };
    const existing = {
      id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      agent_id: settings.agents[0].id,
      schedule: { kind: "interval", minutes: 7, timezone: "UTC" },
      comment: false,
      approve: false,
    };
    settings.repositories[0].assignments = [existing];
    await store("seed_settings", settings);
    await page.goto("/?view=settings");
    await section(page, "Preferences");
    await page.locator("#global-cron").fill(expression);
    await page.locator("#global-timezone").fill(timezone);
    await saveAssignment(page, await assignment(page, "fixture/schedules"));
    let saved = (await store("saved_resources")).settings;
    expect(saved.defaults).toEqual(settings.defaults);
    expect(saved.repositories[0].assignments).toHaveLength(2);
    expect(saved.repositories[0].assignments[0]).toEqual(existing);
    expect(saved.repositories[0].assignments[1].schedule).toEqual(
      settings.defaults.schedule,
    );
    const modal = await assignment(page, "fixture/schedules", 0);
    await modal.getByRole("checkbox", { name: /^Comment/ }).check();
    await saveAssignment(page, modal);
    saved = (await store("saved_resources")).settings;
    expect(saved.repositories[0].assignments[0]).toMatchObject({
      ...existing,
      comment: true,
    });
    expect(saved.defaults).toEqual(settings.defaults);
    await section(page, "Preferences");
    await expect(page.locator("#global-cron")).toHaveValue(expression);
    await expect(page.locator("#global-timezone")).toHaveValue(timezone);
    await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
    await expect(
      page.getByRole("button", { name: "Save preferences", exact: true }),
    ).toBeEnabled();
    await page.reload();
    expect((await store("saved_resources")).settings).toEqual(saved);
    await section(page, "Preferences");
    await expect(page.locator("#global-cron")).toHaveValue(
      settings.defaults.schedule.expression,
    );
    await expect(page.locator("#global-timezone")).toHaveValue(
      settings.defaults.schedule.timezone,
    );
  });
}

test("Agent saves immediately while unrelated preference and repository drafts stay unsaved", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/one" });
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("9");
  const repository = await repositorySettings(page, "fixture/one");
  await repository
    .getByLabel("Review start", { exact: true })
    .selectOption("automatic");
  await closeDialog(page);
  const modal = await editAgent(page);
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("Saved resource prompt.");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  const saved = (await store("saved_resources")).settings;
  expect(saved.agents[0].prompt).toBe("Saved resource prompt.");
  expect(saved.capacity).toBe(4);
  expect(
    saved.repositories[0].overrides?.automatic_agent_start,
  ).toBeUndefined();
  await section(page, "Preferences");
  await expect(page.locator("#global-capacity")).toHaveValue("9");
  const pending = await repositorySettings(page, "fixture/one");
  await expect(pending.getByLabel("Review start", { exact: true })).toHaveValue(
    "automatic",
  );
  await pending
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(pending).toHaveCount(0);
  expect((await store("snapshot")).settings.capacity).toBe(4);
  await section(page, "Preferences");
  await page
    .getByRole("button", { name: "Save preferences", exact: true })
    .click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await page.reload();
  expect((await store("snapshot")).settings).toMatchObject({
    capacity: 9,
    agents: [{ prompt: "Saved resource prompt." }],
    repositories: [{ overrides: { automatic_agent_start: true } }],
  });
});

for (const failure of ["write", "conflict"]) {
  test(`resource ${failure} keeps the Agent form, draft and valid saved state`, async ({
    page,
    store,
    dataRoot,
  }) => {
    await seedAgent(store);
    await page.goto("/?view=settings");
    const modal = await editAgent(page);
    await modal
      .getByRole("textbox", { name: "Prompt", exact: true })
      .fill("Keep my failed draft.");
    if (failure === "write")
      await mkdir(join(dataRoot, "config/settings.json.tmp"));
    else {
      const settings = (await store("snapshot")).settings;
      settings.agents[0].prompt = "Other window.";
      await store("seed_settings", settings);
    }
    const before = await readFile(join(dataRoot, "config/settings.json"));
    await modal
      .getByRole("button", { name: "Save agent", exact: true })
      .click();
    await expect(modal.getByRole("alert")).toBeVisible();
    await expect(
      modal.getByRole("textbox", { name: "Prompt", exact: true }),
    ).toHaveValue("Keep my failed draft.");
    await expect(
      modal.getByRole("button", { name: "Save agent", exact: true }),
    ).toBeEnabled();
    expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
      before,
    );
  });
}

test("legacy single doctrines become ordered selections, rename preserves references, delete requires repair", async ({
  page,
  store,
}) => {
  const settings = await seedAgent(store);
  settings.doctrines = [
    { title: "First", body: "First body." },
    { title: "Second", body: "Second body." },
  ];
  settings.agents[0].doctrine = "Second";
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  let modal = await editAgent(page);
  await expect(
    modal.getByRole("checkbox", { name: "Second", exact: true }),
  ).toBeChecked();
  await modal.getByRole("checkbox", { name: "First", exact: true }).check();
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].doctrines).toEqual([
    "Second",
    "First",
  ]);
  await section(page, "Doctrines");
  await page
    .locator(".doctrine-card")
    .first()
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  modal = page.getByRole("dialog", { name: "Edit doctrine", exact: true });
  await modal.getByLabel("Title", { exact: true }).fill("Renamed");
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].doctrines).toEqual([
    "Second",
    "Renamed",
  ]);
  await page
    .locator(".doctrine-card")
    .first()
    .getByRole("button", { name: "Delete", exact: true })
    .click();
  await expect(page.locator("#error")).toContainText("used by an Agent");
  expect((await store("snapshot")).settings.doctrines).toHaveLength(2);
  modal = await editAgent(page);
  for (const checkbox of await modal.locator("[name=doctrine]").all())
    await checkbox.uncheck();
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  await page.reload();
  expect((await store("snapshot")).settings.agents[0].doctrines).toEqual([]);
});

test("primary and independent opt-ins are reachable without scoped polling or legacy permission escalation", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/primary" });
  const settings = (await store("snapshot")).settings;
  const assignment = {
    id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
    agent_id: settings.agents[0].id,
    schedule: { kind: "interval", minutes: 7, timezone: "UTC" },
    comment: false,
    approve: true,
  };
  settings.repositories[0].assignments = [assignment];
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  let parent = await repositorySettings(page, "fixture/primary");
  await parent
    .locator(".assignment-row")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  let modal = page.getByRole("dialog", {
    name: "Edit assignment",
    exact: true,
  });
  const primary = modal.getByRole("checkbox", { name: /^Primary/ });
  await expect(primary).toBeChecked();
  await expect(primary).toBeDisabled();
  await expect(
    modal.getByRole("checkbox", { name: /^Approve/ }),
  ).not.toBeChecked();
  await expect(
    modal.getByRole("checkbox", { name: /^Merge/ }),
  ).not.toBeChecked();
  await expect(
    modal.locator(
      "[name=frequency],[name=timezone],[name=cron],[name=minutes]",
    ),
  ).toHaveCount(0);
  await modal.getByRole("checkbox", { name: /^Merge/ }).check();
  await modal
    .getByRole("button", { name: "Save assignment", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  let saved = await store("saved_resources");
  expect(saved.settings.repositories[0].assignments[0]).toMatchObject({
    ...assignment,
    actions: { approve: false, merge: true },
  });
  expect(saved.readiness.repositories[0].assignments[0][1]).toEqual({
    primary: true,
    comment: false,
    approve: false,
    merge: true,
  });
  await closeDialog(page);
  saved.settings.repositories[0].assignments.push({
    ...assignment,
    id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
    approve: false,
  });
  await store("seed_settings", saved.settings);
  await page.reload();
  parent = await repositorySettings(page, "fixture/primary");
  await parent
    .locator(".assignment-row")
    .nth(1)
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  modal = page.getByRole("dialog", { name: "Edit assignment", exact: true });
  await modal.getByRole("checkbox", { name: /^Primary/ }).check();
  await expect(
    modal.getByRole("checkbox", { name: /^Approve/ }),
  ).not.toBeChecked();
  await expect(
    modal.getByRole("checkbox", { name: /^Merge/ }),
  ).not.toBeChecked();
  await modal
    .getByRole("button", { name: "Save assignment", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  saved = await store("saved_resources");
  expect(saved.readiness.repositories[0].primary_assignment_id).toBe(
    "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
  );
  expect(saved.readiness.repositories[0].assignments[0][1].merge).toBe(false);
});

test("global cron helper and capacity save through the shared readiness surface without activation", async ({
  page,
  store,
}) => {
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  await expect(page.locator("#global-cron")).toHaveValue("*/15 * * * *");
  await expect(page.locator("#global-capacity")).toHaveValue("4");
  await page.locator("#cron-helper").selectOption("0 9 * * MON-FRI");
  await page.locator("#global-timezone").fill("America/New_York");
  await page.locator("#global-capacity").fill("6");
  await page
    .getByRole("button", { name: "Save preferences", exact: true })
    .click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  const resources = await store("saved_resources");
  expect(resources.settings.defaults.schedule).toEqual({
    kind: "cron",
    expression: "0 9 * * MON-FRI",
    timezone: "America/New_York",
  });
  expect(resources.settings.capacity).toBe(6);
  expect(resources.settings.defaults.automatic_agent_start).toBe(false);
  expect(resources.readiness.configuration_ready).toBe(false);
  await expect(page.locator("[data-readiness]")).toContainText(
    "Saved setup incomplete",
  );
  await page.locator("#global-cron").fill("invalid");
  await page
    .getByRole("button", { name: "Save preferences", exact: true })
    .click();
  await expect(page.locator("#error")).toContainText("five-field cron");
  await expect(page.locator("#global-cron")).toHaveValue("invalid");
  expect((await store("saved_resources")).settings).toEqual(resources.settings);
});
