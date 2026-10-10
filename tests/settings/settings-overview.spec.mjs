import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const back = (page) =>
  page.getByRole("button", { name: "Back to Settings", exact: true });
const row = (page, name) =>
  page.locator(".settings-overview").getByRole("button", { name, exact: true });

test.use({ viewport: { width: 408, height: 744 }, deviceScaleFactor: 2 });

for (const viewport of [
  { width: 408, height: 744 },
  { width: 320, height: 300 },
]) {
  test(`account categories and provider drill-down preserve Back context at ${viewport.width}x${viewport.height}`, async ({
    page,
    store,
  }, testInfo) => {
    await queueFixture(store);
    const saved = (await store("snapshot")).settings;
    await page.setViewportSize(viewport);
    await page.addInitScript(() => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.__accountMutations = [];
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (
          /^(start_|confirm_|disconnect_|cancel_).*(auth|copilot|github)/.test(
            command,
          )
        )
          window.__accountMutations.push(command);
        return invoke(command, args);
      };
    });
    await page.goto("/");
    await tab(page, "Settings").click();
    await row(page, "Accounts").click();
    await expect(row(page, "AI Tooling")).toBeVisible();
    await expect(row(page, "Git Repository")).toBeVisible();
    await expect(page.locator(".settings-savebar")).toBeHidden();
    await expect(page.locator(".account-connection")).toHaveCount(0);
    await page.screenshot({
      path: testInfo.outputPath("accounts-categories.png"),
    });
    for (const [category, provider, unavailable, card, other] of [
      [
        "AI Tooling",
        "GitHub Copilot",
        ["Claude", "Codex", "Grok"],
        ".copilot-auth-card",
        ".github-auth-card",
      ],
      [
        "Git Repository",
        "GitHub",
        ["Azure DevOps", "Bitbucket"],
        ".github-auth-card",
        ".copilot-auth-card",
      ],
    ]) {
      await row(page, category).focus();
      await page.keyboard.press("Enter");
      await expect(page.locator(".settings-heading h1")).toHaveText(category);
      for (const label of unavailable) {
        const planned = page
          .locator(".settings-provider-planned")
          .filter({ hasText: label });
        await expect(planned).toContainText("Coming soon");
        await expect(planned).toHaveAttribute("aria-disabled", "true");
        await expect(planned.locator("button,a,input")).toHaveCount(0);
      }
      await expect(page.locator(".settings-savebar")).toBeHidden();
      await expect(page.locator(card)).toBeHidden();
      await row(page, provider).scrollIntoViewIfNeeded();
      const scroll = await page
        .locator("#content")
        .evaluate((element) => element.scrollTop);
      await row(page, provider).focus();
      await page.screenshot({
        path: testInfo.outputPath(
          `${category.replaceAll(" ", "-")}-providers.png`,
        ),
      });
      await page.keyboard.press("Enter");
      await expect(page.locator(card)).toBeVisible();
      await expect(page.locator(other)).toBeHidden();
      await tab(page, "Queue").click();
      await tab(page, "Settings").click();
      await expect(page.locator(card)).toBeVisible();
      await page
        .getByRole("button", { name: `Back to ${category}`, exact: true })
        .click();
      await expect(row(page, provider)).toBeFocused();
      expect(
        await page.locator("#content").evaluate((element) => element.scrollTop),
      ).toBe(scroll);
      await page
        .getByRole("button", { name: "Back to Accounts", exact: true })
        .click();
      await expect(row(page, category)).toBeFocused();
      expect(
        await page
          .locator("#content")
          .evaluate((element) => element.scrollWidth <= element.clientWidth),
      ).toBe(true);
    }
    await back(page).click();
    await expect(row(page, "Accounts")).toBeFocused();
    expect(await page.evaluate(() => window.__accountMutations)).toEqual([]);
    expect((await store("snapshot")).settings).toEqual(saved);
  });
}

test("provider summaries isolate failed reads and retain an available retry route", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__githubReadFailure = true;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "github_auth_state" && window.__githubReadFailure)
        return Promise.reject("GitHub fixture unavailable");
      if (command === "copilot_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "copilot",
              account_id: "1",
              login: "fixture",
              state: "connected",
            },
            {
              provider: "copilot",
              account_id: "2",
              login: "expired",
              state: "reconnect_required",
              reason: "expired",
            },
          ],
          flow: { state: "idle" },
        });
      return invoke(command, args);
    };
  });
  await page.goto("/");
  await tab(page, "Settings").click();
  await row(page, "Accounts").click();
  await expect(page.locator('[data-settings-count="ai-tooling"]')).toHaveText(
    "1 connected / 1 need attention",
  );
  await expect(
    page.locator('[data-settings-count="git-repository"]'),
  ).toHaveText("Unavailable");
  await row(page, "AI Tooling").click();
  await expect(page.locator('[data-settings-count="copilot"]')).toHaveText(
    "1 connected / 1 need attention",
  );
  await section(page, "Git Repository");
  await expect(page.locator('[data-settings-count="github"]')).toHaveText(
    "Unavailable",
  );
  await row(page, "GitHub").click();
  await expect(page.locator(".github-auth-card")).toContainText(
    "GitHub connection state is unavailable. No connection is assumed.",
  );
  await page.evaluate(() => {
    window.__githubReadFailure = false;
  });
  await page
    .getByRole("button", { name: "Retry reading GitHub accounts", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Add GitHub account", exact: true }),
  ).toBeVisible();
});

