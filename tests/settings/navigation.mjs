import { expect } from "./fixtures.mjs";

export async function section(page, name) {
  const embedded = page.locator(".panel-shell .settings-window");
  if (await page.locator(".panel-shell").count()) {
    await expect(embedded).toBeVisible();
    await expect(embedded).toHaveAttribute("data-settings-section", /.+/);
    const destination = name === "Integrations" ? "Repositories" : name;
    const accountPaths = {
      "AI Tooling": ["Accounts", "AI Tooling"],
      "Git Repository": ["Accounts", "Git Repository"],
      "GitHub Copilot": ["Accounts", "AI Tooling", "GitHub Copilot"],
      GitHub: ["Accounts", "Git Repository", "GitHub"],
    };
    const path = accountPaths[destination] ?? [destination];
    const key =
      destination === "GitHub Copilot"
        ? "copilot"
        : destination.toLowerCase().replaceAll(" ", "-");
    if ((await embedded.getAttribute("data-settings-section")) === key) return;
    const back = page.getByRole("button", {
      name: /^Back to (Settings|Accounts|AI Tooling|Git Repository)$/,
    });
    while (await back.isVisible()) await back.click();
    for (const step of path) {
      await page
        .locator(".settings-overview")
        .getByRole("button", { name: step, exact: true })
        .click();
    }
    return;
  }
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
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await page.evaluate(() => window.__settingsIdle());
  return modal;
}

export async function saveChanges(page) {
  const previous = await page
    .getByLabel("Settings section", { exact: true })
    .inputValue();
  await section(page, "Integrations");
  const dirty = page.locator('.repository-row[data-dirty="true"]');
  while (await dirty.count()) {
    await dirty
      .first()
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    const modal = page.getByRole("dialog");
    await modal
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(modal).toHaveCount(0);
  }
  await section(page, previous[0].toUpperCase() + previous.slice(1));
  const preferences = page.getByRole("button", {
    name: "Save preferences",
    exact: true,
  });
  if (await preferences.isEnabled()) await preferences.click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
}

export async function startupPreference(page) {
  await section(page, "Preferences");
  return page.getByRole("switch", { name: /Open PR Sniper at login/ });
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
  await expect(modal).toHaveCount(0);
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
  await page.evaluate(() => window.__settingsIdle());
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
  const editor = page.getByRole("dialog", {
    name: index === undefined ? "Assign agent" : "Edit assignment",
    exact: true,
  });
  if (index === undefined) {
    await expect(editor.getByLabel("Agent", { exact: true })).toHaveValue("");
    await editor
      .getByLabel("Agent", { exact: true })
      .selectOption(fixtureAgent.id);
  }
  return editor;
}

export async function saveAssignment(page, modal) {
  await modal
    .locator("form")
    .getByRole("button", {
      name: /^(Assign agent|Save assignment)$/,
    })
    .click();
  await expect(modal).toHaveCount(0);
  await closeDialog(page);
}
