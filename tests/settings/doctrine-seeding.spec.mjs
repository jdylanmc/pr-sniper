import { test, expect } from "./fixtures.mjs";
import { copyDiagnostics, recordClipboard } from "./diagnostics-copy.mjs";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  closeDialog,
  saveChanges,
  section,
  editAgent,
  addRepository,
  fixtureAgent,
  repositorySettings,
  setAllPullRequests,
} from "./navigation.mjs";

const canonicalDirectory = new URL(
  "../../src-tauri/doctrines/",
  import.meta.url,
);
const canonical = await Promise.all(
  (await readdir(canonicalDirectory))
    .filter((name) => name.endsWith(".doctrine.md"))
    .map((name) => name.replace(/\.doctrine\.md$/, ""))
    .sort()
    .map(async (title) => {
      const name = `${title}.doctrine.md`;
      const source = await readFile(new URL(name, canonicalDirectory), "utf8");
      const match = source.match(
        /^---\n[\s\S]*?\n---\n\s*# [^\n]+\n([\s\S]*)$/,
      );
      if (!match) throw new Error(`Invalid canonical doctrine: ${name}`);
      return {
        title,
        body: match[1].trim(),
      };
    }),
);

async function openDoctrinePicker(modal) {
  if ((await modal.locator("form").getAttribute("data-view")) !== "doctrines")
    await modal.locator("[data-open-doctrines]").click();
}

const doctrineBox = (modal, title) =>
  modal.locator(`input[name=doctrine][value="${title}"]`);

async function expectSelection(modal, selected, shown, hiddenSelected = 0) {
  await expect(modal.locator("[data-doctrine-summary]")).toHaveText(
    `${selected} selected`,
  );
  await expect(
    modal.locator("[data-doctrine-choice]:not([hidden])"),
  ).toHaveCount(shown);
  await expect(
    modal.locator("[data-doctrine-choice][hidden] input:checked"),
  ).toHaveCount(hiddenSelected);
}

const editorLibrary = (modal) =>
  modal
    .locator("input[name=doctrine]")
    .evaluateAll((inputs) => inputs.map((input) => input.value));

/** Library entries without their generated ids. */
const content = (doctrines) =>
  doctrines.map(({ title, body }) => ({ title, body }));
const idOf = (settings, title) =>
  settings.doctrines.find((d) => d.title === title).id;
/** An Agent's Doctrine references (ids) as library titles, in its order. */
const agentTitles = (settings, index = 0) =>
  (settings.agents[index].doctrine_ids ?? []).map(
    (id) => settings.doctrines.find((d) => d.id === id)?.title ?? id,
  );

async function diskSettings(dataRoot) {
  return JSON.parse(
    await readFile(join(dataRoot, "config/settings.json"), "utf8"),
  );
}

