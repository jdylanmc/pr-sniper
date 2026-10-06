import { test, expect } from "./fixtures.mjs";
import { fixtureAgent, section, repositorySettings } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

const id = (number) =>
  `00000000-0000-4000-8000-${String(number).padStart(12, "0")}`;
const accounts = [
  {
    provider: "github",
    account_id: "22",
    login: "repository-owner",
    state: "connected",
  },
  {
    provider: "github",
    account_id: "44",
    login: "second-owner",
    state: "connected",
  },
];
const modal = (page, name) => page.getByRole("dialog", { name, exact: true });
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const row = (page, name = "fixture/compact") =>
  page.getByRole("button", { name, exact: true });
const close = async (dialog) =>
  dialog.getByRole("button", { name: "Close dialog", exact: true }).click();
const save = async (dialog) => {
  await dialog
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(dialog).toHaveCount(0);
};

async function seed(store, count = 1) {
  const settings = (await store("snapshot")).settings;
  settings.launch_at_login = true;
  settings.defaults.schedule = {
    kind: "cron",
    expression: "7 */2 * * MON-FRI",
    timezone: "America/New_York",
  };
  settings.agents = Array.from({ length: count }, (_, index) => ({
    ...fixtureAgent,
    id: id(100 + index),
    name: `Reviewer ${index + 1}`,
    ai_account: { provider: "copilot", account_id: "33" },
  }));
  settings.repositories = [
    {
      id: id(1),
      provider: "github",
      name: "fixture/compact",
      enabled: false,
      provider_account_id: "22",
      provider_repository_id: "100",
      watched_authors: [{ id: "11", login: "watched-person" }],
      assignments: [],
    },
    { id: id(2), provider: "github", name: "fixture/neighbor", enabled: false },
  ];
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

async function provider(
  page,
  handler = () => {
    throw "Unexpected provider command";
  },
) {
  const calls = [];
  await page.exposeFunction("__repositoryProvider", async (command, args) => {
    calls.push({ command, args });
    try {
      return { ok: await handler(command, args) };
    } catch (error) {
      return { error: typeof error === "string" ? error : error.message };
    }
  });
  await page.addInitScript((accounts) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    const idle = window.__settingsIdle;
    const pending = new Set();
    window.__settingsIdle = async () => {
      do {
        await idle();
        await Promise.allSettled([...pending]);
      } while (pending.size);
      await idle();
    };
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "github_auth_state")
        return { accounts, flow: { state: "idle" } };
      if (
        [
          "resolve_provider_repository",
          "list_provider_repositories",
          "resolve_provider_person",
          "verify_provider_connection",
          "preview_monitoring_activation",
          "apply_monitoring_activation",
          "cancel_monitoring_activation",
        ].includes(command)
      ) {
        const request = window
          .__repositoryProvider(command, args ?? {})
          .then((result) => {
            if ("error" in result) throw result.error;
            return result.ok;
          });
        const settled = request.finally(() => pending.delete(settled));
        pending.add(settled);
        return settled;
      }
      return original(command, args);
    };
  }, accounts);
  return calls;
}

async function start(page, store) {
  // Initialize the owned panel fixture before concurrent UI snapshot readers.
  await store("fixture_show_panel");
  await page.goto("/");
  await page.evaluate(() => window.__settingsIdle());
  await tab(page, "Settings").click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await section(page, "Repositories");
  await page.evaluate(() => window.__settingsIdle());
}

async function capture(page, testInfo, name) {
  await page.evaluate(() => window.__settingsIdle());
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({ path: testInfo.outputPath(`${name}.png`) });
}

test.use({ viewport: { width: 408, height: 744 } });

