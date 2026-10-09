import { test, expect } from "./fixtures.mjs";
import { mkdir, readFile, rmdir } from "node:fs/promises";
import { join } from "node:path";
import {
  fixtureAgent,
  repositorySettings,
  section,
  seedBoundRepositories,
} from "./navigation.mjs";

const secondId = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const assignmentId = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const dialog = (page, name) => page.getByRole("dialog", { name, exact: true });
const button = (scope, name) =>
  scope.getByRole("button", { name, exact: true });

async function seed(page, store, { empty = false, standalone = false } = {}) {
  const settings = await seedBoundRepositories(store, [
    "fixture/assignment",
    "fixture/unrelated",
  ]);
  settings.agents = empty
    ? []
    : [
        fixtureAgent,
        {
          ...fixtureAgent,
          id: secondId,
          name: "Selected reviewer",
          prompt: "The exact second Agent.",
        },
      ];
  settings.repositories.forEach((repository) => {
    repository.enabled = false;
  });
  settings.repositories[0].assignments = empty
    ? []
    : [
        {
          id: assignmentId,
          agent_id: secondId,
          schedule: settings.defaults.schedule,
          comment: false,
          approve: false,
          actions: { reply: false, approve: false, merge: false },
        },
        {
          id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
          agent_id: fixtureAgent.id,
          schedule: settings.defaults.schedule,
          comment: false,
          approve: false,
          actions: { reply: false, approve: false, merge: false },
        },
      ];
  await store("seed_settings", settings);
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__assignmentAccounts = "connected";
    window.__assignmentModelFailure = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "copilot_auth_state") {
        if (window.__assignmentAccounts === "error")
          return Promise.reject("Synthetic account lookup failure");
        return Promise.resolve({
          accounts:
            window.__assignmentAccounts === "connected"
              ? [
                  {
                    provider: "copilot",
                    account_id: "33",
                    login: "fixture-ai",
                    state: "connected",
                  },
                ]
              : [],
          flow: { state: "idle" },
        });
      }
      if (command === "list_copilot_models")
        return window.__assignmentModelFailure
          ? Promise.reject("Synthetic model lookup failure")
          : Promise.resolve([{ id: "fixture-model", name: "Fixture model" }]);
      return invoke(command, args);
    };
  });
  await page.goto(standalone ? "/?view=settings" : "/");
  if (!standalone)
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("9");
  return (await store("snapshot")).settings;
}

async function openAssignment(page, existing = true) {
  const repository = await repositorySettings(page, "fixture/assignment");
  await repository.locator("[data-reviewer-trigger]").selectOption("off");
  if (existing)
    await button(repository.locator(".assignment-row").first(), "Edit").click();
  else await button(repository, "Assign agent").click();
  return dialog(page, existing ? "Edit assignment" : "Assign agent");
}

test.use({ viewport: { width: 408, height: 744 } });