test("stale 23-entry saved catalog is reset before the shared Agent selector opens", async ({
  page,
  store,
  dataRoot,
}) => {
  const settings = (await store("snapshot")).settings;
  delete settings.doctrine_catalog_version;
  settings.doctrines = [
    "boundaries",
    "code",
    "context",
    "cyclomatic-complexity",
    "data-processing",
    "data",
    "debugging",
    "distributed-data",
    "documentation",
    "domain",
    "idempotency",
    "integration-testing",
    "laziness",
    "machine",
    "nimble",
    "pragmatic",
    "scout",
    "sequencing",
    "solid",
    "tactical-strategic",
    "test-seams",
    "testing",
    "worktrees",
  ].map((title) => ({ title, body: `Old ${title} text.` }));
  settings.agents = [
    {
      id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      name: "Retained reviewer",
      model: "copilot",
      doctrines: ["code", "domain", "testing"],
      prompt: "Review correctness.",
      signature: "Fixture",
    },
  ];
  await writeFile(
    join(dataRoot, "config/settings.json"),
    JSON.stringify(settings),
  );
  await page.goto("/?view=settings");
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await expect(page.locator("[data-doctrine-reset]")).toHaveCount(0);
  await expect(page.locator("#content")).not.toContainText(
    "Pre-alpha doctrine library reset",
  );
  await section(page, "Agents");
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New agent", exact: true });
  await expect(modal.locator("[name=doctrine]")).toHaveCount(10);
  await expect(modal.locator("[data-doctrine-summary]")).toHaveText(
    "0 selected",
  );
  await openDoctrinePicker(modal);
  await expectSelection(modal, 0, 10);
  await doctrineBox(modal, "code").check();
  await modal.getByLabel("Filter doctrines", { exact: true }).fill("testing");
  await expectSelection(modal, 1, 1, 1);
  await modal.getByLabel("Filter doctrines", { exact: true }).fill("");
  await expectSelection(modal, 1, 10);
  const catalog = (await store("snapshot")).doctrine_catalog;
  expect(catalog.source_revision).toBe(catalog.effective_revision);
  await closeDialog(page);
  await section(page, "Doctrines");
  await expect(page.locator("[data-doctrine-catalog]")).toContainText(
    "10 doctrines / config/settings.json; defaults: src-tauri/doctrines",
  );
  await expect(page.locator("[data-doctrine-catalog]")).toContainText(
    catalog.effective_revision.slice(0, 12),
  );
  const edited = await editAgent(page, "Retained reviewer");
  await expect(edited.locator("[name=doctrine]")).toHaveCount(10);
  await expect(edited.locator("[data-doctrine-summary]")).toHaveText(
    "2 selected",
  );
  await openDoctrinePicker(edited);
  await edited.getByLabel("Filter doctrines", { exact: true }).fill("testing");
  await expectSelection(edited, 2, 1, 1);
  await closeDialog(page);
  expect(agentTitles(await diskSettings(dataRoot))).toEqual([
    "code",
    "testing",
  ]);
  const reconciled = await diskSettings(dataRoot);
  expect(reconciled.doctrine_reset).toEqual({
    previous_count: 23,
    removed_references: 1,
  });
  expect(content(reconciled.doctrines)).toEqual(canonical);
  const audit = "Doctrine reset: 23 replaced; 1 references removed.";
  await recordClipboard(page);
  await page.goto("/?view=settings");
  let log = await copyDiagnostics(page);
  expect(log).toContain(audit);
  expect(log).toContain(catalog.source);
  expect(log).toContain(catalog.effective_revision);
  await page.goto("/");
  log = await copyDiagnostics(page, { panel: true });
  expect(log).toContain(catalog.effective_revision);
  expect(log).toContain(audit);
  await page.goto("/?view=settings");
  await section(page, "Doctrines");
  await newDoctrine(page, "later-edit", "Preserve this later addition.");
  const laterSettings = await diskSettings(dataRoot);
  expect(content(laterSettings.doctrines)).toEqual([
    ...canonical,
    { title: "later-edit", body: "Preserve this later addition." },
  ]);
  expect(laterSettings.doctrine_reset).toEqual(reconciled.doctrine_reset);
  await page.reload();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await expect(page.locator("[data-doctrine-reset]")).toHaveCount(0);
  expect(await diskSettings(dataRoot)).toEqual(laterSettings);
  await page.goto("/?view=settings");
  log = await copyDiagnostics(page);
  expect(log).toContain(audit);
  expect(log).toContain("11 doctrines");
});

