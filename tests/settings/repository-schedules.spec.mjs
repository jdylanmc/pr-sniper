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
    expect(status.next_run).toBeNull();
    expect(status.configured_next_run).toBeGreaterThan(0);
    expect(status).toMatchObject({
      inherited: false,
      enabled: false,
    });
    expect(status.issue).toContain("disabled");
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
  });
  expect(status.next_run).toBeNull();
  expect(status.configured_next_run).toBeGreaterThan(0);
  expect(status.issue).toContain("paused");
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

async function seedHealth(store, repository, changes = {}) {
  const health = {
    repository_id: repository.id,
    name: repository.name,
    schedule_key: "cron:*/15 * * * *:UTC",
    provider_account_id: repository.provider_account_id,
    provider_repository_id: repository.provider_repository_id,
    enabled: repository.enabled,
    last_attempt: 100,
    last_success: null,
    next_run: 0,
    schedule_available: false,
    last_failure: "network",
    in_flight: false,
    operation: {
      id: "eeeeeeee-eeee-4eee-8eee-000000000001",
      provider: "github",
      account_id: "22",
      configuration_id: repository.id,
      repository_id: "100",
      pull_request_id: null,
      head_sha: null,
      trigger_policy: "[[],true]",
      operation_type: "repository_poll",
      state: "manual_retry",
      attempt_count: 4,
      initial_attempt_at: 100,
      retry_deadline: 1000,
      next_attempt_at: null,
      failure: "network",
      attempted_mutation: null,
      pending_review_id: null,
      owned_thread_id: null,
      triggering_external_comment_id: null,
      confirmed_receipt: null,
    },
    ...changes,
  };
  await store("seed_queue_state", {
    jobs: [],
    reviews: [],
    publications: [],
    follow_ups: [],
    monitoring: { health: { [repository.id]: health } },
  });
}

test("native IPC and shared UI do not promise a scan for exhausted retry or disconnected health after restart", async ({
  page,
  store,
}) => {
  const saved = await seed(store);
  saved.repositories[0].enabled = true;
  await store("seed_settings", saved);
  await seedHealth(store, saved.repositories[0]);
  const before = (await store("snapshot")).settings;
  const status = await store("repository_schedule_status", {
    repositoryId: saved.repositories[0].id,
  });
  expect(status.next_run).toBeNull();
  expect(status.configured_next_run).toBeGreaterThan(0);
  expect(status.issue).toContain("Open Status and Retry");
  await providerFixture(page, store);
  await repositoryPage(page, store);
  const row = page.getByRole("button", { name: "fixture/one", exact: true });
  await expect(row).toContainText("Next scan: Not scheduled.");
  await expect(row).toContainText("Open Status and Retry");
  await expect(row).toContainText("Configured occurrence:");
  await expect(row).not.toContainText(/Next scan: [A-Z][a-z]{2} \d/);
  const editor = await repositorySettings(page, "fixture/one");
  await expect(editor.locator("[data-effective-schedule]")).toContainText(
    "suspended after failed attempts",
  );
  await editor
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await page.reload();
  await section(page, "Repositories");
  await expect(row).toContainText("Next scan: Not scheduled.");
  expect((await store("snapshot")).settings).toEqual(before);
  const exhausted = (await store("monitoring_snapshot")).health[0];
  expect(exhausted.operation).toMatchObject({
    state: "manual_retry",
    attempt_count: 4,
  });
  expect(exhausted.next_run).toBe(0);
  await seedHealth(store, saved.repositories[0], {
    last_failure: "account_disconnected",
    operation: null,
  });
  await page.reload();
  await section(page, "Repositories");
  await expect(row).toContainText("Reconnect the acting GitHub account");
  await expect(row).toContainText("Next scan: Not scheduled.");
  await seedHealth(store, saved.repositories[0], {
    schedule_available: true,
    next_run: 2000000000,
    last_failure: null,
    operation: null,
  });
  const healthy = await store("repository_schedule_status", {
    repositoryId: saved.repositories[0].id,
  });
  expect(healthy.next_run).toBe(2000000000);
  expect(healthy.issue).toBeNull();
  await page.reload();
  await section(page, "Repositories");
  await expect(row).not.toContainText("Not scheduled");
  await expect(row).not.toContainText("cadence preview only");
});

