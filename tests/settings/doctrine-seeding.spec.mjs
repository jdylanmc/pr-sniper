import { test, expect } from "./fixtures.mjs";
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
  await expect(page.locator("[data-doctrine-reset]")).toContainText(
    "replaced 23 doctrines with the 10 shipped defaults",
  );
  await section(page, "Agents");
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New agent", exact: true });
  await expect(modal.locator("[name=doctrine]")).toHaveCount(10);
  await expect(modal.locator("[data-selection-count]")).toHaveText(
    "0 selected / 10 shown",
  );
  await expect(modal.locator("[data-doctrine-catalog]")).toContainText(
    "10 doctrines / config/settings.json; defaults: src-tauri/doctrines",
  );
  await modal.getByRole("checkbox", { name: "code", exact: true }).check();
  await modal.getByLabel("Filter doctrines", { exact: true }).fill("testing");
  await expect(modal.locator("[data-selection-count]")).toHaveText(
    "1 selected (1 hidden by filter) / 1 shown",
  );
  await modal.getByLabel("Filter doctrines", { exact: true }).fill("");
  await expect(modal.locator("[data-selection-count]")).toHaveText(
    "1 selected / 10 shown",
  );
  const catalog = (await store("snapshot")).doctrine_catalog;
  expect(catalog.source_revision).toBe(catalog.effective_revision);
  await closeDialog(page);
  await page
    .locator(".agent-card")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  const edited = page.getByRole("dialog", { name: "Edit agent", exact: true });
  await expect(edited.locator("[name=doctrine]")).toHaveCount(10);
  await expect(edited.locator("[data-selection-count]")).toHaveText(
    "2 selected / 10 shown",
  );
  await edited.getByLabel("Filter doctrines", { exact: true }).fill("testing");
  await expect(edited.locator("[data-selection-count]")).toHaveText(
    "2 selected (1 hidden by filter) / 1 shown",
  );
  await expect(edited.locator("[data-doctrine-catalog]")).toContainText(
    catalog.effective_revision.slice(0, 12),
  );
  await closeDialog(page);
  expect((await diskSettings(dataRoot)).agents[0].doctrines).toEqual([
    "code",
    "testing",
  ]);
  await page.goto("/?view=diagnostics");
  await expect(page.locator("#log")).toContainText(catalog.source);
  await expect(page.locator("#log")).toContainText(catalog.effective_revision);
  await page.goto("/");
  await page.locator("[data-panel-diagnostics]").click();
  await expect(page.locator('[data-panel-view="utility"] pre')).toContainText(
    catalog.effective_revision,
  );
  expect((await diskSettings(dataRoot)).doctrines).toEqual(canonical);
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
  expect(await library(page)).toEqual(resolved.settings.doctrines);
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
  await expect(modal.locator("[data-selection-count]")).toHaveText(
    "0 selected / 12 shown",
  );
});

