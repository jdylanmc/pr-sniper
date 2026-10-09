import { test, expect } from "./fixtures.mjs";
import {
  mkdir,
  readFile,
  readdir,
  rename,
  rmdir,
  writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { section } from "./navigation.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const modal = (page, name) => page.getByRole("dialog", { name, exact: true });
const back = (page) =>
  page.getByRole("button", { name: "Back to Genie", exact: true }).click();
const close = (dialog) =>
  dialog.getByRole("button", { name: "Close dialog", exact: true }).click();
const activate = (page) => page.locator("[data-genie-activate]");
const repositoryId = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const agentId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const screenshots = (page) =>
  test
    .info()
    .outputPath("screenshots", page.context().browser().browserType().name());
const nextControl = (page) =>
  page.keyboard.press(
    page.context().browser().browserType().name() === "webkit" &&
      process.platform === "darwin"
      ? "Alt+Tab"
      : "Tab",
  );

async function fullyVisible(control) {
  const clipping = await control.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    const ring = element.matches(":focus-visible")
      ? Math.max(
          0,
          parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset),
        )
      : 0;
    const failures = [];
    for (
      let parent = element.parentElement;
      parent;
      parent = parent.parentElement
    ) {
      const css = getComputedStyle(parent);
      if (
        !/(auto|scroll|hidden|clip)/.test(`${css.overflowX} ${css.overflowY}`)
      )
        continue;
      const bounds = parent.getBoundingClientRect();
      if (
        rect.left - ring < bounds.left + parent.clientLeft - 1 ||
        rect.right + ring >
          bounds.left + parent.clientLeft + parent.clientWidth + 1 ||
        rect.top - ring < bounds.top + parent.clientTop - 1 ||
        rect.bottom + ring >
          bounds.top + parent.clientTop + parent.clientHeight + 1
      )
        failures.push({
          parent: parent.className,
          rect: rect.toJSON(),
          bounds: bounds.toJSON(),
          ring,
        });
    }
    return failures;
  });
  expect(clipping).toEqual([]);
}

