import { mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";

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
      await page
        .getByLabel("GitHub repository", { exact: true })
        .fill("neighbor/new");
      await page
        .getByRole("button", { name: "Add repository", exact: true })
        .click();
    } else if (action === "enable") {
      await card
        .getByRole("button", { name: "Re-enable", exact: true })
        .click();
    } else if (action === "remove") {
      await card.getByRole("button", { name: "Remove", exact: true }).click();
      await card
        .getByRole("button", { name: "Confirm removal", exact: true })
        .click();
    } else if (action === "defaults") {
      const form = page.getByRole("form", {
        name: "Global defaults",
        exact: true,
      });
      await form
        .getByLabel("Automatic comment publication", { exact: true })
        .check();
      await form
        .getByRole("button", { name: "Save defaults", exact: true })
        .click();
    } else {
      await card.getByRole("button", { name: "Policy", exact: true }).click();
      const form = card.getByRole("form", {
        name: "Repository policy",
        exact: true,
      });
      await form
        .getByLabel("Override automatic agent start", { exact: true })
        .check();
      await form.getByLabel("Automatic agent start", { exact: true }).check();
      await form
        .getByRole("button", { name: "Save policy", exact: true })
        .click();
    }
    await page.evaluate(() => window.__settingsIdle());
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
      await expect(
        card.getByRole("button", { name: "Disable", exact: true }),
      ).toBeVisible();
    } else if (action === "remove") {
      expect(saved.repositories ?? []).toHaveLength(0);
      await expect(card).toHaveCount(0);
    } else if (action === "defaults") {
      expect(saved.defaults.automatic_comment_publication).toBe(true);
      await expect(
        card.getByText("Automatic comment publication: on (Global default)", {
          exact: true,
        }),
      ).toBeVisible();
    } else {
      expect(saved.repositories[0].overrides.automatic_agent_start).toBe(true);
      await expect(
        card.getByText("Automatic agent start: on (Repository override)", {
          exact: true,
        }),
      ).toBeVisible();
    }
    expect(saved.launch_at_login).toBe(true);
  });
}
