import { expect, test } from "./fixtures.mjs";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  addRepository,
  advancedSchedule,
  closeDialog,
  repositorySettings,
  saveChanges,
  section,
} from "./navigation.mjs";

test.beforeEach(async ({ page, store }) => {
  await store("seed_settings", { launch_at_login: true });
  await store("save_repository", { repository: "octo/hello-world" });
  await store("save_repository", { repository: "neighbor/keep-me" });
  await page.goto("/?view=settings");
  await section(page, "Review defaults");
  await expect(page.getByLabel("Review prompt", { exact: true })).toBeVisible();
});

for (const action of ["add", "disable"]) {
  test(`unsaved global policy survives an unrelated repository ${action}`, async ({
    page,
    store,
  }) => {
    const original = (await store("snapshot")).settings;
    const draft = "Unsaved global instructions must not disappear.";
    await page.getByLabel("Review prompt", { exact: true }).fill(draft);
    if (action === "add") {
      await addRepository(page, "third/new");
      await expect(
        page.getByRole("article", { name: "third/new", exact: true }),
      ).toBeVisible();
    } else {
      await section(page, "Repositories");
      await page
        .getByRole("checkbox", {
          name: "Monitor octo/hello-world",
          exact: true,
        })
        .uncheck();
    }
    await section(page, "Review defaults");
    await expect(page.getByLabel("Review prompt", { exact: true })).toHaveValue(
      draft,
    );
    expect((await store("snapshot")).settings).toEqual(original);
    await saveChanges(page);
    const saved = (await store("snapshot")).settings;
    expect(saved.defaults).toEqual({ ...original.defaults, prompt: draft });
    if (action === "add") {
      expect(saved.repositories.map(({ name }) => name)).toContain("third/new");
    } else {
      expect(
        saved.repositories.find(({ name }) => name === "octo/hello-world")
          .enabled,
      ).toBe(false);
    }
  });
}

for (const otherForm of ["global", "neighbor"]) {
  test(`repository policy draft survives editing ${otherForm} until global Save without freezing inherited fields`, async ({
    page,
    store,
  }) => {
    const before = (await store("snapshot")).settings;
    let modal = await repositorySettings(page, "octo/hello-world");
    await modal
      .getByLabel("Override review instructions", { exact: true })
      .check();
    await modal
      .getByLabel("Review prompt", { exact: true })
      .fill("Unsaved primary override.");
    await closeDialog(page);
    if (otherForm === "global") {
      await section(page, "Review defaults");
      await page
        .getByLabel("Review prompt", { exact: true })
        .fill("New authoritative global prompt.");
      await section(page, "Automation");
      await advancedSchedule(page);
      await page.getByLabel("Interval minutes", { exact: true }).fill("25");
    } else {
      const neighbor = await repositorySettings(page, "neighbor/keep-me");
      await neighbor
        .getByLabel("Override review instructions", { exact: true })
        .check();
      await neighbor
        .getByLabel("Review prompt", { exact: true })
        .fill("Saved neighbor instructions.");
      await closeDialog(page);
    }
    expect((await store("snapshot")).settings).toEqual(before);
    modal = await repositorySettings(page, "octo/hello-world");
    await expect(
      modal.getByLabel("Override review instructions", { exact: true }),
    ).toBeChecked();
    await expect(
      modal.getByLabel("Review prompt", { exact: true }),
    ).toHaveValue("Unsaved primary override.");
    if (otherForm === "global") {
      await advancedSchedule(modal);
      await expect(
        modal.getByLabel("Interval minutes", { exact: true }),
      ).toHaveValue("25");
      await expect(
        modal.getByLabel("Override schedule", { exact: true }),
      ).not.toBeChecked();
    }
    await closeDialog(page);
    await saveChanges(page);
    await page.reload();
    const saved = (await store("snapshot")).settings;
    const primary = saved.repositories.find(
      ({ name }) => name === "octo/hello-world",
    );
    expect(primary.overrides).toEqual({ prompt: "Unsaved primary override." });
    if (otherForm === "global") {
      expect(saved.defaults.prompt).toBe("New authoritative global prompt.");
      expect(saved.defaults.schedule.minutes).toBe(25);
    } else {
      expect(
        saved.repositories.find(({ name }) => name === "neighbor/keep-me")
          .overrides,
      ).toEqual({ prompt: "Saved neighbor instructions." });
    }
    modal = await repositorySettings(page, "octo/hello-world");
    await expect(
      modal.getByLabel("Review prompt", { exact: true }),
    ).toHaveValue("Unsaved primary override.");
    await expect(
      modal.getByLabel("Override schedule", { exact: true }),
    ).not.toBeChecked();
  });
}

