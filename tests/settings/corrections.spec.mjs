import { expect, test } from "./fixtures.mjs";
import {
  section,
  repositorySettings,
  closeDialog,
  saveChanges,
} from "./navigation.mjs";

for (const api of ["showModal", "close"]) {
  test(`R1 absent ${api} independently selects the working fallback`, async ({
    page,
    store,
  }) => {
    await page.addInitScript((method) => {
      Object.defineProperty(HTMLDialogElement.prototype, method, {
        configurable: true,
        value: undefined,
      });
    }, api);
    await store("seed_settings", { launch_at_login: false });
    await page.goto("/?view=settings");
    await page
      .getByRole("button", { name: "Add repository manually...", exact: true })
      .click();
    const modal = page.getByRole("dialog", {
      name: "Add repository",
      exact: true,
    });
    await modal
      .getByLabel("GitHub repository", { exact: true })
      .fill("octo/fallback");
    await modal
      .getByRole("button", { name: "Use repository", exact: true })
      .click();
    await saveChanges(page);
    expect((await store("snapshot")).settings.repositories[0].name).toBe(
      "octo/fallback",
    );
    expect(await page.locator(".dialog-fallback").count()).toBe(0);
  });
}
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

test("R1 missing dialog and inert APIs retain a true keyboard modal", async ({
  page,
}, testInfo) => {
  await page.addInitScript(() => {
    Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
      configurable: true,
      value: undefined,
    });
    Object.defineProperty(HTMLDialogElement.prototype, "close", {
      configurable: true,
      value: undefined,
    });
    delete HTMLElement.prototype.inert;
  });
  await page.goto("/?view=settings");
  const opener = page.getByRole("button", {
    name: "Add repository manually...",
    exact: true,
  });
  await opener.click();
  const modal = page.getByRole("dialog", {
    name: "Add repository",
    exact: true,
  });
  await expect(modal).toBeVisible({ timeout: 1500 });
  await modal
    .getByLabel("GitHub repository", { exact: true })
    .fill("octo/project");
  const last = modal.getByRole("button", {
    name: "Use repository",
    exact: true,
  });
  const first = modal.getByRole("button", {
    name: "Close dialog",
    exact: true,
  });
  await last.focus();
  await page.keyboard.press("Tab");
  await expect(first).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(last).toBeFocused();
  expect(
    await page.locator(".settings-sidebar").getAttribute("aria-hidden"),
  ).toBe("true");
  await page
    .locator('[data-section="reviews"]')
    .evaluate((element) => element.focus());
  expect(
    await modal.evaluate((element) => element.contains(document.activeElement)),
  ).toBe(true);
  await page
    .locator('[data-section="reviews"]')
    .evaluate((element) => element.click());
  await expect(modal).toBeVisible();
  await page.screenshot({
    path: testInfo.outputPath("fallback-modal-desktop.png"),
  });
  await page.keyboard.press("Escape");
  await expect(modal).toHaveCount(0);
  await expect(opener).toBeFocused();
  await expect(page.locator(".settings-sidebar")).not.toHaveAttribute(
    "aria-hidden",
    "true",
  );
});

test("R2 dismissed repository reply cannot resurrect a reset draft", async ({
  page,
  store,
  ipc,
}) => {
  await store("seed_settings", { launch_at_login: false });
  await page.goto("/?view=settings");
  const before = (await store("snapshot")).settings;
  const hold = ipc.holdNext("canonical_repository_name");
  try {
    await page
      .getByRole("button", { name: "Add repository manually...", exact: true })
      .click();
    await page
      .getByLabel("GitHub repository", { exact: true })
      .fill("octo/cancelled");
    await page
      .getByRole("button", { name: "Use repository", exact: true })
      .click();
    await hold.arrived;
    await closeDialog(page);
    await section(page, "Review defaults");
    await page
      .getByLabel("Review prompt", { exact: true })
      .fill("Reset this edit.");
    await page
      .getByRole("button", { name: "Reset changes", exact: true })
      .click();
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(
      page.getByRole("button", { name: "Save changes", exact: true }),
    ).toBeDisabled({ timeout: 1500 });
    await page
      .getByLabel("Review prompt", { exact: true })
      .fill("Keep only this edit.");
    await page
      .getByRole("button", { name: "Save changes", exact: true })
      .click();
    await expect(
      page.getByText("All changes saved", { exact: true }),
    ).toBeVisible();
    expect((await store("snapshot")).settings).toEqual({
      ...before,
      defaults: { ...before.defaults, prompt: "Keep only this edit." },
    });
  } finally {
    hold.release();
  }
});