for (const width of [408, 320]) {
  test(`reconciled Settings overview has no notice space at ${width}px`, async ({
    page,
    store,
    dataRoot,
  }, testInfo) => {
    const settings = (await store("snapshot")).settings;
    delete settings.doctrine_catalog_version;
    settings.doctrines = [{ title: "obsolete", body: "Old principles." }];
    await writeFile(
      join(dataRoot, "config/settings.json"),
      JSON.stringify(settings),
    );
    await page.setViewportSize({ width, height: 744 });
    await page.goto("/");
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    const overview = page.locator(".settings-overview");
    await expect(overview).toBeVisible();
    await expect(page.locator("[data-doctrine-reset]")).toHaveCount(0);
    const layout = await page.locator("#content").evaluate((content) => {
      const visible = [...content.children].filter(
        (child) => child.getBoundingClientRect().height > 0,
      );
      const overview = content.querySelector(".settings-overview");
      return {
        firstIsOverview: visible[0] === overview,
        gap:
          overview.getBoundingClientRect().top -
          content.getBoundingClientRect().top,
        padding: parseFloat(getComputedStyle(content).paddingTop),
        top: overview.getBoundingClientRect().top,
        headerBottom: document
          .querySelector(".panel-header")
          .getBoundingClientRect().bottom,
      };
    });
    expect(layout.firstIsOverview).toBe(true);
    expect(Math.abs(layout.gap - layout.padding)).toBeLessThanOrEqual(1);
    expect(layout.top).toBeGreaterThanOrEqual(layout.headerBottom);
    await page.screenshot({ path: testInfo.outputPath("settings.png") });
    const saved = await diskSettings(dataRoot);
    expect(saved.doctrine_reset.previous_count).toBe(1);
    await page.reload();
    await expect(overview).toBeVisible();
    expect(await diskSettings(dataRoot)).toEqual(saved);
  });
}

test("removing reset prose preserves the separate capability notice data", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.doctrine_reset = { previous_count: 23, removed_references: 1 };
  settings.capability_notice =
    "Repository permissions reconciled: obsolete secondary grants disabled.";
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  await expect(page.locator("[data-doctrine-reset]")).toHaveCount(0);
  expect((await store("snapshot")).settings).toEqual(settings);
});

async function library(page) {
  return page.locator(".doctrine-card").evaluateAll((cards) =>
    cards.map((card) => ({
      title: card.querySelector("h3").textContent,
      body: card.querySelector("p").textContent,
    })),
  );
}

async function newDoctrine(page, title, body) {
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New doctrine", exact: true });
  await modal.getByLabel("Title", { exact: true }).fill(title);
  await modal
    .getByRole("textbox", { name: "Principles", exact: true })
    .fill(body);
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await page.evaluate(() => window.__settingsIdle());
}

