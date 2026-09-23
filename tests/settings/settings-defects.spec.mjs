import { expect, test } from "./fixtures.mjs";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

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
  await page.goto("/?view=settings");
  await page
    .getByRole("button", { name: "Review defaults", exact: true })
    .click();
  const prompt = page.getByLabel("Review prompt", { exact: true });
  await prompt.fill("My unsaved local draft");

  const external = (await store("snapshot")).settings.defaults;
  await store("save_defaults", {
    policy: { ...external, prompt: "A concurrent external edit" },
  });

  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Settings changed in another window",
  );
  await expect(prompt).toHaveValue("My unsaved local draft");

  await page
    .getByRole("button", {
      name: "Discard draft and reload",
      exact: true,
    })
    .click();
  await expect(prompt).toHaveValue("A concurrent external edit");

  await prompt.fill("Saved after explicit recovery");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  expect((await store("snapshot")).settings.defaults.prompt).toBe(
    "Saved after explicit recovery",
  );
});

test("advanced schedule edits immediately match the displayed and saved draft", async ({
  browser,
  store,
  baseURL,
}) => {
  const context = await browser.newContext({
    baseURL,
    timezoneId: "America/New_York",
  });
  const page = await context.newPage();
  await page.exposeFunction("__settingsInvoke", store);
  await page.addInitScript(() => {
    window.__TAURI_INTERNALS__ = { invoke: window.__settingsInvoke };
  });
  try {
    await page.goto("/?view=settings");
    await page.getByRole("button", { name: "Automation", exact: true }).click();
    await page.getByText("Advanced scheduling", { exact: true }).click();
    await page.getByLabel("Interval minutes", { exact: true }).fill("42");

    const frequency = page.getByLabel("Check frequency", { exact: true });
    await expect(frequency).toHaveValue("42");
    await expect(frequency.locator("option:checked")).toHaveText(
      "Every 42 minutes",
    );
    await page.getByLabel("Time zone", { exact: true }).selectOption("UTC");
    await expect(page.getByText(/Time zone: UTC \(selected\)/)).toBeVisible();

    await page
      .getByRole("button", { name: "Save changes", exact: true })
      .click();
    await expect(
      page.getByText("All changes saved", { exact: true }),
    ).toBeVisible();
    expect((await store("snapshot")).settings.defaults.schedule).toEqual({
      kind: "interval",
      minutes: 42,
      timezone: "UTC",
    });

    await page.reload();
    await page.getByRole("button", { name: "Automation", exact: true }).click();
    await page.getByText("Advanced scheduling", { exact: true }).click();
    await expect(frequency).toHaveValue("42");
    await expect(
      page.getByLabel("Interval minutes", { exact: true }),
    ).toHaveValue("42");
    await expect(page.getByLabel("Time zone", { exact: true })).toHaveValue(
      "UTC",
    );
  } finally {
    await context.close();
  }
});