test("R3 a held focus reply preserves newly opened repository text and focus", async ({
  page,
  store,
  ipc,
}) => {
  await store("seed_settings", { launch_at_login: false });
  await page.goto("/?view=settings");
  await expect(
    page.getByRole("button", {
      name: "Add repository manually...",
      exact: true,
    }),
  ).toBeVisible();
  const hold = ipc.holdNext("snapshot");
  try {
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await hold.arrived;
    await page
      .getByRole("button", { name: "Add repository manually...", exact: true })
      .click();
    const input = page.getByLabel("GitHub repository", { exact: true });
    await input.fill("octo/unsubmitted");
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(input).toHaveValue("octo/unsubmitted", { timeout: 1500 });
    await expect(input).toBeFocused();
  } finally {
    hold.release();
  }
});

test("R4 canonical clones share one selection with every path searchable", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("save_repository", { repository: "octo/project" });
  const original = (await store("snapshot")).settings.repositories[0];
  const root = join(dataRoot, "clones");
  for (const name of ["first-clone", "second-clone"]) {
    await mkdir(join(root, name, ".git"), { recursive: true });
    await writeFile(
      join(root, name, ".git/config"),
      '[remote "origin"]\nurl = git@github.com:Octo/Project.git\n',
    );
  }
  await mkdir(join(root, "local-only/.git"), { recursive: true });
  await writeFile(
    join(root, "local-only/.git/config"),
    "[core]\nrepositoryformatversion = 0\n",
  );
  await page.exposeFunction("__chooseClones", () =>
    store("discover_repositories", { root }),
  );
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "choose_repository_folder"
        ? window.__chooseClones()
        : invoke(command, args);
  });
  await page.goto("/?view=settings");
  await page
    .getByRole("button", { name: "Choose folder...", exact: true })
    .click();
  await expect(
    page.getByText("3 local repositories discovered", { exact: true }),
  ).toBeVisible();
  const selection = page.getByRole("checkbox", {
    name: "Monitor octo/project",
    exact: true,
  });
  await expect(selection).toHaveCount(1, { timeout: 1500 });
  await selection.uncheck();
  await expect(page.locator("#selected-count")).toHaveText("0 selected");
  await page
    .getByLabel("Find a repository", { exact: true })
    .fill("second-clone");
  await expect(selection).not.toBeChecked();
  await selection.check();
  await page
    .getByLabel("Find a repository", { exact: true })
    .fill("first-clone");
  await expect(selection).toBeChecked();
  await page.getByLabel("Find a repository", { exact: true }).fill("");
  await expect(
    page.getByRole("checkbox", { name: "Monitor local-only", exact: true }),
  ).toBeDisabled();
  await saveChanges(page);
  expect((await store("snapshot")).settings.repositories).toEqual([original]);
  await page.reload();
  await expect(selection).toBeChecked();
  await page
    .getByRole("button", { name: "Scan chosen folder", exact: true })
    .click();
  await expect(
    page.getByText("3 local repositories discovered", { exact: true }),
  ).toBeVisible();
  await expect(selection).toHaveCount(1);
  const clones = page.getByRole("article", {
    name: "octo/project",
    exact: true,
  });
  await clones.getByText("2 local clones", { exact: true }).click();
  await expect(clones.locator("li")).toHaveCount(2);
  await expect(clones.locator("li").nth(0)).toContainText("first-clone");
  await expect(clones.locator("li").nth(1)).toContainText("second-clone");
  expect((await store("snapshot")).settings.repositories).toEqual([original]);
});