async function deleteDoctrine(page, card) {
  await card.getByRole("button", { name: "Delete", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Delete this doctrine?", exact: true })
    .getByRole("button", { name: "Delete doctrine", exact: true })
    .click();
  await page.evaluate(() => window.__settingsIdle());
}

test("resource save refreshes the whole resolved library and its selection counts after an external addition", async ({
  page,
  store,
}) => {
  await page.goto("/?view=settings");
  await section(page, "Doctrines");
  const expected = (await store("snapshot")).settings;
  const external = structuredClone(expected);
  external.doctrines.push({
    title: "external-principle",
    body: "Saved from another window.",
  });
  await store("save_preferences", { settings: external, expected });
  await newDoctrine(page, "local-principle", "Saved in this window.");
  const resolved = await store("snapshot");
  expect(await library(page)).toEqual(content(resolved.settings.doctrines));
  await expect(page.locator("[data-doctrine-catalog]")).toContainText(
    "12 doctrines",
  );
  await expect(page.locator("[data-doctrine-catalog]")).toContainText(
    resolved.doctrine_catalog.effective_revision.slice(0, 12),
  );
  await section(page, "Agents");
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New agent", exact: true });
  await expect(modal.locator("[name=doctrine]")).toHaveCount(12);
  await expect(modal.locator("[data-doctrine-summary]")).toHaveText(
    "0 selected",
  );
  await openDoctrinePicker(modal);
  await expectSelection(modal, 0, 12);
});

for (const [representation, references, selected] of [
  ["ordered", { doctrines: ["code"] }, 1],
  ["legacy", { doctrine: "code" }, 1],
  ["inactive legacy", { doctrine: "code", doctrines: [] }, 0],
]) {
  for (const concurrentEdit of [false, true]) {
    test(`external selected-doctrine rename leaves the Agent untouched after an unrelated resource save (${representation})${concurrentEdit ? " without accepting a concurrent Agent edit" : ""}`, async ({
      page,
      store,
      dataRoot,
    }) => {
      // Legacy title references migrate to ids when seeded.
      const settings = (await store("snapshot")).settings;
      settings.agents = [{ ...fixtureAgent, ...references }];
      await store("seed_settings", settings);
      await store("save_repository", { repository: "fixture/rename" });
      const initial = (await store("snapshot")).settings;
      expect(agentTitles(initial)).toEqual(selected ? ["code"] : []);
      await page.goto("/?view=settings");
      await section(page, "Preferences");
      await page.locator("#global-capacity").fill("9");
      const repository = await repositorySettings(page, "fixture/rename");
      await setAllPullRequests(repository, false);
      await closeDialog(page);

      const code = initial.doctrines.find(({ title }) => title === "code");
      await store("save_resource", {
        edit: {
          kind: "doctrine",
          id: code.id,
          expected: code,
          value: { ...code, title: "renamed-code" },
        },
      });
      // The rename changed the library entry only.
      expect((await store("snapshot")).settings.agents).toEqual(initial.agents);
      if (concurrentEdit) {
        const agent = (await store("snapshot")).settings.agents[0];
        await store("save_resource", {
          edit: {
            kind: "agent",
            id: agent.id,
            expected: agent,
            value: { ...agent, prompt: "Concurrent Agent prompt." },
          },
        });
      }
      await section(page, "Doctrines");
      await newDoctrine(page, "local-principle", "Saved in this window.");
      const resolved = await store("snapshot");
      let modal;
      if (concurrentEdit) {
        // Another window changed the Agent: explicit reload before saving it.
        await expect(page.locator("[data-catalog-conflict]")).toBeVisible();
        expect(await library(page)).toEqual(content(initial.doctrines));
        modal = await editAgent(page);
        await modal
          .locator("textarea[name=prompt]")
          .fill("Keep my draft before explicit reload.");
        await modal
          .getByRole("button", { name: "Save agent", exact: true })
          .click();
        await expect(modal.getByRole("alert")).toContainText(
          "Resource changed in another window",
        );
        await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
          "Keep my draft before explicit reload.",
        );
        expect(await diskSettings(dataRoot)).toEqual(resolved.settings);
        await closeDialog(page);
        await page.getByRole("button", { name: "Reload", exact: true }).click();
      }
      await expect(page.locator("[data-catalog-conflict]")).toBeHidden();
      await section(page, "Doctrines");
      await expect(page.locator("[data-doctrine-catalog]")).toContainText(
        resolved.doctrine_catalog.effective_revision.slice(0, 12),
      );
      modal = await editAgent(page);
      await openDoctrinePicker(modal);
      await expect(doctrineBox(modal, "renamed-code")).toBeChecked({
        checked: selected === 1,
      });
      await expect(modal.locator("[name=doctrine]")).toHaveCount(11);
      await expectSelection(modal, selected, 11);
      await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
        concurrentEdit ? "Concurrent Agent prompt." : fixtureAgent.prompt,
      );
      await modal
        .locator("textarea[name=prompt]")
        .fill("Unrelated local Agent edit.");
      await modal
        .getByRole("button", { name: "Save agent", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      expect((await diskSettings(dataRoot)).agents[0]).toEqual({
        ...resolved.settings.agents[0],
        prompt: "Unrelated local Agent edit.",
      });
      expect((await diskSettings(dataRoot)).capacity).toBe(initial.capacity);
      expect((await diskSettings(dataRoot)).repositories).toEqual(
        initial.repositories,
      );
      await section(page, "Preferences");
      await expect(page.locator("#global-capacity")).toHaveValue("9");
      const pending = await repositorySettings(page, "fixture/rename");
      await expect(
        pending.getByRole("switch", { name: "All pull requests" }),
      ).not.toBeChecked();
    });
  }
}

test("unrelated library refresh does not accept a concurrent still-valid Agent selection edit", async ({
  page,
  store,
  dataRoot,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.agents = [{ ...fixtureAgent, doctrines: ["code"] }];
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  const agent = (await store("snapshot")).settings.agents[0];
  await store("save_resource", {
    edit: {
      kind: "agent",
      id: agent.id,
      expected: agent,
      value: { ...agent, doctrine_ids: [idOf(settings, "testing")] },
    },
  });
  await section(page, "Doctrines");
  await newDoctrine(page, "local-principle", "Saved in this window.");
  const resolved = (await store("snapshot")).settings;
  const modal = await editAgent(page);
  await openDoctrinePicker(modal);
  await expect(doctrineBox(modal, "code")).toBeChecked();
  await modal.locator("textarea[name=prompt]").fill("Keep my draft.");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toContainText(
    "Resource changed in another window",
  );
  await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
    "Keep my draft.",
  );
  expect(await diskSettings(dataRoot)).toEqual(resolved);
});

for (const [scenario, references, selection] of [
  ["old title reused", { doctrines: ["code"] }, null],
  [
    "concurrent reorder",
    { doctrines: ["code", "testing"] },
    ["testing", "renamed-code"],
  ],
  [
    "concurrent addition",
    { doctrines: ["code", "testing"] },
    ["renamed-code", "testing", "solid"],
  ],
  ["concurrent removal", { doctrines: ["code", "testing"] }, ["testing"]],
  [
    "inactive singular and active replacement",
    { doctrine: "code", doctrines: ["testing"] },
    ["solid"],
  ],
]) {
  test(`external rename with ${scenario} keeps references by id${selection ? " and requires explicit coherent reload" : ""}`, async ({
    page,
    store,
    dataRoot,
  }) => {
    const settings = (await store("snapshot")).settings;
    settings.agents = [{ ...fixtureAgent, ...references }];
    await store("seed_settings", settings);
    await page.goto("/?view=settings");
    const initial = await store("snapshot");
    await section(page, "Preferences");
    await page.locator("#global-capacity").fill("9");
    const code = initial.settings.doctrines.find(
      ({ title }) => title === "code",
    );
    await store("save_resource", {
      edit: {
        kind: "doctrine",
        id: code.id,
        expected: code,
        value: { ...code, title: "renamed-code" },
      },
    });
    if (!selection) {
      // A new entry reusing the old title is a different id: never selected.
      await store("save_resource", {
        edit: {
          kind: "doctrine",
          id: "9b2f6d1e-3c4a-4b5e-8f7a-1d2c3e4f5a6b",
          expected: null,
          value: {
            id: "9b2f6d1e-3c4a-4b5e-8f7a-1d2c3e4f5a6b",
            title: "code",
            body: "Unrelated replacement text.",
          },
        },
      });
    } else {
      const current = (await store("snapshot")).settings;
      const agent = current.agents[0];
      await store("save_resource", {
        edit: {
          kind: "agent",
          id: agent.id,
          expected: agent,
          value: {
            ...agent,
            doctrine_ids: selection.map((title) => idOf(current, title)),
          },
        },
      });
    }
    await section(page, "Doctrines");
    await newDoctrine(page, "local-principle", "Saved in this window.");
    const resolved = await store("snapshot");
    let modal;
    if (selection) {
      await expect(page.locator("[data-catalog-conflict]")).toBeVisible();
      await page.evaluate(async () => {
        window.dispatchEvent(new Event("focus"));
        await window.__settingsIdle();
      });
      await expect(page.locator("[data-catalog-conflict]")).toBeVisible();
      expect(await library(page)).toEqual(content(initial.settings.doctrines));
      modal = await editAgent(page);
      await openDoctrinePicker(modal);
      expect(
        await modal
          .locator("[name=doctrine]:checked")
          .evaluateAll((inputs) => inputs.map((input) => input.value)),
      ).toEqual(agentTitles(initial.settings));
      expect(await editorLibrary(modal)).toEqual(
        initial.settings.doctrines.map(({ title }) => title),
      );
      await modal
        .locator("textarea[name=prompt]")
        .fill("Keep my blocked draft.");
      await modal
        .getByRole("button", { name: "Save agent", exact: true })
        .click();
      await expect(modal.getByRole("alert")).toContainText(
        "Resource changed in another window",
      );
      await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
        "Keep my blocked draft.",
      );
      expect(await diskSettings(dataRoot)).toEqual(resolved.settings);
      await closeDialog(page);
      await page.getByRole("button", { name: "Reload", exact: true }).click();
    }
    await expect(page.locator("[data-catalog-conflict]")).toBeHidden();
    await section(page, "Doctrines");
    await expect(page.locator("[data-doctrine-catalog]")).toContainText(
      resolved.doctrine_catalog.effective_revision.slice(0, 12),
    );
    modal = await editAgent(page);
    await openDoctrinePicker(modal);
    const titles = agentTitles(resolved.settings);
    expect(titles).toEqual(selection ?? ["renamed-code"]);
    expect(
      (
        await modal
          .locator("[name=doctrine]:checked")
          .evaluateAll((inputs) => inputs.map((input) => input.value))
      ).sort(),
    ).toEqual([...titles].sort());
    await expectSelection(
      modal,
      titles.length,
      resolved.settings.doctrines.length,
    );
    if (!selection) {
      await expect(doctrineBox(modal, "code")).not.toBeChecked();
      await expect(doctrineBox(modal, "renamed-code")).toBeChecked();
    }
    await modal
      .locator("textarea[name=prompt]")
      .fill("Explicitly refreshed edit.");
    await modal
      .getByRole("button", { name: "Save agent", exact: true })
      .click();
    await expect(modal).toHaveCount(0);
    expect((await diskSettings(dataRoot)).agents[0]).toEqual({
      ...resolved.settings.agents[0],
      prompt: "Explicitly refreshed edit.",
    });
    expect(agentTitles(await diskSettings(dataRoot))).toEqual(titles);
    await section(page, "Preferences");
    await expect(page.locator("#global-capacity")).toHaveValue("9");
    expect((await diskSettings(dataRoot)).capacity).toBe(settings.capacity);
  });
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "copilot_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "copilot",
              account_id: "101",
              login: "fixture-doctrines",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        });
      if (command === "github_auth_state")
        return Promise.resolve({ accounts: [], flow: { state: "idle" } });
      return original(command, args);
    };
  });
});