async function synthetic(page, store, connected = false) {
  const state = {
    ai: connected,
    code: connected,
    models: [{ id: "fixture-model", name: "Fixture model" }],
    modelCatalogs: {},
    previews: {},
    applied: [],
    immediate: 0,
    beforeApply: undefined,
    beforeModels: undefined,
    beforeRead: undefined,
    aiGenerations: {},
    extraAi: {},
    cancellations: [],
    aiFlow: { state: "idle" },
    codeFlow: { state: "idle" },
    commands: [],
  };
  const account = (role) => ({
    provider: role === "ai" ? "copilot" : "github",
    account_id: role === "ai" ? "33" : "22",
    login: role === "ai" ? "fixture-ai" : "fixture-code",
    state: "connected",
  });
  const view = (role) => ({
    accounts: state[role] ? [account(role)] : [],
    flow: state[`${role}Flow`],
  });
  const context = () => ({
    repositoryAccounts: state.code
      ? { 22: { login: "fixture-code", connected: true } }
      : {},
    aiAccounts: {
      ...(state.ai ? { 33: { login: "fixture-ai", connected: true } } : {}),
      ...state.extraAi,
    },
    aiGenerations: state.aiGenerations,
  });
  await page.exposeFunction("__genieFixture", async (command, args = {}) => {
    state.commands.push(command);
    if (command === "github_auth_state") return view("code");
    if (command === "copilot_auth_state") return view("ai");
    if (
      command === "start_copilot_auth" ||
      command === "start_github_browser_auth"
    ) {
      const role = command.includes("copilot") ? "ai" : "code";
      state[`${role}Flow`] = {
        ...account(role),
        state: "pending_account_confirmation",
      };
      return view(role);
    }
    if (
      command === "confirm_copilot_account" ||
      command === "confirm_github_account"
    ) {
      const role = command.includes("copilot") ? "ai" : "code";
      state[role] = true;
      state[`${role}Flow`] = { state: "idle" };
      return view(role);
    }
    if (command === "list_copilot_models") {
      await state.beforeModels?.(args);
      return state.modelCatalogs[args.accountId] ?? state.models;
    }
    if (command === "cancel_copilot_models") {
      state.cancellations.push(args);
      return null;
    }
    if (
      command === "cancel_copilot_models" ||
      command === "cancel_monitoring_activation"
    )
      return null;
    if (command === "resolve_provider_repository")
      return {
        identity: { id: args.accountId, login: "fixture-code" },
        repository: { id: "100", name: args.repository },
      };
    if (command === "monitoring_setup_review") {
      await state.beforeRead?.();
      return store("fixture_setup_review", context());
    }
    if (command === "preview_monitoring_activation") {
      const evidence = await store("fixture_setup_preview", args);
      state.previews[evidence.preview.preview_id] = evidence;
      return evidence.preview;
    }
    if (command === "apply_monitoring_setup") {
      state.applied.push(args);
      await state.beforeApply?.();
      return store("fixture_apply_setup", {
        ...args,
        ...context(),
        previews: state.previews,
      });
    }
    if (command === "apply_monitoring_activation") {
      state.immediate++;
      throw "Immediate monitoring activation is not allowed in a guided fixture.";
    }
    throw new Error(`Unexpected Genie fixture command: ${command}`);
  });
  await page.exposeFunction("__genieEvidence", async () => ({
    accounts: context(),
    models: state.models,
    modelCatalogs: state.modelCatalogs,
    saved: (await store("snapshot")).settings,
  }));
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    const pending = new Set();
    window.__genieIdle = async () => {
      while (pending.size) await Promise.all([...pending]);
      await window.__settingsIdle();
    };
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      const result = [
        "github_auth_state",
        "copilot_auth_state",
        "start_copilot_auth",
        "start_github_browser_auth",
        "confirm_copilot_account",
        "confirm_github_account",
        "list_copilot_models",
        "cancel_copilot_models",
        "resolve_provider_repository",
        "monitoring_setup_review",
        "preview_monitoring_activation",
        "cancel_monitoring_activation",
        "apply_monitoring_setup",
        "apply_monitoring_activation",
      ].includes(command)
        ? window.__genieFixture(command, args)
        : original(command, args);
      const settled = result
        .then(
          () => {},
          () => {},
        )
        .finally(() => pending.delete(settled));
      pending.add(settled);
      return result;
    };
  });
  // The native host initializes one Store before its readers. Process-per-command
  // browser fixtures must do that explicitly, not race first-use initialization.
  await store("snapshot");
  await store("panel_snapshot");
  return state;
}

async function seed(store) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [
    {
      id: agentId,
      name: "My reviewer",
      model: "fixture-model",
      ai_account: { provider: "copilot", account_id: "33" },
      doctrines: [],
      prompt: "Review correctness.",
      signature: "machine",
    },
  ];
  settings.repositories = [
    {
      id: repositoryId,
      provider: "github",
      name: "fixture/genie",
      enabled: true,
      provider_account_id: "22",
      provider_repository_id: "100",
      assignments: [
        {
          id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          agent_id: agentId,
          schedule: settings.defaults.schedule,
          comment: false,
          actions: { reply: false, approve: false, merge: false },
        },
      ],
    },
  ];
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

async function saveConfiguration(page) {
  await page.locator('[data-genie-edit="repositories"]').first().click();
  await page
    .locator("[data-repository]")
    .filter({ hasText: "fixture/genie" })
    .click();
  const repository = modal(page, "Settings for fixture/genie");
  await repository
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(repository).toHaveCount(0);
  await back(page);
  await expect(page.getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "4",
  );
  await page.locator("[data-genie-next]").click();
  await expect(activate(page)).toBeEnabled();
}