test("impossible inherited cron is unconfigured; override removal and enable failure retain drafts, bytes and neighbors", async ({
  page,
  store,
}) => {
  const settings = await seed(store);
  settings.defaults.schedule = {
    kind: "cron",
    expression: "0 0 31 2 *",
    timezone: "UTC",
  };
  settings.repositories[0].enabled = true;
  await store("seed_settings", settings);
  const before = (await store("snapshot")).settings;
  const readiness = (await store("saved_resources")).readiness;
  expect(readiness.configuration_ready).toBe(false);
  expect(readiness.repositories[0].issues.join(" ")).toContain(
    "no next occurrence",
  );
  const unconfigured = await store("repository_schedule_status", {
    repositoryId: settings.repositories[0].id,
  });
  expect(unconfigured).toMatchObject({
    next_run: null,
    configured_next_run: null,
  });
  await providerFixture(page, store);
  await repositoryPage(page, store);
  let editor = await repositorySettings(page, "fixture/one");
  await expect(editor.locator("[data-effective-schedule]")).toContainText(
    "no next occurrence",
  );
  await expect(editor.locator("[data-effective-schedule]")).not.toContainText(
    "Configured occurrence:",
  );
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor.locator("[data-resource-error]")).toContainText(
    "no next occurrence",
  );
  expect((await store("snapshot")).settings).toEqual(before);
  await editor.getByLabel("Schedule", { exact: true }).selectOption("override");
  await editor
    .getByLabel("Cadence", { exact: true })
    .selectOption("*/15 * * * *");
  await save(editor);
  const valid = (await store("snapshot")).settings;
  expect((await store("saved_resources")).readiness.configuration_ready).toBe(
    true,
  );
  expect(valid.repositories[1]).toEqual(before.repositories[1]);
  await page.reload();
  await section(page, "Repositories");
  editor = await repositorySettings(page, "fixture/one");
  await editor.getByLabel("Schedule", { exact: true }).selectOption("inherit");
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor.locator("[data-resource-error]")).toContainText(
    "no next occurrence",
  );
  await expect(editor.getByLabel("Schedule", { exact: true })).toHaveValue(
    "inherit",
  );
  expect((await store("snapshot")).settings).toEqual(valid);
  await editor.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(editor.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  await expect(editor.getByLabel("Schedule", { exact: true })).toHaveValue(
    "inherit",
  );
  await save(editor);
  const disabled = (await store("snapshot")).settings;
  expect(disabled.repositories[0].enabled).toBe(false);
  expect(disabled.repositories[0].overrides?.schedule).toBeUndefined();
  editor = await repositorySettings(page, "fixture/one");
  await editor.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(editor.locator("[data-resource-error]")).toContainText(
    "no next occurrence",
  );
  await expect(
    editor.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  expect((await store("snapshot")).settings).toEqual(disabled);
});

test("saved-only monitoring toggles retain cadence and Preferences drafts and supersede stale next-scan replies", async ({
  page,
  store,
  ipc,
}) => {
  const settings = await seed(store);
  settings.repositories[0].enabled = true;
  settings.repositories[0].overrides = {
    schedule: { kind: "cron", expression: "0 * * * *", timezone: "UTC" },
  };
  await store("seed_settings", settings);
  await seedHealth(store, settings.repositories[0], {
    schedule_key: "cron:0 * * * *:UTC",
    schedule_available: true,
    next_run: 2000000000,
    last_failure: null,
    operation: null,
  });
  await providerFixture(page, store);
  await repositoryPage(page, store);
  await section(page, "Preferences");
  await page.locator("#global-cron").fill("invalid global draft");
  await page.locator("#global-capacity").fill("9");
  await section(page, "Repositories");
  await page.evaluate(() => window.__settingsIdle());
  const held = ipc.holdNext("repository_schedule_status");
  try {
    const editor = await repositorySettings(page, "fixture/one");
    await held.arrived;
    await editor
      .getByLabel("Cron expression", { exact: true })
      .fill("invalid repository draft");
    const switchControl = editor.getByRole("switch", {
      name: "Monitor fixture/one",
    });
    await switchControl.click();
    await expect(
      editor.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Disabled");
    await expect(editor.locator("[data-effective-schedule]")).toContainText(
      "Next scan: Not scheduled.",
    );
    await expect(editor.locator("[data-effective-schedule]")).toContainText(
      "Repository is disabled",
    );
    held.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(editor.locator("[data-effective-schedule]")).toContainText(
      "Repository is disabled",
    );
    await expect(
      editor.getByLabel("Cron expression", { exact: true }),
    ).toHaveValue("invalid repository draft");
    const disabled = (await store("snapshot")).settings;
    expect(disabled.repositories[0].enabled).toBe(false);
    expect(disabled.repositories[0].overrides.schedule).toEqual(
      settings.repositories[0].overrides.schedule,
    );
    expect(disabled.repositories[1]).toEqual(settings.repositories[1]);
    expect(disabled.defaults).toEqual(settings.defaults);
    expect(disabled.capacity).toBe(settings.capacity);
    await store("set_automation_paused", { paused: true });
    await switchControl.click();
    await expect(
      editor.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Enabled");
    await expect(editor.locator("[data-effective-schedule]")).toContainText(
      "Global monitoring is paused",
    );
    await expect(
      editor.getByLabel("Cron expression", { exact: true }),
    ).toHaveValue("invalid repository draft");
    const enabled = (await store("snapshot")).settings;
    expect(enabled.repositories[0].enabled).toBe(true);
    expect(enabled.repositories[0].overrides.schedule).toEqual(
      settings.repositories[0].overrides.schedule,
    );
    expect(enabled.defaults).toEqual(settings.defaults);
    expect(enabled.capacity).toBe(settings.capacity);
    expect((await store("automation_snapshot")).paused).toBe(true);
    await editor
      .getByRole("button", { name: "Close dialog", exact: true })
      .click();
    await section(page, "Preferences");
    await expect(page.locator("#global-cron")).toHaveValue(
      "invalid global draft",
    );
    await expect(page.locator("#global-capacity")).toHaveValue("9");
  } finally {
    held.release();
  }
});
