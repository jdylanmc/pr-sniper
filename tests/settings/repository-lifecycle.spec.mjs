import { expect, test } from "./fixtures.mjs";
import {
  providerFixture,
  repositoryPage,
  addByUrl,
  reviewer,
} from "./repository-provider-fixture.mjs";
import { repositorySettings, section, pickAgent } from "./navigation.mjs";

test("adding, reopening, configuring, pausing and removing preserves identities and neighbors", async ({
  page,
  store,
}) => {
  const initial = (await store("snapshot")).settings;
  initial.launch_at_login = true;
  initial.agents = [reviewer];
  await store("seed_settings", initial);
  await providerFixture(page, store);
  await repositoryPage(page, store, false);
  for (const name of ["fixture/one", "fixture/two"]) {
    const editor = await addByUrl(page, name);
    await expect(editor).toBeVisible();
    await editor
      .getByRole("button", { name: "Cancel repository changes" })
      .click();
  }
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories).toHaveLength(2);
  expect(saved.repositories.every((r) => !r.enabled)).toBe(true);
  expect(new Set(saved.repositories.map((r) => r.id)).size).toBe(2);
  const neighbor = saved.repositories[1];
  const reopened = await addByUrl(page, "fixture/one");
  await expect(reopened).toBeVisible();
  expect((await store("snapshot")).settings).toEqual(saved);
  await reopened
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assignment = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await pickAgent(assignment, reviewer.id);
  await assignment
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await reopened
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(reopened).toHaveCount(0);
  for (const enabled of [false, true]) {
    const editor = await repositorySettings(page, "fixture/one");
    await editor.getByRole("switch", { name: "Monitor fixture/one" }).click();
    await expect(
      editor.locator("[data-repository-monitoring-state]"),
    ).toHaveText(enabled ? "Enabled" : "Disabled");
    const before = (await store("snapshot")).settings;
    expect(before.repositories[0].enabled).toBe(enabled);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor).toHaveCount(0);
    const current = (await store("snapshot")).settings;
    expect(current.repositories[0].id).toBe(saved.repositories[0].id);
    expect(current.repositories[0].enabled).toBe(enabled);
    expect(current.repositories[1]).toEqual(neighbor);
    await page.reload();
    await section(page, "Repositories");
  }
  const editor = await repositorySettings(page, "fixture/one");
  await editor.locator(".repository-group > summary").click();
  const remove = editor.getByRole("button", {
    name: "Remove repository",
    exact: true,
  });
  const confirmName = editor.getByLabel("Type repository name to confirm");
  await expect(remove).toBeDisabled();
  await confirmName.fill("Fixture/One");
  await expect(remove).toBeDisabled();
  await confirmName.fill(" fixture/one ");
  await expect(remove).toBeEnabled();
  expect((await store("snapshot")).settings.repositories).toHaveLength(2);
  await remove.click();
  await expect(editor).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories).toEqual([neighbor]);
  expect((await store("snapshot")).settings.launch_at_login).toBe(true);
  await page.reload();
  await section(page, "Repositories");
  await expect(
    page.getByRole("button", { name: "fixture/one", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "fixture/two", exact: true }),
  ).toBeVisible();
});

test("re-adding a repository by URL cannot reset an already configured stable repository", async ({
  page,
  store,
}) => {
  await providerFixture(page, store);
  await repositoryPage(page, store);
  const first = await addByUrl(page);
  await first
    .getByRole("button", { name: "Cancel repository changes" })
    .click();
  const before = (await store("snapshot")).settings;
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const binding = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await binding
    .getByLabel("Repository URL", { exact: true })
    .fill("https://github.com/FIXTURE/ONE.git");
  await binding
    .getByLabel("Acting GitHub account", { exact: true })
    .selectOption("22");
  await binding
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "fixture/one", exact: true }),
  ).toBeVisible();
  expect((await store("snapshot")).settings).toEqual(before);
});
