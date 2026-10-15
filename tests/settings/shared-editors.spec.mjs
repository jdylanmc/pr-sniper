import { test, expect } from "./fixtures.mjs";
import { fixtureAgent, section, seedBoundRepositories } from "./navigation.mjs";
import { mkdir, readFile, rm } from "node:fs/promises";
import { join } from "node:path";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const editor = (page, name) => page.getByRole("dialog", { name, exact: true });
const card = (page, kind, name) =>
  page.locator(`.${kind}-card`).filter({
    has: page.getByRole("heading", { name, exact: true }),
  });
const openAgent = async (page, name = fixtureAgent.name) => {
  await section(page, "Agents");
  await card(page, "agent", name)
    .getByRole("heading", { name, exact: true })
    .click({ force: true });
  return editor(page, `Editing ${name}`);
};
const openPicker = async (page, modal) => {
  await modal.locator("[data-open-doctrines]").click();
  await expect(modal.locator('form[data-view="doctrines"]')).toBeVisible();
};
const closePicker = async (page, modal) => {
  await page.locator("[data-panel-back]").click();
  await expect(modal.locator('form[data-view="doctrines"]')).toHaveCount(0);
};
const openDoctrine = async (page, title) => {
  await card(page, "doctrine", title)
    .getByRole("heading", { name: title, exact: true })
    .click({ force: true });
  return editor(page, `Editing ${title}`);
};
const closeEditor = (page) => page.locator("[data-panel-back]").click();
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
  // Stored shape: library ids assigned, title references migrated to ids.
  return (await store("snapshot")).settings;
}

