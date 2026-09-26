import { expect, test } from "./fixtures.mjs";
import { mkdir, writeFile, readFile, realpath } from "node:fs/promises";
import { join } from "node:path";
import {
  section,
  seedAgent,
  fixtureAgent,
  newDoctrine,
  editAgent,
  assignment,
  saveAssignment,
  saveChanges,
  repositorySettings,
  closeDialog,
} from "./navigation.mjs";

test.use({ timezoneId: "America/New_York" });

test("Settings exposes exactly four approved tabs and no prototype or retired controls", async ({
  page,
}) => {
  await page.goto("/?view=settings");
  const nav = page.getByRole("navigation", { name: "Settings sections" });
  await expect(nav.getByRole("button")).toHaveText([
    "Integrations",
    "Doctrines",
    "Agents",
    "Preferences",
  ]);
  await expect(page.getByText("Interactive design preview")).toHaveCount(0);
  for (const name of [
    "People",
    "Review defaults",
    "Automation",
    "Review presets",
    "Setup Doctor",
  ])
    await expect(nav.getByRole("button", { name, exact: true })).toHaveCount(0);
  await section(page, "Agents");
  await expect(
    page.getByRole("button", { name: "New agent", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByLabel("Reviewer assignment", { exact: true }),
  ).toHaveCount(0);
  await section(page, "Preferences");
  await expect(
    page.getByRole("switch", { name: /Open PR Sniper at login/ }),
  ).toBeDisabled();
  await expect(page.getByRole("switch", { name: /Notify me/ })).toBeDisabled();
  await expect(
    page.getByRole("button", {
      name: "Open redacted diagnostics",
      exact: true,
    }),
  ).toBeVisible();
});

test("new assignments use local time while seeded settings never opt into startup or automation", async ({
  page,
  store,
  dataRoot,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/local-time" });
  await page.goto("/?view=settings");
  const initial = (await store("snapshot")).settings;
  expect(initial.doctrines).toHaveLength(23);
  expect(initial.launch_at_login).toBe(false);
  expect(initial.defaults.automatic_agent_start).toBe(false);
  expect(initial.defaults.automatic_comment_publication).toBe(false);
  const zone = await page.evaluate(
    () => Intl.DateTimeFormat().resolvedOptions().timeZone,
  );
  expect(zone).toBe("America/New_York");
  let modal = await assignment(page, "fixture/local-time");
  await expect(modal.getByLabel("Time zone", { exact: true })).toHaveValue(
    zone,
  );
  await modal
    .getByRole("combobox", { name: "Check for pull requests", exact: true })
    .selectOption("30");
  await saveAssignment(page, modal);
  expect((await store("snapshot")).settings).toEqual(initial);
  await page.locator("#reset-settings").click();
  modal = await assignment(page, "fixture/local-time");
  await expect(
    modal.getByRole("combobox", {
      name: "Check for pull requests",
      exact: true,
    }),
  ).toHaveValue("15");
  await expect(modal.getByLabel("Time zone", { exact: true })).toHaveValue(
    zone,
  );
  await modal
    .getByRole("combobox", { name: "Check for pull requests", exact: true })
    .selectOption("30");
  await saveAssignment(page, modal);
  await saveChanges(page);
  const saved = JSON.parse(
    await readFile(join(dataRoot, "config/settings.json"), "utf8"),
  );
  expect(saved.repositories[0].assignments[0].schedule).toEqual({
    kind: "interval",
    minutes: 30,
    timezone: zone,
  });
  expect(saved.defaults).toEqual(initial.defaults);
  expect(saved.launch_at_login).toBe(false);
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
  await page.exposeFunction("__chooseFolder", () => {
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
  await saveChanges(page);
  const persisted = (await store("snapshot")).settings;
  expect(persisted.root_folder).toBe(await realpath(root));
  expect(persisted.repositories[0]).toMatchObject({
    name: "orbit-labs/atlas-desktop",
    enabled: true,
  });
  await page.reload();
  expect(calls).toHaveLength(1);
  await monitored.uncheck();
  await saveChanges(page);
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

test("repository People resolves stable identity, isolates neighbors and reports failed lookup", async ({
  page,
  store,
}) => {
  for (const repository of ["fixture/one", "fixture/two"])
    await store("save_repository", { repository });
  const initial = (await store("snapshot")).settings;
  initial.repositories[0].provider_account_id = "101";
  initial.repositories[0].provider_repository_id = "1";
  await store("seed_settings", initial);
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
              account_id: "101",
              login: "fixture-owner",
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
  let parent = await repositorySettings(page, "fixture/one");
  await parent.getByRole("button", { name: "Add people", exact: true }).click();
  let picker = page.getByRole("dialog", { name: "Add people", exact: true });
  await picker.getByLabel("GitHub login", { exact: true }).fill("@octocat");
  await picker.getByRole("button", { name: "Add person", exact: true }).click();
  await expect(parent.getByText("@octocat", { exact: true })).toBeVisible();
  expect(calls).toEqual([
    { provider: "github", accountId: "101", login: "octocat" },
  ]);
  await closeDialog(page);
  await saveChanges(page);
  expect(
    (await store("snapshot")).settings.repositories[0].watched_authors,
  ).toEqual([{ id: "42", login: "octocat" }]);
  expect((await store("snapshot")).settings.repositories[1]).toEqual(
    initial.repositories[1],
  );
  disconnected = true;
  parent = await repositorySettings(page, "fixture/one");
  await parent.getByRole("button", { name: "Add people", exact: true }).click();
  picker = page.getByRole("dialog", { name: "Add people", exact: true });
  await picker.getByLabel("GitHub login", { exact: true }).fill("someone");
  await picker.getByRole("button", { name: "Add person", exact: true }).click();
  await expect(picker.getByRole("alert")).toContainText(
    "Connect the PR Sniper GitHub OAuth App",
  );
  expect(
    (await store("snapshot")).settings.repositories[0].watched_authors,
  ).toHaveLength(1);
  await closeDialog(page);
  await parent
    .getByRole("button", { name: "Remove octocat", exact: true })
    .click();
  await expect(
    parent.getByText(
      "No trusted authors yet. Only pull requests requesting this signed-in account are eligible.",
    ),
  ).toBeVisible();
  await closeDialog(page);
  await saveChanges(page);
  await page.reload();
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].watched_authors ?? []).toEqual([]);
  expect(saved.repositories[1]).toEqual(initial.repositories[1]);
  expect(saved.defaults).toEqual(initial.defaults);
});

test("doctrine authoring stays inert and rejects unknown schema and credentials without replacing legacy presets", async ({
  page,
  store,
  dataRoot,
}) => {
  const settings = await seedAgent(store);
  settings.presets = [
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      name: "Old preset",
      body: "Preserve old preset.",
    },
  ];
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  const body =
    "<script>window.doctrineExecuted=true</script>\nFind compatibility defects.";
  await newDoctrine(page, "api-review", body);
  let modal = await editAgent(page);
  await modal
    .getByRole("combobox", { name: "Doctrine", exact: true })
    .selectOption("api-review");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await saveChanges(page);
  await section(page, "Doctrines");
  const card = page.locator(".doctrine-card").filter({
    has: page.getByRole("heading", { name: "api-review", exact: true }),
  });
  await card.getByRole("button", { name: "Edit", exact: true }).click();
  modal = page.getByRole("dialog", { name: "Edit doctrine", exact: true });
  await modal.getByLabel("Title", { exact: true }).fill("api-renamed");
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await saveChanges(page);
  const saved = (await store("snapshot")).settings;
  expect(saved.agents[0]).toEqual({ ...fixtureAgent, doctrine: "api-renamed" });
  expect(saved.presets).toEqual(settings.presets);
  expect(await page.evaluate(() => window.doctrineExecuted)).toBeUndefined();
  await expect(page.locator(".doctrine-list script")).toHaveCount(0);
  const path = join(dataRoot, "config/settings.json");
  const bytes = await readFile(path);
  await expect(
    store("seed_settings", {
      ...saved,
      doctrines: [
        { title: "unknown-shape", body: "Review", command: "touch sentinel" },
      ],
    }),
  ).rejects.toBeTruthy();
  expect(await readFile(path)).toEqual(bytes);
  await newDoctrine(page, "unsafe-input", "ghp_synthetic-secret");
  await page.locator("#save-settings").click();
  await expect(page.locator("#error")).toBeVisible();
  expect(await readFile(path)).toEqual(bytes);
  await page.reload();
  expect((await store("snapshot")).settings).toEqual(saved);
});

for (const viewport of [
  { width: 1180, height: 800 },
  { width: 390, height: 844 },
]) {
  test(`four-tab layout at ${viewport.width}x${viewport.height} keeps focus, content and save footer reachable`, async ({
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
    await expect(page.locator(".repository-row")).toHaveCount(5);
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
    for (const title of [
      "Integrations",
      "Doctrines",
      "Agents",
      "Preferences",
    ]) {
      await section(page, title);
      await expect(
        page.getByRole("heading", { name: title, exact: true }).first(),
      ).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      const footer = await page.locator("#save-settings").boundingBox();
      expect(footer.y).toBeGreaterThanOrEqual(0);
      expect(footer.y + footer.height).toBeLessThanOrEqual(viewport.height);
      await page.screenshot({
        path: testInfo.outputPath(`settings-${viewport.width}-${title}.png`),
      });
    }
  });
}

test("assignment comment choice stays independent of disabled Approve and preserves legacy automation", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/project" });
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
  let modal = await assignment(page, "fixture/project");
  await modal.getByRole("checkbox", { name: /^Comment/ }).uncheck();
  await expect(
    modal.getByRole("checkbox", { name: /^Approve/ }),
  ).toBeDisabled();
  await expect(
    modal.getByRole("checkbox", { name: /^Approve/ }),
  ).not.toBeChecked();
  await saveAssignment(page, modal);
  await saveChanges(page);
  const saved = (await store("snapshot")).settings;
  expect(saved.defaults).toEqual(initial.defaults);
  expect(saved.repositories[0].assignments[0]).toMatchObject({
    comment: false,
    approve: false,
  });
  await page.reload();
  modal = await assignment(page, "fixture/project", 0);
  await expect(
    modal.getByRole("checkbox", { name: /^Comment/ }),
  ).not.toBeChecked();
  await modal.getByRole("checkbox", { name: /^Comment/ }).check();
  await saveAssignment(page, modal);
  await page.locator("#reset-settings").click();
  modal = await assignment(page, "fixture/project", 0);
  await expect(
    modal.getByRole("checkbox", { name: /^Comment/ }),
  ).not.toBeChecked();
  expect((await store("snapshot")).settings).toEqual(saved);
});