test("grouped overview uses saved counts and all six destinations return focus and scroll", async ({
  page,
  store,
}, testInfo) => {
  await queueFixture(store);
  const saved = (await store("snapshot")).settings;
  await page.goto("/");
  await tab(page, "Settings").click();
  await expect(
    page.getByText("Your review setup", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Shared agents. Your rules.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByLabel("Settings section", { exact: true }),
  ).toHaveCount(0);
  await expect(page.locator(".settings-savebar")).toBeHidden();
  for (const [key, value] of Object.entries({
    accounts: "0 connected",
    agents: String(saved.agents.length),
    repositories: String(saved.repositories.length),
    doctrines: String(saved.doctrines.length),
    capacity: String(saved.capacity),
  })) {
    await expect(page.locator(`[data-settings-count="${key}"]`)).toHaveText(
      value,
    );
  }
  await page.screenshot({
    path: testInfo.outputPath("settings-production-408x744.png"),
  });
  for (const [name, control] of [
    ["Accounts", page.getByRole("button", { name: "AI Tooling", exact: true })],
    ["Agents", page.getByRole("button", { name: "New agent", exact: true })],
    [
      "Repositories",
      page.getByRole("button", {
        name: "Add repository by URL",
        exact: true,
      }),
    ],
    [
      "Doctrines",
      page.getByRole("button", { name: "New doctrine", exact: true }),
    ],
    ["Concurrent reviews", page.getByLabel("AI capacity", { exact: true })],
    ["Preferences", page.getByLabel("Cron expression", { exact: true })],
  ]) {
    await row(page, name).scrollIntoViewIfNeeded();
    const scroll = await page
      .locator("#content")
      .evaluate((element) => element.scrollTop);
    await row(page, name).focus();
    await page.keyboard.press("Enter");
    await expect(control).toBeVisible();
    if (name === "Accounts")
      await expect(page.locator(".repository-library")).toHaveCount(0);
    if (name === "Repositories")
      await expect(page.locator(".github-auth")).toBeHidden();
    await back(page).click();
    await expect(row(page, name)).toBeFocused();
    expect(
      await page.locator("#content").evaluate((element) => element.scrollTop),
    ).toBe(scroll);
  }
  expect((await store("snapshot")).settings).toEqual(saved);
  await page
    .getByRole("button", { name: "Set up with Genie", exact: true })
    .click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Genie");
  await page.locator('[data-genie-edit="agents"]').click();
  await expect(
    page.getByRole("button", { name: "New agent", exact: true }),
  ).toBeVisible();
  await expect(back(page)).toBeHidden();
  await page
    .getByRole("button", { name: "Back to Genie", exact: true })
    .click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Genie");
  await tab(page, "Running").click();
  await tab(page, "Settings").click();
  await back(page).click();
  await expect(row(page, "Agents")).toBeFocused();
});

test("overview retains preference and repository drafts without claiming them in saved summaries", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const saved = (await store("snapshot")).settings;
  await page.goto("/");
  await tab(page, "Settings").click();
  await section(page, "Preferences");
  await page.getByLabel("AI capacity", { exact: true }).fill("7");
  await page
    .getByLabel("Cron expression", { exact: true })
    .fill("*/23 * * * *");
  await back(page).click();
  await expect(page.locator('[data-settings-count="capacity"]')).toHaveText(
    String(saved.capacity),
  );
  await section(page, "Repositories");
  await page
    .locator("[data-repository]")
    .filter({ hasText: "example/repo" })
    .click();
  await page
    .getByLabel("Reviewer requests", { exact: true })
    .selectOption("off");
  await page.getByRole("switch", { name: "Monitor example/repo" }).click();
  await expect(page.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await back(page).click();
  await section(page, "Preferences");
  await expect(page.getByLabel("AI capacity", { exact: true })).toHaveValue(
    "7",
  );
  await expect(page.getByLabel("Cron expression", { exact: true })).toHaveValue(
    "*/23 * * * *",
  );
  await page
    .getByRole("button", { name: "Save preferences", exact: true })
    .click();
  await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
  await back(page).click();
  await expect(page.locator('[data-settings-count="capacity"]')).toHaveText(
    "7",
  );
  await section(page, "Repositories");
  await page
    .locator("[data-repository]")
    .filter({ hasText: "example/repo" })
    .click();
  await expect(
    page.getByRole("switch", { name: "Monitor example/repo" }),
  ).not.toBeChecked();
  expect((await store("snapshot")).settings.repositories).toEqual(
    saved.repositories.map((repository) => ({ ...repository, enabled: false })),
  );
});

test("saved resource changes and background refresh keep overview counts and keyboard context", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const count = (await store("snapshot")).settings.doctrines.length;
  await page.goto("/");
  await tab(page, "Settings").click();
  await section(page, "Doctrines");
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const editor = page.getByRole("dialog", {
    name: "New doctrine",
    exact: true,
  });
  await editor.getByLabel("Title", { exact: true }).fill("Overview regression");
  await editor
    .getByLabel("Principles", { exact: true })
    .fill("Keep account and resource boundaries explicit.");
  await tab(page, "Running").click();
  await tab(page, "Settings").click();
  await expect(editor.getByLabel("Title", { exact: true })).toHaveValue(
    "Overview regression",
  );
  await editor
    .getByRole("button", { name: "Save doctrine", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
  await back(page).click();
  await expect(page.locator('[data-settings-count="doctrines"]')).toHaveText(
    String(count + 1),
  );
  await row(page, "Preferences").scrollIntoViewIfNeeded();
  await row(page, "Preferences").focus();
  const scroll = await page
    .locator("#content")
    .evaluate((element) => element.scrollTop);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await page.evaluate(() => window.__settingsIdle());
  await expect(row(page, "Preferences")).toBeFocused();
  expect(
    await page.locator("#content").evaluate((element) => element.scrollTop),
  ).toBe(scroll);
  expect((await store("snapshot")).settings.doctrines).toHaveLength(count + 1);
});

test("unavailable account summary is explicit and Accounts remains retryable", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    let failed = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "github_auth_state" && !failed) {
        failed = true;
        return Promise.reject("Synthetic account read failed");
      }
      return invoke(command, args);
    };
  });
  await page.goto("/");
  await tab(page, "Settings").click();
  await expect(page.locator('[data-settings-count="accounts"]')).toHaveText(
    "Unavailable",
  );
  await expect(page.locator("#error")).toContainText(
    "Synthetic account read failed",
  );
  await section(page, "GitHub");
  await expect(
    page.getByRole("button", { name: "Add GitHub account", exact: true }),
  ).toBeVisible();
});

