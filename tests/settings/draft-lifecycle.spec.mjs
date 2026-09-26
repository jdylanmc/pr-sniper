import { expect, test } from "./fixtures.mjs";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  addRepository,
  assignment,
  saveAssignment,
  closeDialog,
  saveChanges,
  section,
  seedAgent,
  setAgentPrompt,
  editAgent,
  setSchedule,
} from "./navigation.mjs";

test.beforeEach(async ({ page, store }) => {
  await store("seed_settings", { launch_at_login: true });
  await seedAgent(store);
  for (const repository of ["octo/hello-world", "neighbor/keep-me"])
    await store("save_repository", { repository });
  await page.goto("/?view=settings");
});

for (const action of ["add", "disable"]) {
  test(`unsaved Agent prompt survives an unrelated repository ${action}`, async ({
    page,
    store,
  }) => {
    const original = (await store("snapshot")).settings;
    const prompt = "Unsaved Agent instructions must not disappear.";
    await setAgentPrompt(page, prompt);
    if (action === "add") await addRepository(page, "third/new");
    else {
      await section(page, "Integrations");
      await page
        .getByRole("checkbox", {
          name: "Monitor octo/hello-world",
          exact: true,
        })
        .uncheck();
    }
    const modal = await editAgent(page);
    await expect(
      modal.getByRole("textbox", { name: "Prompt", exact: true }),
    ).toHaveValue(prompt);
    await closeDialog(page);
    expect((await store("snapshot")).settings).toEqual(original);
    await saveChanges(page);
    const saved = (await store("snapshot")).settings;
    expect(saved.agents[0]).toEqual({ ...original.agents[0], prompt });
    expect(saved.defaults).toEqual(original.defaults);
    if (action === "add")
      expect(saved.repositories.map(({ name }) => name)).toContain("third/new");
    else expect(saved.repositories[0].enabled).toBe(false);
  });
}

for (const other of ["agent", "neighbor"]) {
  test(`repository assignment draft survives editing ${other} until global Save with independent timers`, async ({
    page,
    store,
  }) => {
    const before = (await store("snapshot")).settings;
    let modal = await assignment(page, "octo/hello-world");
    await setSchedule(modal, { kind: "interval", minutes: 7, timezone: "UTC" });
    await saveAssignment(page, modal);
    if (other === "agent")
      await setAgentPrompt(page, "Updated reusable Agent.");
    else {
      modal = await assignment(page, "neighbor/keep-me");
      await setSchedule(modal, {
        kind: "interval",
        minutes: 25,
        timezone: "Europe/London",
      });
      await saveAssignment(page, modal);
    }
    expect((await store("snapshot")).settings).toEqual(before);
    modal = await assignment(page, "octo/hello-world", 0);
    await expect(
      modal.getByLabel("Interval minutes", { exact: true }),
    ).toHaveValue("7");
    await closeDialog(page);
    await closeDialog(page);
    await saveChanges(page);
    await page.reload();
    const saved = (await store("snapshot")).settings;
    expect(saved.repositories[0].assignments[0].schedule.minutes).toBe(7);
    if (other === "agent")
      expect(saved.agents[0].prompt).toBe("Updated reusable Agent.");
    else expect(saved.repositories[1].assignments[0].schedule.minutes).toBe(25);
    expect(saved.defaults).toEqual(before.defaults);
  });
}

test("a clean-focus snapshot cannot overwrite Agent edits entered before its reply", async ({
  page,
  store,
  ipc,
}) => {
  const original = (await store("snapshot")).settings;
  await section(page, "Agents");
  const hold = ipc.holdNext("snapshot");
  try {
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await hold.arrived;
    const modal = await editAgent(page);
    const prompt = modal.getByRole("textbox", { name: "Prompt", exact: true });
    await prompt.fill("Draft entered after focus snapshot began.");
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(prompt).toHaveValue(
      "Draft entered after focus snapshot began.",
    );
    expect((await store("snapshot")).settings).toEqual(original);
  } finally {
    hold.release();
  }
});

for (const target of ["agent", "assignment"]) {
  test(`${target} save disables editors and navigation while its reply is pending`, async ({
    page,
    store,
    ipc,
  }) => {
    if (target === "agent")
      await setAgentPrompt(page, "Submitted before held reply.");
    else await saveAssignment(page, await assignment(page, "octo/hello-world"));
    const hold = ipc.holdNext("save_preferences");
    try {
      await page.locator("#save-settings").click();
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
      if (target === "agent")
        expect(saved.agents[0].prompt).toBe("Submitted before held reply.");
      else expect(saved.repositories[0].assignments).toHaveLength(1);
      await expect(
        page
          .getByRole("navigation", { name: "Settings sections" })
          .getByRole("button", { name: "Preferences" }),
      ).toBeEnabled();
    } finally {
      hold.release();
    }
  });
}

test("failed assignment save restores editable fields but keeps Approve disabled", async ({
  page,
  dataRoot,
}) => {
  const before = await readFile(join(dataRoot, "config/settings.json"));
  let modal = await assignment(page, "octo/hello-world");
  await setSchedule(modal, { kind: "interval", minutes: 42, timezone: "UTC" });
  await saveAssignment(page, modal);
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await page.locator("#save-settings").click();
  await expect(page.locator("#error")).toBeVisible();
  await expect(page.locator("#save-settings")).toBeEnabled();
  modal = await assignment(page, "octo/hello-world", 0);
  await expect(
    modal.getByRole("combobox", {
      name: "Check for pull requests",
      exact: true,
    }),
  ).toBeEnabled();
  await expect(
    modal.getByLabel("Interval minutes", { exact: true }),
  ).toHaveValue("42");
  await expect(
    modal.getByRole("checkbox", { name: /^Approve/ }),
  ).toBeDisabled();
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});