test("first open persists exact canonical content and offers every doctrine to Agents before visiting Doctrines", async ({
  page,
  store,
  dataRoot,
}) => {
  expect(canonical).toHaveLength(10);
  expect(new Set(canonical.map(({ title }) => title)).size).toBe(10);
  await expect(
    readFile(join(dataRoot, "config/settings.json")),
  ).rejects.toMatchObject({ code: "ENOENT" });
  await page.goto("/?view=settings");
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  const initial = await diskSettings(dataRoot);
  expect(content(initial.doctrines)).toEqual(canonical);
  expect(initial.launch_at_login).toBe(false);
  expect(initial.defaults.automatic_agent_start).toBe(false);
  expect(initial.defaults.automatic_comment_publication).toBe(false);
  expect(initial.agents ?? []).toEqual([]);
  const snapshot = await store("snapshot");
  expect(snapshot.settings_persisted).toBe(true);
  expect(snapshot.settings).toEqual(initial);
  await section(page, "Agents");
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New agent", exact: true });
  await expect(modal.locator("[name=doctrine]")).toHaveCount(canonical.length);
  expect(
    await modal
      .locator("[name=doctrine]")
      .evaluateAll((inputs) => inputs.map((input) => input.value)),
  ).toEqual(canonical.map(({ title }) => title));
  await expect(modal.locator("[name=doctrine]:checked")).toHaveCount(0);
  await expect(modal.getByLabel("AI account", { exact: true })).toHaveValue("");
  await expect(modal.getByLabel("Model", { exact: true })).toHaveValue("");
  await closeDialog(page);
  await page.reload();
  await section(page, "Doctrines");
  expect(await library(page)).toEqual(canonical);
  expect(await diskSettings(dataRoot)).toEqual(initial);
});