async function capture(page, name, size = { width: 408, height: 744 }) {
  await page.setViewportSize(size);
  await mkdir(screenshots(page), {
    recursive: true,
  });
  await expect(page.locator(".panel-art")).toBeVisible();
  await expect(page.locator("[data-panel-navigation]")).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await captureImage(page, name);
}

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
async function captureImage(page, name) {
  await expect(page.locator("[data-panel-error]")).toBeHidden();
  const directory = screenshots(page);
  await mkdir(directory, { recursive: true });
  const files = execFileSync("git", [
    "ls-files",
    "-z",
    "src",
    "src-tauri/src",
    "src-tauri/examples",
    "tests/settings",
    "package-lock.json",
  ])
    .toString()
    .split("\0")
    .filter(Boolean);
  const sources = Object.fromEntries(
    await Promise.all(
      files.map(async (file) => [file, hash(await readFile(file))]),
    ),
  );
  const fixture = await page.evaluate(() => window.__genieEvidence());
  const renderer = Object.fromEntries(
    await Promise.all(
      [
        "dist/index.html",
        ...(await readdir("dist/assets"))
          .sort()
          .map((file) => `dist/assets/${file}`),
      ].map(async (file) => [file, hash(await readFile(file))]),
    ),
  );
  const image = await page.screenshot({ path: join(directory, `${name}.png`) });
  await writeFile(
    join(directory, `${name}.json`),
    JSON.stringify(
      {
        kind: "synthetic candidate; not original POC or native acceptance",
        capturedAt: new Date().toISOString(),
        head: execFileSync("git", ["rev-parse", "HEAD"]).toString().trim(),
        sourceSha256: hash(JSON.stringify(sources)),
        sources,
        renderer,
        rendererSha256: hash(JSON.stringify(renderer)),
        fixture,
        fixtureSha256: hash(JSON.stringify(fixture)),
        browser: {
          name: page.context().browser().browserType().name(),
          version: page.context().browser().version(),
          userAgent: await page.evaluate(() => navigator.userAgent),
        },
        viewport: page.viewportSize(),
        reducedMotion: await page.evaluate(
          () => matchMedia("(prefers-reduced-motion: reduce)").matches,
        ),
        test: test.info().titlePath,
        imageSha256: hash(image),
      },
      null,
      2,
    ),
  );
}

test.use({ viewport: { width: 408, height: 744 } });