/** The first Agent's Doctrine references (ids) as titles, in its order. */
const agentDoctrines = async (store) => {
  const settings = (await store("snapshot")).settings;
  return (settings.agents[0].doctrine_ids ?? []).map(
    (id) => settings.doctrines.find((d) => d.id === id)?.title ?? id,
  );
};

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
  await expect(
    page.getByRole("button", { name: "New agent", exact: true }),
  ).toBeEnabled();
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
  await modal.locator("textarea[name=prompt]").fill(longText);
  await modal.locator("input[name=signature]").fill("Stored custom signature");
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
  await expect(modal.locator("textarea[name=prompt]")).toHaveValue(longText);
  await expect(modal.locator("input[name=signature]")).toHaveValue(
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
  modal = await openDoctrine(page, "Principle 01");
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
  await openPicker(page, modal);
  const filter = modal.getByLabel("Filter doctrines", { exact: true });
  const summary = modal.locator("[data-doctrine-summary]");
  const shown = modal.locator("[data-doctrine-choice]:not([hidden])");
  const first = modal.locator(`input[name=doctrine][value="Principle 01"]`);
  await first.check();
  await filter.fill("36");
  await expect(summary).toHaveText("1 selected");
  await expect(shown).toHaveCount(1);
  const last = modal.locator(`input[name=doctrine][value="Principle 36"]`);
  await last.focus();
  await page.keyboard.press("Space");
  await filter.fill("no match");
  await expect(modal.locator("[data-no-doctrines]")).toContainText(
    "No matches",
  );
  await expect(summary).toHaveText("2 selected");
  await expect(shown).toHaveCount(0);
  await filter.fill("");
  await first.focus();
  for (let i = 0; i < 70; i++) {
    if (await last.evaluate((node) => node === document.activeElement)) break;
    await page.keyboard.press("Tab");
  }
  await expect(last).toBeFocused();
  await expect(last).toBeChecked();
  expect(
    await last.evaluate((node) => {
      for (let n = node; n; n = n.parentElement)
        if (n.scrollTop > 0) return n.scrollTop;
      return 0;
    }),
  ).toBeGreaterThan(0);
  await capture(page, testInfo, "doctrine-keyboard-scroll");
  await closePicker(page, modal);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect(await agentDoctrines(store)).toEqual(["Principle 01", "Principle 36"]);
  await page.reload();
  modal = await openAgent(page);
  await openPicker(page, modal);
  await modal.locator(`input[name=doctrine][value="Principle 01"]`).uncheck();
  await closePicker(page, modal);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect(await agentDoctrines(store)).toEqual(["Principle 36"]);
  modal = await openAgent(page);
  await openPicker(page, modal);
  await modal.locator(`input[name=doctrine][value="Principle 36"]`).uncheck();
  await modal.getByLabel("Filter doctrines", { exact: true }).fill("01");
  await closePicker(page, modal);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  await page.reload();
  expect(await agentDoctrines(store)).toEqual([]);
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
    const intendedTitles = ["Principle 03", "Principle 02", "Principle 04"];
    const intended = {
      ...original.agents[0],
      prompt: "Keep these unfinished review instructions.",
      signature: "Deliberately saved signature",
      doctrine_ids: intendedTitles.map(
        (title) => original.doctrines.find((d) => d.title === title).id,
      ),
    };

    for (const action of ["Cancel", "Save agent"]) {
      const modal = await openAgent(page);
      const prompt = modal.locator("textarea[name=prompt]");
      const signature = modal.locator("input[name=signature]");
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
      await openPicker(page, modal);
      await modal
        .locator(`input[name=doctrine][value="Principle 01"]`)
        .uncheck();
      await modal.locator(`input[name=doctrine][value="Principle 04"]`).check();
      await modal.locator(`input[name=doctrine][value="Principle 02"]`).check();
      await filter.fill(query);
      await expect(modal.locator("[data-doctrine-summary]")).toHaveText(
        "3 selected",
      );
      await expect(
        modal.locator("[data-doctrine-choice]:not([hidden])"),
      ).toHaveCount(query === "03" ? 1 : 0);
      await expect(filter).toBeFocused();
      await filter.press("Enter");
      await page.evaluate(() => window.__settingsIdle());
      expect.soft((await store("snapshot")).settings).toEqual(original);
      await expect(modal).toBeVisible();
      await expect(filter).toBeFocused();
      await expect(filter).toHaveValue(query);
      await expect(prompt).toHaveValue(intended.prompt);
      await expect(signature).toHaveValue(intended.signature);
      await expect(modal.locator("[data-doctrine-summary]")).toHaveText(
        "3 selected",
      );
      await expect(modal.locator("[data-no-doctrines]")).toHaveJSProperty(
        "hidden",
        query !== "no match",
      );
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
          checked: intendedTitles.includes(title),
          hidden: !title.includes(query),
        })),
      );
      await closePicker(page, modal);
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
  const prompt = modal.locator("textarea[name=prompt]");
  await prompt.fill("First line");
  await prompt.press("End");
  await prompt.press("Enter");
  await expect(prompt).toHaveValue("First line\n");
  await expect(modal).toBeVisible();
  const signature = modal.locator("input[name=signature]");
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
    .locator("textarea[name=prompt]")
    .fill("Successfully saved prompt.");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  for (const action of ["Cancel", "Back"]) {
    modal = await openAgent(page);
    await modal
      .locator("textarea[name=prompt]")
      .fill("Discard only this text.");
    await openPicker(page, modal);
    await modal.locator(`input[name=doctrine][value="Principle 01"]`).check();
    await closePicker(page, modal);
    if (action === "Cancel")
      await modal.getByRole("button", { name: action, exact: true }).click();
    else await closeEditor(page);
    await expect(modal).toHaveCount(0);
    // Panel Back leaves focus on the hidden Back button (known gap); Cancel restores it.
    if (action === "Cancel")
      await expect(
        card(page, "agent", fixtureAgent.name).locator("[data-edit]"),
      ).toBeFocused();
  }
  await section(page, "Doctrines");
  modal = await openDoctrine(page, "Principle 01");
  await modal
    .getByRole("textbox", { name: "Principles", exact: true })
    .fill("Saved edited principles.");
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  modal = await openDoctrine(page, "Principle 01");
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
    id: initial.doctrines[0].id,
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
  // Deleting an assigned Agent now asks first and unassigns it; Keep leaves everything intact.
  await expect(modal.locator("[data-delete-confirmation]")).toBeVisible();
  await modal.getByRole("button", { name: "Keep agent" }).click();
  await expect(modal.locator("[data-delete-confirmation]")).toBeHidden();
  await expect(page.getByRole("dialog")).toHaveCount(1);
  expect((await store("snapshot")).settings.repositories).toEqual(
    settings.repositories,
  );
  await capture(page, testInfo, "agent-delete-guard");
  await openPicker(page, modal);
  await modal.locator(`input[name=doctrine][value="Principle 01"]`).uncheck();
  await closePicker(page, modal);
  await modal.getByRole("button", { name: "Cancel", exact: true }).click();
  await section(page, "Doctrines");
  modal = await openDoctrine(page, "Principle 01");
  await expect(modal.locator(".resource-impact")).toContainText(
    fixtureAgent.name,
  );
  await modal
    .getByRole("button", { name: "Delete doctrine", exact: true })
    .click();
  await expect(modal.getByRole("alert")).toContainText("In use by an Agent");
  await capture(page, testInfo, "doctrine-delete-guard");
  await modal.getByRole("button", { name: "Cancel", exact: true }).click();
  modal = await openAgent(page);
  await openPicker(page, modal);
  await modal.locator(`input[name=doctrine][value="Principle 01"]`).uncheck();
  await closePicker(page, modal);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  await section(page, "Doctrines");
  modal = await openDoctrine(page, "Principle 01");
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
  await expect(page.locator(".doctrine-card")).toHaveCount(0);
  await page.reload();
  expect((await store("snapshot")).settings.doctrines).toEqual([]);
  modal = await openAgent(page);
  await expect(modal.locator("[data-no-doctrines]")).toContainText("None");
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
  await modal.locator("textarea[name=prompt]").fill("Retry this exact draft.");
  const path = join(dataRoot, "config/settings.json");
  const before = await readFile(path);
  const obstruction = join(dataRoot, "config/settings.json.tmp");
  await mkdir(obstruction);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toBeVisible();
  expect(await readFile(path)).toEqual(before);
  await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
    "Retry this exact draft.",
  );
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
    .locator("textarea[name=prompt]")
    .fill("Retain on destination changes.");
  await tab(page, "Running").click();
  await tab(page, "Settings").click();
  await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
    "Retain on destination changes.",
  );
  await closeEditor(page);
  await page.reload();
  expect((await store("snapshot")).settings.agents[0].prompt).toBe(
    "Retry this exact draft.",
  );
  await tab(page, "Settings").click();
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
});

test("deleting an assigned Agent confirms, then removes its repository assignments", async ({
  page,
  store,
}) => {
  await seed(page, store, 1);
  const settings = await seedBoundRepositories(store, ["fixture/shared"]);
  settings.repositories[0].enabled = false;
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
  const modal = await openAgent(page);
  await modal
    .getByRole("button", { name: "Delete agent", exact: true })
    .click();
  await modal.getByRole("button", { name: "Confirm deletion" }).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(modal).toHaveCount(0);
  const saved = (await store("snapshot")).settings;
  expect(saved.agents ?? []).toEqual([]);
  expect(saved.repositories[0].assignments ?? []).toEqual([]);
});