for (const kind of ["add", "rename"]) {
  for (const completion of ["valid", "error"]) {
    test(`R2 stale ${kind} ${completion} reply preserves a replacement editor and later persisted state`, async ({
      page,
      store,
      ipc,
    }) => {
      await store("save_repository", { repository: "octo/original" });
      const before = (await store("snapshot")).settings;
      await page.goto("/?view=settings");
      if (kind === "rename") {
        const repository = await repositorySettings(page, "octo/original");
        await repository
          .getByText("Repository and connection", { exact: true })
          .click();
        await repository
          .getByRole("button", { name: "Edit repository", exact: true })
          .click();
      } else
        await page
          .getByRole("button", {
            name: "Add repository manually...",
            exact: true,
          })
          .click();
      const hold = ipc.holdNext("canonical_repository_name");
      try {
        await page
          .getByLabel("GitHub repository", { exact: true })
          .fill(completion === "valid" ? "octo/dismissed" : "invalid");
        await page
          .getByRole("button", { name: "Use repository", exact: true })
          .click();
        await hold.arrived;
        await closeDialog(page);
        await section(page, "Review defaults");
        await page
          .getByLabel("Review prompt", { exact: true })
          .fill("An independent saved edit.");
        await page
          .getByRole("button", { name: "Save changes", exact: true })
          .click();
        await expect(
          page.getByText("All changes saved", { exact: true }),
        ).toBeVisible();
        await section(page, "Repositories");
        await page
          .getByRole("button", {
            name: "Add repository manually...",
            exact: true,
          })
          .click();
        const replacement = page.getByRole("dialog", {
          name: "Add repository",
          exact: true,
        });
        await replacement
          .getByLabel("GitHub repository", { exact: true })
          .fill("octo/new-draft");
        hold.release();
        await page.evaluate(() => window.__settingsIdle());
        await expect(replacement).toBeVisible();
        await expect(
          replacement.getByLabel("GitHub repository", { exact: true }),
        ).toHaveValue("octo/new-draft");
        await expect(
          replacement.getByLabel("GitHub repository", { exact: true }),
        ).toBeFocused();
        await expect(replacement.getByRole("alert")).toBeHidden();
        await closeDialog(page);
        await section(page, "Review defaults");
        await page
          .getByLabel("Review prompt", { exact: true })
          .fill("A later independent save.");
        await saveChanges(page);
        expect((await store("snapshot")).settings).toEqual({
          ...before,
          defaults: { ...before.defaults, prompt: "A later independent save." },
        });
      } finally {
        hold.release();
      }
    });
  }
}

test("R2 repeated submit dispatch cannot start a second repository request", async ({
  page,
  store,
  ipc,
}) => {
  await store("seed_settings", { launch_at_login: false });
  await page.goto("/?view=settings");
  await page.evaluate(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.repositoryRequests = 0;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "canonical_repository_name") window.repositoryRequests++;
      return invoke(command, args);
    };
  });
  const hold = ipc.holdNext("canonical_repository_name");
  try {
    await page
      .getByRole("button", { name: "Add repository manually...", exact: true })
      .click();
    await page
      .getByLabel("GitHub repository", { exact: true })
      .fill("octo/once");
    await page
      .getByRole("button", { name: "Use repository", exact: true })
      .click();
    await hold.arrived;
    await page
      .getByRole("dialog")
      .locator("form")
      .evaluate((form) =>
        form.dispatchEvent(
          new Event("submit", { bubbles: true, cancelable: true }),
        ),
      );
    expect(await page.evaluate(() => window.repositoryRequests)).toBe(1);
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await saveChanges(page);
    expect((await store("snapshot")).settings.repositories).toHaveLength(1);
  } finally {
    hold.release();
  }
});

