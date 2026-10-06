import { test, expect } from "./fixtures.mjs";
import { section, repositorySettings } from "./navigation.mjs";
import {
  providerFixture,
  repositoryPage,
  addByUrl,
  reviewer,
} from "./repository-provider-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

async function seed(store) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  settings.repositories = ["one", "two"].map((name, index) => ({
    id: `bbbbbbbb-bbbb-4bbb-8bbb-${String(index + 1).padStart(12, "0")}`,
    provider: "github",
    name: `fixture/${name}`,
    enabled: false,
    provider_account_id: "22",
    provider_repository_id: String((index + 1) * 100),
    assignments: [
      {
        id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        agent_id: reviewer.id,
        schedule: settings.defaults.schedule,
        comment: false,
      },
    ],
  }));
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

async function save(editor) {
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
}

for (const embedded of [true, false]) {
  test(`saved inheritance, override, global edits and restart (${embedded ? "Settings/Genie" : "standalone"})`, async ({
    page,
    store,
  }, info) => {
    const before = await seed(store);
    await providerFixture(page, store);
    await repositoryPage(page, store, embedded);
    let editor = await repositorySettings(page, "fixture/one");
    await expect(editor.getByLabel("Schedule", { exact: true })).toHaveValue(
      "inherit",
    );
    await expect(editor.locator("[data-effective-schedule]")).toContainText(
      "Every 15 minutes / UTC. Next scan:",
    );
    await expect(editor.locator("#repository-cron")).toBeHidden();
    await editor
      .getByLabel("Schedule", { exact: true })
      .selectOption("override");
    await editor
      .getByLabel("Cadence", { exact: true })
      .selectOption("0 9 * * MON-FRI");
    await editor
      .getByLabel("Time zone", { exact: true })
      .fill("America/New_York");
    await editor.getByLabel("Time zone", { exact: true }).focus();
    await expect(editor.getByLabel("Time zone", { exact: true })).toBeFocused();
    await editor.locator("[data-repository-schedule]").scrollIntoViewIfNeeded();
    await page.screenshot({ path: info.outputPath("repository-override.png") });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await save(editor);
    let saved = (await store("snapshot")).settings;
    expect(saved.repositories[0]).toMatchObject({
      enabled: false,
      overrides: {
        schedule: {
          kind: "cron",
          expression: "0 9 * * MON-FRI",
          timezone: "America/New_York",
        },
      },
    });
    expect(saved.repositories[0].assignments).toEqual(
      before.repositories[0].assignments,
    );
    expect(saved.repositories[1]).toEqual(before.repositories[1]);
    await expect(page.locator("[data-repository]").first()).toContainText(
      "Repository override: Weekdays at 09:00 / America/New_York. Next scan:",
    );
    const status = await store("repository_schedule_status", {
      repositoryId: saved.repositories[0].id,
    });
    expect(status.next_run).toBeGreaterThan(0);
    expect(status).toMatchObject({
      inherited: false,
      enabled: false,
      issue: null,
    });
    await section(page, "Preferences");
    await page.locator("#cron-helper").selectOption("0 * * * *");
    await page.locator("#global-timezone").fill("Asia/Tokyo");
    await page.locator("#save-settings").click();
    await expect(page.locator("#save-status")).toHaveText("All changes saved");
    await page.reload();
    await section(page, "Repositories");
    await expect(
      page.getByRole("button", { name: "fixture/one", exact: true }),
    ).toContainText("America/New_York");
    await expect(
      page.getByRole("button", { name: "fixture/two", exact: true }),
    ).toContainText("Use global schedule: Every hour / Asia/Tokyo");
    editor = await repositorySettings(page, "fixture/one");
    await expect(editor.getByLabel("Schedule", { exact: true })).toHaveValue(
      "override",
    );
    await expect(editor.getByLabel("Time zone", { exact: true })).toHaveValue(
      "America/New_York",
    );
    await editor
      .getByLabel("Schedule", { exact: true })
      .selectOption("inherit");
    await save(editor);
    saved = (await store("snapshot")).settings;
    expect(saved.repositories[0].overrides?.schedule).toBeUndefined();
    await expect(
      page.getByRole("button", { name: "fixture/one", exact: true }),
    ).toContainText("Use global schedule: Every hour / Asia/Tokyo");
  });
}

