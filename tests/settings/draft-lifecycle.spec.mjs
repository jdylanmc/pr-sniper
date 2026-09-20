import { expect, test } from "./fixtures.mjs";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";

test.beforeEach(async ({ page, store }) => {
  await store("seed_settings", { launch_at_login: true });
  await store("save_repository", { repository: "octo/hello-world" });
  await store("save_repository", { repository: "neighbor/keep-me" });
  await page.goto("/?view=settings");
  await expect(
    page.getByRole("form", { name: "Global defaults", exact: true }),
  ).toBeVisible();
});

for (const action of ["add", "disable"]) {
  test(`unsaved global policy survives an unrelated repository ${action}`, async ({
    page,
    store,
  }) => {
    const form = page.getByRole("form", {
      name: "Global defaults",
      exact: true,
    });
    const original = (await store("snapshot")).settings.defaults;
    const draft = "Unsaved global instructions must not disappear.";
    await form.getByLabel("Review prompt", { exact: true }).fill(draft);
    if (action === "add") {
      await page
        .getByLabel("GitHub repository", { exact: true })
        .fill("third/new");
      await page
        .getByRole("button", { name: "Add repository", exact: true })
        .click();
      await expect(
        page.getByRole("article", { name: "third/new", exact: true }),
      ).toBeVisible();
    } else {
      const card = page.getByRole("article", {
        name: "octo/hello-world",
        exact: true,
      });
      await card.getByRole("button", { name: "Disable", exact: true }).click();
      await expect(
        card.getByRole("button", { name: "Re-enable", exact: true }),
      ).toBeVisible();
    }
    await expect(form.getByLabel("Review prompt", { exact: true })).toHaveValue(
      draft,
    );
    expect((await store("snapshot")).settings.defaults).toEqual(original);
  });
}

for (const otherForm of ["global", "neighbor"]) {
  test(`repository policy draft survives saving ${otherForm} without freezing inherited fields`, async ({
    page,
    store,
  }) => {
    const card = page.getByRole("article", {
      name: "octo/hello-world",
      exact: true,
    });
    await card.getByRole("button", { name: "Policy", exact: true }).click();
    const form = card.getByRole("form", {
      name: "Repository policy",
      exact: true,
    });
    await form.getByLabel("Override prompt", { exact: true }).check();
    await form
      .getByLabel("Review prompt", { exact: true })
      .fill("Unsaved primary override.");

    if (otherForm === "global") {
      const global = page.getByRole("form", {
        name: "Global defaults",
        exact: true,
      });
      await global
        .getByLabel("Review prompt", { exact: true })
        .fill("New authoritative global prompt.");
      await global.getByLabel("Interval minutes", { exact: true }).fill("25");
      await global
        .getByRole("button", { name: "Save defaults", exact: true })
        .click();
    } else {
      const neighbor = page.getByRole("article", {
        name: "neighbor/keep-me",
        exact: true,
      });
      await neighbor
        .getByRole("button", { name: "Policy", exact: true })
        .click();
      const editor = neighbor.getByRole("form", {
        name: "Repository policy",
        exact: true,
      });
      await editor.getByLabel("Override prompt", { exact: true }).check();
      await editor
        .getByLabel("Review prompt", { exact: true })
        .fill("Saved neighbor instructions.");
      await editor
        .getByRole("button", { name: "Save policy", exact: true })
        .click();
    }
    await page.evaluate(() => window.__settingsIdle());
    const saved = (await store("snapshot")).settings;
    const primary = saved.repositories.find(
      ({ name }) => name === "octo/hello-world",
    );
    expect(primary.overrides?.prompt).toBeUndefined();
    await expect(form).toBeVisible();
    await expect(
      form.getByLabel("Override prompt", { exact: true }),
    ).toBeChecked();
    await expect(form.getByLabel("Review prompt", { exact: true })).toHaveValue(
      "Unsaved primary override.",
    );
    if (otherForm === "global") {
      expect(saved.defaults.prompt).toBe("New authoritative global prompt.");
      await expect(
        form.getByLabel("Interval minutes", { exact: true }),
      ).toHaveValue("25");
      await expect(
        form.getByLabel("Override schedule", { exact: true }),
      ).not.toBeChecked();
    } else {
      expect(
        saved.repositories.find(({ name }) => name === "neighbor/keep-me")
          .overrides.prompt,
      ).toBe("Saved neighbor instructions.");
    }
  });
}

test("a clean-focus snapshot cannot overwrite edits entered before its reply", async ({
  page,
  store,
  ipc,
}) => {
  const form = page.getByRole("form", { name: "Global defaults", exact: true });
  const original = (await store("snapshot")).settings.defaults;
  const hold = ipc.holdNext("snapshot");
  try {
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await hold.arrived;
    await form
      .getByLabel("Review prompt", { exact: true })
      .fill("Draft entered after focus snapshot began.");
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(form.getByLabel("Review prompt", { exact: true })).toHaveValue(
      "Draft entered after focus snapshot began.",
    );
    expect((await store("snapshot")).settings.defaults).toEqual(original);
  } finally {
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
  }
});

for (const target of ["global", "repository"]) {
  test(`${target} policy editor disables all its controls while its save reply is pending`, async ({
    page,
    store,
    ipc,
  }) => {
    let form = page.getByRole("form", { name: "Global defaults", exact: true });
    if (target === "repository") {
      const card = page.getByRole("article", {
        name: "octo/hello-world",
        exact: true,
      });
      await card.getByRole("button", { name: "Policy", exact: true }).click();
      form = card.getByRole("form", { name: "Repository policy", exact: true });
      await form.getByLabel("Override prompt", { exact: true }).check();
    }
    await form
      .getByLabel("Review prompt", { exact: true })
      .fill("Policy submitted before held reply.");
    const hold = ipc.holdNext(
      target === "global" ? "save_defaults" : "save_repository_policy",
    );
    try {
      await form
        .getByRole("button", {
          name: target === "global" ? "Save defaults" : "Save policy",
          exact: true,
        })
        .click();
      await hold.arrived;
      const controls = await form.locator("input,textarea,select,button").all();
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
  const card = page.getByRole("article", {
    name: "octo/hello-world",
    exact: true,
  });
  await card.getByRole("button", { name: "Policy", exact: true }).click();
  const form = card.getByRole("form", {
    name: "Repository policy",
    exact: true,
  });
  await form.getByLabel("Override prompt", { exact: true }).check();
  await form
    .getByLabel("Review prompt", { exact: true })
    .fill("Keep editable after failed save.");
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await form.getByRole("button", { name: "Save policy", exact: true }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(form.getByLabel("Review prompt", { exact: true })).toBeEnabled();
  await expect(
    form.getByLabel("Override prompt", { exact: true }),
  ).toBeEnabled();
  await expect(
    form.getByLabel("Schedule type", { exact: true }),
  ).toBeDisabled();
  await expect(
    form.getByRole("button", { name: "Save policy", exact: true }),
  ).toBeEnabled();
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
});
