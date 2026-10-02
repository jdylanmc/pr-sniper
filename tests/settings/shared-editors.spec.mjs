import { test, expect } from "./fixtures.mjs";
import { fixtureAgent } from "./navigation.mjs";
import { mkdir, readFile, rm } from "node:fs/promises";
import { join } from "node:path";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const editor = (page, name) => page.getByRole("dialog", { name, exact: true });
const section = async (page, name) => {
  const control = page.getByLabel("Settings section", { exact: true });
  await expect(control).toBeVisible();
  await control.selectOption({ label: name });
};
const card = (page, kind, name) =>
  page.locator(`.${kind}-card`).filter({
    has: page.getByRole("heading", { name, exact: true }),
  });
const openAgent = async (page, name = fixtureAgent.name) => {
  await section(page, "Agents");
  await card(page, "agent", name)
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  return editor(page, "Edit agent");
};
const longText = Array.from(
  { length: 120 },
  (_, i) => `Principle ${i + 1}: preserve evidence and review carefully.`,
).join("\n");

async function accountFixture(page) {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "copilot_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "copilot",
              account_id: "101",
              login: "fixture-ai",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        });
      if (command === "list_copilot_models")
        return Promise.resolve([
          { id: "fixture-model", name: "Fixture model" },
        ]);
      return original(command, args);
    };
  });
}

async function seed(page, store, count = 36) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [{ ...fixtureAgent, prompt: longText, doctrines: [] }];
  settings.doctrines = Array.from({ length: count }, (_, i) => ({
    title: `Principle ${String(i + 1).padStart(2, "0")}`,
    body: `${longText}\nLibrary item ${i + 1}.`,
  }));
  await store("seed_settings", settings);
  await accountFixture(page);
  await page.goto("/");
  await tab(page, "Settings").click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  return settings;
}

async function capture(page, testInfo, name) {
  await page.screenshot({ path: testInfo.outputPath(`${name}.png`) });
}

test.use({ viewport: { width: 408, height: 744 } });

test("compact libraries expose full editors and unlimited Agent saves without granting authority", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seed(page, store);
  initial.agents = Array.from({ length: 12 }, (_, i) => ({
    ...fixtureAgent,
    id: `${String(i + 1).padStart(8, "0")}-aaaa-4aaa-8aaa-aaaaaaaaaaaa`,
    name: `Reviewer ${i + 1}`,
  }));
  await store("seed_settings", initial);
  await page.reload();
  await section(page, "Agents");
  await expect(page.locator(".agent-card")).toHaveCount(12);
  await expect(page.locator(".resource-toolbar")).toContainText(
    "Unlimited saved configurations",
  );
  await capture(page, testInfo, "agent-library");
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  let modal = editor(page, "New agent");
  await expect(modal.getByLabel("AI account", { exact: true })).toHaveValue("");
  await expect(modal.getByLabel("Model", { exact: true })).toHaveValue("");
  await expect(modal.getByLabel("Model", { exact: true })).toBeDisabled();
  await capture(page, testInfo, "new-agent-explicit-selection");
  await modal.getByLabel("Name", { exact: true }).fill("Thirteenth reviewer");
  await modal.getByLabel("AI account", { exact: true }).selectOption("101");
  await expect(modal.getByLabel("Model", { exact: true })).toBeEnabled();
  await expect(modal.getByLabel("Model", { exact: true })).toHaveValue("");
  await modal
    .getByLabel("Model", { exact: true })
    .selectOption("fixture-model");
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill(longText);
  await modal
    .getByLabel("Signature", { exact: true })
    .fill("Stored custom signature");
  await expect(
    modal.getByRole("checkbox", { name: /Comment|Approve|Merge|Primary/i }),
  ).toHaveCount(0);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  const saved = (await store("snapshot")).settings;
  expect(saved.agents).toHaveLength(13);
  expect(saved.capacity).toBe(initial.capacity);
  expect(saved.repositories).toEqual(initial.repositories);
  await page.reload();
  modal = await openAgent(page, "Thirteenth reviewer");
  await expect(
    modal.getByRole("textbox", { name: "Prompt", exact: true }),
  ).toHaveValue(longText);
  await expect(modal.getByLabel("Signature", { exact: true })).toHaveValue(
    "Stored custom signature",
  );
  await capture(page, testInfo, "agent-editor");
  await modal.getByRole("button", { name: "Cancel", exact: true }).click();
  await section(page, "Doctrines");
  await page.locator("#content").evaluate((element) => {
    element.scrollTop = 0;
  });
  await capture(page, testInfo, "doctrine-library");
  const rows = await page
    .locator(".doctrine-card")
    .evaluateAll((elements) =>
      elements.map((element) => element.getBoundingClientRect().height),
    );
  expect(Math.max(...rows)).toBeLessThan(115);
  await card(page, "doctrine", "Principle 01")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  modal = editor(page, "Edit doctrine");
  await expect(
    modal.getByRole("textbox", { name: "Principles", exact: true }),
  ).toHaveValue(initial.doctrines[0].body);
  await capture(page, testInfo, "doctrine-editor");
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .scrollIntoViewIfNeeded();
  await capture(page, testInfo, "doctrine-editor-actions");
});