test("a clean-focus snapshot cannot overwrite edits entered before its reply", async ({
  page,
  store,
  ipc,
}) => {
  const original = (await store("snapshot")).settings.defaults;
  const hold = ipc.holdNext("snapshot");
  try {
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await hold.arrived;
    await page
      .getByLabel("Review prompt", { exact: true })
      .fill("Draft entered after focus snapshot began.");
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(page.getByLabel("Review prompt", { exact: true })).toHaveValue(
      "Draft entered after focus snapshot began.",
    );
    expect((await store("snapshot")).settings.defaults).toEqual(original);
  } finally {
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
  }
});

for (const target of ["global", "repository"]) {
  test(`${target} policy save disables the global editor and navigation while its reply is pending`, async ({
    page,
    store,
    ipc,
  }) => {
    if (target === "repository") {
      const modal = await repositorySettings(page, "octo/hello-world");
      await modal
        .getByLabel("Override review instructions", { exact: true })
        .check();
      await modal
        .getByLabel("Review prompt", { exact: true })
        .fill("Policy submitted before held reply.");
      await closeDialog(page);
    } else {
      await page
        .getByLabel("Review prompt", { exact: true })
        .fill("Policy submitted before held reply.");
    }
    const hold = ipc.holdNext("save_preferences");
    try {
      await page
        .getByRole("button", { name: "Save changes", exact: true })
        .click();
      await hold.arrived;
      const controls = await page
        .locator(
          ".settings-window input,.settings-window textarea,.settings-window select,.settings-window button",
        )
        .all();
      expect(controls.length).toBeGreaterThan(0);
      for (const control of controls) await expect(control).toBeDisabled();
      hold.release();
      await page.evaluate(() => window.__settingsIdle());
      const saved = (await store("snapshot")).settings;
      const prompt =
        target === "global"
          ? saved.defaults.prompt
          : saved.repositories.find(({ name }) => name === "octo/hello-world")
              .overrides.prompt;
      expect(prompt).toBe("Policy submitted before held reply.");
      if (target === "repository") {
        const modal = await repositorySettings(page, "octo/hello-world");
        await expect(
          modal.getByLabel("Review prompt", { exact: true }),
        ).toBeEnabled();
        await expect(
          modal.getByLabel("Check frequency", { exact: true }),
        ).toBeDisabled();
      } else {
        await expect(
          page.getByLabel("Review prompt", { exact: true }),
        ).toBeEnabled();
      }
    } finally {
      hold.release();
      await page.evaluate(() => window.__settingsIdle());
    }
  });
}

test("failed repository policy save restores editable overrides but not inherited controls", async ({
  page,
  dataRoot,
}) => {
  const before = await readFile(join(dataRoot, "config/settings.json"));
  let modal = await repositorySettings(page, "octo/hello-world");
  await modal
    .getByLabel("Override review instructions", { exact: true })
    .check();
  await modal
    .getByLabel("Review prompt", { exact: true })
    .fill("Keep editable after failed save.");
  await closeDialog(page);
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Save changes", exact: true }),
  ).toBeEnabled();
  modal = await repositorySettings(page, "octo/hello-world");
  await expect(
    modal.getByLabel("Review prompt", { exact: true }),
  ).toBeEnabled();
  await expect(modal.getByLabel("Review prompt", { exact: true })).toHaveValue(
    "Keep editable after failed save.",
  );
  await expect(
    modal.getByLabel("Override review instructions", { exact: true }),
  ).toBeEnabled();
  await expect(
    modal.getByLabel("Check frequency", { exact: true }),
  ).toBeDisabled();
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});