test("assignment opens the exact selected shared Agent without losing either draft", async ({
  page,
  store,
}) => {
  const initial = await seed(page, store);
  const assignment = await openAssignment(page);
  await assignment.getByRole("checkbox", { name: /^Primary/ }).check();
  await assignment.getByRole("checkbox", { name: /^Publish Comment/ }).check();
  await assignment.getByRole("checkbox", { name: /^Reply Comment/ }).check();
  await assignment.getByRole("radio", { name: "Approve", exact: true }).check();
  await button(assignment, "Edit Agent").click();
  const agent = dialog(page, "Edit agent");
  await expect(agent.getByLabel("Name", { exact: true })).toHaveValue(
    "Selected reviewer",
  );
  await agent
    .getByLabel("Name", { exact: true })
    .fill("Edited selected reviewer");
  await button(agent, "Save agent").click();
  await expect(agent).toHaveCount(0);
  await expect(assignment.getByLabel("Agent", { exact: true })).toHaveValue(
    secondId,
  );
  await expect(assignment.locator("[name=agent] option:checked")).toHaveText(
    "Edited selected reviewer",
  );
  await expect(button(assignment, "Edit Agent")).toBeFocused();
  await expect(
    assignment.getByRole("checkbox", { name: /^Primary/ }),
  ).toBeChecked();
  await expect(
    assignment.getByRole("checkbox", { name: /^Reply Comment/ }),
  ).toBeChecked();
  await expect(
    assignment.getByRole("radio", { name: "Approve", exact: true }),
  ).toBeChecked();
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories).toEqual(initial.repositories);
  expect(saved.capacity).toBe(initial.capacity);
  expect(saved.agents).toEqual([
    initial.agents[0],
    { ...initial.agents[1], name: "Edited selected reviewer" },
  ]);
  const replacement = await createAgent(
    page,
    assignment,
    "Replacement reviewer",
  );
  await button(replacement, "Save agent").click();
  await expect(replacement).toHaveCount(0);
  const replacementId = (await store("snapshot")).settings.agents.at(-1).id;
  await expect(assignment.getByLabel("Agent", { exact: true })).toHaveValue(
    replacementId,
  );
  await expect(
    assignment.getByRole("checkbox", { name: /^Primary/ }),
  ).toBeChecked();
  await expect(
    assignment.getByRole("checkbox", { name: /^Reply Comment/ }),
  ).toBeChecked();
  await expect(
    assignment.getByRole("radio", { name: "Approve", exact: true }),
  ).toBeChecked();
  await button(assignment, "Save assignment").click();
  await expect(assignment).toHaveCount(0);
  const repository = dialog(page, "Settings for fixture/assignment");
  await expect(repository.locator("[data-reviewer-trigger]")).toHaveValue(
    "off",
  );
  await expect(repository.locator(".assignment-row").first()).toContainText(
    "Replacement reviewer",
  );
  await button(repository, "Close dialog").click();
  await section(page, "Preferences");
  await expect(page.locator("#global-capacity")).toHaveValue("9");
  const committed = (await store("snapshot")).settings;
  expect(committed.repositories[0].primary_assignment_id).toBe(assignmentId);
  expect(committed.repositories[0].assignments[0].agent_id).toBe(replacementId);
  expect(committed.repositories[0].assignments[0].actions).toEqual({
    reply: true,
    approve: true,
    merge: false,
  });
  expect(committed.repositories[1]).toEqual(initial.repositories[1]);
});

async function createAgent(page, assignment, name = "New shared reviewer") {
  await button(assignment, "Create new Agent").click();
  const agent = dialog(page, "New agent");
  await expect(agent.getByLabel("AI account", { exact: true })).toHaveValue("");
  await expect(agent.getByLabel("Model", { exact: true })).toHaveValue("");
  await agent.getByLabel("Name", { exact: true }).fill(name);
  await agent.getByLabel("AI account", { exact: true }).selectOption("33");
  await expect(agent.getByLabel("Model", { exact: true })).toBeEnabled();
  await agent
    .getByLabel("Model", { exact: true })
    .selectOption("fixture-model");
  return agent;
}

async function discard(page, resource) {
  const confirm = dialog(page, `Discard ${resource} changes?`);
  await expect(confirm).toBeVisible();
  await button(confirm, "Discard changes").click();
}

