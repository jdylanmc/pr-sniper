import { expect, test } from "./fixtures.mjs";
import {
  section,
  installRepositoryFixture,
  repositorySettings,
  closeDialog,
  saveChanges,
  seedAgent,
  setAgentPrompt,
  editAgent,
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
    await installRepositoryFixture(page);
    await section(page, "Repositories");
    await page
      .getByRole("button", { name: "Add repository by URL", exact: true })
      .click();
    const modal = page.getByRole("dialog", {
      name: "Add repository by URL",
      exact: true,
    });
    await modal
      .getByLabel("Repository URL", { exact: true })
      .fill("octo/fallback");
    await page
      .getByLabel("Acting GitHub account", { exact: true })
      .selectOption("22");
    await modal
      .getByRole("button", { name: "Add & configure", exact: true })
      .click();
    await page
      .getByRole("dialog", { name: "Settings for octo/fallback", exact: true })
      .getByRole("button", { name: "Cancel repository changes" })
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
  await installRepositoryFixture(page);
  await section(page, "Repositories");
  const opener = page.getByRole("button", {
    name: "Add repository by URL",
    exact: true,
  });
  await opener.click();
  const modal = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await expect(modal).toBeVisible({ timeout: 1500 });
  await modal
    .getByLabel("Repository URL", { exact: true })
    .fill("octo/project");
  const last = modal.getByRole("button", {
    name: "Add & configure",
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
    .locator('[data-section="agents"]')
    .evaluate((element) => element.focus());
  expect(
    await modal.evaluate((element) => element.contains(document.activeElement)),
  ).toBe(true);
  await page
    .locator('[data-section="agents"]')
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
  await seedAgent(store);
  await page.goto("/?view=settings");
  await installRepositoryFixture(page);
  await section(page, "Repositories");
  const before = (await store("snapshot")).settings;
  const hold = ipc.holdNext("canonical_repository_name");
  try {
    await page
      .getByRole("button", { name: "Add repository by URL", exact: true })
      .click();
    await page
      .getByLabel("Repository URL", { exact: true })
      .fill("octo/cancelled");
    await page
      .getByLabel("Acting GitHub account", { exact: true })
      .selectOption("22");
    await page
      .getByRole("button", { name: "Add & configure", exact: true })
      .click();
    await hold.arrived;
    await closeDialog(page);
    const agent = await editAgent(page);
    await agent
      .getByRole("textbox", { name: "Prompt", exact: true })
      .fill("Reset this edit.");
    await closeDialog(page);
    await section(page, "Preferences");
    await page.locator("#global-capacity").fill("7");
    await page
      .getByRole("button", { name: "Reset changes", exact: true })
      .click();
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(
      page.getByRole("button", { name: "Save preferences", exact: true }),
    ).toBeDisabled({ timeout: 1500 });
    await setAgentPrompt(page, "Keep only this edit.");
    await saveChanges(page);
    await expect(
      page.getByText("All changes saved", { exact: true }),
    ).toBeVisible();
    expect((await store("snapshot")).settings).toEqual({
      ...before,
      agents: [{ ...before.agents[0], prompt: "Keep only this edit." }],
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
  await installRepositoryFixture(page);
  await section(page, "Repositories");
  await expect(
    page.getByRole("button", {
      name: "Add repository by URL",
      exact: true,
    }),
  ).toBeVisible();
  const hold = ipc.holdNext("snapshot");
  try {
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await hold.arrived;
    await page
      .getByRole("button", { name: "Add repository by URL", exact: true })
      .click();
    const input = page.getByLabel("Repository URL", { exact: true });
    await input.fill("octo/unsubmitted");
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(input).toHaveValue("octo/unsubmitted", { timeout: 1500 });
    await expect(input).toBeFocused();
  } finally {
    hold.release();
  }
});

for (const kind of ["add", "rename"]) {
  for (const completion of ["valid", "error"]) {
    test(`R2 stale ${kind} ${completion} reply preserves a replacement editor and later persisted state`, async ({
      page,
      store,
      ipc,
    }) => {
      await store("save_repository", { repository: "octo/original" });
      await seedAgent(store);
      const before = (await store("snapshot")).settings;
      await page.goto("/?view=settings");
      await installRepositoryFixture(page);
      await section(page, "Repositories");
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
            name: "Add repository by URL",
            exact: true,
          })
          .click();
      const hold = ipc.holdNext("canonical_repository_name");
      try {
        await page
          .getByLabel("Repository URL", { exact: true })
          .fill(completion === "valid" ? "octo/dismissed" : "invalid");
        await page
          .getByLabel("Acting GitHub account", { exact: true })
          .selectOption("22");
        await page
          .getByRole("button", { name: "Add & configure", exact: true })
          .click();
        await hold.arrived;
        await closeDialog(page);
        await setAgentPrompt(page, "An independent saved edit.");
        await saveChanges(page);
        await expect(
          page.getByText("All changes saved", { exact: true }),
        ).toBeVisible();
        await section(page, "Integrations");
        await page
          .getByRole("button", {
            name: "Add repository by URL",
            exact: true,
          })
          .click();
        const replacement = page.getByRole("dialog", {
          name: "Add repository by URL",
          exact: true,
        });
        await replacement
          .getByLabel("Repository URL", { exact: true })
          .fill("octo/new-draft");
        hold.release();
        await page.evaluate(() => window.__settingsIdle());
        await expect(replacement).toBeVisible();
        await expect(
          replacement.getByLabel("Repository URL", { exact: true }),
        ).toHaveValue("octo/new-draft");
        await expect(
          replacement.getByLabel("Repository URL", { exact: true }),
        ).toBeFocused();
        await expect(replacement.getByRole("alert")).toBeHidden();
        await closeDialog(page);
        await setAgentPrompt(page, "A later independent save.");
        await saveChanges(page);
        expect((await store("snapshot")).settings).toEqual({
          ...before,
          agents: [
            { ...before.agents[0], prompt: "A later independent save." },
          ],
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
  await installRepositoryFixture(page);
  await section(page, "Repositories");
  await page.evaluate(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.repositoryRequests = 0;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "resolve_provider_repository")
        window.repositoryRequests++;
      return invoke(command, args);
    };
  });
  const hold = ipc.holdNext("canonical_repository_name");
  try {
    await page
      .getByRole("button", { name: "Add repository by URL", exact: true })
      .click();
    await page.getByLabel("Repository URL", { exact: true }).fill("octo/once");
    await page
      .getByLabel("Acting GitHub account", { exact: true })
      .selectOption("22");
    await page
      .getByRole("button", { name: "Add & configure", exact: true })
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
    await page
      .getByRole("dialog", { name: "Settings for octo/once", exact: true })
      .getByRole("button", { name: "Cancel repository changes" })
      .click();
    await saveChanges(page);
    expect((await store("snapshot")).settings.repositories).toHaveLength(1);
  } finally {
    hold.release();
  }
});

for (const editor of [
  "edit-repository",
  "new-doctrine",
  "edit-doctrine",
  "edit-agent",
]) {
  test(`R3 focus reply retains ${editor} text and focus; intentional close permits refresh`, async ({
    page,
    store,
    ipc,
  }) => {
    await store("save_repository", { repository: "octo/original" });
    await seedAgent(store);
    const before = (await store("snapshot")).settings;
    before.doctrines = [
      {
        title: "existing-doctrine",
        body: "Existing instructions.",
      },
    ];
    await store("seed_settings", before);
    await page.goto("/?view=settings");
    await installRepositoryFixture(page);
    await section(page, "Repositories");
    await section(
      page,
      editor === "edit-repository"
        ? "Integrations"
        : editor === "edit-agent"
          ? "Agents"
          : "Doctrines",
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
        label = "Repository URL";
      } else if (editor === "new-doctrine") {
        await page
          .getByRole("button", { name: "New doctrine", exact: true })
          .click();
        label = "Principles";
      } else if (editor === "edit-doctrine") {
        await page.getByRole("button", { name: "Edit", exact: true }).click();
        label = "Principles";
      } else {
        await editAgent(page);
        label = "Prompt";
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
      before.agents[0].prompt = "External update after deliberate close.";
      await store("seed_settings", before);
      await page.evaluate(async () => {
        window.dispatchEvent(new Event("focus"));
        await window.__settingsIdle();
      });
      const agent = await editAgent(page);
      await expect(
        agent.getByRole("textbox", { name: "Prompt", exact: true }),
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
  test(`R1 fallback nested repository People and doctrine editors remain modal at ${viewport.width}`, async ({
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
    await installRepositoryFixture(page);
    await section(page, "Repositories");
    await page
      .getByRole("button", { name: "octo/project", exact: true })
      .click();
    let modal = page.getByRole("dialog", {
      name: "Settings for octo/project",
      exact: true,
    });
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
    await section(page, "Doctrines");
    await page
      .getByRole("button", { name: "New doctrine", exact: true })
      .click();
    modal = page.getByRole("dialog", {
      name: "New doctrine",
      exact: true,
    });
    await modal.getByLabel("Title", { exact: true }).fill("fallback-doctrine");
    await modal
      .getByRole("textbox", { name: "Principles", exact: true })
      .fill("Review compatibility.");
    await modal
      .getByRole("button", { name: "Save doctrine", exact: true })
      .click();
    await saveChanges(page);
    expect((await store("snapshot")).settings.doctrines.at(-1).title).toBe(
      "fallback-doctrine",
    );
    const doctrine = page.locator(".doctrine-card").filter({
      has: page.getByRole("heading", {
        name: "fallback-doctrine",
        exact: true,
      }),
    });
    await doctrine.getByRole("button", { name: "Edit", exact: true }).click();
    await page.keyboard.press("Escape");
    await expect(
      doctrine.getByRole("button", { name: "Edit", exact: true }),
    ).toBeFocused();
    const footer = await page
      .getByRole("button", { name: "Save preferences", exact: true })
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