test("repository saves leave unsaved Preferences and the saved global schedule independent", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seed(store);
  await provider(page);
  await start(page, store);
  await section(page, "Preferences");
  await page.locator("#global-cron").fill("invalid unsaved cron");
  await page.locator("#global-capacity").fill("9");
  const editor = await repositorySettings(page, "fixture/compact");
  await expect(editor.locator("[data-global-schedule]")).toContainText(
    "7 */2 * * MON-FRI",
  );
  await expect(editor.locator("[data-global-schedule]")).toContainText(
    "America/New_York",
  );
  await expect(
    editor.locator("[name=cron],[name=minutes],[name=timezone],#global-cron"),
  ).toHaveCount(0);
  await editor.locator("[data-global-schedule]").scrollIntoViewIfNeeded();
  await capture(page, testInfo, "saved-global-schedule");
  await editor
    .getByLabel("Reviewer requests", { exact: true })
    .selectOption("off");
  await expect(
    editor.getByRole("button", { name: "Configure scope", exact: true }),
  ).toHaveCount(0);
  await save(editor);
  const saved = (await store("snapshot")).settings;
  expect(saved.defaults).toEqual(initial.defaults);
  expect(saved.capacity).toBe(initial.capacity);
  expect(saved.repositories[1]).toEqual(initial.repositories[1]);
  await section(page, "Preferences");
  await expect(page.locator("#global-cron")).toHaveValue(
    "invalid unsaved cron",
  );
  await expect(page.locator("#global-capacity")).toHaveValue("9");
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await expect(page.locator("#global-cron")).toHaveValue(
    initial.defaults.schedule.expression,
  );
  expect((await store("snapshot")).settings).toEqual(saved);
});

test("all Comment Approve Merge combinations remain independent on the automatic sole primary", async ({
  page,
  store,
}) => {
  const initial = await seed(store);
  initial.repositories[0].assignments = [
    {
      id: id(200),
      agent_id: id(100),
      schedule: initial.defaults.schedule,
      comment: false,
      approve: true,
    },
  ];
  await store("seed_settings", initial);
  await provider(page);
  await start(page, store);
  const parent = await repositorySettings(page, "fixture/compact");
  for (let mask = 0; mask < 8; mask++) {
    await parent
      .locator(".assignment-row")
      .getByRole("button", { name: "Edit", exact: true })
      .click();
    const editor = modal(page, "Edit assignment");
    await expect(
      editor.getByRole("checkbox", { name: /^Primary/ }),
    ).toBeChecked();
    await expect(
      editor.getByRole("checkbox", { name: /^Primary/ }),
    ).toBeDisabled();
    if (!mask)
      await expect(
        editor.getByRole("checkbox", { name: /^Approve/ }),
      ).not.toBeChecked();
    const permissions = {
      comment: !!(mask & 1),
      approve: !!(mask & 2),
      merge: !!(mask & 4),
    };
    for (const [name, checked] of Object.entries(permissions))
      await editor.locator(`[name=${name}]`).setChecked(checked);
    await editor.getByRole("button", { name: "Save assignment" }).click();
    await expect(editor).toHaveCount(0);
    const saved = await store("saved_resources");
    expect(saved.readiness.repositories[0].assignments).toEqual([
      [id(200), { primary: true, ...permissions }],
    ]);
    expect(saved.settings.repositories[0].assignments[0]).toEqual({
      ...initial.repositories[0].assignments[0],
      comment: permissions.comment,
      actions: { approve: permissions.approve, merge: permissions.merge },
    });
    expect(saved.settings.defaults).toEqual(initial.defaults);
    expect(saved.settings.agents).toEqual(initial.agents);
  }
});