for (const empty of [false, true]) {
  test(`create and select an Agent with ${empty ? "no" : "existing"} Agents never grants capabilities`, async ({
    page,
    store,
  }) => {
    const initial = await seed(page, store, { empty });
    const assignment = await openAssignment(page, false);
    await expect(button(assignment, "Edit Agent")).toBeDisabled();
    const agent = await createAgent(page, assignment);
    await button(agent, "Save agent").click();
    await expect(agent).toHaveCount(0);
    await expect(button(assignment, "Create new Agent")).toBeFocused();
    const saved = (await store("snapshot")).settings;
    const created = saved.agents.at(-1);
    expect(saved.repositories).toEqual(initial.repositories);
    expect(created.ai_account).toEqual({
      provider: "copilot",
      account_id: "33",
    });
    await expect(assignment.getByLabel("Agent", { exact: true })).toHaveValue(
      created.id,
    );
    await expect(
      assignment.getByRole("checkbox", { name: /^Primary/ }),
    ).toBeChecked({ checked: empty });
    await expect(
      assignment.getByRole("checkbox", { name: /^Publish Comment/ }),
    ).not.toBeChecked();
    await button(assignment, "Edit Agent").click();
    const edit = dialog(page, "Edit agent");
    await expect(edit.getByLabel("Name", { exact: true })).toHaveValue(
      created.name,
    );
    await edit
      .getByLabel("Name", { exact: true })
      .fill("Immediately edited new Agent");
    await button(edit, "Save agent").click();
    await expect(edit).toHaveCount(0);
    await expect(assignment.locator("[name=agent] option:checked")).toHaveText(
      "Immediately edited new Agent",
    );
    await button(assignment, "Assign agent").click();
    await expect(assignment).toHaveCount(0);
    const committed = (await store("snapshot")).settings;
    expect(committed.repositories[0].assignments.at(-1)).toMatchObject({
      agent_id: created.id,
      comment: false,
      approve: false,
      actions: { reply: false, approve: false, merge: false },
    });
    expect(committed.repositories[0].primary_assignment_id).toBeUndefined();
    expect(committed.repositories[0].overrides.reviewer_assignment).toBe(false);
    expect(committed.capacity).toBe(initial.capacity);
    await page.reload();
    expect((await store("snapshot")).settings).toEqual(committed);
  });
}

for (const standalone of [false, true]) {
  for (const action of ["Cancel", "Back"]) {
    test(`${standalone ? "standalone" : "panel"} ${action} warns independently for Agent and assignment drafts`, async ({
      page,
      store,
    }) => {
      const initial = await seed(page, store, { standalone });
      const assignment = await openAssignment(page);
      await assignment
        .getByRole("checkbox", { name: /^Publish Comment/ })
        .check();
      await button(assignment, "Edit Agent").click();
      const agent = dialog(page, "Edit agent");
      const prompt = agent.getByRole("textbox", {
        name: "Prompt",
        exact: true,
      });
      await prompt.fill("Retain until explicitly discarded.");
      const close = async (scope) => {
        if (standalone && action === "Back")
          await page.keyboard.press("Escape");
        else
          await button(
            scope,
            action === "Back" ? "Close dialog" : "Cancel",
          ).click();
      };
      await close(agent);
      let confirm = dialog(page, "Discard Agent changes?");
      await button(confirm, "Keep editing").click();
      await expect(prompt).toHaveValue("Retain until explicitly discarded.");
      await close(agent);
      await discard(page, "Agent");
      await expect(agent).toHaveCount(0);
      await expect(button(assignment, "Edit Agent")).toBeFocused();
      await expect(
        assignment.getByRole("checkbox", { name: /^Publish Comment/ }),
      ).toBeChecked();
      await close(assignment);
      confirm = dialog(page, "Discard assignment changes?");
      await button(confirm, "Keep editing").click();
      await expect(assignment.getByLabel("Agent", { exact: true })).toHaveValue(
        secondId,
      );
      await close(assignment);
      await discard(page, "assignment");
      await expect(assignment).toHaveCount(0);
      const repository = dialog(page, "Settings for fixture/assignment");
      await expect(
        button(repository.locator(".assignment-row").first(), "Edit"),
      ).toBeFocused();
      await expect(repository.locator("[data-reviewer-trigger]")).toHaveValue(
        "off",
      );
      expect((await store("snapshot")).settings).toEqual(initial);
      await button(repository, "Close dialog").click();
      await section(page, "Preferences");
      await expect(page.locator("#global-capacity")).toHaveValue("9");
    });
  }
}

