import { test, expect } from "./fixtures.mjs";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { closeDialog, saveChanges, section } from "./navigation.mjs";

const canonicalDirectory = new URL(
  "../../.agents/skills/doctrine/doctrines/",
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
}

async function deleteDoctrine(page, card) {
  await card.getByRole("button", { name: "Delete", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Delete this doctrine?", exact: true })
    .getByRole("button", { name: "Delete doctrine", exact: true })
    .click();
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
  expect(canonical).toHaveLength(23);
  expect(new Set(canonical.map(({ title }) => title)).size).toBe(23);
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
  await expect(
    modal
      .getByRole("combobox", { name: "Doctrine", exact: true })
      .locator("option"),
  ).toHaveText(["None", ...canonical.map(({ title }) => title)]);
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
  await page
    .getByRole("button", { name: "Add repository manually...", exact: true })
    .click();
  const modal = page.getByRole("dialog", {
    name: "Add repository",
    exact: true,
  });
  await modal
    .getByLabel("GitHub repository", { exact: true })
    .fill("fixture/project");
  await modal
    .getByRole("button", { name: "Use repository", exact: true })
    .click();
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
  test(`existing ${name} is preserved without seed insertion`, async ({
    page,
    store,
    dataRoot,
  }) => {
    const config = join(dataRoot, "config");
    await mkdir(config);
    const original = JSON.stringify({ launch_at_login: false, doctrines });
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

test("doctrine drafts retain global conflict protection and explicit discard recovery", async ({
  page,
  store,
  dataRoot,
}) => {
  await page.goto("/?view=settings");
  await section(page, "Doctrines");
  await newDoctrine(page, "unsaved", "Preserve this draft until discarded.");
  const expected = (await store("snapshot")).settings;
  const external = structuredClone(expected);
  external.doctrines[0].body = "External user edit.";
  await store("save_preferences", { settings: external, expected });
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.locator("#error")).toContainText(
    "Settings changed in another window",
  );
  expect((await library(page)).at(-1).title).toBe("unsaved");
  expect((await diskSettings(dataRoot)).doctrines).toEqual(external.doctrines);
  await page
    .getByRole("button", { name: "Discard draft and reload", exact: true })
    .click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  expect(await library(page)).toEqual(external.doctrines);
});
