import { expect, test } from "./fixtures.mjs";

test("editing, disabling and removing a repository preserves its identity and its neighbor", async ({
  page,
  store,
}) => {
  await store("seed_settings", { launch_at_login: true });
  await page.goto("/?view=settings");
  await expect(page.getByLabel("Request launch at login")).toBeChecked();

  await test.step("add two independent repositories through the working Settings form", async () => {
    for (const [repository, canonical] of [
      ["Octo/Hello-World", "octo/hello-world"],
      ["Neighbor/Keep-Me", "neighbor/keep-me"],
    ]) {
      await page
        .getByLabel("GitHub repository", { exact: true })
        .fill(repository);
      await page
        .getByRole("button", { name: "Add repository", exact: true })
        .click();
      await expect(page.getByText(canonical, { exact: true })).toBeVisible();
      await expect(page.getByRole("alert")).toBeHidden();
    }
  });
  const original = (await store("snapshot")).settings;
  expect(original.repositories).toHaveLength(2);
  const primary = original.repositories.find(
    (repository) => repository.name === "octo/hello-world",
  );
  const neighbor = original.repositories.find(
    (repository) => repository.name === "neighbor/keep-me",
  );
  expect(primary).toMatchObject({ provider: "github", enabled: true });
  expect(neighbor).toMatchObject({ provider: "github", enabled: true });
  expect(primary.id).not.toBe(neighbor.id);

  const repositoryCard = (name) =>
    page.getByRole("article", { name, exact: true });
  let current = { ...primary, name: "octo/renamed" };
  const assertSaved = async (repositories) => {
    const saved = (await store("snapshot")).settings;
    expect(saved.launch_at_login).toBe(true);
    expect(saved.repositories).toHaveLength(repositories.length);
    expect(saved.repositories).toEqual(expect.arrayContaining(repositories));
  };

  await test.step("rename by stable ID using a canonical GitHub URL", async () => {
    const card = repositoryCard("octo/hello-world");
    await expect(
      card.getByRole("button", { name: "Edit", exact: true }),
    ).toBeVisible();
    await card.getByRole("button", { name: "Edit", exact: true }).click();
    await page
      .getByLabel("Repository name", { exact: true })
      .fill("https://github.com/Octo/Renamed.git/");
    await page
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(repositoryCard("octo/renamed")).toBeVisible();
    await expect(repositoryCard("octo/hello-world")).toHaveCount(0);
    await expect(page.getByRole("alert")).toBeHidden();
    await assertSaved([current, neighbor]);
    await page.reload();
    await expect(repositoryCard("octo/renamed")).toBeVisible();
  });

  await test.step("reject a canonical duplicate URL without touching either repository", async () => {
    await page
      .getByLabel("GitHub repository", { exact: true })
      .fill("https://github.com/OCTO/RENAMED.git/");
    await page
      .getByRole("button", { name: "Add repository", exact: true })
      .click();
    await expect(page.getByRole("alert")).toContainText(/already configured/i);
    await assertSaved([current, neighbor]);
  });

  await test.step("reject renaming onto another configured repository", async () => {
    await repositoryCard("octo/renamed")
      .getByRole("button", { name: "Edit", exact: true })
      .click();
    await page
      .getByLabel("Repository name", { exact: true })
      .fill("NEIGHBOR/KEEP-ME");
    await page
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(page.getByRole("alert")).toContainText(/already configured/i);
    await assertSaved([current, neighbor]);
    await page.reload();
    await expect(repositoryCard("octo/renamed")).toBeVisible();
  });

  await test.step("disable and re-enable without changing either stable identity", async () => {
    await repositoryCard("octo/renamed")
      .getByRole("button", { name: "Disable", exact: true })
      .click();
    await expect(
      repositoryCard("octo/renamed").getByRole("button", {
        name: "Re-enable",
        exact: true,
      }),
    ).toBeVisible();
    current = { ...current, enabled: false };
    await assertSaved([current, neighbor]);
    await page.reload();
    await repositoryCard("octo/renamed")
      .getByRole("button", { name: "Re-enable", exact: true })
      .click();
    await expect(
      repositoryCard("octo/renamed").getByRole("button", {
        name: "Disable",
        exact: true,
      }),
    ).toBeVisible();
    current = { ...current, enabled: true };
    await assertSaved([current, neighbor]);
  });

  await test.step("remove only the selected repository after explicit confirmation", async () => {
    const card = repositoryCard("octo/renamed");
    await card.getByRole("button", { name: "Remove", exact: true }).click();
    await expect(
      card.getByRole("button", { name: "Confirm removal", exact: true }),
    ).toBeVisible();
    await assertSaved([current, neighbor]);
    await card
      .getByRole("button", { name: "Confirm removal", exact: true })
      .click();
    await expect(repositoryCard("octo/renamed")).toHaveCount(0);
    await assertSaved([neighbor]);
    await page.reload();
    await expect(repositoryCard("octo/renamed")).toHaveCount(0);
    await expect(repositoryCard("neighbor/keep-me")).toBeVisible();
    await expect(page.getByLabel("Request launch at login")).toBeChecked();
    await expect(page.getByRole("alert")).toBeHidden();
  });
});