test("clean cancel, new cancel and switching selection retain exact focus and prior saves", async ({
  page,
  store,
}) => {
  const initial = await seed(page, store);
  const assignment = await openAssignment(page);
  await assignment
    .getByLabel("Agent", { exact: true })
    .selectOption(fixtureAgent.id);
  await button(assignment, "Edit Agent").click();
  let agent = dialog(page, "Edit agent");
  await expect(agent.getByLabel("Name", { exact: true })).toHaveValue(
    fixtureAgent.name,
  );
  await button(agent, "Cancel").click();
  await expect(agent).toHaveCount(0);
  await expect(button(assignment, "Edit Agent")).toBeFocused();
  await button(assignment, "Create new Agent").click();
  agent = dialog(page, "New agent");
  await button(agent, "Close dialog").click();
  await expect(agent).toHaveCount(0);
  await expect(button(assignment, "Create new Agent")).toBeFocused();
  await expect(assignment.getByLabel("Agent", { exact: true })).toHaveValue(
    fixtureAgent.id,
  );
  agent = await createAgent(page, assignment);
  await button(agent, "Save agent").click();
  await expect(agent).toHaveCount(0);
  await button(assignment, "Cancel").click();
  await discard(page, "assignment");
  const saved = (await store("snapshot")).settings;
  expect(saved.agents).toHaveLength(3);
  expect(saved.repositories).toEqual(initial.repositories);
  await page.reload();
  expect((await store("snapshot")).settings).toEqual(saved);
});

test("Agent write failure retains nested fields and both parent drafts for retry", async ({
  page,
  store,
  dataRoot,
}) => {
  const initial = await seed(page, store);
  const assignment = await openAssignment(page);
  await assignment.getByRole("checkbox", { name: /^Publish Comment/ }).check();
  await button(assignment, "Edit Agent").click();
  const agent = dialog(page, "Edit agent");
  await agent
    .getByLabel("Name", { exact: true })
    .fill("Retry this exact Agent");
  const settingsPath = join(dataRoot, "config/settings.json");
  const obstruction = `${settingsPath}.tmp`;
  const before = await readFile(settingsPath);
  await mkdir(obstruction);
  try {
    await button(agent, "Save agent").click();
    await expect(agent.getByRole("alert")).toContainText(
      "Cannot write settings",
    );
    expect(await readFile(settingsPath)).toEqual(before);
    await expect(agent.getByLabel("Name", { exact: true })).toHaveValue(
      "Retry this exact Agent",
    );
  } finally {
    await rmdir(obstruction);
  }
  await button(agent, "Save agent").click();
  await expect(agent).toHaveCount(0);
  await expect(
    assignment.getByRole("checkbox", { name: /^Publish Comment/ }),
  ).toBeChecked();
  expect((await store("snapshot")).settings.repositories).toEqual(
    initial.repositories,
  );
});

test("nested pending Save locks dismissal and returns without a confirmation or lost draft", async ({
  page,
  store,
  ipc,
}) => {
  await seed(page, store);
  const assignment = await openAssignment(page);
  await assignment.getByRole("checkbox", { name: /^Publish Comment/ }).check();
  await button(assignment, "Edit Agent").click();
  const agent = dialog(page, "Edit agent");
  await agent.getByLabel("Name", { exact: true }).fill("Held save");
  const hold = ipc.holdNext("save_resource");
  try {
    await button(agent, "Save agent").click();
    await hold.arrived;
    await expect(button(agent, "Cancel")).toBeDisabled();
    await expect(button(agent, "Close dialog")).toBeDisabled();
    hold.release();
    await expect(agent).toHaveCount(0);
    await expect(button(assignment, "Edit Agent")).toBeFocused();
    await expect(
      assignment.getByRole("checkbox", { name: /^Publish Comment/ }),
    ).toBeChecked();
    await expect(dialog(page, "Discard Agent changes?")).toHaveCount(0);
  } finally {
    hold.release();
  }
});