test("seven UI assignments stay distinct and explicit multiple-primary selection enables no permissions", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seed(store, 7);
  await provider(page);
  await start(page, store);
  let parent = await repositorySettings(page, "fixture/compact");
  for (let index = 0; index < 7; index++) {
    await parent
      .getByRole("button", { name: "Assign agent", exact: true })
      .click();
    const editor = modal(page, "Assign agent");
    await testInfo.attach(`assignment-${index}-accessibility`, {
      body: await editor.ariaSnapshot(),
      contentType: "text/plain",
    });
    await editor
      .getByRole("combobox", { name: "Agent", exact: true })
      .selectOption(id(100 + index));
    await editor.locator("[name=comment]").uncheck();
    await expect(editor.locator("[name=approve]")).not.toBeChecked();
    await expect(editor.locator("[name=merge]")).not.toBeChecked();
    await editor
      .getByRole("button", { name: "Assign agent", exact: true })
      .click();
    await expect(editor).toHaveCount(0);
  }
  let saved = await store("saved_resources");
  const assignments = saved.settings.repositories[0].assignments;
  expect(assignments).toHaveLength(7);
  expect(new Set(assignments.map((assignment) => assignment.id)).size).toBe(7);
  expect(
    new Set(assignments.map((assignment) => assignment.agent_id)).size,
  ).toBe(7);
  expect(saved.readiness.repositories[0].primary_assignment_id).toBeNull();
  expect(saved.settings.defaults).toEqual(initial.defaults);
  for (const index of [3, 5]) {
    await parent
      .locator(".assignment-row")
      .nth(index)
      .getByRole("button", { name: "Edit", exact: true })
      .click();
    const editor = modal(page, "Edit assignment");
    await expect(editor.locator("[name=merge]")).toBeDisabled();
    await editor.locator("[name=primary]").check();
    await expect(editor.locator("[name=merge]")).toBeEnabled();
    await expect(editor.locator("[name=merge]")).not.toBeChecked();
    await expect(editor.locator("[name=approve]")).not.toBeChecked();
    await expect(editor.locator("[name=comment]")).not.toBeChecked();
    await capture(page, testInfo, `independent-permissions-${index}`);
    await editor
      .getByRole("button", { name: "Save assignment", exact: true })
      .click();
    await expect(editor).toHaveCount(0);
    saved = await store("saved_resources");
    expect(saved.readiness.repositories[0].primary_assignment_id).toBe(
      assignments[index].id,
    );
    expect(
      saved.readiness.repositories[0].assignments.filter(
        ([, authority]) => authority.primary,
      ),
    ).toHaveLength(1);
    for (const [, authority] of saved.readiness.repositories[0].assignments)
      expect(authority).toMatchObject({
        comment: false,
        approve: false,
        merge: false,
      });
  }
  await parent.locator(".assignment-list").scrollIntoViewIfNeeded();
  await testInfo.attach("persisted-seven-assignment-readiness", {
    body: JSON.stringify(saved, null, 2),
    contentType: "application/json",
  });
  await writeFile(
    testInfo.outputPath("persisted-seven-assignment-readiness.json"),
    JSON.stringify(saved, null, 2),
  );
  await capture(page, testInfo, "seven-assignments");
  await close(parent);
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  parent = await repositorySettings(page, "fixture/compact");
  await expect(parent.locator(".assignment-row")).toHaveCount(7);
  expect(
    (await store("snapshot")).settings.repositories[0].assignments,
  ).toEqual(saved.settings.repositories[0].assignments);
});

test("seven persisted normal jobs retain their identities across repository presentation saves", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  fixture.settings.agents = Array.from({ length: 7 }, (_, index) => ({
    ...fixture.settings.agents[0],
    id: id(100 + index),
    name: `Reviewer ${index + 1}`,
  }));
  const repository = fixture.settings.repositories[0];
  repository.assignments = fixture.settings.agents.map((agent, index) => ({
    id: id(200 + index),
    agent_id: agent.id,
    schedule: fixture.settings.defaults.schedule,
    comment: false,
    approve: false,
  }));
  await store("seed_settings", fixture.settings);
  const jobs = repository.assignments.map((assignment, index) => ({
    ...fixture.review(9).job,
    assignment_id: assignment.id,
    work: {
      id: `compact-normal-${index}`,
      item_id: "compact-iteration",
      iteration_id: "first",
      iteration: 1,
      agent_id: assignment.agent_id,
      enqueue_order: index + 1,
      pass_ordinal: 1,
      trigger: "admission",
      admission: {
        watched_author: true,
        all_authors: false,
        requested_reviewer: false,
      },
    },
  }));
  await store("seed_queue_state", {
    jobs,
    reviews: [],
    publications: [],
    follow_ups: [],
  });
  const before = await store("monitoring_snapshot");
  expect(before.reviews).toHaveLength(7);
  await provider(page);
  await start(page, store);
  const editor = await repositorySettings(page, "example/repo");
  await expect(editor.locator(".assignment-row")).toHaveCount(7);
  await save(editor);
  const after = await store("monitoring_snapshot");
  expect(after.jobs).toEqual(before.jobs);
  expect(after.reviews).toHaveLength(7);
  await tab(page, "Running").click();
  await expect(page.locator("[data-running-list] article")).toHaveCount(7);
  expect(new Set(after.jobs.map((job) => job.work.id)).size).toBe(7);
});

