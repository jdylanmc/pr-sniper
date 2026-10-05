import { test, expect } from "./fixtures.mjs";
import { join } from "node:path";
import {
  providerFixture,
  repositoryPage,
  addByUrl,
  reviewer,
} from "./repository-provider-fixture.mjs";
import { section } from "./navigation.mjs";

for (const embedded of [true, false]) {
  for (const size of [
    { width: 408, height: 744 },
    { width: 320, height: 300 },
  ]) {
    test(`repository Save authorizes without a scope step (${embedded ? "panel" : "standalone"} ${size.width})`, async ({
      page,
      store,
      dataRoot,
    }, info) => {
      await page.setViewportSize(size);
      const settings = (await store("snapshot")).settings;
      settings.agents = [reviewer];
      settings.root_folder = join(dataRoot, "legacy-unused-folder");
      await store("seed_settings", settings);
      const fixture = await providerFixture(page, store);
      await repositoryPage(page, store, embedded);
      await expect(page.locator(".repository-list")).toHaveCSS(
        "border-radius",
        "12px",
      );
      await expect(page.locator(".settings-savebar")).toBeHidden();
      await expect(
        page.getByText(/Choose folder|Select visible|Scan chosen folder/),
      ).toHaveCount(0);
      await page.screenshot({ path: info.outputPath("repositories.png") });
      let editor = await addByUrl(page);
      await expect(editor).toBeVisible();
      let saved = (await store("snapshot")).settings;
      expect(saved.repositories[0]).toMatchObject({
        enabled: false,
        provider_account_id: "22",
        provider_repository_id: "100",
      });
      expect(saved.repository_authorizations).toBeUndefined();
      await editor
        .getByRole("button", { name: "Cancel repository changes" })
        .click();
      await page.locator("[data-repository]").click();
      editor = page.getByRole("dialog", {
        name: "Settings for fixture/one",
        exact: true,
      });
      await expect(
        editor.getByRole("button", { name: /Configure scope|Confirm scope/ }),
      ).toHaveCount(0);
      await editor
        .getByRole("button", { name: "Assign agent", exact: true })
        .click();
      const assignment = page.getByRole("dialog", {
        name: "Assign agent",
        exact: true,
      });
      await assignment
        .getByLabel("Agent", { exact: true })
        .selectOption(reviewer.id);
      await assignment
        .getByRole("button", { name: "Assign agent", exact: true })
        .click();
      saved = (await store("snapshot")).settings;
      expect(saved.repositories[0].enabled).toBe(false);
      expect(Object.values(saved.repository_authorizations ?? {})).toEqual([
        null,
      ]);
      await page.screenshot({ path: info.outputPath("repository-editor.png") });
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(editor).toHaveCount(0);
      saved = (await store("snapshot")).settings;
      expect(saved.repositories[0].enabled).toBe(true);
      expect(Object.keys(saved.repository_authorizations)).toEqual([
        saved.repositories[0].id,
      ]);
      expect(saved.root_folder).toBe(settings.root_folder);
      const status = await store("monitoring_activation_status", {
        repositoryId: saved.repositories[0].id,
      });
      expect(status).toMatchObject({
        active: true,
        mode: "all_open_and_future",
      });
      expect(
        fixture.calls.some((c) =>
          /activation|preview|pull_requests/.test(c.command),
        ),
      ).toBe(false);
      await page.reload();
      await section(page, "Repositories");
      await expect(page.locator("[data-repository]")).toContainText("Enabled");
      await page.screenshot({ path: info.outputPath("repository-saved.png") });
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
    });
  }
}

test("owner browsing isolates results, retries errors, searches and reopens stable bindings", async ({
  page,
  store,
}, info) => {
  const fixture = await providerFixture(page, store);
  await repositoryPage(page, store);
  const browse = async () => {
    await page
      .getByRole("button", {
        name: "Browse repositories as fixture",
        exact: true,
      })
      .click();
    return page.getByRole("dialog", {
      name: "Browse repositories as fixture",
      exact: true,
    });
  };
  let browser = await browse();
  await expect(browser.getByLabel("Repository owner")).toHaveValue("");
  await browser.getByLabel("Repository owner").selectOption("orbit");
  await expect(browser.locator("[data-pick]")).toHaveCount(2);
  await expect(browser.locator("[data-owner-results]")).toContainText(
    "orbit/one",
  );
  await page.screenshot({ path: info.outputPath("organization-browser.png") });
  await browser.getByLabel("Find a repository").fill("absent");
  await expect(browser.getByRole("status")).toContainText("No matching");
  await browser.getByLabel("Find a repository").fill("one");
  await browser.locator("[data-pick]").click();
  const editor = page.getByRole("dialog", {
    name: "Settings for orbit/one",
    exact: true,
  });
  await expect(editor).toBeVisible();
  await editor
    .getByRole("button", { name: "Cancel repository changes" })
    .click();
  const before = (await store("snapshot")).settings;
  browser = await browse();
  fixture.handler = (command) => {
    if (command === "list_provider_repositories")
      throw new Error("Synthetic access denied");
  };
  await browser.getByLabel("Repository owner").selectOption("orbit");
  await expect(browser.getByRole("alert")).toContainText(
    "Synthetic access denied",
  );
  await expect(browser.locator("[data-pick]")).toHaveCount(0);
  fixture.handler = undefined;
  await browser.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(browser.locator("[data-pick]").first()).toContainText("Added");
  await browser.locator("[data-pick]").first().click();
  await expect(editor).toBeVisible();
  expect((await store("snapshot")).settings).toEqual(before);
});