test("deleting an unassigned selected Agent retains an explicit stale reference without selecting another", async ({
  page,
  store,
}) => {
  await seed(page, store, { empty: true });
  const assignment = await openAssignment(page, false);
  const agent = await createAgent(page, assignment);
  await button(agent, "Save agent").click();
  await expect(agent).toHaveCount(0);
  const selected = await assignment
    .getByLabel("Agent", { exact: true })
    .inputValue();
  await assignment.getByRole("checkbox", { name: /^Publish Comment/ }).check();
  await button(assignment, "Edit Agent").click();
  const edit = dialog(page, "Edit agent");
  await button(edit, "Delete agent").click();
  await button(edit, "Confirm deletion").click();
  await expect(edit).toHaveCount(0);
  await expect(assignment.getByLabel("Agent", { exact: true })).toHaveValue(
    selected,
  );
  await expect(assignment.getByLabel("Agent", { exact: true })).toBeFocused();
  await expect(button(assignment, "Edit Agent")).toBeDisabled();
  await expect(assignment.locator("[data-agent-selection]")).toContainText(
    "unavailable",
  );
  await expect(
    assignment.getByRole("checkbox", { name: /^Publish Comment/ }),
  ).toBeChecked();
  await button(assignment, "Assign agent").click();
  await expect(assignment.getByRole("alert")).toContainText("unavailable");
  expect((await store("snapshot")).settings.agents ?? []).toEqual([]);
  expect(
    (await store("snapshot")).settings.repositories[0].assignments ?? [],
  ).toEqual([]);
});

test("concurrent Agent changes fail compare-and-save without replacing either version", async ({
  page,
  store,
}) => {
  const initial = await seed(page, store);
  const assignment = await openAssignment(page);
  await button(assignment, "Edit Agent").click();
  const agent = dialog(page, "Edit agent");
  await agent.getByLabel("Name", { exact: true }).fill("Local unfinished name");
  const concurrent = {
    ...initial.agents[1],
    prompt: "Saved in another window",
  };
  await store("save_resource", {
    edit: {
      kind: "agent",
      id: secondId,
      expected: initial.agents[1],
      value: concurrent,
    },
  });
  await button(agent, "Save agent").click();
  await expect(agent.getByRole("alert")).toContainText("Resource changed");
  await expect(agent.getByLabel("Name", { exact: true })).toHaveValue(
    "Local unfinished name",
  );
  expect((await store("snapshot")).settings.agents[1]).toEqual(concurrent);
  expect((await store("snapshot")).settings.repositories).toEqual(
    initial.repositories,
  );
});

test("account and model failures stay explicit, retryable and never grant an implicit selection", async ({
  page,
  store,
}) => {
  const initial = await seed(page, store, { empty: true });
  const assignment = await openAssignment(page, false);
  await page.evaluate(() => {
    window.__assignmentAccounts = "error";
  });
  await button(assignment, "Create new Agent").click();
  const agent = dialog(page, "New agent");
  await expect(agent.locator("[data-editor-accounts]")).toContainText(
    "unavailable",
  );
  await expect(agent.getByLabel("AI account", { exact: true })).toHaveValue("");
  await page.evaluate(() => {
    window.__assignmentAccounts = "empty";
  });
  await button(agent, "Retry Copilot accounts").click();
  await expect(agent.locator("[data-editor-accounts]")).toContainText(
    "No verified Copilot connection",
  );
  await page.evaluate(() => {
    window.__assignmentAccounts = "connected";
    window.__assignmentModelFailure = true;
  });
  await button(agent, "Retry Copilot accounts").click();
  await agent.getByLabel("AI account", { exact: true }).selectOption("33");
  await expect(agent.locator("[data-model-status]")).toContainText(
    "Synthetic model lookup failure",
  );
  await expect(agent.getByLabel("Model", { exact: true })).toHaveValue("");
  await agent
    .getByLabel("Name", { exact: true })
    .fill("Must not save with no model");
  await button(agent, "Save agent").click();
  await expect(agent.getByRole("alert")).toContainText(
    "Choose a verified AI account",
  );
  expect((await store("snapshot")).settings).toEqual(initial);
  await page.evaluate(() => {
    window.__assignmentModelFailure = false;
  });
  await button(agent, "Retry model list").click();
  await expect(agent.getByLabel("Model", { exact: true })).toBeEnabled();
  await expect(agent.getByLabel("Model", { exact: true })).toHaveValue("");
  await agent
    .getByLabel("Model", { exact: true })
    .selectOption("fixture-model");
  await button(agent, "Save agent").click();
  await expect(agent).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories).toEqual(
    initial.repositories,
  );
});

