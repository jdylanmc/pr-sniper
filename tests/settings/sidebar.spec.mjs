import { expect, test } from "./fixtures.mjs";

test("Settings opens the approved sidebar without prototype or reviewer controls", async ({
  page,
}) => {
  await page.goto("/?view=settings");
  const nav = page.getByRole("navigation", { name: "Settings sections" });
  await expect(nav).toBeVisible();
  for (const name of [
    "Repositories",
    "People",
    "Review defaults",
    "Automation",
    "Review presets",
  ]) {
    await expect(nav.getByRole("button", { name, exact: true })).toBeVisible();
  }
  await expect(page.getByText("Interactive design preview")).toHaveCount(0);
  await nav
    .getByRole("button", { name: "Review defaults", exact: true })
    .click();
  await expect(page.getByLabel("Model", { exact: true })).toBeVisible();
  await expect(
    page.getByLabel("Model", { exact: true }).locator("option").first(),
  ).toHaveText("Default");
  await expect(
    page.getByLabel("Reviewer assignment", { exact: true }),
  ).toHaveCount(0);
});

test("separate automation choices preserve legacy policy and survive reload", async ({
  page,
  store,
}) => {
  const initial = (await store("snapshot")).settings;
  initial.defaults.reviewer_assignment = false;
  initial.defaults.selector = { kind: "agent", value: "my-reviewer" };
  initial.defaults.schedule = {
    kind: "cron",
    expression: "0 9 * * MON-FRI",
    timezone: "Europe/London",
  };
  initial.defaults.prompt = "Keep this exact custom prompt.\nAnd its newline.";
  await store("seed_settings", initial);
  await page.goto("/?view=settings");
  await page.getByRole("button", { name: "Automation", exact: true }).click();
  await page
    .getByRole("switch", { name: "Run reviews automatically", exact: true })
    .check();
  await expect(
    page.getByRole("switch", {
      name: "Post review comments automatically",
      exact: true,
    }),
  ).not.toBeChecked();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  expect((await store("snapshot")).settings.defaults).toEqual({
    ...initial.defaults,
    automatic_agent_start: true,
  });
  await page.reload();
  await page.getByRole("button", { name: "Automation", exact: true }).click();
  await expect(
    page.getByRole("switch", {
      name: "Run reviews automatically",
      exact: true,
    }),
  ).toBeChecked();
  await page
    .getByRole("switch", {
      name: "Post review comments automatically",
      exact: true,
    })
    .check();
  await page
    .getByRole("button", { name: "Reset changes", exact: true })
    .click();
  await expect(
    page.getByRole("switch", {
      name: "Post review comments automatically",
      exact: true,
    }),
  ).not.toBeChecked();
});
