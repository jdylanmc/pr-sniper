import { expect, test } from "./fixtures.mjs";
import { mkdir, writeFile, readFile, realpath } from "node:fs/promises";
import { join } from "node:path";

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

test("a new profile uses local time without writing or enabling anything until explicit save", async ({
  browser,
  store,
  dataRoot,
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
    await expect(
      page.getByText(/America\/New_York \(system local\)/),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Save changes", exact: true }),
    ).toBeDisabled();
    expect((await store("snapshot")).settings_persisted).toBe(false);
    await page
      .getByRole("switch", { name: "Run reviews automatically", exact: true })
      .check();
    await page
      .getByRole("button", { name: "Reset changes", exact: true })
      .click();
    await expect(
      page.getByText(/America\/New_York \(system local\)/),
    ).toBeVisible();
    await expect(
      page.getByRole("switch", {
        name: "Run reviews automatically",
        exact: true,
      }),
    ).not.toBeChecked();
    await page
      .getByLabel("Check frequency", { exact: true })
      .selectOption("30");
    await page
      .getByRole("button", { name: "Save changes", exact: true })
      .click();
    await expect(
      page.getByText("All changes saved", { exact: true }),
    ).toBeVisible();
    const saved = JSON.parse(
      await readFile(join(dataRoot, "config/settings.json"), "utf8"),
    );
    expect(saved.defaults.schedule).toEqual({
      kind: "interval",
      minutes: 30,
      timezone: "America/New_York",
    });
    expect(saved.defaults.automatic_agent_start).toBe(false);
    expect(saved.defaults.automatic_comment_publication).toBe(false);
  } finally {
    await context.close();
  }
});

test("chosen-root discovery uses real metadata, searchable selection and stable saved identities", async ({
  page,
  store,
  dataRoot,
}) => {
  const root = join(dataRoot, "repositories");
  await mkdir(join(root, "atlas/.git"), { recursive: true });
  await writeFile(
    join(root, "atlas/.git/config"),
    '[remote "origin"]\nurl = git@github.com:Orbit-Labs/Atlas-Desktop.git\n',
  );
  await mkdir(join(root, "local-only/.git"), { recursive: true });
  await writeFile(
    join(root, "local-only/.git/config"),
    "[core]\nrepositoryformatversion = 0\n",
  );
  const calls = [];
  await page.exposeFunction("__chooseFolder", async () => {
    calls.push(root);
    return store("discover_repositories", { root });
  });
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "choose_repository_folder"
        ? window.__chooseFolder()
        : invoke(command, args);
  });
  await page.goto("/?view=settings");
  expect(calls).toHaveLength(0);
  await page
    .getByRole("button", { name: "Choose folder...", exact: true })
    .click();
  await expect(page.getByText("2 local repositories discovered")).toBeVisible();
  await expect(
    page.getByRole("checkbox", { name: "Monitor local-only", exact: true }),
  ).toBeDisabled();
  const monitored = page.getByRole("checkbox", {
    name: "Monitor orbit-labs/atlas-desktop",
    exact: true,
  });
  await monitored.check();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  const persisted = (await store("snapshot")).settings;
  expect(persisted.root_folder).toBe(await realpath(root));
  expect(persisted.repositories[0]).toMatchObject({
    name: "orbit-labs/atlas-desktop",
    enabled: true,
  });
  await page.reload();
  expect(calls).toHaveLength(1);
  await monitored.uncheck();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  expect((await store("snapshot")).settings.repositories[0]).toEqual({
    ...persisted.repositories[0],
    enabled: false,
  });
  await page.getByLabel("Find a repository", { exact: true }).fill("missing");
  await expect(page.getByText("No matching repositories")).toBeVisible();
  expect(await readFile(join(root, "atlas/.git/config"), "utf8")).toContain(
    "Orbit-Labs/Atlas-Desktop",
  );
});

test("People resolves a login to stable identity and reports disconnected lookup", async ({
  page,
  store,
}) => {
  let disconnected = false;
  const calls = [];
  await page.exposeFunction("__personLookup", (args) => {
    calls.push(args);
    return disconnected
      ? { error: "signed_out" }
      : { ok: { id: "42", login: "octocat" } };
  });
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "github_auth_state")
        return {
          accounts: [
            {
              provider: "github",
              state: "connected",
              account_id: "6954990",
              login: "jdylanmc",
            },
          ],
          flow: { state: "idle" },
        };
      if (command !== "resolve_provider_person") return invoke(command, args);
      const result = await window.__personLookup(args);
      if (result.error) throw result.error;
      return result.ok;
    };
  });
  await page.goto("/?view=settings");
  await page.getByRole("button", { name: "People", exact: true }).click();
  await page.getByRole("button", { name: "Add people", exact: true }).click();
  await page.getByLabel("GitHub login", { exact: true }).fill("@octocat");
  await page.getByRole("button", { name: "Add person", exact: true }).click();
  await expect(page.getByText("@octocat", { exact: true })).toBeVisible();
  expect(calls).toEqual([
    { provider: "github", accountId: "6954990", login: "octocat" },
  ]);
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  expect((await store("snapshot")).settings.defaults.watched_authors).toEqual([
    { id: "42", login: "octocat" },
  ]);
  disconnected = true;
  await page.getByRole("button", { name: "Add people", exact: true }).click();
  await page.getByLabel("GitHub login", { exact: true }).fill("someone");
  await page.getByRole("button", { name: "Add person", exact: true }).click();
  await expect(page.locator(".person-lookup [role=alert]")).toContainText(
    "Connect the PR Sniper GitHub OAuth App",
  );
  expect(
    (await store("snapshot")).settings.defaults.watched_authors,
  ).toHaveLength(1);
});