test("URL requires an acting account, rejects malformed input and preserves alternate-account bindings", async ({
  page,
  store,
}) => {
  await providerFixture(page, store);
  await repositoryPage(page, store);
  await page.getByRole("button", { name: "Add repository by URL" }).click();
  const form = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await expect(form.getByLabel("Acting GitHub account")).toHaveValue("");
  await expect(
    form.getByLabel("Acting GitHub account").locator("option"),
  ).toHaveCount(3);
  await form
    .getByLabel("Repository URL")
    .fill("https://dev.azure.com/org/project/repo");
  await form.getByLabel("Acting GitHub account").selectOption("22");
  await form.getByRole("button", { name: "Add & configure" }).click();
  await expect(form.getByRole("alert")).toBeVisible();
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
  await form.getByRole("button", { name: "Cancel", exact: true }).click();
  for (const accountId of ["22", "44", "22"]) {
    const editor = await addByUrl(page, "fixture/one", accountId);
    await expect(editor).toBeVisible();
    await editor
      .getByRole("button", { name: "Cancel repository changes" })
      .click();
  }
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories).toHaveLength(2);
  expect(saved.repositories.map((r) => r.provider_account_id)).toEqual([
    "22",
    "44",
  ]);
  expect(saved.repositories.every((r) => !r.enabled)).toBe(true);
});

test("late owner and account results cannot add a stale repository", async ({
  page,
  store,
}) => {
  const held = Promise.withResolvers();
  const arrived = Promise.withResolvers();
  const fixture = await providerFixture(page, store, async (command, args) => {
    if (command === "list_provider_repositories" && args.owner === "fixture") {
      arrived.resolve();
      await held.promise;
      return {
        identity: { id: "22" },
        repositories: [{ id: "300", name: "fixture/stale" }],
      };
    }
  });

  await repositoryPage(page, store);
  await page
    .getByRole("button", {
      name: "Browse repositories as fixture",
      exact: true,
    })
    .click();
  const browser = page.getByRole("dialog", {
    name: "Browse repositories as fixture",
    exact: true,
  });
  try {
    await browser.getByLabel("Repository owner").selectOption("fixture");
    await arrived.promise;
    await browser.getByLabel("Repository owner").selectOption("orbit");
    await expect(browser.locator("[data-pick]")).toHaveCount(2);
    held.resolve();
    await expect(browser.locator("[data-owner-results]")).not.toContainText(
      "stale",
    );
    fixture.accounts = [];
    await page.evaluate(() =>
      window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
    );
    await expect(page.locator("[data-repository-accounts]")).toContainText(
      "Connect a GitHub",
    );
    await browser.locator("[data-pick]").first().click();
    await expect(browser.getByRole("alert")).toContainText("disconnected");
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
  } finally {
    held.resolve();
  }
});

test("late URL resolution cannot add or navigate after leaving Settings", async ({
  page,
  store,
}) => {
  const held = Promise.withResolvers();
  const entered = Promise.withResolvers();
  const fixture = await providerFixture(page, store, async (command, args) => {
    if (command === "resolve_provider_repository") {
      entered.resolve();
      await held.promise;
      return {
        identity: { id: args.accountId },
        repository: { id: "100", name: "fixture/late" },
        account_generation: 0,
      };
    }
  });
  await repositoryPage(page, store);
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await dialog.getByLabel("Repository URL").fill("fixture/late");
  await dialog.getByLabel("Acting GitHub account").selectOption("22");
  try {
    await dialog.getByRole("button", { name: "Add & configure" }).click();
    await entered.promise;
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Running", exact: true })
      .click();
    await expect(page.locator("[data-panel-heading]")).toHaveText("Work queue");
    held.resolve();
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    await expect(
      dialog.getByRole("button", { name: "Add & configure" }),
    ).toBeEnabled();
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
    expect(
      fixture.calls.filter((c) => c.command === "resolve_provider_repository"),
    ).toHaveLength(1);
  } finally {
    held.resolve();
  }
});

test("repository account read failure is explicit and retry does not imply an empty successful catalog", async ({
  page,
  store,
}) => {
  let fail = true;
  await providerFixture(page, store, (command) => {
    if (command === "github_auth_state" && fail)
      throw new Error("Synthetic account read failure");
  });
  await store("fixture_show_panel");
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await section(page, "Repositories");
  await expect(page.locator("[data-repository-accounts]")).toContainText(
    "GitHub accounts are unavailable",
  );
  await expect(page.locator("[data-browse-account]")).toHaveCount(0);
  fail = false;
  await page
    .getByRole("button", { name: "Retry reading accounts", exact: true })
    .click();
  await expect(
    page.getByRole("button", {
      name: "Browse repositories as fixture",
      exact: true,
    }),
  ).toBeEnabled();
});
