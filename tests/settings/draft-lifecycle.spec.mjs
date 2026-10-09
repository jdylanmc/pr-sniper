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
  repositorySettings,
  seedBoundRepositories,
} from "./navigation.mjs";

test.beforeEach(async ({ page, store }) => {
  await store("seed_settings", { launch_at_login: true });
  await seedAgent(store, "33");
  await seedBoundRepositories(store, ["octo/hello-world", "neighbor/keep-me"]);
  await page.goto("/?view=settings");
});

for (const action of ["add", "disable"]) {
  test(`saved Agent prompt survives an unrelated repository ${action}`, async ({
    page,
    store,
  }) => {
    const original = (await store("snapshot")).settings;
    const prompt = "Unsaved Agent instructions must not disappear.";
    await setAgentPrompt(page, prompt);
    if (action === "add") await addRepository(page, "third/new");
    else {
      const repository = await repositorySettings(page, "octo/hello-world");
      await repository
        .getByRole("switch", { name: "Monitor octo/hello-world" })
        .click();
      await expect(
        repository.locator("[data-repository-monitoring-state]"),
      ).toHaveText("Monitoring: Disabled");
      await closeDialog(page);
    }
    const modal = await editAgent(page);
    await expect(
      modal.getByRole("textbox", { name: "Prompt", exact: true }),
    ).toHaveValue(prompt);
    await closeDialog(page);
    expect((await store("snapshot")).settings.agents[0]).toEqual({
      ...original.agents[0],
      prompt,
    });
    expect((await store("snapshot")).settings.defaults).toEqual(
      original.defaults,
    );
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
  test(`saved repository assignment survives independently saving ${other}`, async ({
    page,
    store,
  }) => {
    const before = (await store("snapshot")).settings;
    let modal = await assignment(page, "octo/hello-world");
    await modal.getByRole("checkbox", { name: /^Publish Comment/ }).uncheck();
    await saveAssignment(page, modal);
    if (other === "agent")
      await setAgentPrompt(page, "Updated reusable Agent.");
    else {
      modal = await assignment(page, "neighbor/keep-me");
      await modal.getByRole("checkbox", { name: /^Publish Comment/ }).check();
      await saveAssignment(page, modal);
    }
    expect(
      (await store("snapshot")).settings.repositories[0].assignments[0].comment,
    ).toBe(false);
    modal = await assignment(page, "octo/hello-world", 0);
    await expect(
      modal.getByRole("checkbox", { name: /^Publish Comment/ }),
    ).not.toBeChecked();
    await closeDialog(page);
    await closeDialog(page);
    await saveChanges(page);
    await page.reload();
    const saved = (await store("snapshot")).settings;
    expect(saved.repositories[0].assignments[0].schedule).toEqual(
      before.defaults.schedule,
    );
    if (other === "agent")
      expect(saved.agents[0].prompt).toBe("Updated reusable Agent.");
    else expect(saved.repositories[1].assignments[0].comment).toBe(true);
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
    const modal =
      target === "agent"
        ? await editAgent(page)
        : await assignment(page, "octo/hello-world");
    if (target === "agent")
      await modal
        .getByRole("textbox", { name: "Prompt", exact: true })
        .fill("Submitted before held reply.");
    const hold = ipc.holdNext("save_resource");
    try {
      await modal
        .locator("form")
        .getByRole("button", {
          name: target === "agent" ? "Save agent" : "Assign agent",
          exact: true,
        })
        .click();
      await hold.arrived;
      const controls = await page
        .locator(
          ".settings-window input,.settings-window textarea,.settings-window select,.settings-window button",
        )
        .all();
      await expect(page.locator("#save-status")).toHaveText("Working...");
      expect(controls.length).toBeGreaterThan(0);
      for (const control of controls) await expect(control).toBeDisabled();
      hold.release();
      await page.evaluate(() => window.__settingsIdle());
      const saved = (await store("snapshot")).settings;
      if (target === "agent")
        expect(saved.agents[0].prompt).toBe("Submitted before held reply.");
      else expect(saved.repositories[0].assignments).toHaveLength(1);
      if (target === "assignment") await closeDialog(page);
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

test("failed assignment save restores the draft and independent opt-in controls", async ({
  page,
  dataRoot,
}) => {
  const before = await readFile(join(dataRoot, "config/settings.json"));
  const modal = await assignment(page, "octo/hello-world");
  await modal.getByRole("checkbox", { name: /^Publish Comment/ }).uncheck();
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await modal
    .locator("form")
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await expect(modal.getByRole("alert")).toBeVisible();
  await expect(
    modal
      .locator("form")
      .getByRole("button", { name: "Assign agent", exact: true }),
  ).toBeEnabled();
  await expect(
    modal.getByRole("combobox", { name: "Agent", exact: true }),
  ).toBeEnabled();
  await expect(
    modal.getByRole("checkbox", { name: /^Publish Comment/ }),
  ).not.toBeChecked();
  await expect(
    modal.getByRole("radio", { name: "Approve", exact: true }),
  ).toBeEnabled();
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});