for (const failure of ["write", "conflict"]) {
  test(`repository ${failure} preserves draft, explicit rejection and unrelated saved resources`, async ({
    page,
    store,
    dataRoot,
  }, testInfo) => {
    const initial = await seed(store);
    await provider(page);
    await start(page, store);
    const editor = await repositorySettings(page, "fixture/compact");
    await editor
      .getByLabel("Reviewer requests", { exact: true })
      .selectOption("off");
    const temporary = join(dataRoot, "config/settings.json.tmp");
    if (failure === "write") await mkdir(temporary);
    else {
      const concurrent = structuredClone(initial);
      concurrent.repositories[0].enabled = true;
      await store("seed_settings", concurrent);
    }
    const before = await readFile(join(dataRoot, "config/settings.json"));
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-resource-error]")).toBeVisible();
    const rejection = await editor
      .locator("[data-resource-error]")
      .textContent();
    await editor.getByLabel("Reviewer requests", { exact: true }).focus();
    await page.evaluate(() => window.__settingsIdle());
    await expect(editor.locator("[data-resource-error]")).toHaveText(rejection);
    await expect(
      editor.getByLabel("Reviewer requests", { exact: true }),
    ).toHaveValue("off");
    expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
      before,
    );
    await editor.locator("[data-resource-error]").scrollIntoViewIfNeeded();
    await capture(page, testInfo, `repository-${failure}`);
    if (failure === "write") {
      await rm(temporary, { recursive: true });
      await save(editor);
      expect(
        (await store("snapshot")).settings.repositories[0].overrides
          .reviewer_assignment,
      ).toBe(false);
    } else {
      await editor
        .getByRole("button", { name: "Cancel repository changes" })
        .click();
      await page
        .getByRole("button", { name: "Discard draft and reload" })
        .click();
      await expect(page.locator("#save-status")).toHaveText(
        "All changes saved",
      );
      expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
        true,
      );
    }
    expect((await store("snapshot")).settings.repositories[1]).toEqual(
      initial.repositories[1],
    );
  });
}

