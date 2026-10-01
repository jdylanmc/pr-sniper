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
} from "./navigation.mjs";

test("discovery preserves the fetch URL and reports ambiguous remotes", async ({
  store,
  dataRoot,
}) => {
  const repeated = join(dataRoot, "repeated");
  await mkdir(join(repeated, ".git"), { recursive: true });
  await writeFile(
    join(repeated, ".git/config"),
    [
      '[remote "origin"]',
      "url = https://github.com/octo/first.git",
      "url = https://github.com/octo/second.git",
      "",
    ].join("\n"),
  );

  const repeatedResult = await store("discover_repositories", {
    root: repeated,
  });
  expect(repeatedResult.repositories).toEqual([
    expect.objectContaining({
      name: "octo/first",
      unavailable: null,
    }),
  ]);

  const ambiguous = join(dataRoot, "ambiguous");
  await mkdir(join(ambiguous, ".git"), { recursive: true });
  await writeFile(
    join(ambiguous, ".git/config"),
    [
      '[remote "upstream"]',
      "url = https://github.com/octo/first.git",
      '[remote "backup"]',
      "url = https://github.com/octo/second.git",
      "",
    ].join("\n"),
  );

  const ambiguousResult = await store("discover_repositories", {
    root: ambiguous,
  });
  expect(ambiguousResult.repositories).toEqual([
    expect.objectContaining({
      name: null,
      unavailable:
        "Multiple GitHub remotes without an origin. Add the intended repository manually.",
    }),
  ]);
});

test("a settings conflict keeps the draft until explicit discard and reload", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await page.goto("/?view=settings");
  const modal = await editAgent(page);
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("My unsaved local draft");

  const external = (await store("snapshot")).settings;
  external.agents[0].prompt = "A concurrent external edit";
  await store("seed_settings", external);

  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toContainText(
    "Resource changed in another window",
  );
  await expect(
    modal.getByRole("textbox", { name: "Prompt", exact: true }),
  ).toHaveValue("My unsaved local draft");
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

test("global schedule helper matches the displayed and saved draft", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/project" });
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  await page.locator("#cron-helper").selectOption("0 9 * * MON-FRI");
  await expect(page.locator("#global-cron")).toHaveValue("0 9 * * MON-FRI");
  await page.locator("#global-timezone").fill("Europe/London");
  await saveChanges(page);
  expect((await store("snapshot")).settings.defaults.schedule).toEqual({
    kind: "cron",
    expression: "0 9 * * MON-FRI",
    timezone: "Europe/London",
  });

  await page.reload();
  await section(page, "Preferences");
  await expect(page.locator("#global-cron")).toHaveValue("0 9 * * MON-FRI");
  await expect(page.locator("#global-timezone")).toHaveValue("Europe/London");
});
