import { expect } from "./fixtures.mjs";

export async function section(page, name) {
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("button", { name, exact: true })
    .click();
}

export async function closeDialog(page) {
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
}

export async function repositorySettings(page, name) {
  await section(page, "Repositories");
  await page
    .getByRole("article", { name, exact: true })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  return page.getByRole("dialog", {
    name: `Settings for ${name}`,
    exact: true,
  });
}

export async function addRepository(page, name) {
  await section(page, "Repositories");
  await page
    .getByRole("button", { name: "Add repository manually...", exact: true })
    .click();
  const modal = page.getByRole("dialog", {
    name: "Add repository",
    exact: true,
  });
  await modal.getByLabel("GitHub repository", { exact: true }).fill(name);
  await modal
    .getByRole("button", { name: "Use repository", exact: true })
    .click();
  return modal;
}

export async function saveChanges(page) {
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  await page.evaluate(() => window.__settingsIdle());
}

export async function startupPreference(page) {
  await section(page, "Automation");
  await page.getByText("Startup and diagnostics", { exact: true }).click();
  return page.getByLabel("Request launch at login", { exact: true });
}

export async function advancedSchedule(root) {
  const details = root.locator("details").filter({
    hasText: "Advanced scheduling",
  });
  if (!(await details.evaluate((element) => element.open))) {
    await details.getByText("Advanced scheduling", { exact: true }).click();
  }
}