test("saving Integrations first preserves the catalog through fresh Store processes and reopening", async ({
  page,
  store,
  dataRoot,
}) => {
  await page.goto("/?view=settings");
  await addRepository(page, "fixture/project");
  await saveChanges(page);
  expect(content((await diskSettings(dataRoot)).doctrines)).toEqual(canonical);
  expect((await store("snapshot")).settings.repositories[0].name).toBe(
    "fixture/project",
  );
  await page.reload();
  await section(page, "Doctrines");
  expect(await library(page)).toEqual(canonical);
});

test("doctrine edits, creation, deletion and delete-all persist without reseeding or breaking Agent references", async ({
  page,
  store,
  dataRoot,
}) => {
  const initial = (await store("snapshot")).settings;
  initial.agents = [
    {
      id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      name: "Fixture reviewer",
      model: "copilot",
      doctrine: canonical[0].title,
      prompt: "Keep this prompt.",
      signature: "Fixture",
    },
  ];
  await store("seed_settings", initial);
  await page.goto("/?view=settings");
  await section(page, "Doctrines");
  await page
    .locator(".doctrine-card")
    .first()
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  const modal = page.getByRole("dialog", {
    name: `Editing ${canonical[0].title}`,
    exact: true,
  });
  const edited = {
    title: "edited-principle",
    body: "Keep my exact edited text.\n\nSecond paragraph.",
  };
  await modal.getByLabel("Title", { exact: true }).fill(edited.title);
  await modal
    .getByRole("textbox", { name: "Principles", exact: true })
    .fill(edited.body);
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  const created = { title: "my-principle", body: "Keep my custom doctrine." };
  await newDoctrine(page, created.title, created.body);
  await deleteDoctrine(page, page.locator(".doctrine-card").nth(1));
  const expected = [edited, ...canonical.slice(2), created];
  expect(await library(page)).toEqual(expected);
  await saveChanges(page);
  const renamed = (await store("snapshot")).settings;
  expect(agentTitles(renamed)).toEqual([edited.title]);
  expect(renamed.agents[0].doctrine).toBeUndefined();
  expect(content((await diskSettings(dataRoot)).doctrines)).toEqual(expected);
  await page.reload();
  await section(page, "Doctrines");
  expect(await library(page)).toEqual(expected);
  await page
    .locator(".doctrine-card")
    .first()
    .getByRole("button", { name: "Delete", exact: true })
    .click();
  await expect(page.locator("#error")).toContainText("In use by an Agent");
  expect(content((await store("snapshot")).settings.doctrines)).toEqual(
    expected,
  );
  const agent = await editAgent(page);
  await openDoctrinePicker(agent);
  await doctrineBox(agent, edited.title).uncheck();
  await agent.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(agent).toHaveCount(0);
  await section(page, "Doctrines");
  for (let remaining = expected.length; remaining > 0; remaining--)
    await deleteDoctrine(page, page.locator(".doctrine-card").first());
  await expect(page.locator(".doctrine-card")).toHaveCount(0);
  await section(page, "Agents");
  await section(page, "Doctrines");
  await expect(page.locator(".doctrine-card")).toHaveCount(0);
  await saveChanges(page);
  const saved = await diskSettings(dataRoot);
  expect(saved.doctrines).toEqual([]);
  expect(saved.agents[0].doctrine).toBeUndefined();
  expect(saved.agents[0].doctrine_ids).toBeUndefined();
  expect(saved.agents[0].prompt).toBe("Keep this prompt.");
  await page.reload();
  await section(page, "Doctrines");
  await expect(page.locator(".doctrine-card")).toHaveCount(0);
  expect((await store("snapshot")).settings).toEqual(saved);
});

