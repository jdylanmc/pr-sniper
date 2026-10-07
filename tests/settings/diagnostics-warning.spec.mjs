import { mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";
import {
  addRepository,
  repositorySettings,
  saveChanges,
  seedAgent,
  setAgentPrompt,
  assignment,
  saveAssignment,
  closeDialog,
} from "./navigation.mjs";

for (const action of ["add", "enable", "remove", "agent", "assignment"]) {
  test(`diagnostics failure after ${action} keeps committed state visible with a warning`, async ({
    page,
    store,
    dataRoot,
  }) => {
    await store("seed_settings", { launch_at_login: true });
    await seedAgent(store, "33");
    await store("save_repository", { repository: "octo/hello-world" });
    const repository = (await store("snapshot")).settings.repositories[0];
    if (action === "enable") {
      await store("update_repository", {
        id: repository.id,
        repository: repository.name,
        enabled: false,
      });
      const configured = (await store("snapshot")).settings;
      configured.repositories[0].provider_account_id = "22";
      configured.repositories[0].provider_repository_id = "100";
      configured.repositories[0].assignments = [
        {
          id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          agent_id: configured.agents[0].id,
          schedule: configured.defaults.schedule,
          comment: false,
          approve: false,
        },
      ];
      await store("seed_settings", configured);
    }
    if (action === "assignment") {
      const configured = (await store("snapshot")).settings;
      configured.repositories[0].provider_account_id = "22";
      configured.repositories[0].provider_repository_id = "100";
      await store("seed_settings", configured);
    }
    const before = (await store("snapshot")).settings;
    await page.goto("/?view=settings");
    const log = join(dataRoot, "state/diagnostics.jsonl");
    await rm(log);
    await mkdir(log);
    const card = page.getByRole("button", {
      name: repository.name,
      exact: true,
    });
    if (action === "add") await addRepository(page, "neighbor/new");
    else if (action === "enable") {
      const editor = await repositorySettings(page, repository.name);
      await editor
        .getByRole("switch", { name: "Monitor octo/hello-world" })
        .click();
      await expect(
        editor.locator("[data-repository-monitoring-state]"),
      ).toHaveText("Enabled");
      await closeDialog(page);
    } else if (action === "remove") {
      const modal = await repositorySettings(page, repository.name);
      await modal
        .getByText("Repository and connection", { exact: true })
        .click();
      await modal
        .getByRole("button", { name: "Remove repository", exact: true })
        .click();
      await page
        .getByRole("dialog", { name: "Remove repository?", exact: true })
        .getByRole("button", { name: "Remove from settings", exact: true })
        .click();
    } else if (action === "agent")
      await setAgentPrompt(page, "Persist despite diagnostics failure.");
    else {
      const modal = await assignment(page, repository.name);
      await modal.getByRole("checkbox", { name: /^Publish Comment/ }).uncheck();
      await saveAssignment(page, modal);
    }
    await page.evaluate(() => window.__settingsIdle());
    expect((await store("snapshot")).settings).not.toEqual(before);
    await saveChanges(page);
    await expect(page.locator("#error")).toContainText(/diagnostic/i);
    const saved = (await store("snapshot")).settings;
    if (action === "add") {
      expect(saved.repositories.map(({ name }) => name)).toContain(
        "neighbor/new",
      );
      await expect(
        page.getByRole("button", { name: "neighbor/new", exact: true }),
      ).toBeVisible();
    } else if (action === "enable") {
      expect(saved.repositories[0].enabled).toBe(true);
      const editor = await repositorySettings(page, repository.name);
      await expect(
        editor.getByRole("switch", { name: "Monitor octo/hello-world" }),
      ).toBeChecked();
      await closeDialog(page);
    } else if (action === "remove") {
      expect(saved.repositories ?? []).toHaveLength(0);
      await expect(card).toHaveCount(0);
    } else if (action === "agent") {
      expect(saved.agents[0].prompt).toBe(
        "Persist despite diagnostics failure.",
      );
      await expect(page.locator(".agent-card")).toContainText(
        saved.agents[0].prompt,
      );
    } else {
      expect(saved.repositories[0].assignments[0]).toMatchObject({
        comment: false,
        approve: false,
      });
      const editor = await repositorySettings(page, repository.name);
      await expect(editor.locator(".assignment-row")).toHaveCount(1);
      await closeDialog(page);
    }
    expect(saved.launch_at_login).toBe(true);
    expect(saved.defaults).toEqual(before.defaults);
    await page.reload();
    expect((await store("snapshot")).settings).toEqual(saved);
  });
}