for (const editor of [
  "edit-repository",
  "new-preset",
  "edit-preset",
  "import-preset",
]) {
  test(`R3 focus reply retains ${editor} text and focus; intentional close permits refresh`, async ({
    page,
    store,
    ipc,
  }) => {
    await store("save_repository", { repository: "octo/original" });
    const before = (await store("snapshot")).settings;
    before.presets = [
      {
        id: "a17a5695-0e09-489b-bcb2-1da94dc3b8aa",
        name: "Existing preset",
        body: "Existing instructions.",
      },
    ];
    await store("seed_settings", before);
    await page.goto("/?view=settings");
    await section(
      page,
      editor === "edit-repository" ? "Repositories" : "Review presets",
    );
    const hold = ipc.holdNext("snapshot");
    try {
      await page.evaluate(() => window.dispatchEvent(new Event("focus")));
      await hold.arrived;
      let label;
      if (editor === "edit-repository") {
        const repository = await repositorySettings(page, "octo/original");
        await repository
          .getByText("Repository and connection", { exact: true })
          .click();
        await repository
          .getByRole("button", { name: "Edit repository", exact: true })
          .click();
        label = "GitHub repository";
      } else if (editor === "new-preset") {
        await page
          .getByRole("button", { name: "New preset", exact: true })
          .click();
        label = "Review instructions";
      } else if (editor === "edit-preset") {
        await page.getByRole("button", { name: "Edit", exact: true }).click();
        label = "Review instructions";
      } else {
        await page
          .getByRole("button", { name: "Import...", exact: true })
          .click();
        label = "Preset JSON";
      }
      const input = page
        .getByRole("dialog")
        .getByRole("textbox", { name: label, exact: true });
      const expected =
        editor === "edit-repository"
          ? "octo/unsaved-repository"
          : "Exact unsaved text\nstill editing";
      await input.fill(expected);
      hold.release();
      await page.evaluate(() => window.__settingsIdle());
      await expect(input).toHaveValue(expected);
      await expect(input).toBeFocused();
      await closeDialog(page);
      await store("save_defaults", {
        policy: {
          ...before.defaults,
          prompt: "External update after deliberate close.",
        },
      });
      await page.evaluate(async () => {
        window.dispatchEvent(new Event("focus"));
        await window.__settingsIdle();
      });
      await section(page, "Review defaults");
      await expect(
        page.getByLabel("Review prompt", { exact: true }),
      ).toHaveValue("External update after deliberate close.");
    } finally {
      hold.release();
    }
  });
}

for (const viewport of [
  { width: 1180, height: 800 },
  { width: 390, height: 844 },
]) {
  test(`R1 fallback nested policy, People and presets remain modal at ${viewport.width}`, async ({
    page,
    store,
  }, testInfo) => {
    await page.setViewportSize(viewport);
    await page.addInitScript(() => {
      Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
        configurable: true,
        value: undefined,
      });
      Object.defineProperty(HTMLDialogElement.prototype, "close", {
        configurable: true,
        value: undefined,
      });
      delete HTMLElement.prototype.inert;
    });
    await store("save_repository", { repository: "octo/project" });
    await page.goto("/?view=settings");
    await page
      .getByRole("article", { name: "octo/project", exact: true })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    let modal = page.getByRole("dialog", {
      name: "Settings for octo/project",
      exact: true,
    });
    await modal
      .getByRole("checkbox", { name: "Override people", exact: true })
      .check();
    await modal
      .getByRole("button", { name: "Add people", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Add people",
      exact: true,
    });
    await expect(
      picker.getByLabel("GitHub login", { exact: true }),
    ).toBeFocused();
    await expect(modal).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(modal).toBeVisible();
    await expect(
      modal.getByRole("button", { name: "Add people", exact: true }),
    ).toBeFocused();
    await modal.getByText("Repository and connection", { exact: true }).click();
    await expect(
      modal.getByRole("button", { name: "Edit repository", exact: true }),
    ).toBeVisible();
    const bounds = await modal.boundingBox();
    expect(bounds.y).toBeGreaterThanOrEqual(0);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(viewport.height);
    await page.screenshot({
      path: testInfo.outputPath(`fallback-policy-${viewport.width}.png`),
    });
    await closeDialog(page);
    if (viewport.width < 600)
      await page
        .getByLabel("Settings section", { exact: true })
        .selectOption("presets");
    else await section(page, "Review presets");
    await page.getByRole("button", { name: "New preset", exact: true }).click();
    modal = page.getByRole("dialog", {
      name: "New review preset",
      exact: true,
    });
    await modal
      .getByLabel("Preset name", { exact: true })
      .fill("Fallback preset");
    await modal
      .getByLabel("Review instructions", { exact: true })
      .fill("Review compatibility.");
    await modal
      .getByRole("button", { name: "Save preset", exact: true })
      .click();
    await saveChanges(page);
    expect((await store("snapshot")).settings.presets[0].name).toBe(
      "Fallback preset",
    );
    await page.getByRole("button", { name: "Edit", exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(
      page.getByRole("button", { name: "Edit", exact: true }),
    ).toBeFocused();
    const footer = await page
      .getByRole("button", { name: "Save changes", exact: true })
      .boundingBox();
    expect(footer.y + footer.height).toBeLessThanOrEqual(viewport.height);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: testInfo.outputPath(`fallback-footer-${viewport.width}.png`),
    });
  });
}