test("invalid and unsupported schedules preserve repository drafts and saved bytes without consulting Preferences drafts", async ({
  page,
  store,
}) => {
  const before = await seed(store);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  await section(page, "Preferences");
  await page.locator("#global-cron").fill("unrelated invalid draft");
  let editor = await repositorySettings(page, "fixture/one");
  await expect(editor.locator("[data-effective-schedule]")).toContainText(
    "Every 15 minutes / UTC",
  );
  await editor.getByLabel("Schedule", { exact: true }).selectOption("override");
  for (const [expression, timezone, message] of [
    ["not a cron", "UTC", "five-field"],
    ["0 */5 * * * *", "UTC", "five-field"],
    ["*/5 * * * *", "Not/A_Zone", "IANA"],
    ["0 0 31 2 *", "UTC", "no next occurrence"],
  ]) {
    await editor
      .getByLabel("Cron expression", { exact: true })
      .fill(expression);
    await editor.getByLabel("Time zone", { exact: true }).fill(timezone);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-resource-error]")).toContainText(
      message,
    );
    await expect(
      editor.getByLabel("Cron expression", { exact: true }),
    ).toHaveValue(expression);
    await expect(editor.getByLabel("Time zone", { exact: true })).toHaveValue(
      timezone,
    );
    expect((await store("snapshot")).settings).toEqual(before);
  }
  await editor
    .getByLabel("Cron expression", { exact: true })
    .fill("*/5 * * * *");
  await editor.getByLabel("Time zone", { exact: true }).fill("Asia/Tokyo");
  await editor
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  editor = await repositorySettings(page, "fixture/one");
  await expect(
    editor.getByLabel("Cron expression", { exact: true }),
  ).toHaveValue("*/5 * * * *");
  await save(editor);
  await section(page, "Preferences");
  await expect(page.locator("#global-cron")).toHaveValue(
    "unrelated invalid draft",
  );
  expect((await store("snapshot")).settings.defaults.schedule).toEqual(
    before.defaults.schedule,
  );
});

test("new repositories inherit; valid override Save honors intentional pause and works with a legacy global", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  settings.defaults.schedule = {
    kind: "interval",
    minutes: 7,
    timezone: "America/New_York",
  };
  await store("seed_settings", settings);
  await store("set_automation_paused", { paused: true });
  await providerFixture(page, store);
  await repositoryPage(page, store);
  const editor = await addByUrl(page);
  await expect(editor.getByLabel("Schedule", { exact: true })).toHaveValue(
    "inherit",
  );
  await expect(editor.locator("[data-effective-schedule]")).toContainText(
    "repair the saved global schedule",
  );
  await editor.getByLabel("Schedule", { exact: true }).selectOption("override");
  await editor
    .getByLabel("Cadence", { exact: true })
    .selectOption("*/15 * * * *");
  await editor
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assignment = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await assignment
    .getByLabel("Agent", { exact: true })
    .selectOption(reviewer.id);
  await assignment
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await save(editor);
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].enabled).toBe(true);
  const status = await store("repository_schedule_status", {
    repositoryId: saved.repositories[0].id,
  });
  expect(status).toMatchObject({
    inherited: false,
    paused: true,
    enabled: true,
    issue: null,
  });
  expect(status.schedule.timezone).toBe("America/New_York");
  expect((await store("saved_resources")).readiness.configuration_ready).toBe(
    true,
  );
  const reopened = await repositorySettings(page, "fixture/one");
  await reopened
    .getByLabel("Schedule", { exact: true })
    .selectOption("inherit");
  await reopened
    .getByRole("button", { name: "Cancel repository changes" })
    .click();
  expect((await store("snapshot")).settings).toEqual(saved);
});
