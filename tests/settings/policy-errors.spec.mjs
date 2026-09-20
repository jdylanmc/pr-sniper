import { chmod, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";

test("invalid time zone reports an error without replacing valid settings", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("seed_settings", { launch_at_login: true });
  const path = join(dataRoot, "config/settings.json");
  const before = await readFile(path);
  await page.goto("/?view=settings");
  const form = page.getByRole("form", { name: "Global defaults", exact: true });
  await form.getByLabel("Time zone", { exact: true }).fill("Mars/Olympus_Mons");
  await form
    .getByRole("button", { name: "Save defaults", exact: true })
    .click();
  await expect(
    form.getByRole("button", { name: "Save defaults", exact: true }),
  ).toBeEnabled();
  await expect.soft(page.getByRole("alert")).toBeVisible();
  expect(await readFile(path)).toEqual(before);
  await page.reload();
  await expect(form.getByLabel("Time zone", { exact: true })).toHaveValue(
    "UTC",
  );
});

for (const failure of ["malformed", "unreadable"]) {
  test(`${failure} saved settings stay visible as an error, never an empty editable profile`, async ({
    page,
    store,
    dataRoot,
  }) => {
    await store("seed_settings", { launch_at_login: true });
    const path = join(dataRoot, "config/settings.json");
    if (failure === "malformed") await writeFile(path, "{broken configuration");
    const before = await readFile(path);
    if (failure === "unreadable") await chmod(path, 0o000);
    try {
      await page.goto("/?view=settings");
      await expect(
        page.getByRole("heading", { name: "Settings", exact: true }),
      ).toBeVisible();
      await expect(page.getByRole("alert")).toBeVisible();
      await expect(page.getByRole("alert")).not.toHaveText("");
      await expect(
        page.getByRole("button", { name: "Add repository", exact: true }),
      ).toBeDisabled();
      await expect(page.getByLabel("Request launch at login")).toBeDisabled();
    } finally {
      if (failure === "unreadable") await chmod(path, 0o600);
    }
    expect(await readFile(path)).toEqual(before);
  });
}

test("a failed policy write is visible and preserves the previous config bytes", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("seed_settings", { launch_at_login: true });
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await page.goto("/?view=settings");
  const form = page.getByRole("form", { name: "Global defaults", exact: true });
  await form
    .getByLabel("Review prompt", { exact: true })
    .fill("New valid unsaved prompt.");
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await form
    .getByRole("button", { name: "Save defaults", exact: true })
    .click();
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(
    form.getByRole("button", { name: "Save defaults", exact: true }),
  ).toBeEnabled();
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});

test("returning focus to Settings preserves an unsaved policy edit", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("seed_settings", { launch_at_login: true });
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await page.goto("/?view=settings");
  const prompt = page
    .getByRole("form", { name: "Global defaults", exact: true })
    .getByLabel("Review prompt", { exact: true });
  await prompt.fill("Keep this unsaved review instruction.");
  await page.evaluate(async () => {
    window.dispatchEvent(new Event("focus"));
    await window.__settingsIdle();
  });
  await expect(prompt).toHaveValue("Keep this unsaved review instruction.");
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});

test("Settings displays forty independent persisted repositories after reload", async ({
  page,
  store,
}) => {
  await store("seed_settings", { launch_at_login: true });
  for (let index = 0; index < 40; index++) {
    await store("save_repository", {
      repository: `octo/repository-${index}`,
    });
  }
  await page.goto("/?view=settings");
  await expect(page.getByRole("article")).toHaveCount(40);
  await page.reload();
  await expect(page.getByRole("article")).toHaveCount(40);
  await expect(
    page.getByRole("article", { name: "octo/repository-39", exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Request launch at login")).toBeChecked();
  await expect(page.getByRole("alert")).toBeHidden();
});
