import { expect, test } from "./fixtures.mjs";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  seedAgent,
  setAgentPrompt,
  saveChanges,
  assignment,
  saveAssignment,
  advancedSchedule,
  closeDialog,
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
  await setAgentPrompt(page, "My unsaved local draft");

  const external = (await store("snapshot")).settings;
  external.agents[0].prompt = "A concurrent external edit";
  await store("seed_settings", external);

  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.locator("#error")).toContainText(
    "Settings changed in another window",
  );
  await expect(page.locator(".agent-card")).toContainText(
    "My unsaved local draft",
  );

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

test("advanced schedule edits immediately match the displayed and saved draft", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/project" });
  await page.goto("/?view=settings");
  let modal = await assignment(page, "fixture/project");
  await advancedSchedule(modal);
  await modal.getByLabel("Interval minutes", { exact: true }).fill("42");

  let frequency = modal.getByRole("combobox", {
    name: "Check for pull requests",
    exact: true,
  });
  await expect(frequency).toHaveValue("42");
  await expect(frequency.locator("option:checked")).toHaveText(
    "Every 42 minutes",
  );
  await modal.getByLabel("Time zone", { exact: true }).fill("UTC");
  await saveAssignment(page, modal);
  await saveChanges(page);
  expect(
    (await store("snapshot")).settings.repositories[0].assignments[0].schedule,
  ).toEqual({
    kind: "interval",
    minutes: 42,
    timezone: "UTC",
  });

  await page.reload();
  modal = await assignment(page, "fixture/project", 0);
  await advancedSchedule(modal);
  frequency = modal.getByRole("combobox", {
    name: "Check for pull requests",
    exact: true,
  });
  await expect(frequency).toHaveValue("42");
  await expect(
    modal.getByLabel("Interval minutes", { exact: true }),
  ).toHaveValue("42");
  await expect(modal.getByLabel("Time zone", { exact: true })).toHaveValue(
    "UTC",
  );
  await closeDialog(page);
  await closeDialog(page);
});