for (const [representation, references, selected] of [
  ["ordered", { doctrines: ["code"] }, 1],
  ["legacy", { doctrine: "code" }, 1],
  ["inactive legacy", { doctrine: "code", doctrines: [] }, 0],
]) {
  for (const concurrentEdit of [false, true]) {
    test(`external selected-doctrine rename stays coherent after an unrelated resource save (${representation})${concurrentEdit ? " without accepting a concurrent Agent edit" : ""}`, async ({
      page,
      store,
      dataRoot,
    }) => {
      const settings = (await store("snapshot")).settings;
      settings.agents = [{ ...fixtureAgent, ...references }];
      await store("seed_settings", settings);
      await store("save_repository", { repository: "fixture/rename" });
      const initial = (await store("snapshot")).settings;
      await page.goto("/?view=settings");
      await section(page, "Preferences");
      await page.locator("#global-capacity").fill("9");
      const repository = await repositorySettings(page, "fixture/rename");
      await repository
        .getByLabel("Review start", { exact: true })
        .selectOption("automatic");
      await closeDialog(page);

      const code = initial.doctrines.find(({ title }) => title === "code");
      await store("save_resource", {
        edit: {
          kind: "doctrine",
          title: "code",
          expected: code,
          value: { ...code, title: "renamed-code" },
        },
      });
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
      expect(resolved.settings.agents[0]).toMatchObject(
        representation === "ordered"
          ? { doctrines: ["renamed-code"] }
          : { doctrine: "renamed-code" },
      );
      expect(await library(page)).toEqual(resolved.settings.doctrines);
      const modal = await editAgent(page);
      await expect(
        modal.getByRole("checkbox", { name: "renamed-code", exact: true }),
      ).toBeChecked({ checked: selected === 1 });
      await expect(modal.locator("[name=doctrine]")).toHaveCount(11);
      await expect(modal.locator("[data-selection-count]")).toHaveText(
        `${selected} selected / 11 shown`,
      );
      await expect(modal.locator("[data-doctrine-catalog]")).toContainText(
        resolved.doctrine_catalog.effective_revision.slice(0, 12),
      );
      await expect(
        modal.getByRole("textbox", { name: "Prompt", exact: true }),
      ).toHaveValue(fixtureAgent.prompt);
      await modal
        .getByRole("textbox", { name: "Prompt", exact: true })
        .fill("Unrelated local Agent edit.");
      await modal
        .getByRole("button", { name: "Save agent", exact: true })
        .click();
      if (concurrentEdit) {
        await expect(modal.getByRole("alert")).toContainText(
          "Resource changed in another window",
        );
        await expect(
          modal.getByRole("textbox", { name: "Prompt", exact: true }),
        ).toHaveValue("Unrelated local Agent edit.");
        expect(await diskSettings(dataRoot)).toEqual(resolved.settings);
        await closeDialog(page);
      } else {
        await expect(modal).toHaveCount(0);
        const expectedAgent = {
          ...resolved.settings.agents[0],
          prompt: "Unrelated local Agent edit.",
        };
        if (representation === "inactive legacy") delete expectedAgent.doctrine;
        expect((await diskSettings(dataRoot)).agents[0]).toEqual(expectedAgent);
      }
      expect((await diskSettings(dataRoot)).capacity).toBe(initial.capacity);
      expect((await diskSettings(dataRoot)).repositories).toEqual(
        initial.repositories,
      );
      await section(page, "Preferences");
      await expect(page.locator("#global-capacity")).toHaveValue("9");
      const pending = await repositorySettings(page, "fixture/rename");
      await expect(
        pending.getByLabel("Review start", { exact: true }),
      ).toHaveValue("automatic");
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
      value: { ...agent, doctrines: ["testing"] },
    },
  });
  await section(page, "Doctrines");
  await newDoctrine(page, "local-principle", "Saved in this window.");
  const resolved = (await store("snapshot")).settings;
  const modal = await editAgent(page);
  await expect(
    modal.getByRole("checkbox", { name: "code", exact: true }),
  ).toBeChecked();
  await modal
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("Keep my draft.");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toContainText(
    "Resource changed in another window",
  );
  await expect(
    modal.getByRole("textbox", { name: "Prompt", exact: true }),
  ).toHaveValue("Keep my draft.");
  expect(await diskSettings(dataRoot)).toEqual(resolved);
});

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
  expect(initial.doctrines).toEqual(canonical);
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
  expect((await diskSettings(dataRoot)).doctrines).toEqual(canonical);
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
    name: "Edit doctrine",
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
  expect((await store("snapshot")).settings.agents[0].doctrine).toBe(
    edited.title,
  );
  expect((await diskSettings(dataRoot)).doctrines).toEqual(expected);
  await page.reload();
  await section(page, "Doctrines");
  expect(await library(page)).toEqual(expected);
  await page
    .locator(".doctrine-card")
    .first()
    .getByRole("button", { name: "Delete", exact: true })
    .click();
  await expect(page.locator("#error")).toContainText("used by an Agent");
  expect((await store("snapshot")).settings.doctrines).toEqual(expected);
  const agent = await editAgent(page);
  await agent
    .getByRole("checkbox", { name: edited.title, exact: true })
    .uncheck();
  await agent.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(agent).toHaveCount(0);
  await section(page, "Doctrines");
  for (let remaining = expected.length; remaining > 0; remaining--)
    await deleteDoctrine(page, page.locator(".doctrine-card").first());
  await expect(
    page.getByText("No doctrines yet", { exact: true }),
  ).toBeVisible();
  await section(page, "Agents");
  await section(page, "Doctrines");
  await expect(page.locator(".doctrine-card")).toHaveCount(0);
  await saveChanges(page);
  const saved = await diskSettings(dataRoot);
  expect(saved.doctrines).toEqual([]);
  expect(saved.agents[0].doctrine).toBeUndefined();
  expect(saved.agents[0].prompt).toBe("Keep this prompt.");
  await page.reload();
  await section(page, "Doctrines");
  await expect(page.locator(".doctrine-card")).toHaveCount(0);
  expect((await store("snapshot")).settings).toEqual(saved);
});

for (const [name, doctrines] of [
  ["legacy missing field", undefined],
  ["explicit empty library", []],
  ["custom library", [{ title: "mine", body: "Keep only my principles." }]],
]) {
  test(`versioned ${name} is preserved without seed insertion`, async ({
    page,
    store,
    dataRoot,
  }) => {
    const config = join(dataRoot, "config");
    await mkdir(config);
    const original = JSON.stringify({
      launch_at_login: false,
      doctrine_catalog_version: 1,
      doctrines,
    });
    await writeFile(join(config, "settings.json"), original);
    await page.goto("/?view=settings");
    await section(page, "Agents");
    await section(page, "Doctrines");
    expect(await library(page)).toEqual(doctrines ?? []);
    expect(await readFile(join(config, "settings.json"), "utf8")).toBe(
      original,
    );
    await newDoctrine(page, "additional", "An explicitly added principle.");
    await saveChanges(page);
    const expected = [
      ...(doctrines ?? []),
      { title: "additional", body: "An explicitly added principle." },
    ];
    await page.reload();
    await section(page, "Doctrines");
    expect(await library(page)).toEqual(expected);
    expect((await store("snapshot")).settings.doctrines).toEqual(expected);
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
  expect((await diskSettings(dataRoot)).doctrines).toEqual(canonical);
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
  const modal = page.getByRole("dialog", {
    name: "Edit doctrine",
    exact: true,
  });
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
  expect(await library(page)).toEqual(external.doctrines);
});
