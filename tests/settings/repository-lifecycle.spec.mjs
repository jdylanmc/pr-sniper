import { expect, test } from "./fixtures.mjs";
import {
  providerFixture,
  repositoryPage,
  addByUrl,
  reviewer,
} from "./repository-provider-fixture.mjs";
import { repositorySettings, section } from "./navigation.mjs";

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
  await assignment
    .getByLabel("Agent", { exact: true })
    .selectOption(reviewer.id);
  await assignment
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await reopened
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(reopened).toHaveCount(0);
  for (const enabled of [false, true]) {
    const editor = await repositorySettings(page, "fixture/one");
    await editor
      .getByLabel("Enable repository monitoring on Save")
      .setChecked(enabled);
    const before = (await store("snapshot")).settings;
    expect(before.repositories[0].enabled).toBe(!enabled);
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
  await editor.getByText("Repository and connection", { exact: true }).click();
  await editor
    .getByRole("button", { name: "Remove repository", exact: true })
    .click();
  const confirm = page.getByRole("dialog", {
    name: "Remove repository?",
    exact: true,
  });
  expect((await store("snapshot")).settings.repositories).toHaveLength(2);
  await confirm
    .getByRole("button", { name: "Remove from settings", exact: true })
    .click();
  await expect(confirm).toHaveCount(0);
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

test("editing a binding cannot reset an already configured stable repository", async ({
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
  const editor = await repositorySettings(page, "fixture/one");
  await editor.getByText("Repository and connection", { exact: true }).click();
  await editor
    .getByRole("button", { name: "Edit repository", exact: true })
    .click();
  const binding = page.getByRole("dialog", {
    name: "Edit repository",
    exact: true,
  });
  await binding
    .getByLabel("Repository URL")
    .fill("https://github.com/FIXTURE/ONE.git");
  await binding
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  await expect(editor).toBeVisible();
  expect((await store("snapshot")).settings).toEqual(before);
});