test("assignment keyboard navigation and discard confirmation fit the compact high-contrast flow", async ({
  page,
  store,
}, testInfo) => {
  await seed(page, store);
  const nextKey =
    page.context().browser().browserType().name() === "webkit" &&
    process.platform === "darwin"
      ? "Alt+Tab"
      : "Tab";
  const previousKey = nextKey === "Alt+Tab" ? "Alt+Shift+Tab" : "Shift+Tab";
  await page.setViewportSize({ width: 320, height: 300 });
  await page.emulateMedia({ reducedMotion: "reduce", forcedColors: "active" });
  const assignment = await openAssignment(page);
  const select = assignment.getByLabel("Agent", { exact: true });
  await select.focus();
  await page.keyboard.press(nextKey);
  await expect(button(assignment, "Edit Agent")).toBeFocused();
  await expect(button(assignment, "Edit Agent")).toBeInViewport();
  await page.keyboard.press("Enter");
  const agent = dialog(page, "Edit agent");
  await expect(button(agent, "Close dialog")).toBeFocused();
  await page.keyboard.press(nextKey);
  await expect(agent.getByLabel("Name", { exact: true })).toBeFocused();
  await agent.getByLabel("Name", { exact: true }).fill("Keyboard draft");
  await expect(agent.getByLabel("Name", { exact: true })).toBeInViewport();
  await page.screenshot({
    path: testInfo.outputPath("nested-agent-320x300.png"),
  });
  await page.keyboard.press(previousKey);
  await expect(button(agent, "Close dialog")).toBeFocused();
  await page.keyboard.press("Enter");
  const confirm = dialog(page, "Discard Agent changes?");
  await expect(button(confirm, "Close dialog")).toBeFocused();
  await page.keyboard.press(nextKey);
  await expect(button(confirm, "Keep editing")).toBeFocused();
  await expect(button(confirm, "Keep editing")).toBeInViewport();
  await page.keyboard.press("Enter");
  await expect(button(agent, "Close dialog")).toBeFocused();
  await page.keyboard.press("Enter");
  await discard(page, "Agent");
  await expect(button(assignment, "Edit Agent")).toBeFocused();
  await page.keyboard.press(nextKey);
  await expect(button(assignment, "Create new Agent")).toBeFocused();
  await expect(button(assignment, "Create new Agent")).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

test("late account responses cannot change another editor or steal focus after Back", async ({
  page,
  store,
}) => {
  await seed(page, store);
  const assignment = await openAssignment(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    let first = true;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "copilot_auth_state" && first) {
        first = false;
        return new Promise((resolve) => {
          window.__releaseOldAccounts = () =>
            resolve({
              accounts: [],
              flow: { state: "idle" },
            });
        });
      }
      return invoke(command, args);
    };
  });
  await button(assignment, "Create new Agent").click();
  const abandoned = dialog(page, "New agent");
  await expect(abandoned.locator("[data-editor-accounts]")).toHaveText(
    "Reading Copilot accounts...",
  );
  await button(abandoned, "Close dialog").click();
  await button(assignment, "Edit Agent").click();
  const agent = dialog(page, "Edit agent");
  await expect(agent.locator("[data-editor-accounts]")).toContainText(
    "Choose an AI account",
  );
  await agent.getByLabel("Name", { exact: true }).fill("Keep current focus");
  await page.evaluate(() => window.__releaseOldAccounts());
  await expect(agent.getByLabel("Name", { exact: true })).toBeFocused();
  await expect(agent.getByLabel("Name", { exact: true })).toHaveValue(
    "Keep current focus",
  );
  await expect(agent.locator("[data-editor-accounts]")).toContainText(
    "Choose an AI account",
  );
  await expect(agent.locator("[name=ai-account] option")).toHaveCount(2);
});
