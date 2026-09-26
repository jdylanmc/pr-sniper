import { chmod, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";
import {
  assignment,
  saveAssignment,
  seedAgent,
  setAgentPrompt,
  editAgent,
  setSchedule,
  startupPreference,
} from "./navigation.mjs";

test("invalid assignment time zone reports an error without replacing valid settings", async ({
  page,
  store,
  dataRoot,
}) => {
  await seedAgent(store);
  await store("save_repository", { repository: "fixture/project" });
  const path = join(dataRoot, "config/settings.json");
  const before = await readFile(path);
  await page.goto("/?view=settings");
  const modal = await assignment(page, "fixture/project");
  await setSchedule(modal, {
    kind: "interval",
    minutes: 15,
    timezone: "Mars/Olympus_Mons",
  });
  await saveAssignment(page, modal);
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.locator("#error")).toBeVisible();
  await expect(page.locator("#save-settings")).toBeEnabled();
  expect(await readFile(path)).toEqual(before);
  await page.reload();
  expect(
    (await store("snapshot")).settings.repositories[0].assignments ?? [],
  ).toEqual([]);
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
        page.getByRole("navigation", { name: "Settings sections" }),
      ).toBeVisible();
      await expect(page.locator("#error")).toBeVisible();
      await expect(page.locator("#error")).not.toHaveText("");
      await expect(page.locator("#save-settings")).toBeDisabled();
      await expect(page.locator("#reset-settings")).toBeDisabled();
      await expect(
        page.getByRole("button", {
          name: "Add repository manually...",
          exact: true,
        }),
      ).toHaveCount(0);
      await expect(page.locator("#login")).toHaveCount(0);
    } finally {
      if (failure === "unreadable") await chmod(path, 0o600);
    }
    expect(await readFile(path)).toEqual(before);
  });
}

test("a failed Agent write is visible and preserves the previous config bytes", async ({
  page,
  store,
  dataRoot,
}) => {
  await seedAgent(store);
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await page.goto("/?view=settings");
  await setAgentPrompt(page, "New valid unsaved prompt.");
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await page.locator("#save-settings").click();
  await expect(page.locator("#error")).toBeVisible();
  await expect(page.locator("#save-settings")).toBeEnabled();
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});

test("returning focus preserves an unsaved Agent editor", async ({
  page,
  store,
  dataRoot,
}) => {
  await seedAgent(store);
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await page.goto("/?view=settings");
  const modal = await editAgent(page);
  const prompt = modal.getByRole("textbox", { name: "Prompt", exact: true });
  await prompt.fill("Keep this unsaved review instruction.");
  await page.evaluate(async () => {
    window.dispatchEvent(new Event("focus"));
    await window.__settingsIdle();
  });
  await expect(prompt).toHaveValue("Keep this unsaved review instruction.");
  await expect(prompt).toBeFocused();
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});

test("Settings displays forty independent persisted repositories after reload", async ({
  page,
  store,
}) => {
  await store("seed_settings", { launch_at_login: true });
  for (let index = 0; index < 40; index++)
    await store("save_repository", { repository: `octo/repository-${index}` });
  await page.goto("/?view=settings");
  await expect(page.locator(".repository-row")).toHaveCount(40);
  await page.reload();
  await expect(page.locator(".repository-row")).toHaveCount(40);
  await expect(
    page.getByRole("article", { name: "octo/repository-39", exact: true }),
  ).toBeVisible();
  await expect(await startupPreference(page)).toBeChecked();
  await expect(page.locator("#error")).toBeHidden();
});