test("unbind cancellation, write rejection and retry retain permissions, neighbors and global schedule", async ({
  page,
  store,
  dataRoot,
}, testInfo) => {
  const initial = await seed(store);
  initial.repositories[0].assignments = [
    {
      id: id(200),
      agent_id: id(100),
      schedule: initial.defaults.schedule,
      comment: true,
      approve: false,
      actions: { approve: true, merge: false },
    },
  ];
  await store("seed_settings", initial);
  await provider(page);
  await start(page, store);
  let parent = await repositorySettings(page, "fixture/compact");
  await parent.getByText("Repository and connection", { exact: true }).click();
  const opener = parent.getByRole("button", {
    name: "Unbind account",
    exact: true,
  });
  await opener.click();
  let confirmation = modal(page, "Unbind repository account?");
  await close(confirmation);
  await expect(opener).toBeFocused();
  expect((await store("snapshot")).settings).toEqual(initial);
  await opener.click();
  confirmation = modal(page, "Unbind repository account?");
  const temporary = join(dataRoot, "config/settings.json.tmp");
  await mkdir(temporary);
  await confirmation
    .getByRole("button", { name: "Unbind account", exact: true })
    .click();
  await expect(confirmation.getByRole("alert")).toBeVisible();
  expect((await store("snapshot")).settings).toEqual(initial);
  await capture(page, testInfo, "unbind-rejected");
  await rm(temporary, { recursive: true });
  await confirmation
    .getByRole("button", { name: "Unbind account", exact: true })
    .click();
  await expect(confirmation).toHaveCount(0);
  const expected = structuredClone(initial);
  delete expected.repositories[0].provider_account_id;
  delete expected.repositories[0].provider_repository_id;
  expected.repository_authorizations = { [expected.repositories[0].id]: null };
  expect((await store("snapshot")).settings).toEqual(expected);
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  parent = await repositorySettings(page, "fixture/compact");
  await expect(parent.locator(".repository-identity")).toContainText("Unbound");
  await expect(
    parent.getByRole("button", { name: "Configure scope" }),
  ).toHaveCount(0);
  await parent.getByText("Repository and connection", { exact: true }).click();
  await expect(
    parent.getByRole("button", { name: "Verify GitHub connection" }),
  ).toBeDisabled();
  await parent
    .getByRole("button", { name: "Remove repository", exact: true })
    .click();
  confirmation = modal(page, "Remove repository?");
  await close(confirmation);
  expect((await store("snapshot")).settings).toEqual(expected);
});

test("unavailable bound accounts remain identified and cannot silently rebind during edit", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seed(store);
  // The default boundary returns no connected accounts, not a replacement actor.
  await start(page, store);
  await expect(row(page)).toContainText("GitHub / 22");
  await expect(row(page)).toContainText("Reconnect account");
  const parent = await repositorySettings(page, "fixture/compact");
  await parent.getByText("Repository and connection", { exact: true }).click();
  await expect(
    parent.getByRole("button", { name: "Verify GitHub connection" }),
  ).toBeDisabled();
  await expect(parent.locator(".connection-status")).toContainText(
    "must be reconnected",
  );
  await parent
    .getByRole("button", { name: "Edit repository", exact: true })
    .click();
  const editor = modal(page, "Edit repository");
  await expect(editor.getByLabel("Acting GitHub account")).toHaveValue("22");
  await expect(editor.locator("[name=account] option:checked")).toHaveText(
    "22 - reconnect required",
  );
  await editor
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  await expect(editor.getByRole("alert")).toContainText(
    "Choose a connected GitHub account",
  );
  await capture(page, testInfo, "unavailable-acting-account");
  expect((await store("snapshot")).settings).toEqual(initial);
  await editor.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(row(page)).toBeFocused();
});

test("verified watched people, draft cancellation and keyboard controls remain usable at 320x300", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seed(store);
  const calls = await provider(page, (command) => {
    if (command === "resolve_provider_person")
      return { id: "77", login: "verified-person" };
    throw "Unexpected provider command";
  });
  await start(page, store);
  let parent = await repositorySettings(page, "fixture/compact");
  await parent.getByRole("button", { name: "Add people" }).click();
  const picker = modal(page, "Add people");
  await expect(picker.getByLabel("Acting GitHub account")).toHaveValue("22");
  await picker.getByLabel("GitHub login").fill("@verified-person");
  await picker.getByRole("button", { name: "Add person", exact: true }).click();
  await expect(picker).toHaveCount(0);
  await expect(parent.locator(".watchlist")).toContainText("GitHub ID 77");
  await parent.locator(".watchlist").scrollIntoViewIfNeeded();
  await capture(page, testInfo, "watched-people");
  expect(calls).toEqual([
    {
      command: "resolve_provider_person",
      args: { provider: "github", accountId: "22", login: "verified-person" },
    },
  ]);
  expect((await store("snapshot")).settings).toEqual(initial);
  await save(parent);
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].watched_authors).toEqual([
    { id: "11", login: "watched-person" },
    { id: "77", login: "verified-person" },
  ]);
  parent = await repositorySettings(page, "fixture/compact");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 320, height: 300 });
  await parent.getByRole("button", { name: "Remove verified-person" }).focus();
  await expect(
    parent.getByRole("button", { name: "Remove verified-person" }),
  ).toBeInViewport();
  await page.keyboard.press("Enter");
  await parent
    .getByRole("button", { name: "Cancel repository changes" })
    .focus();
  await expect(
    parent.getByRole("button", { name: "Cancel repository changes" }),
  ).toBeInViewport();
  const saveButton = await parent
    .getByRole("button", { name: "Save repository", exact: true })
    .boundingBox();
  expect(saveButton.width).toBeGreaterThanOrEqual(110);
  expect(saveButton.height).toBeLessThanOrEqual(60);
  await capture(page, testInfo, "repository-320x300-keyboard");
  await page.keyboard.press("Enter");
  await expect(parent).toHaveCount(0);
  expect((await store("snapshot")).settings).toEqual(saved);
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  parent = await repositorySettings(page, "fixture/compact");
  await expect(parent.locator(".watchlist")).toContainText("GitHub ID 77");
});

