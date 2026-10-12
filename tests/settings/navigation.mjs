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
      name: /^Back( to (Settings|Accounts|AI Tooling|Git Repository))?$/,
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
  const back = page.locator("[data-panel-back]");
  if (await back.isVisible()) return back.click();
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
    name,
    exact: true,
  });
}

export function repositoryConnectionStatus(dialog) {
  return dialog.locator(".connection").getByRole("status");
}

export async function seedBoundRepositories(store, names) {
  for (const name of names)
    await store("save_repository", { repository: name });
  const settings = (await store("snapshot")).settings;
  for (const [index, name] of names.entries()) {
    const repository = settings.repositories.find((r) => r.name === name);
    Object.assign(repository, {
      provider_account_id: "22",
      provider_repository_id: String(100 + index),
    });
  }
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
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
  if (await choices.isVisible()) await choices.selectOption({ index: 1 });
  else await expect(choices).not.toHaveValue("");
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

export async function chooseRepositoryAccount(dialog, accountId) {
  const choice = dialog.getByLabel("Acting GitHub account", { exact: true });
  if (!(await choice.isVisible()))
    await dialog.getByRole("button", { name: "Change", exact: true }).click();
  await choice.selectOption(accountId);
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
    await page.evaluate(() => window.__settingsIdle());
    await expect(modal.locator("[data-resource-error]")).toBeHidden();
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
    .locator("[data-edit]")
    .click({ force: true });
  return page.getByRole("dialog", { name: `Editing ${name}`, exact: true });
}

export async function setAgentPrompt(page, prompt, name = fixtureAgent.name) {
  const modal = await editAgent(page, name);
  await modal.locator("textarea[name=prompt]").fill(prompt);
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
      .locator("[data-edit]")
      .click({ force: true });
  const editor = page.getByRole("dialog", {
    name: index === undefined ? "Assign agent" : "Edit assignment",
    exact: true,
  });
  if (index === undefined) {
    await expect(editor.locator("input[name=agent]")).toHaveValue("");
    await editor.getByRole("button", { name: "Choose Agent" }).click();
    await editor.getByRole("radio", { name: fixtureAgent.name }).click();
    await expect(editor.locator("input[name=agent]")).toHaveValue(
      fixtureAgent.id,
    );
  }
  return editor;
}

export async function assignmentView(page, editor, view) {
  await editor
    .locator(view === "agent" ? "[data-open-agent]" : "[data-open-permissions]")
    .click();
  return editor;
}

export async function assignmentBack(page, editor) {
  const back = page.locator("[data-panel-back]");
  if (await back.isVisible()) await back.click();
  else await editor.locator(".admission-back:visible").click();
}

export async function pickAgent(editor, agentId) {
  await editor.locator("[data-open-agent]").click();
  const radio = editor.locator(`input[name=agent-choice][value="${agentId}"]`);
  const alreadyChecked = await radio.isChecked();
  await radio.check();
  if (alreadyChecked) await assignmentBack(editor.page(), editor);
  await expect(editor.locator("input[name=agent]")).toHaveValue(agentId);
}

export async function openPermissions(editor) {
  await editor.locator("[data-open-permissions]").click();
  return editor;
}

export async function withPermissions(page, editor, action) {
  await openPermissions(editor);
  await action(editor);
  await assignmentBack(page, editor);
}

export async function saveAssignment(page, modal) {
  const save = modal.getByRole("button", {
    name: /^(Assign agent|Save assignment)$/,
  });
  if (!(await save.isVisible())) {
    await assignmentBack(page, modal);
    await expect(save).toBeVisible();
  }
  await save.click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(modal.getByRole("alert")).toBeHidden();
  await expect(modal).toHaveCount(0);
  await closeDialog(page);
}

export async function backFromEditor(page, dialog) {
  const back = page.locator("[data-panel-back]");
  if (await back.isVisible()) await back.click();
  else
    await dialog
      .getByRole("button", { name: "Close dialog", exact: true })
      .click();
}

export async function setAllPullRequests(editor, enabled) {
  const toggle = editor.getByRole("switch", { name: "All pull requests" });
  if ((await toggle.isChecked()) !== enabled) await toggle.setChecked(enabled);
  return toggle;
}

export async function openFilters(page, editor) {
  await editor.locator("[data-open-filters]").click();
  const heading = page.locator("[data-panel-heading]");
  if (await heading.count())
    await expect(heading).toContainText("Filtered pull requests");
  await expect(
    page.locator(".admission-rule[data-rule=assigned]"),
  ).toBeVisible();
}

export const ruleLabels = {
  assigned: "Assigned to me for review",
  mentioned: "Mentioning me in comments",
  include_users: "Include users",
  exclude_users: "Exclude users",
  include_paths: "Include paths",
  exclude_paths: "Exclude paths",
};

export async function setRule(page, kind, enabled) {
  const toggle = page
    .locator(`.admission-rule[data-rule=${kind}]`)
    .locator(".admission-switch");
  if ((await toggle.isChecked()) !== enabled) await toggle.setChecked(enabled);
}

export async function openRule(page, kind) {
  await page
    .locator(`.admission-rule[data-rule=${kind}] .admission-open`)
    .click();
}

export async function addPath(page, kind, pattern) {
  await openRule(page, kind);
  await page.getByLabel("Path pattern", { exact: true }).fill(pattern);
  await page
    .locator(".admission-path-form")
    .getByRole("button", { name: "Add", exact: true })
    .click();
}

export async function addUser(page, kind, login) {
  await openRule(page, kind);
  await page.locator(".admission-add").click();
  const lookup = page.getByRole("dialog", { name: "Add people", exact: true });
  await lookup.getByLabel("GitHub login", { exact: true }).fill(login);
  await lookup.getByRole("button", { name: "Add person", exact: true }).click();
  await page.evaluate(() => window.__settingsIdle());
  return lookup;
}

export async function backOne(page) {
  const back = page.locator("[data-panel-back]");
  if (await back.isVisible()) await back.click();
  else await page.locator(".admission-back").last().click();
}