test("fresh Genie uses shared account choices and authorizes at repository Save, not a final consent", async ({
  page,
  store,
}) => {
  const state = await synthetic(page, store);
  await page.goto("/");
  await page
    .getByRole("button", { name: "Set up with Genie", exact: true })
    .click();
  await page.locator("[data-genie-next]").click();
  const ai = page.locator(".copilot-auth");
  await ai
    .getByRole("button", { name: "Connect Copilot account", exact: true })
    .click();
  expect(state.ai).toBe(false);
  await ai
    .getByRole("button", { name: "Confirm Copilot account", exact: true })
    .click();
  expect(state.code).toBe(false);
  await back(page);
  await page.locator("[data-genie-next]").click();
  const code = page.locator(".github-auth");
  await code
    .getByRole("button", { name: "Add GitHub account", exact: true })
    .click();
  await code.getByRole("button", { name: "Confirm", exact: true }).click();
  await back(page);
  await page.locator("[data-genie-next]").click();
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const agent = modal(page, "New agent");
  await expect(agent.getByLabel("AI account", { exact: true })).toHaveValue("");
  await agent.getByLabel("Name", { exact: true }).fill("My reviewer");
  await agent.getByLabel("AI account", { exact: true }).selectOption("33");
  await expect(agent.getByLabel("Model", { exact: true })).toBeEnabled();
  await expect(agent.getByLabel("Model", { exact: true })).toHaveValue("");
  await agent
    .getByLabel("Model", { exact: true })
    .selectOption("fixture-model");
  await agent.getByRole("button", { name: "Save agent", exact: true }).click();
  await back(page);
  await page.locator("[data-genie-next]").click();
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const add = modal(page, "Add repository by URL");
  await expect(add.getByLabel("Acting GitHub account")).toHaveValue("22");
  await expect(add.getByLabel("Acting GitHub account")).toBeHidden();
  await expect(add.getByRole("status")).toContainText("GitHub / fixture-code");
  await add.getByLabel("Repository URL").fill("fixture/genie");
  await add
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  const repository = modal(page, "Settings for fixture/genie");
  await expect(repository).toBeVisible();
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await repository
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assignment = modal(page, "Assign agent");
  const saved = (await store("snapshot")).settings;
  await assignment
    .getByLabel("Agent", { exact: true })
    .selectOption(saved.agents[0].id);
  for (const permission of ["Publish Comment", "Reply Comment"])
    await expect(
      assignment.getByRole("checkbox", { name: new RegExp(`^${permission}`) }),
    ).not.toBeChecked();
  await expect(
    assignment.getByRole("radio", { name: "Neither", exact: true }),
  ).toBeChecked();
  await expect(
    assignment.getByRole("radio", { name: "Approve", exact: true }),
  ).not.toBeChecked();
  await expect(
    assignment.getByRole("radio", { name: "Approve & Merge", exact: true }),
  ).not.toBeChecked();
  await assignment
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await repository
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(repository).toHaveCount(0);
  const authorized = (await store("snapshot")).settings;
  expect(
    await store("monitoring_activation_status", {
      repositoryId: authorized.repositories[0].id,
    }),
  ).toMatchObject({ active: true, mode: "all_open_and_future" });
  expect(state.previews).toEqual({});
  expect(state.applied).toEqual([]);
  await back(page);
  await expect(page.getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "4",
  );
  await page.locator("[data-genie-next]").click();
  await expect(page.locator("[data-genie-confirm]")).toHaveCount(0);
  await expect(page.locator(".genie-repository")).toContainText(
    "Automatic when eligible",
  );
  await expect(page.locator(".genie-repository")).not.toContainText(
    "Manual start",
  );
  await expect(page.getByLabel("Review start", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Finish setup", exact: true }).click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
  expect((await store("snapshot")).settings).toEqual(authorized);
});

test("shared drafts, save failures, cancellation and hide/reopen retain exact ownership", async ({
  page,
  store,
  dataRoot,
}) => {
  await synthetic(page, store, true);
  await seed(store);
  await page.goto("/");
  await tab(page, "Settings").click();
  await section(page, "Preferences");
  await page.getByLabel("AI capacity", { exact: true }).fill("9");
  await page
    .getByRole("button", { name: "Set up with Genie", exact: true })
    .click();
  await page.locator('[data-genie-edit="agents"]').click();
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  let agent = modal(page, "New agent");
  await agent.getByLabel("Name", { exact: true }).fill("Unsaved reviewer");
  await agent.getByLabel("AI account", { exact: true }).selectOption("33");
  await agent
    .getByLabel("Model", { exact: true })
    .selectOption("fixture-model");
  await agent
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("Retained across hiding.");
  await agent.getByRole("textbox", { name: "Prompt", exact: true }).focus();
  const scroll = await agent
    .locator(".dialog-body")
    .evaluate((e) => e.scrollTop);
  await page.keyboard.press("Escape");
  await expect(page.locator(".panel-shell")).toHaveAttribute(
    "data-native-visible",
    "false",
  );
  await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("fixture_show_panel", {}),
  );
  await expect(
    agent.getByRole("textbox", { name: "Prompt", exact: true }),
  ).toBeFocused();
  expect(await agent.locator(".dialog-body").evaluate((e) => e.scrollTop)).toBe(
    scroll,
  );
  const file = join(dataRoot, "config/settings.json");
  const backup = join(dataRoot, "config/settings.saved.json");
  await rename(file, backup);
  await mkdir(file);
  try {
    await agent
      .getByRole("button", { name: "Save agent", exact: true })
      .click();
    await expect(agent.getByRole("alert")).toContainText(
      "Cannot read settings",
    );
    await expect(agent.getByLabel("Name", { exact: true })).toHaveValue(
      "Unsaved reviewer",
    );
  } finally {
    await rmdir(file);
    await rename(backup, file);
  }
  await agent.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(agent).toHaveCount(0);
  expect((await store("snapshot")).settings.agents).toHaveLength(2);
  expect((await store("snapshot")).settings.capacity).toBe(4);
  await back(page);
  await expect(page.locator('[data-genie-edit="agents"]')).toBeFocused();
  await page.locator('[data-genie-edit="preferences"]').click();
  await expect(page.getByLabel("AI capacity", { exact: true })).toHaveValue(
    "9",
  );
  await back(page);
  await page.locator('[data-genie-edit="agents"]').click();
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  agent = modal(page, "New agent");
  await agent.getByLabel("Name", { exact: true }).fill("Cancelled");
  await agent.getByRole("button", { name: "Cancel", exact: true }).click();
  expect((await store("snapshot")).settings.agents).toHaveLength(2);
  await page.reload();
  expect((await store("snapshot")).settings.agents).toHaveLength(2);
});

test("existing authorized re-entry leaves scope bytes and global pause unchanged", async ({
  page,
  store,
  dataRoot,
}) => {
  const state = await synthetic(page, store, true);
  await seed(store);
  await store("set_automation_paused", { paused: true });
  await page.goto("/");
  await page.locator("[data-genie-next]").click();
  await saveConfiguration(page);
  await activate(page).click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
  const file = join(dataRoot, "config/settings.json");
  const before = await readFile(file, "utf8");
  await tab(page, "Settings").click();
  const refresh = Promise.withResolvers();
  state.beforeRead = () => refresh.promise;
  try {
    await page
      .getByRole("button", { name: "Set up with Genie", exact: true })
      .click();
    await expect(page.locator("[data-genie-next]")).toBeVisible();
  } finally {
    state.beforeRead = undefined;
    refresh.resolve();
  }
  await page.getByRole("button", { name: "Review setup", exact: true }).click();
  await expect(activate(page)).toBeEnabled();
  await expect(page.locator(".genie-page")).toContainText(
    "Authorized by saved configuration",
  );
  await expect(activate(page)).toHaveText("Finish setup");
  await activate(page).click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
  expect(await readFile(file, "utf8")).toBe(before);
  expect((await store("automation_snapshot")).paused).toBe(true);
});

test("legacy Settings offers the same Genie and editor save path", async ({
  page,
  store,
}) => {
  await synthetic(page, store, true);
  await seed(store);
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto("/?view=settings");
  await page
    .getByRole("button", { name: "Set up with Genie", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Genie", exact: true }),
  ).toBeVisible();
  await page.locator('[data-genie-edit="doctrines"]').click();
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const doctrine = modal(page, "New doctrine");
  await doctrine
    .getByLabel("Title", { exact: true })
    .fill("Genie shared principles");
  await doctrine
    .getByLabel("Principles", { exact: true })
    .fill("Preserve account boundaries.");
  await doctrine
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await back(page);
  await page
    .getByRole("button", { name: "Back to Settings", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Genie shared principles", exact: true }),
  ).toBeVisible();
});

test("late shared-save replies cannot return from a newer destination", async ({
  page,
  store,
  ipc,
}) => {
  await synthetic(page, store, true);
  await seed(store);
  await page.goto("/");
  await page.locator('[data-genie-edit="agents"]').click();
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const editor = modal(page, "New agent");
  await editor.getByLabel("Name", { exact: true }).fill("Saved while away");
  await editor.getByLabel("AI account", { exact: true }).selectOption("33");
  await editor
    .getByLabel("Model", { exact: true })
    .selectOption("fixture-model");
  const hold = ipc.holdNext("save_resource");
  try {
    await editor
      .getByRole("button", { name: "Save agent", exact: true })
      .click();
    await hold.arrived;
    await tab(page, "Running").click();
    await expect(page.locator("[data-panel-heading]")).toHaveText("Work queue");
    await tab(page, "Running").focus();
    hold.release();
    await page.evaluate(() => window.__genieIdle());
    await expect(page.locator("[data-panel-heading]")).toHaveText("Work queue");
    await expect(tab(page, "Running")).toBeFocused();
    expect((await store("snapshot")).settings.agents).toHaveLength(2);
    await tab(page, "Settings").click();
    await expect(page.locator("[data-return-genie]")).toBeHidden();
  } finally {
    hold.release();
  }
});

test("repository saves keep mounted account confirmation ownership", async ({
  page,
  store,
}) => {
  const state = await synthetic(page, store, true);
  await seed(store);
  await page.goto("/");
  await page.locator('[data-genie-edit="ai"]').click();
  const ai = page.locator(".copilot-auth");
  await ai
    .getByRole("button", { name: "Connect Copilot account", exact: true })
    .click();
  await expect(
    ai.getByRole("button", { name: "Confirm Copilot account", exact: true }),
  ).toBeVisible();
  await ai.evaluate((element) => {
    element.dataset.ownership = "original-flow";
  });
  await back(page);
  await page.locator('[data-genie-edit="repositories"]').click();
  await page
    .locator("[data-repository]")
    .filter({ hasText: "fixture/genie" })
    .click();
  await modal(page, "Settings for fixture/genie")
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(modal(page, "Settings for fixture/genie")).toHaveCount(0);
  await expect(ai).toHaveAttribute("data-ownership", "original-flow");
  await back(page);
  await page.locator('[data-genie-edit="repository-account"]').click();
  await expect(ai).toHaveAttribute("data-ownership", "original-flow");
  await expect(ai).toBeHidden();
  await expect(page.locator(".github-auth")).toBeVisible();
  await back(page);
  await page.locator('[data-genie-edit="ai"]').click();
  await expect(ai).toHaveAttribute("data-ownership", "original-flow");
  await expect(
    ai.getByRole("button", { name: "Confirm Copilot account", exact: true }),
  ).toBeVisible();
  expect(
    state.commands.filter((command) => command === "start_copilot_auth"),
  ).toHaveLength(1);
});

test("guided account controls keep full keyboard focus inside compact scrollports", async ({
  page,
  store,
}) => {
  await synthetic(page, store, true);
  await seed(store);
  await page.goto("/");
  for (const [width, height] of [
    [320, 300],
    [408, 441],
    [408, 744],
  ]) {
    await page.setViewportSize({ width, height });
    await page.locator('[data-genie-edit="ai"]').click();
    await expect(page.locator("[data-return-genie]")).toBeVisible();
    const buttons = page.locator(".copilot-auth button, .copilot-auth summary");
    await expect(buttons).toHaveCount(5);
    await buttons.first().focus();
    for (let index = 0; index < 5; index++) {
      if (index) await nextControl(page);
      await expect(buttons.nth(index)).toBeFocused();
      await fullyVisible(buttons.nth(index));
    }
    await captureImage(page, `shared-ai-${width}x${height}`);
    await back(page);
    await expect(page.locator('[data-genie-edit="ai"]')).toBeFocused();
  }
});

for (const [name, inherited, local, override, expected] of [
  ["inherited", ["11"], [], undefined, ["11"]],
  ["local", [], ["12"], undefined, ["12"]],
  ["combined", ["11"], ["12"], undefined, ["11", "12"]],
  ["overlap", ["11"], ["11", "12"], undefined, ["11", "12"]],
  ["explicit override", ["11"], ["12"], ["13"], ["12", "13"]],
  ["explicit empty override", ["11"], [], [], []],
]) {
  test(`GEN1 final authors match native admission: ${name}`, async ({
    page,
    store,
  }) => {
    await synthetic(page, store, true);
    const saved = await seed(store);
    const identities = (ids) =>
      ids.map((id) => ({ id, login: `author-${id}` }));
    saved.defaults.watched_authors = identities(inherited);
    saved.repositories[0].watched_authors = identities(local);
    if (override)
      saved.repositories[0].overrides = {
        watched_authors: identities(override),
      };
    await store("seed_settings", saved);
    await page.goto("/");
    // Native admission tests separately prove the effective filter.
    await page.locator("[data-genie-next]").click();
    await saveConfiguration(page);
    const authors = page
      .locator(".genie-repository dt")
      .filter({ hasText: /^Watch choices$/ })
      .locator("+ dd");
    await expect(authors).toHaveText(
      [
        expected.length
          ? expected.map((id) => `@author-${id}`).join(", ")
          : "All authors",
        "Review requested from your GitHub account",
        "@Mentions of your GitHub account",
      ].join("; "),
    );
  });
}

test("GEN3 reconnect of completed account A while B catalog waits requires new evidence", async ({
  page,
  store,
}) => {
  const state = await synthetic(page, store, true);
  const saved = await seed(store);
  const second = structuredClone(saved.agents[0]);
  second.id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
  second.name = "Second reviewer";
  second.ai_account.account_id = "22"; // Same numeric ID as the independent repository role.
  saved.agents.push(second);
  saved.repositories[0].assignments.push({
    ...saved.repositories[0].assignments[0],
    id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
    agent_id: second.id,
  });
  state.extraAi["22"] = { login: "fixture-code", connected: true };
  state.aiGenerations = { 33: 0, 22: 0 };
  await store("seed_settings", saved);
  await page.goto("/");
  await page.locator("[data-genie-next]").click();
  await saveConfiguration(page);
  const held = Promise.withResolvers();
  const entered = Promise.withResolvers();
  const catalogs = [];
  state.beforeModels = async ({ accountId }) => {
    catalogs.push(accountId);
    if (accountId === "22") {
      entered.resolve();
      await held.promise;
    }
  };
  try {
    await activate(page).click();
    await entered.promise;
    expect(catalogs).toEqual(["33", "22"]);
    state.aiGenerations["33"]++;
    state.modelCatalogs["33"] = [];
    held.resolve();
    await expect(page.locator(".genie-error")).toContainText("Setup changed");
    expect(state.applied).toHaveLength(0);
    state.beforeModels = undefined;
    await page.locator("[data-genie-refresh]").click();
    await expect(page.locator(".genie-error")).toContainText(
      "model is unavailable",
    );
    await expect(activate(page)).toBeDisabled();
    expect(
      (await store("monitoring_activation_status", { repositoryId })).active,
    ).toBe(true);
  } finally {
    held.resolve();
  }
});

test("GEN4 unavailable automation remains accessible on Welcome without claiming fresh recovery", async ({
  page,
  store,
}) => {
  await synthetic(page, store);
  await page.clock.install();
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "automation_snapshot" && window.__unavailableAutomation
        ? Promise.reject("Synthetic automation unavailable.")
        : original(command, args);
  });
  await page.goto("/");
  await expect(page.locator("[data-panel-heading]")).toHaveText("Welcome");
  await page.evaluate(() => {
    window.__unavailableAutomation = true;
  });
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__genieIdle());
  await expect(
    page.locator("#automation-controls [data-toggle-automation]"),
  ).toHaveAccessibleName("Monitoring unavailable");
  await expect(
    page.locator("#automation-controls [data-automation-error]"),
  ).toHaveText("Synthetic automation unavailable.");
  await expect(page.locator("[data-panel-heading]")).toHaveText("Welcome");
});