test("Accounts retains the mounted sign-in flow and reports actual connected and blocked identities", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    let connecting = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "start_github_browser_auth") connecting = true;
      if (["github_auth_state", "start_github_browser_auth"].includes(command))
        return Promise.resolve({
          accounts: [
            {
              provider: "github",
              account_id: "22",
              login: "fixture",
              state: "connected",
            },
            {
              provider: "github",
              account_id: "23",
              login: "expired",
              state: "reconnect_required",
              reason: "expired",
            },
          ],
          flow: connecting
            ? {
                state: "connecting",
                user_code: "TEST-CODE",
                verification_uri: "https://github.com/login/device",
              }
            : { state: "idle" },
        });
      return invoke(command, args);
    };
  });
  await page.goto("/");
  await tab(page, "Settings").click();
  await expect(page.locator('[data-settings-count="accounts"]')).toHaveText(
    "1 connected / 1 need attention",
  );
  await section(page, "GitHub");
  await page.locator(".github-auth-card").evaluate((element) => {
    window.__accountCard = element;
  });
  await page
    .getByRole("button", { name: "Add GitHub account", exact: true })
    .click();
  await expect(page.locator(".github-auth-card")).toContainText("TEST-CODE");
  await section(page, "GitHub Copilot");
  await expect(page.locator(".github-auth-card")).toBeHidden();
  await expect(page.locator(".copilot-auth-card")).toBeVisible();
  await section(page, "Agents");
  await back(page).click();
  await section(page, "GitHub");
  await expect(page.locator(".github-auth-card")).toContainText("TEST-CODE");
  expect(
    await page
      .locator(".github-auth-card")
      .evaluate((element) => element === window.__accountCard),
  ).toBe(true);
});

