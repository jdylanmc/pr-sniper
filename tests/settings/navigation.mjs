import { expect } from "./fixtures.mjs";

export async function section(page, name) {
  const mobile = page.getByLabel("Settings section", { exact: true });
  if (await mobile.isVisible()) {
    await mobile.selectOption({ label: name });
    return;
  }
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
  await section(page, "Integrations");
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
  await section(page, "Integrations");
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
  await section(page, "Preferences");
  return page.getByRole("switch", { name: /Open PR Sniper at login/ });
}

export async function advancedSchedule(root) {
  const details = root.locator("details").filter({
    hasText: "Advanced scheduling",
  });
  if (!(await details.evaluate((element) => element.open))) {
    await details.getByText("Advanced scheduling", { exact: true }).click();
  }
}

export const fixtureAgent = {
  id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  name: "Fixture reviewer",
  model: "saved-legacy-model",
  prompt: "Keep the saved prompt.",
  signature: "Fixture signature",
};

export async function seedAgent(store) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [structuredClone(fixtureAgent)];
  await store("seed_settings", settings);
  return settings;
}

export async function editAgent(page, name = fixtureAgent.name) {
  await section(page, "Agents");
  await page
    .locator(".agent-card")
    .filter({
      has: page.getByRole("heading", { name, exact: true }),
    })
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  return page.getByRole("dialog", { name: "Edit agent", exact: true });
}

export async function setAgentPrompt(page, prompt, name = fixtureAgent.name) {
  const modal = await editAgent(page, name);
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill(prompt);
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
}

export async function newDoctrine(page, title, body) {
  await section(page, "Doctrines");
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const modal = page.getByRole("dialog", {
    name: "New doctrine",
    exact: true,
  });
  await modal.getByLabel("Title", { exact: true }).fill(title);
  await modal
    .getByRole("textbox", { name: "Principles", exact: true })
    .fill(body);
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
}

export async function assignment(page, repository, index) {
  const parent = await repositorySettings(page, repository);
  if (index === undefined)
    await parent
      .getByRole("button", { name: "Assign agent", exact: true })
      .click();
  else
    await parent
      .locator(".assignment-row")
      .nth(index)
      .getByRole("button", { name: "Edit", exact: true })
      .click();
  return page.getByRole("dialog", {
    name: index === undefined ? "Assign agent" : "Edit assignment",
    exact: true,
  });
}

export async function saveAssignment(page, modal) {
  await modal
    .locator("form")
    .getByRole("button", {
      name: /^(Assign agent|Save assignment)$/,
    })
    .click();
  await closeDialog(page);
}

export async function setSchedule(modal, schedule) {
  await modal
    .getByRole("combobox", { name: "Check for pull requests", exact: true })
    .selectOption(schedule.kind === "cron" ? "cron" : "15");
  await advancedSchedule(modal);
  await modal
    .getByLabel(
      schedule.kind === "cron" ? "Cron expression" : "Interval minutes",
      { exact: true },
    )
    .fill(
      schedule.kind === "cron" ? schedule.expression : String(schedule.minutes),
    );
  await modal.getByLabel("Time zone", { exact: true }).fill(schedule.timezone);
}
