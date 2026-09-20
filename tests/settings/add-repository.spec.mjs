import { expect, test } from "./fixtures.mjs";

test("adding a repository in Settings persists its canonical name after restart", async ({
  page,
  store,
}) => {
  await store("seed_settings", { launch_at_login: true });

  await test.step("existing Settings renders real persisted startup preference", async () => {
    await page.goto("/?view=settings");
    await expect(
      page.getByRole("heading", { name: "Settings", exact: true }),
    ).toBeVisible();
    await expect(page.getByLabel("Request launch at login")).toBeChecked();
    await expect(page.getByLabel("Request launch at login")).toBeDisabled();
    await expect(page.getByRole("alert")).toBeHidden();
  });

  await test.step("add one GitHub repository using Settings", async () => {
    await expect(
      page.getByRole("button", { name: "Add repository", exact: true }),
    ).toBeVisible();
    await page
      .getByLabel("GitHub repository", { exact: true })
      .fill("Octo/Hello-World");
    await page
      .getByRole("button", { name: "Add repository", exact: true })
      .click();
    await expect(
      page.getByText("octo/hello-world", { exact: true }),
    ).toBeVisible();
    await expect(page.getByRole("alert")).toBeHidden();
  });

  await test.step("fresh Store process reads canonical data without losing host settings", async () => {
    const saved = await store("snapshot");
    expect(saved.settings.launch_at_login).toBe(true);
    expect(saved.settings.repositories).toEqual([
      expect.objectContaining({
        provider: "github",
        name: "octo/hello-world",
        enabled: true,
      }),
    ]);
  });

  await test.step("reloaded Settings reads the saved repository again", async () => {
    await page.reload();
    await expect(
      page.getByText("octo/hello-world", { exact: true }),
    ).toBeVisible();
    await expect(page.getByLabel("Request launch at login")).toBeChecked();
    await expect(page.getByRole("alert")).toBeHidden();
  });
});
