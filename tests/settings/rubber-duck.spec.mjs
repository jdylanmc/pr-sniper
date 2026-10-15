import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "./fixtures.mjs";
import {
  fixtureAgent,
  repositorySettings,
  section,
  seedBoundRepositories,
  setAllPullRequests,
  assignmentBack,
  saveAssignment,
} from "./navigation.mjs";

const duckId = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const primaryId = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const teammateId = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";
const toolSwitch = (editor, name) =>
  editor.getByRole("switch", { name, exact: true });
const duckRow = (editor) => editor.locator("[data-open-duck]");
const duckSummary = (editor) => editor.locator("[data-duck-summary]");
const toolSummary = (editor) => editor.locator("[data-tool-summary]");

async function seed(page, store, { teammate = false } = {}) {
  const settings = await seedBoundRepositories(store, ["fixture/duck"]);
  settings.agents = [
    { ...fixtureAgent },
    { ...fixtureAgent, id: duckId, name: "Duck", prompt: "Check one claim." },
  ];
  const assignment = (id, agent) => ({
    id,
    agent_id: agent,
    schedule: settings.defaults.schedule,
    comment: false,
    approve: false,
    actions: { reply: false, approve: false, merge: false },
  });
  const repository = settings.repositories[0];
  repository.enabled = false;
  repository.assignments = [assignment(primaryId, fixtureAgent.id)];
  if (teammate) {
    repository.assignments.push(assignment(teammateId, duckId));
    repository.primary_assignment_id = primaryId;
  }
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
}

async function openAssignment(page, index = 0) {
  const repository = await repositorySettings(page, "fixture/duck");
  await setAllPullRequests(repository, false);
  await repository
    .locator(".assignment-row")
    .nth(index)
    .getByRole("button", { name: "Edit assignment", exact: true })
    .click();
  return page.getByRole("dialog", { name: "Edit assignment", exact: true });
}

async function pickDuck(page, editor, name) {
  await duckRow(editor).click();
  await editor
    .getByRole("radiogroup", { name: "Rubber duck" })
    .getByRole("radio", { name, exact: true })
    .check();
  await expect(duckRow(editor)).toBeVisible();
  await expect(duckRow(editor)).toBeFocused();
}

const saved = async (store, index = 0) =>
  (await store("snapshot")).settings.repositories[0].assignments[index];