test("mouse and keyboard reach scrolling doctrines and preserve zero, one and many through filters", async ({
  page,
  store,
}, testInfo) => {
  await seed(page, store);
  let modal = await openAgent(page);
  const filter = modal.getByLabel("Filter doctrines", { exact: true });
  const first = modal.getByRole("checkbox", {
    name: "Principle 01",
    exact: true,
  });
  await first.check();
  await filter.fill("36");
  await expect(modal.locator("[data-selection-count]")).toHaveText(
    "1 selected (1 hidden by filter) / 1 shown",
  );
  const last = modal.getByRole("checkbox", {
    name: "Principle 36",
    exact: true,
  });
  await last.focus();
  await page.keyboard.press("Space");
  await filter.fill("no match");
  await expect(modal.locator("[data-no-doctrines]")).toContainText(
    "No matching doctrines",
  );
  await expect(modal.locator("[data-selection-count]")).toHaveText(
    "2 selected (2 hidden by filter) / 0 shown",
  );
  await filter.fill("");
  await first.focus();
  for (let i = 0; i < 35; i++) await page.keyboard.press("Tab");
  await expect(last).toBeFocused();
  await expect(last).toBeChecked();
  expect(
    await modal.locator(".doctrine-choices").evaluate((node) => node.scrollTop),
  ).toBeGreaterThan(0);
  await capture(page, testInfo, "doctrine-keyboard-scroll");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].doctrines).toEqual([
    "Principle 01",
    "Principle 36",
  ]);
  await page.reload();
  modal = await openAgent(page);
  await modal
    .getByRole("checkbox", { name: "Principle 01", exact: true })
    .uncheck();
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].doctrines).toEqual([
    "Principle 36",
  ]);
  modal = await openAgent(page);
  await modal
    .getByRole("checkbox", { name: "Principle 36", exact: true })
    .uncheck();
  await modal.getByLabel("Filter doctrines", { exact: true }).fill("01");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  await page.reload();
  expect((await store("snapshot")).settings.agents[0].doctrines).toEqual([]);
});

for (const query of ["03", "no match"]) {
  test(`Enter in doctrine filter "${query}" keeps Agent edits unsaved until keyboard Save`, async ({
    page,
    store,
  }) => {
    const settings = await seed(page, store, 4);
    await page.evaluate(() => window.__settingsIdle());
    settings.agents[0].doctrines = ["Principle 03", "Principle 01"];
    await store("seed_settings", settings);
    await page.reload();
    await page.evaluate(() => window.__settingsIdle());
    const original = (await store("snapshot")).settings;
    const intended = {
      ...original.agents[0],
      prompt: "Keep these unfinished review instructions.",
      signature: "Deliberately saved signature",
      doctrines: ["Principle 03", "Principle 02", "Principle 04"],
    };

    for (const action of ["Cancel", "Save agent"]) {
      const modal = await openAgent(page);
      const prompt = modal.getByRole("textbox", {
        name: "Prompt",
        exact: true,
      });
      const signature = modal.getByLabel("Signature", { exact: true });
      const filter = modal.getByLabel("Filter doctrines", { exact: true });
      await expect(prompt).toHaveValue(original.agents[0].prompt);
      await expect(signature).toHaveValue(original.agents[0].signature);
      expect(
        await modal
          .locator("[name=doctrine]:checked")
          .evaluateAll((inputs) => inputs.map((input) => input.value)),
      ).toEqual(["Principle 01", "Principle 03"]);
      await prompt.fill(intended.prompt);
      await signature.fill(intended.signature);
      await modal
        .getByRole("checkbox", { name: "Principle 01", exact: true })
        .uncheck();
      await modal
        .getByRole("checkbox", { name: "Principle 04", exact: true })
        .check();
      await modal
        .getByRole("checkbox", { name: "Principle 02", exact: true })
        .check();
      await filter.fill(query);
      const count =
        query === "03"
          ? "3 selected (2 hidden by filter) / 1 shown"
          : "3 selected (3 hidden by filter) / 0 shown";
      await expect(modal.locator("[data-selection-count]")).toHaveText(count);
      await expect(filter).toBeFocused();
      await filter.press("Enter");
      await page.evaluate(() => window.__settingsIdle());
      expect.soft((await store("snapshot")).settings).toEqual(original);
      await expect(modal).toBeVisible();
      await expect(filter).toBeFocused();
      await expect(filter).toHaveValue(query);
      await expect(prompt).toHaveValue(intended.prompt);
      await expect(signature).toHaveValue(intended.signature);
      await expect(modal.locator("[data-selection-count]")).toHaveText(count);
      await expect(modal.locator("[data-no-doctrines]")).toBeVisible({
        visible: query === "no match",
      });
      expect(
        await modal.locator("[name=doctrine]").evaluateAll((inputs) =>
          inputs.map((input) => ({
            title: input.value,
            checked: input.checked,
            hidden: input.closest("[data-doctrine-choice]").hidden,
          })),
        ),
      ).toEqual(
        original.doctrines.map(({ title }) => ({
          title,
          checked: intended.doctrines.includes(title),
          hidden: !title.includes(query),
        })),
      );
      const button = modal.getByRole("button", { name: action, exact: true });
      await button.focus();
      await expect(button).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(modal).toHaveCount(0);
      await page.evaluate(() => window.__settingsIdle());
      expect((await store("snapshot")).settings).toEqual(
        action === "Cancel" ? original : { ...original, agents: [intended] },
      );
    }
    await page.reload();
    await page.evaluate(() => window.__settingsIdle());
    expect((await store("snapshot")).settings).toEqual({
      ...original,
      agents: [intended],
    });
  });
}