for (const [name, doctrines] of [
  ["legacy missing field", undefined],
  ["explicit empty library", []],
  [
    "custom library",
    [
      {
        id: "3f2a1b0c-9d8e-4f7a-8b6c-5d4e3f2a1b0c",
        title: "mine",
        body: "Keep only my principles.",
      },
    ],
  ],
]) {
  test(`versioned ${name} is preserved without seed insertion`, async ({
    page,
    store,
    dataRoot,
  }) => {
    const config = join(dataRoot, "config");
    await mkdir(config);
    // Voice-versioned too: no unrelated one-time Voice seed write on load.
    const original = JSON.stringify({
      launch_at_login: false,
      doctrine_catalog_version: 1,
      voice_catalog_version: 1,
      doctrines,
    });
    await writeFile(join(config, "settings.json"), original);
    await page.goto("/?view=settings");
    await page.evaluate(() => window.__settingsIdle());
    await section(page, "Agents");
    await section(page, "Doctrines");
    expect(await library(page)).toEqual(content(doctrines ?? []));
    expect(await readFile(join(config, "settings.json"), "utf8")).toBe(
      original,
    );
    await newDoctrine(page, "additional", "An explicitly added principle.");
    await saveChanges(page);
    const expected = [
      ...content(doctrines ?? []),
      { title: "additional", body: "An explicitly added principle." },
    ];
    await page.reload();
    await page.evaluate(() => window.__settingsIdle());
    await section(page, "Doctrines");
    expect(await library(page)).toEqual(expected);
    const saved = (await store("snapshot")).settings.doctrines;
    expect(content(saved)).toEqual(expected);
    // Existing ids survive; the new entry got one.
    if (doctrines?.length) expect(saved[0].id).toBe(doctrines[0].id);
  });
}