for (const width of [408, 320]) {
  test.describe(`at ${width}px`, () => {
    test.use({ viewport: { width, height: 744 } });

    test("the primary picks a Rubber duck; its tool follows and saves", async ({
      page,
      store,
    }, testInfo) => {
      await seed(page, store);
      const editor = await openAssignment(page);
      await expect(duckSummary(editor)).toHaveText("None");
      await expect(toolSummary(editor)).toHaveText("9");
      await pickDuck(page, editor, "Duck");
      await expect(duckSummary(editor)).toHaveText("Duck");
      await expect(toolSummary(editor)).toHaveText("10");
      await editor.locator("[data-open-tools]").click();
      await expect(toolSwitch(editor, "Rubber duck")).toBeChecked();
      await expect(
        editor.locator('[data-tool-row="rubber_duck"] [data-tool-detail]'),
      ).toHaveText("Second Agent checks one finding");
      await page.screenshot({
        path: testInfo.outputPath(`duck-tools-${width}.png`),
      });
      await assignmentBack(page, editor);
      await saveAssignment(page, editor);
      const first = await saved(store);
      expect(first.rubber_duck).toBe(duckId);
      expect(first.tools).toBeUndefined();
      const repository = await repositorySettings(page, "fixture/duck");
      await expect(repository.locator(".assignment-row").first()).toContainText(
        "Rubber duck",
      );

      // The same Agent may be its own duck.
      const again = await openAssignment(page);
      await expect(duckSummary(again)).toHaveText("Duck");
      await pickDuck(page, again, fixtureAgent.name);
      await saveAssignment(page, again);
      expect((await saved(store)).rubber_duck).toBe(fixtureAgent.id);

      // None turns the tool off.
      const cleared = await openAssignment(page);
      await pickDuck(page, cleared, "None");
      await expect(duckSummary(cleared)).toHaveText("None");
      await cleared.locator("[data-open-tools]").click();
      await expect(toolSwitch(cleared, "Rubber duck")).toBeDisabled();
      await expect(toolSwitch(cleared, "Rubber duck")).not.toBeChecked();
      await saveAssignment(page, cleared);
      expect((await saved(store)).rubber_duck).toBeUndefined();
    });

    test("an explicit tool list gains rubber_duck when a duck is chosen", async ({
      page,
      store,
    }) => {
      await seed(page, store);
      const editor = await openAssignment(page);
      await editor.locator("[data-open-tools]").click();
      await toolSwitch(editor, "Tree").uncheck();
      await assignmentBack(page, editor);
      await pickDuck(page, editor, "Duck");
      await saveAssignment(page, editor);
      const value = await saved(store);
      expect(value.tools).toContain("rubber_duck");
      expect(value.tools).not.toContain("list_tree");
      expect(value.rubber_duck).toBe(duckId);
    });

    test("a teammate has no Rubber duck control and no duck tool", async ({
      page,
      store,
    }) => {
      await seed(page, store, { teammate: true });
      const editor = await openAssignment(page, 1);
      await expect(duckRow(editor)).toBeHidden();
      await editor.locator("[data-open-tools]").click();
      await expect(toolSwitch(editor, "Rubber duck")).toBeHidden();
      await assignmentBack(page, editor);
      await saveAssignment(page, editor);
      expect((await saved(store, 1)).rubber_duck).toBeUndefined();
    });

    test("Preferences tune the duck limits and refuse out-of-range values", async ({
      page,
      store,
      dataRoot,
    }, testInfo) => {
      await store("snapshot");
      await page.goto("/?view=settings");
      await page.evaluate(() => window.__settingsIdle());
      await section(page, "Preferences");
      await page.evaluate(() => window.__settingsIdle());
      const group = page.getByRole("group", { name: "Rubber duck" });
      const limit = (key) => group.locator(`[data-duck-limit="${key}"]`);
      await expect(limit("max_calls")).toHaveValue("8");
      await expect(limit("call_timeout_seconds")).toHaveValue("180");
      await expect(limit("primary_window_minutes")).toHaveValue("15");
      await group.scrollIntoViewIfNeeded();
      const box = await group.boundingBox();
      expect(box.x + box.width).toBeLessThanOrEqual(width);
      await page.screenshot({
        path: testInfo.outputPath(`duck-preferences-${width}.png`),
      });
      await limit("max_calls").fill("12");
      await limit("call_timeout_seconds").fill("240");
      await limit("primary_window_minutes").fill("30");
      await page.locator("#save-settings").click();
      await page.evaluate(() => window.__settingsIdle());
      await expect(page.locator("#save-status")).toHaveText(
        "All changes saved",
      );
      expect((await store("snapshot")).settings.rubber_duck).toEqual({
        max_calls: 12,
        call_timeout_seconds: 240,
        primary_window_minutes: 30,
      });
      const before = await readFile(join(dataRoot, "config/settings.json"));
      for (const [key, value] of [
        ["max_calls", "0"],
        ["max_calls", "33"],
        ["call_timeout_seconds", "29"],
        ["primary_window_minutes", "121"],
      ]) {
        await limit(key).fill(value);
        await page.locator("#save-settings").click();
        await page.evaluate(() => window.__settingsIdle());
        await expect(page.locator("#error")).toBeVisible();
        await expect(page.locator("#save-status")).toHaveText(
          "Unsaved changes",
        );
        await expect(limit(key)).toHaveValue(value);
        expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
          before,
        );
        await limit(key).fill(
          {
            max_calls: "12",
            call_timeout_seconds: "240",
            primary_window_minutes: "30",
          }[key],
        );
      }
    });
  });
}