test("Enter outside the doctrine filter retains ordinary Agent form behavior", async ({
  page,
  store,
}) => {
  await seed(page, store);
  const modal = await openAgent(page);
  await page.evaluate(() => window.__settingsIdle());
  const original = (await store("snapshot")).settings;
  const prompt = modal.getByRole("textbox", { name: "Prompt", exact: true });
  await prompt.fill("First line");
  await prompt.press("End");
  await prompt.press("Enter");
  await expect(prompt).toHaveValue("First line\n");
  await expect(modal).toBeVisible();
  const signature = modal.getByLabel("Signature", { exact: true });
  await signature.fill("Ordinary text-field submission");
  await signature.press("Enter");
  await expect(modal).toHaveCount(0);
  await page.evaluate(() => window.__settingsIdle());
  expect((await store("snapshot")).settings).toEqual({
    ...original,
    agents: [
      {
        ...original.agents[0],
        prompt: "First line\n",
        signature: "Ordinary text-field submission",
      },
    ],
  });
});

test("Cancel and Back discard only unsaved fields while saved shared resources and other drafts survive", async ({
  page,
  store,
}) => {
  const initial = await seed(page, store);
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("9");
  let modal = await openAgent(page);
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("Successfully saved prompt.");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  for (const action of ["Cancel", "Close dialog"]) {
    modal = await openAgent(page);
    await modal
      .getByRole("textbox", { name: "Prompt", exact: true })
      .fill("Discard only this text.");
    await modal
      .getByRole("checkbox", { name: "Principle 01", exact: true })
      .check();
    await modal.getByRole("button", { name: action, exact: true }).click();
    await expect(modal).toHaveCount(0);
    await expect(
      card(page, "agent", fixtureAgent.name).getByRole("button", {
        name: "Edit",
        exact: true,
      }),
    ).toBeFocused();
  }
  await section(page, "Doctrines");
  await card(page, "doctrine", "Principle 01")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  modal = editor(page, "Edit doctrine");
  await modal
    .getByRole("textbox", { name: "Principles", exact: true })
    .fill("Saved edited principles.");
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  await card(page, "doctrine", "Principle 01")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  modal = editor(page, "Edit doctrine");
  await modal.getByLabel("Title", { exact: true }).fill("Not saved");
  await modal.getByRole("button", { name: "Cancel", exact: true }).click();
  await section(page, "Preferences");
  await expect(page.locator("#global-capacity")).toHaveValue("9");
  await page.reload();
  const saved = (await store("snapshot")).settings;
  expect(saved.capacity).toBe(initial.capacity);
  expect(saved.agents[0]).toEqual({
    ...initial.agents[0],
    prompt: "Successfully saved prompt.",
  });
  expect(saved.doctrines[0]).toEqual({
    title: "Principle 01",
    body: "Saved edited principles.",
  });
});

