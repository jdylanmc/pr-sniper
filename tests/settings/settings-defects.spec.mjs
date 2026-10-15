import { expect, test } from "./fixtures.mjs";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  seedAgent,
  setAgentPrompt,
  saveChanges,
  closeDialog,
  editAgent,
  section,
  repositorySettings,
} from "./navigation.mjs";

test("a settings conflict keeps the draft until explicit discard and reload", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await page.goto("/?view=settings");
  const modal = await editAgent(page);
  await modal.locator("textarea[name=prompt]").fill("My unsaved local draft");

  const external = (await store("snapshot")).settings;
  external.agents[0].prompt = "A concurrent external edit";
  await store("seed_settings", external);

  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toContainText(
    "Resource changed in another window",
  );
  await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
    "My unsaved local draft",
  );
  await closeDialog(page);
  await page
    .getByRole("button", {
      name: "Discard draft and reload",
      exact: true,
    })
    .click();
  await expect(page.locator(".agent-card")).toContainText(
    "A concurrent external edit",
  );

  await setAgentPrompt(page, "Saved after explicit recovery");
  await saveChanges(page);
  expect((await store("snapshot")).settings.agents[0].prompt).toBe(
    "Saved after explicit recovery",
  );
});

test("repository schedule builder matches the displayed and saved draft", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/project" });
  const settings = (await store("snapshot")).settings;
  settings.repositories[0].enabled = false;
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  let editor = await repositorySettings(page, "fixture/project");
  await expect(editor.locator("#schedule-mode-every")).toBeChecked();
  await expect(editor.locator("#schedule-every")).toHaveValue("30");
  await editor.locator("#schedule-mode-daily").check();
  await editor.locator("#schedule-time-0").fill("09:00");
  for (const day of [0, 6])
    await editor.locator(`#schedule-day-${day}`).uncheck();
  await expect(editor.locator("[data-schedule-runs] li")).toHaveCount(3);
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories[0].cron).toBe(
    "0 9 * * 1-5",
  );

  await page.reload();
  editor = await repositorySettings(page, "fixture/project");
  await expect(editor.locator("#schedule-mode-daily")).toBeChecked();
  await expect(editor.locator("#schedule-time-0")).toHaveValue("09:00");
  await expect(editor.locator("#schedule-day-0")).not.toBeChecked();
  await expect(editor.locator("#schedule-day-1")).toBeChecked();
});