test("GEN4 configured inactive restart preserves Queue and manual Settings without reset", async ({
  page,
  store,
}) => {
  await synthetic(page, store, true);
  const saved = await seed(store);
  saved.repositories[0].enabled = false;
  await store("seed_settings", saved);
  await page.goto("/");
  await page.evaluate(() => window.__genieIdle());
  await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
  await expect(page.locator('[data-panel-view="genie"]')).toBeHidden();
  await expect(page.locator(".queue-item")).toHaveCount(0);
  await tab(page, "Settings").click();
  await page.reload();
  await expect(page.locator(".settings-overview")).toBeVisible();
  await page
    .getByRole("button", { name: "Set up with Genie", exact: true })
    .click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Genie");
  expect((await store("snapshot")).settings).toEqual(saved);
});

for (const failed of [false, true]) {
  test(`GEN4 late initial setup ${failed ? "failure" : "success"} cannot own a newer route or header`, async ({
    page,
    store,
    ipc,
  }) => {
    await store("snapshot");
    const held = ipc.holdNext("monitoring_setup_review");
    let mounting;
    if (failed)
      await page.addInitScript(() => {
        const original = window.__TAURI_INTERNALS__.invoke;
        window.__TAURI_INTERNALS__.invoke = async (command, args) => {
          const result = await original(command, args);
          if (command === "monitoring_setup_review")
            throw "Synthetic setup read failed.";
          return result;
        };
      });
    try {
      await page.goto("/");
      await held.arrived;
      await expect(page.locator("[data-panel-version]")).toHaveText(/^v/);
      mounting = ipc.holdNext("snapshot");
      await tab(page, "Settings").click();
      await expect(page.locator(".settings-window")).toBeVisible();
      await mounting.arrived;
      mounting.release();
      await expect(page.locator("#save-status")).toHaveText(
        "All changes saved",
      );
      // Visible controls precede mount completion and its navigation focus.
      await expect(page.locator("[data-panel-heading]")).toBeFocused();
      await tab(page, "Settings").focus();
      await expect(tab(page, "Settings")).toBeFocused();
      held.release();
      await page.evaluate(() => window.__settingsIdle());
      await expect(page.locator("[data-panel-heading]")).toHaveText("Settings");
      await expect(tab(page, "Settings")).toBeFocused();
      await expect(page.locator("[data-header-automation]")).toBeVisible();
      await expect(page.locator('[data-panel-view="genie"]')).toBeHidden();
    } finally {
      mounting?.release();
      held.release();
    }
  });
}