for (const resolution of ["Save", "Cancel"]) {
  test(`R1 correction: dirty unbind requires explicit ${resolution} without consuming repository or Preferences drafts`, async ({
    page,
    store,
    dataRoot,
  }, testInfo) => {
    const initial = await seed(store);
    initial.repositories[0].assignments = [
      {
        id: id(200),
        agent_id: id(100),
        schedule: initial.defaults.schedule,
        comment: true,
        approve: false,
        actions: { approve: true, merge: false },
      },
    ];
    await store("seed_settings", initial);
    await provider(page);
    await start(page, store);
    const settingsPath = join(dataRoot, "config/settings.json");
    const before = await readFile(settingsPath);
    await section(page, "Preferences");
    await page.locator("#global-cron").fill("invalid unsaved cron");
    await page.locator("#global-capacity").fill("9");
    let editor = await repositorySettings(page, "fixture/compact");
    await editor
      .locator(".assignment-row")
      .getByRole("button", { name: "Remove", exact: true })
      .click();
    await editor
      .getByLabel("Reviewer requests", { exact: true })
      .selectOption("off");
    await editor.getByLabel("Enable repository monitoring").uncheck();
    await editor
      .getByText("Repository and connection", { exact: true })
      .click();
    const unbind = editor.getByRole("button", {
      name: "Unbind account",
      exact: true,
    });
    await unbind.focus();
    await unbind.press("Enter");
    await expect(modal(page, "Unbind repository account?")).toHaveCount(0);
    await expect(editor.getByRole("alert")).toContainText(
      "Save or Cancel repository changes before unbinding",
    );
    await expect(unbind).toBeFocused();
    await expect(editor.locator(".assignment-row")).toHaveCount(0);
    await expect(
      editor.getByLabel("Reviewer requests", { exact: true }),
    ).toHaveValue("off");
    await expect(
      editor.getByLabel("Enable repository monitoring"),
    ).not.toBeChecked();
    expect(await readFile(settingsPath)).toEqual(before);
    expect((await store("snapshot")).settings).toEqual(initial);
    await editor.getByRole("alert").scrollIntoViewIfNeeded();
    await capture(page, testInfo, `dirty-unbind-${resolution}`);

    await close(editor);
    editor = await repositorySettings(page, "fixture/compact");
    await expect(editor.locator(".assignment-row")).toHaveCount(0);
    await expect(
      editor.getByLabel("Reviewer requests", { exact: true }),
    ).toHaveValue("off");
    await expect(
      editor.getByLabel("Enable repository monitoring"),
    ).not.toBeChecked();
    if (resolution === "Save") await save(editor);
    else
      await editor
        .getByRole("button", { name: "Cancel repository changes" })
        .click();
    const explicitlySaved = (await store("snapshot")).settings;
    if (resolution === "Cancel") {
      expect(explicitlySaved).toEqual(initial);
      expect(await readFile(settingsPath)).toEqual(before);
    } else {
      expect(explicitlySaved.repositories[0].assignments ?? []).toEqual([]);
      expect(explicitlySaved.repositories[0].overrides).toEqual({
        reviewer_assignment: false,
      });
      expect(explicitlySaved.repositories[0].enabled).toBe(false);
    }
    expect(explicitlySaved.defaults).toEqual(initial.defaults);
    expect(explicitlySaved.capacity).toBe(initial.capacity);
    editor = await repositorySettings(page, "fixture/compact");
    await editor
      .getByText("Repository and connection", { exact: true })
      .click();
    await editor
      .getByRole("button", { name: "Unbind account", exact: true })
      .click();
    const confirmation = modal(page, "Unbind repository account?");
    await expect(confirmation).toContainText(
      "Assignments, permissions and completed evidence are retained",
    );
    await confirmation
      .getByRole("button", { name: "Unbind account", exact: true })
      .click();
    await expect(confirmation).toHaveCount(0);
    const expected = structuredClone(explicitlySaved);
    delete expected.repositories[0].provider_account_id;
    delete expected.repositories[0].provider_repository_id;
    expected.repository_authorizations = {
      [expected.repositories[0].id]: null,
    };
    expect((await store("snapshot")).settings).toEqual(expected);
    await section(page, "Preferences");
    await expect(page.locator("#global-cron")).toHaveValue(
      "invalid unsaved cron",
    );
    await expect(page.locator("#global-capacity")).toHaveValue("9");
    await page.reload();
    await page.evaluate(() => window.__settingsIdle());
    expect((await store("snapshot")).settings).toEqual(expected);
  });
}

