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
  if (name === "Integrations") name = "Repositories";
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
    .locator("[data-repository]")
    .filter({ has: page.locator("strong", { hasText: name }) })
    .click();
  return page.getByRole("dialog", {
    name: `Settings for ${name}`,
    exact: true,
  });
}

export async function installRepositoryFixture(page) {
  const installFixture = () => {
    if (window.__urlTestAccount) return;
    window.__urlTestAccount = true;
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "github_auth_state")
        return {
          accounts: [
            {
              provider: "github",
              account_id: "22",
              login: "fixture",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        };
      if (command === "resolve_provider_repository") {
        const canonical = await invoke("canonical_repository_name", {
          repository: args.repository,
        });
        let id = 1;
        for (const c of canonical) id = (id * 31 + c.charCodeAt(0)) % 100000000;
        return {
          identity: { id: args.accountId, login: "fixture" },
          repository: { id: String(id + 1), name: canonical },
        };
      }
      return invoke(command, args);
    };
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts"));
  };
  const accounts = await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("github_auth_state"),
  );
  if (!accounts.accounts.length) {
    await page.addInitScript(installFixture);
    await page.evaluate(installFixture);
  }
}

export async function addRepository(page, name) {
  await installRepositoryFixture(page);
  await section(page, "Integrations");
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const modal = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await modal.getByLabel("Repository URL", { exact: true }).fill(name);
  const choices = modal.getByLabel("Acting GitHub account");
  await choices.selectOption({ index: 1 });
  await modal
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(
    page.locator(
      'dialog[open] [data-save-repository], dialog[open] [role="alert"]:not([hidden])',
    ),
  ).toBeVisible();
  const configured = page
    .getByRole("dialog")
    .filter({ has: page.locator("[data-save-repository]") });
  if (await configured.count())
    await configured
      .getByRole("button", { name: "Cancel repository changes" })
      .click();
  return modal;
}

export async function saveChanges(page) {
  const previous = await page
    .getByLabel("Settings section", { exact: true })
    .inputValue();
  await section(page, "Integrations");
  const dirty = page.locator('[data-repository][data-dirty="true"]');
  while (await dirty.count()) {
    await dirty.first().click();
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
  if ((await preferences.isVisible()) && (await preferences.isEnabled()))
    await preferences.click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
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

export async function seedAgent(store, aiAccountId) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [
    {
      ...structuredClone(fixtureAgent),
      ...(aiAccountId
        ? { ai_account: { provider: "copilot", account_id: aiAccountId } }
        : {}),
    },
  ];
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