test("GEN4 failed saved setup read is explicit, never guessed fresh", async ({
  page,
  store,
}) => {
  const state = await synthetic(page, store);
  state.beforeRead = () => {
    throw "Synthetic saved setup unavailable.";
  };
  await page.goto("/");
  await expect(page.locator("[data-panel-error]")).toContainText(
    "Synthetic saved setup unavailable",
  );
  await expect(page.locator('[data-panel-view="genie"]')).toBeHidden();
  await tab(page, "Settings").click();
  await expect(page.locator(".settings-overview")).toBeVisible();
});

for (const size of [
  { width: 320, height: 300 },
  { width: 408, height: 441 },
  { width: 408, height: 744 },
]) {
  for (const reducedMotion of ["no-preference", "reduce"]) {
    test(`GEN5 sequential checklist focus ${size.width}x${size.height} ${reducedMotion}`, async ({
      page,
      store,
    }) => {
      await synthetic(page, store);
      await page.setViewportSize(size);
      await page.emulateMedia({ reducedMotion });
      await page.goto("/");
      await expect(page.locator("[data-genie-next]")).toBeEnabled();
      const rows = page.locator(".genie-checklist button");
      const visited = [];
      for (let step = 0; step < 24 && visited.length < 4; step++) {
        await nextControl(page);
        const index = await rows.evaluateAll((buttons) =>
          buttons.indexOf(document.activeElement),
        );
        if (index < 0) continue;
        expect(index).toBe(visited.length);
        visited.push(index);
        await expect(rows.nth(index)).toBeFocused();
        await expect(rows.nth(index)).toHaveCSS("outline-style", "solid");
        await expect(rows.nth(index)).toHaveCSS("outline-width", "3px");
        await fullyVisible(rows.nth(index));
        await captureImage(page, `checklist-${index + 1}`);
      }
      expect(visited).toEqual([0, 1, 2, 3]);
    });
  }
}