for (const metadata of ["native", "missing", "rejected"]) {
  test(`footer ${metadata} version remains inline across every tab with working utility controls`, async ({
    page,
    store,
    ipc,
  }) => {
    await queueFixture(store);
    const nativeVersion = (await store("snapshot")).version;
    if (metadata !== "native")
      await page.addInitScript((failure) => {
        const invoke = window.__TAURI_INTERNALS__.invoke;
        let first = true;
        window.__TAURI_INTERNALS__.invoke = async (command, args) => {
          if (command === "snapshot" && first) {
            first = false;
            if (failure === "rejected")
              throw "Synthetic native metadata failure";
            return { ...(await invoke(command, args)), version: null };
          }
          return invoke(command, args);
        };
      }, metadata);
    await page.goto("/");
    const version = page.locator("[data-panel-version]");
    let navigation;
    for (const name of ["Queue", "Running", "Reviewed", "Settings"]) {
      if (name === "Settings") navigation = ipc.holdNext("panel_navigate");
      await tab(page, name).click();
      await expect(version).toHaveText(
        metadata === "native" ? `v${nativeVersion}` : "Version unavailable",
      );
      await expect(version).toBeVisible();
      await expect(page.locator(".panel-hide-hint")).toHaveText(
        "Close hides only. Quit from the tray menu.",
      );
    }
    navigation.release();
    await expect(page.locator(".settings-overview")).toBeVisible();
    await page.evaluate(() => window.__settingsIdle());
    await page.locator("[data-panel-status]").focus();
    await expect(page.locator("[data-panel-status]")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-panel-heading]")).toHaveText("Status");
    await expect(version).toHaveText(`v${nativeVersion}`);
    await page.locator("[data-panel-diagnostics]").click();
    await expect(page.locator("[data-panel-heading]")).toHaveText(
      "Diagnostics",
    );
    await expect(page.locator('[data-panel-view="utility"] pre')).toHaveText(
      /\n\nNo diagnostic events recorded\.$/,
    );
    await expect(version).toHaveText(`v${nativeVersion}`);
    await expect(version).toBeVisible();
  });
}

test("late missing metadata cannot replace a running version already read through Status", async ({
  page,
  store,
  ipc,
}) => {
  await queueFixture(store);
  const nativeVersion = (await store("snapshot")).version;
  const held = ipc.holdNext("snapshot");
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    let first = true;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const missing = command === "snapshot" && first;
      if (missing) first = false;
      const result = await invoke(command, args);
      return missing ? { ...result, version: null } : result;
    };
  });
  try {
    await page.goto("/");
    await held.arrived;
    await page.locator("[data-panel-status]").click();
    await expect(page.locator("[data-panel-version]")).toHaveText(
      `v${nativeVersion}`,
    );
    held.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(page.locator("[data-panel-version]")).toHaveText(
      `v${nativeVersion}`,
    );
  } finally {
    held.release();
  }
});

for (const size of [
  { width: 320, height: 300 },
  { width: 408, height: 441 },
  { width: 408, height: 744 },
]) {
  test(`overview and inline footer fit ${size.width}x${size.height} at 200% display scale`, async ({
    store,
    page,
  }, testInfo) => {
    // CSS zoom additionally exercises text enlargement, independently of the
    // context's physical pixel density.
    await queueFixture(store);
    await page.setViewportSize(size);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await tab(page, "Settings").click();
    await expect(row(page, "Accounts")).toBeVisible();
    for (const zoom of [1, 2]) {
      await page.locator(".panel-footer").evaluate((element, value) => {
        element.style.zoom = value;
      }, zoom);
      for (const selector of [
        "[data-panel-status]",
        "[data-panel-diagnostics]",
        "[data-panel-version]",
        ".panel-hide-hint",
      ]) {
        const element = page.locator(selector);
        await expect(element).toBeInViewport({ ratio: 1 });
        expect(
          await element.evaluate((element) => {
            const bounds = element.getBoundingClientRect();
            return (
              bounds.left >= 0 &&
              bounds.right <= innerWidth &&
              bounds.top >= 0 &&
              bounds.bottom <= innerHeight
            );
          }),
        ).toBe(true);
      }
      const boxes = await page
        .locator(".panel-footer > *")
        .evaluateAll((elements) =>
          elements.map((element) => element.getBoundingClientRect().toJSON()),
        );
      for (let i = 0; i < boxes.length; i++)
        for (let j = i + 1; j < boxes.length; j++)
          expect(
            boxes[i].right <= boxes[j].left ||
              boxes[j].right <= boxes[i].left ||
              boxes[i].bottom <= boxes[j].top ||
              boxes[j].bottom <= boxes[i].top,
          ).toBe(true);
    }
    await page.screenshot({
      path: testInfo.outputPath(
        `settings-${size.width}x${size.height}-footer-zoom2.png`,
      ),
    });
  });
}