test("initialization write failure stays visible and unsaved until a successful retry", async ({
  page,
  store,
  dataRoot,
}) => {
  const blocked = join(dataRoot, "config/settings.json.tmp");
  await mkdir(blocked, { recursive: true });
  await page.goto("/?view=settings");
  await expect(page.locator("#error")).toHaveText("Cannot write settings.");
  await expect(page.locator("#save-status")).not.toHaveText(
    "All changes saved",
  );
  await expect(page.locator("#save-settings")).toBeDisabled();
  const failed = await store("snapshot");
  expect(failed.settings).toBeNull();
  expect(failed.settings_persisted).toBe(false);
  await rm(blocked, { recursive: true });
  await page.reload();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  expect(content((await diskSettings(dataRoot)).doctrines)).toEqual(canonical);
});

test("doctrine drafts retain same-resource conflict protection and explicit discard recovery", async ({
  page,
  store,
  dataRoot,
}) => {
  await page.goto("/?view=settings");
  await section(page, "Doctrines");
  await newDoctrine(page, "unsaved", "Preserve this draft until discarded.");
  await page
    .locator(".doctrine-card")
    .last()
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  const modal = page.getByRole("dialog", { name: /^Editing / });
  await modal
    .getByRole("textbox", { name: "Principles", exact: true })
    .fill("My local draft.");
  const expected = (await store("snapshot")).settings;
  const external = structuredClone(expected);
  external.doctrines.at(-1).body = "External user edit.";
  await store("save_preferences", { settings: external, expected });
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await expect(modal.getByRole("alert")).toContainText(
    "Resource changed in another window",
  );
  await expect(
    modal.getByRole("textbox", { name: "Principles", exact: true }),
  ).toHaveValue("My local draft.");
  expect((await diskSettings(dataRoot)).doctrines).toEqual(external.doctrines);
  await closeDialog(page);
  await page
    .getByRole("button", { name: "Discard draft and reload", exact: true })
    .click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  expect(await library(page)).toEqual(content(external.doctrines));
});
