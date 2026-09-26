import { expect, test } from "./fixtures.mjs";
import {
  addRepository,
  closeDialog,
  repositorySettings,
  saveChanges,
  section,
  startupPreference,
} from "./navigation.mjs";

test("editing, disabling and removing a repository preserves its identity and its neighbor", async ({
  page,
  store,
}) => {
  await store("seed_settings", { launch_at_login: true });
  await page.goto("/?view=settings");
  await expect(await startupPreference(page)).toBeChecked();

  await test.step("add two independent repositories and explicitly save", async () => {
    for (const [repository, canonical] of [
      ["Octo/Hello-World", "octo/hello-world"],
      ["Neighbor/Keep-Me", "neighbor/keep-me"],
    ]) {
      await addRepository(page, repository);
      await expect(page.getByText(canonical, { exact: true })).toBeVisible();
      await expect(page.getByRole("alert")).toBeHidden();
    }
    expect((await store("snapshot")).settings.repositories ?? []).toEqual([]);
    await saveChanges(page);
  });
  const original = (await store("snapshot")).settings;
  expect(original.repositories).toHaveLength(2);
  const primary = original.repositories.find(
    (r) => r.name === "octo/hello-world",
  );
  const neighbor = original.repositories.find(
    (r) => r.name === "neighbor/keep-me",
  );
  expect(primary).toMatchObject({ provider: "github", enabled: true });
  expect(neighbor).toMatchObject({ provider: "github", enabled: true });
  expect(primary.id).not.toBe(neighbor.id);
  const card = (name) => page.getByRole("article", { name, exact: true });
  let current = { ...primary, name: "octo/renamed" };
  const assertSaved = async (repositories) => {
    const saved = (await store("snapshot")).settings;
    expect(saved.launch_at_login).toBe(true);
    expect(saved.repositories).toHaveLength(repositories.length);
    expect(saved.repositories).toEqual(expect.arrayContaining(repositories));
  };
  const edit = async (name) => {
    const modal = await repositorySettings(page, name);
    await modal.getByText("Repository and connection", { exact: true }).click();
    await modal
      .getByRole("button", { name: "Edit repository", exact: true })
      .click();
    return page.getByRole("dialog", { name: "Edit repository", exact: true });
  };

  await test.step("rename by stable ID using a canonical GitHub URL", async () => {
    const modal = await edit("octo/hello-world");
    await modal
      .getByLabel("GitHub repository", { exact: true })
      .fill("https://github.com/Octo/Renamed.git/");
    await modal
      .getByRole("button", { name: "Use repository", exact: true })
      .click();
    await expect(card("octo/renamed")).toBeVisible();
    await expect(card("octo/hello-world")).toHaveCount(0);
    await assertSaved([primary, neighbor]);
    await saveChanges(page);
    await assertSaved([current, neighbor]);
    await page.reload();
    await expect(card("octo/renamed")).toBeVisible();
  });

  await test.step("reject a canonical duplicate URL without changing either repository", async () => {
    const modal = await addRepository(
      page,
      "https://github.com/OCTO/RENAMED.git/",
    );
    await expect(modal.getByRole("alert")).toContainText(/already configured/i);
    await assertSaved([current, neighbor]);
    await closeDialog(page);
  });

  await test.step("reject renaming onto another configured repository", async () => {
    const modal = await edit("octo/renamed");
    await modal
      .getByLabel("GitHub repository", { exact: true })
      .fill("NEIGHBOR/KEEP-ME");
    await modal
      .getByRole("button", { name: "Use repository", exact: true })
      .click();
    await expect(modal.getByRole("alert")).toContainText(/already configured/i);
    await assertSaved([current, neighbor]);
    await page.reload();
    await expect(card("octo/renamed")).toBeVisible();
  });

  await test.step("disable and re-enable without changing stable identities", async () => {
    for (const enabled of [false, true]) {
      await card("octo/renamed")
        .getByRole("checkbox", { name: "Monitor octo/renamed", exact: true })
        .setChecked(enabled);
      await assertSaved([current, neighbor]);
      await saveChanges(page);
      current = { ...current, enabled };
      await assertSaved([current, neighbor]);
      await page.reload();
      await expect(card("octo/renamed").getByRole("checkbox")).toBeChecked({
        checked: enabled,
      });
    }
  });

  await test.step("remove only the selected repository after confirmation and Save changes", async () => {
    const modal = await repositorySettings(page, "octo/renamed");
    await modal.getByText("Repository and connection", { exact: true }).click();
    await modal
      .getByRole("button", { name: "Remove repository", exact: true })
      .click();
    const confirmation = page.getByRole("dialog", {
      name: "Remove repository?",
      exact: true,
    });
    await expect(confirmation).toBeVisible();
    await assertSaved([current, neighbor]);
    await confirmation
      .getByRole("button", { name: "Remove from settings", exact: true })
      .click();
    await expect(card("octo/renamed")).toHaveCount(0);
    await assertSaved([current, neighbor]);
    await saveChanges(page);
    await assertSaved([neighbor]);
    await page.reload();
    await expect(card("octo/renamed")).toHaveCount(0);
    await expect(card("neighbor/keep-me")).toBeVisible();
    await expect(await startupPreference(page)).toBeChecked();
    await section(page, "Integrations");
    await expect(page.getByRole("alert")).toBeHidden();
  });
});