test("shared-use guards and inline deletion preserve references, confirmation cancellation and empty libraries", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seed(page, store, 1);
  await store("save_repository", { repository: "fixture/shared" });
  const settings = (await store("snapshot")).settings;
  settings.agents[0].doctrines = ["Principle 01"];
  settings.repositories[0].assignments = [
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      agent_id: settings.agents[0].id,
      schedule: settings.defaults.schedule,
      comment: false,
      approve: false,
    },
  ];
  await store("seed_settings", settings);
  await page.reload();
  let modal = await openAgent(page);
  await expect(modal.locator(".resource-impact")).toContainText(
    "fixture/shared",
  );
  await modal
    .getByRole("button", { name: "Delete agent", exact: true })
    .click();
  await expect(modal.getByRole("alert")).toContainText(
    "assigned to a repository",
  );
  await expect(page.getByRole("dialog")).toHaveCount(1);
  await capture(page, testInfo, "agent-delete-guard");
  await modal
    .getByRole("checkbox", { name: "Principle 01", exact: true })
    .uncheck();
  await modal.getByRole("button", { name: "Cancel", exact: true }).click();
  await section(page, "Doctrines");
  await card(page, "doctrine", "Principle 01")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  modal = editor(page, "Edit doctrine");
  await expect(modal.locator(".resource-impact")).toContainText(
    fixtureAgent.name,
  );
  await modal
    .getByRole("button", { name: "Delete doctrine", exact: true })
    .click();
  await expect(modal.getByRole("alert")).toContainText("used by an Agent");
  await capture(page, testInfo, "doctrine-delete-guard");
  await modal.getByRole("button", { name: "Cancel", exact: true }).click();
  modal = await openAgent(page);
  await modal
    .getByRole("checkbox", { name: "Principle 01", exact: true })
    .uncheck();
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  await section(page, "Doctrines");
  await card(page, "doctrine", "Principle 01")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  modal = editor(page, "Edit doctrine");
  await modal
    .getByRole("button", { name: "Delete doctrine", exact: true })
    .click();
  await expect(
    modal.getByRole("button", { name: "Confirm deletion" }),
  ).toBeFocused();
  await modal.getByRole("button", { name: "Keep doctrine" }).click();
  expect((await store("snapshot")).settings.doctrines).toEqual(
    initial.doctrines,
  );
  await modal
    .getByRole("button", { name: "Delete doctrine", exact: true })
    .click();
  await modal.getByRole("button", { name: "Confirm deletion" }).click();
  await expect(modal).toHaveCount(0);
  await expect(page.locator(".settings-heading h1")).toBeFocused();
  await page.reload();
  expect((await store("snapshot")).settings.doctrines).toEqual([]);
  modal = await openAgent(page);
  await expect(modal.locator("[data-no-doctrines]")).toContainText(
    "No doctrines yet",
  );
  expect((await store("snapshot")).settings.repositories).toEqual(
    settings.repositories,
  );
});

test("failed saves retain the draft and valid disk state, retry and tiny viewport remain usable", async ({
  page,
  store,
  dataRoot,
}, testInfo) => {
  await seed(page, store);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 320, height: 300 });
  let modal = await openAgent(page);
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("Retry this exact draft.");
  const path = join(dataRoot, "config/settings.json");
  const before = await readFile(path);
  const obstruction = join(dataRoot, "config/settings.json.tmp");
  await mkdir(obstruction);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toBeVisible();
  expect(await readFile(path)).toEqual(before);
  await expect(
    modal.getByRole("textbox", { name: "Prompt", exact: true }),
  ).toHaveValue("Retry this exact draft.");
  await expect(tab(page, "Running")).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await capture(page, testInfo, "save-failure-320x300");
  await rm(obstruction, { recursive: true });
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  modal = await openAgent(page);
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("Retain on destination changes.");
  await tab(page, "Running").click();
  await tab(page, "Settings").click();
  await expect(
    modal.getByRole("textbox", { name: "Prompt", exact: true }),
  ).toHaveValue("Retain on destination changes.");
  await modal
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await page.reload();
  expect((await store("snapshot")).settings.agents[0].prompt).toBe(
    "Retry this exact draft.",
  );
  modal = await openAgent(page);
  await modal
    .getByRole("button", { name: "Delete agent", exact: true })
    .click();
  await mkdir(obstruction);
  const beforeDeletion = await readFile(path);
  await modal
    .getByRole("button", { name: "Confirm deletion", exact: true })
    .click();
  await expect(modal.getByRole("alert")).toContainText("Cannot write settings");
  expect(await readFile(path)).toEqual(beforeDeletion);
  await rm(obstruction, { recursive: true });
  await modal
    .getByRole("button", { name: "Confirm deletion", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  await page.reload();
  expect((await store("snapshot")).settings.agents).toBeUndefined();
  await section(page, "Agents");
  await expect(page.locator(".agent-card")).toHaveCount(0);
  await expect(page.getByText("No agents yet", { exact: true })).toBeVisible();
});