test("R1 correction: concurrent saved repository changes still reject a clean unbind", async ({
  page,
  store,
  dataRoot,
}) => {
  const initial = await seed(store);
  await provider(page);
  await start(page, store);
  const editor = await repositorySettings(page, "fixture/compact");
  await editor.getByText("Repository and connection", { exact: true }).click();
  await editor
    .getByRole("button", { name: "Unbind account", exact: true })
    .click();
  const confirmation = modal(page, "Unbind repository account?");
  const concurrent = structuredClone(initial);
  concurrent.repositories[0].enabled = true;
  await store("seed_settings", concurrent);
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await confirmation
    .getByRole("button", { name: "Unbind account", exact: true })
    .click();
  await expect(confirmation.getByRole("alert")).toContainText(
    "Resource changed",
  );
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
  expect((await store("snapshot")).settings).toEqual(concurrent);
  await close(confirmation);
  await expect(editor.locator(".repository-identity")).toContainText("22");
  await expect(
    editor.getByLabel("Enable repository monitoring"),
  ).not.toBeChecked();
});

test("R2 correction: saved legacy interval and timezone remain visible but explicitly block polling", async ({
  page,
  store,
  dataRoot,
}, testInfo) => {
  const initial = await seed(store);
  initial.defaults.schedule = {
    kind: "interval",
    minutes: 7,
    timezone: "America/New_York",
  };
  await store("seed_settings", initial);
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await provider(page);
  await start(page, store);
  const editor = await repositorySettings(page, "fixture/compact");
  const schedule = editor.locator("[data-global-schedule]");
  await expect(schedule).toContainText("Every 7 minutes");
  await expect(schedule).toContainText("America/New_York");
  await expect(schedule).toContainText(
    "Polling is blocked until you choose a global five-field cron schedule in Preferences",
  );
  await expect(
    editor.locator("[name=cron],[name=minutes],[name=timezone],#global-cron"),
  ).toHaveCount(0);
  await schedule.scrollIntoViewIfNeeded();
  await capture(page, testInfo, "legacy-schedule-blocked");
  const resources = await store("saved_resources");
  expect(resources.readiness.configuration_ready).toBe(false);
  expect(resources.settings).toEqual(initial);
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});