test("local presets create, edit, import inertly and retain independent repository inheritance", async ({
  page,
  store,
}) => {
  await store("save_repository", { repository: "octo/project" });
  await page.goto("/?view=settings");
  await page
    .getByRole("button", { name: "Review presets", exact: true })
    .click();
  await page.getByRole("button", { name: "New preset", exact: true }).click();
  let modal = page.getByRole("dialog");
  await modal.getByLabel("Preset name", { exact: true }).fill("API review");
  await modal
    .getByLabel("Review instructions", { exact: true })
    .fill("Find breaking API changes.");
  await modal.getByRole("button", { name: "Save preset", exact: true }).click();
  await page
    .getByRole("button", { name: "Review defaults", exact: true })
    .click();
  await page
    .getByLabel("Review preset", { exact: true })
    .selectOption({ label: "API review" });
  await page.getByRole("button", { name: "Repositories", exact: true }).click();
  await page
    .getByRole("article", { name: "octo/project", exact: true })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  modal = page.getByRole("dialog");
  await modal
    .getByRole("checkbox", {
      name: "Override review instructions",
      exact: true,
    })
    .check();
  await modal
    .getByLabel("Review preset", { exact: true })
    .selectOption({ label: "API review" });
  await modal
    .getByRole("checkbox", {
      name: "Override run reviews automatically",
      exact: true,
    })
    .check();
  await expect(
    modal.getByRole("switch", {
      name: "Run reviews automatically",
      exact: true,
    }),
  ).not.toBeChecked();
  await modal
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  let saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].overrides).toEqual({
    prompt: "Find breaking API changes.",
    automatic_agent_start: false,
  });
  expect(saved.default_review_preset).toBe(saved.repositories[0].review_preset);
  await page
    .getByRole("button", { name: "Review presets", exact: true })
    .click();
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("textbox", { name: "Review instructions", exact: true })
    .fill("Check compatibility and tests.");
  await page.getByRole("button", { name: "Save preset", exact: true }).click();
  await page.getByRole("button", { name: "Import...", exact: true }).click();
  await page.getByLabel("Preset JSON", { exact: true }).fill(
    JSON.stringify({
      name: "Unsafe shape",
      body: "Review",
      command: "touch sentinel",
    }),
  );
  await page
    .getByRole("button", { name: "Import preset", exact: true })
    .click();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
    "only name and body",
  );
  await page.getByLabel("Preset JSON", { exact: true }).fill(
    JSON.stringify({
      name: "Inert HTML",
      body: "<script>window.presetExecuted=true</script>",
    }),
  );
  await page
    .getByRole("button", { name: "Import preset", exact: true })
    .click();
  expect(await page.evaluate(() => window.presetExecuted)).toBeUndefined();
  await expect(page.locator(".preset-list script")).toHaveCount(0);
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(
    page.getByText("All changes saved", { exact: true }),
  ).toBeVisible();
  await page.reload();
  saved = (await store("snapshot")).settings;
  expect(saved.presets).toHaveLength(2);
  expect(saved.defaults.prompt).toBe("Check compatibility and tests.");
  expect(saved.repositories[0].overrides.prompt).toBe(
    "Check compatibility and tests.",
  );
});

for (const viewport of [
  { width: 1180, height: 800 },
  { width: 390, height: 844 },
]) {
  test(`approved A layout at ${viewport.width}x${viewport.height} keeps focus, content and save footer reachable`, async ({
    page,
    store,
  }, testInfo) => {
    await page.setViewportSize(viewport);
    for (const name of [
      "atlas-desktop",
      "relay-api",
      "design-system",
      "developer-docs",
      "orbit-cli",
    ])
      await store("save_repository", { repository: `orbit-labs/${name}` });
    await page.goto("/?view=settings");
    await expect(page.getByRole("article")).toHaveCount(5);
    await expect(
      page.getByRole("button", { name: "Save changes", exact: true }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page
      .getByRole("button", { name: "Choose folder...", exact: true })
      .focus();
    expect(
      await page.evaluate(
        () => getComputedStyle(document.activeElement).outlineStyle,
      ),
    ).toBe("solid");
    await page.keyboard.press("Tab");
    await expect(
      page.getByLabel("Find a repository", { exact: true }),
    ).toBeFocused();
    await page.screenshot({
      path: testInfo.outputPath(`settings-${viewport.width}-repositories.png`),
    });
    for (const [section, title] of [
      ["people", "People"],
      ["reviews", "Review defaults"],
      ["automation", "Automation"],
      ["presets", "Review presets"],
    ]) {
      if (viewport.width < 600)
        await page
          .getByLabel("Settings section", { exact: true })
          .selectOption(section);
      else
        await page
          .getByRole("navigation", { name: "Settings sections" })
          .getByRole("button", { name: title, exact: true })
          .click();
      await expect(
        page.getByRole("heading", { name: title, exact: true }).first(),
      ).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      const footer = await page
        .getByRole("button", { name: "Save changes", exact: true })
        .boundingBox();
      expect(footer.y + footer.height).toBeLessThanOrEqual(viewport.height);
      await page.screenshot({
        path: testInfo.outputPath(`settings-${viewport.width}-${section}.png`),
      });
    }
  });
}

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
