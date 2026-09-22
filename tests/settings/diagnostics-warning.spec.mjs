import { mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";
import {
  addRepository,
  closeDialog,
  repositorySettings,
  saveChanges,
  section,
} from "./navigation.mjs";

for (const action of ["add", "enable", "remove", "defaults", "override"]) {
  test(`diagnostics failure after ${action} keeps committed state visible with a warning`, async ({
    page,
    store,
    dataRoot,
  }) => {
    await store("seed_settings", { launch_at_login: true });
    await store("save_repository", { repository: "octo/hello-world" });
    const repository = (await store("snapshot")).settings.repositories[0];
    if (action === "enable") {
      await store("update_repository", {
        id: repository.id,
        repository: repository.name,
        enabled: false,
      });
    }
    const before = (await store("snapshot")).settings;
    await page.goto("/?view=settings");
    const card = page.getByRole("article", {
      name: repository.name,
      exact: true,
    });
    await expect(card).toBeVisible();
    const log = join(dataRoot, "state/diagnostics.jsonl");
    await rm(log);
    await mkdir(log);

    if (action === "add") {
      await addRepository(page, "neighbor/new");
    } else if (action === "enable") {
      await card
        .getByRole("checkbox", {
          name: `Monitor ${repository.name}`,
          exact: true,
        })
        .check();
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
    } else if (action === "defaults") {
      await section(page, "Automation");
      await page
        .getByRole("switch", {
          name: "Post review comments automatically",
          exact: true,
        })
        .check();
    } else {
      const modal = await repositorySettings(page, repository.name);
      await modal
        .getByLabel("Override run reviews automatically", { exact: true })
        .check();
      await modal
        .getByRole("switch", { name: "Run reviews automatically", exact: true })
        .check();
      await closeDialog(page);
    }
    expect((await store("snapshot")).settings).toEqual(before);
    await saveChanges(page);
    const saved = (await store("snapshot")).settings;
    await expect.soft(page.getByRole("alert")).toBeVisible();
    await expect.soft(page.getByRole("alert")).toContainText(/diagnostic/i);
    if (action === "add") {
      expect(saved.repositories.map(({ name }) => name)).toContain(
        "neighbor/new",
      );
      await expect(
        page.getByRole("article", { name: "neighbor/new", exact: true }),
      ).toBeVisible();
    } else if (action === "enable") {
      expect(saved.repositories[0].enabled).toBe(true);
      await expect(card.getByRole("checkbox")).toBeChecked();
    } else if (action === "remove") {
      expect(saved.repositories ?? []).toHaveLength(0);
      await expect(card).toHaveCount(0);
    } else if (action === "defaults") {
      expect(saved.defaults.automatic_comment_publication).toBe(true);
      const modal = await repositorySettings(page, repository.name);
      await expect(
        modal.getByRole("switch", {
          name: "Post review comments automatically",
          exact: true,
        }),
      ).toBeChecked();
      await expect(
        modal.getByLabel("Override post review comments automatically", {
          exact: true,
        }),
      ).not.toBeChecked();
    } else {
      expect(saved.repositories[0].overrides.automatic_agent_start).toBe(true);
      const modal = await repositorySettings(page, repository.name);
      await expect(
        modal.getByRole("switch", {
          name: "Run reviews automatically",
          exact: true,
        }),
      ).toBeChecked();
      await expect(
        modal.getByLabel("Override run reviews automatically", { exact: true }),
      ).toBeChecked();
    }
    expect(saved.launch_at_login).toBe(true);
  });
}
