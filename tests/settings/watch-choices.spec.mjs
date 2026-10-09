import { expect, test } from "./fixtures.mjs";
import { repositorySettings, seedBoundRepositories } from "./navigation.mjs";

test("direct watch choices persist combined, subset and empty selections", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seedBoundRepositories(store, ["example/repo"]);
  initial.repositories[0].enabled = false;
  await store("seed_settings", initial);
  await page.setViewportSize({ width: 408, height: 744 });
  await page.goto("/?view=settings");
  let editor = await repositorySettings(page, "example/repo");
  const names = [
    "All Pull Requests",
    "Pull requests by user",
    "Pull requests where my review is requested",
    "Reply to @Mentions",
  ];
  for (const name of names)
    await expect(
      editor.getByRole("checkbox", { name, exact: true }),
    ).toBeVisible();
  await editor.getByRole("checkbox", { name: names[0], exact: true }).uncheck();
  await editor.getByRole("checkbox", { name: names[0], exact: true }).check();
  for (const name of names)
    await expect(
      editor.getByRole("checkbox", { name, exact: true }),
    ).toBeChecked();
  await editor.locator("[data-watch-mentions]").scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("watch-all.png") });
  const save = async () => {
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-resource-error]")).toBeHidden();
    await expect(editor).toHaveCount(0);
    await page.reload();
    editor = await repositorySettings(page, "example/repo");
    return (await store("snapshot")).settings.repositories[0];
  };
  let saved = await save();
  expect(saved.overrides.watch).toEqual({
    all_pull_requests: true,
    by_user: true,
    mentions: true,
  });
  expect(saved.overrides.reviewer_assignment).toBe(true);
  await editor.getByRole("checkbox", { name: names[2], exact: true }).uncheck();
  saved = await save();
  expect(saved.overrides.watch).toEqual({
    all_pull_requests: false,
    by_user: true,
    mentions: true,
  });
  expect(saved.overrides.reviewer_assignment).toBe(false);
  await editor.getByRole("checkbox", { name: names[1], exact: true }).uncheck();
  await editor.getByRole("checkbox", { name: names[3], exact: true }).uncheck();
  saved = await save();
  expect(saved.overrides.watch).toEqual({
    all_pull_requests: false,
    by_user: false,
    mentions: false,
  });
  for (const name of names)
    await expect(
      editor.getByRole("checkbox", { name, exact: true }),
    ).not.toBeChecked();
  await expect(editor.locator("[data-watch-summary]")).toContainText(
    "No new pull requests",
  );
  expect(saved.assignments ?? []).toEqual([]);
  await editor.locator("[data-watch-mentions]").scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("watch-empty.png") });
});

test("Edit Users searches under the saved provider identity, retains selections and surfaces loading, empty and errors", async ({
  page,
  store,
}, testInfo) => {
  const initial = await seedBoundRepositories(store, [
    "example/repo",
    "neighbor/repo",
  ]);
  initial.repositories.forEach((repository) => (repository.enabled = false));
  initial.defaults.watched_authors = [{ id: "9", login: "inherited" }];
  initial.repositories[0].watched_authors = [{ id: "9", login: "stale-label" }];
  await store("seed_settings", initial);
  let finish;
  const calls = [];
  await page.exposeFunction("__watchSearch", (args) => {
    calls.push(args);
    return new Promise((resolve) => {
      finish = resolve;
    });
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "github_auth_state")
        return {
          accounts: [
            {
              provider: "github",
              account_id: "22",
              login: "selected-provider",
              state: "connected",
              connection_generation: 1,
            },
            {
              provider: "github",
              account_id: "33",
              login: "not-the-provider",
              state: "connected",
              connection_generation: 1,
            },
          ],
          flow: { state: "idle" },
        };
      if (command === "search_provider_people") {
        const response = await window.__watchSearch(args);
        if (response.error) throw response.error;
        return response.people;
      }
      return original(command, args);
    };
  });
  await page.goto("/?view=settings");
  const editor = await repositorySettings(page, "example/repo");
  await editor.getByRole("button", { name: "Edit Users", exact: true }).click();
  const picker = page.getByRole("dialog", { name: "Edit Users", exact: true });
  await expect(picker).toContainText("GitHub account: selected-provider");
  const input = picker.getByRole("searchbox", { name: "GitHub login" });
  await expect(input).toBeFocused();
  await picker
    .getByRole("checkbox", { name: "@inherited", exact: true })
    .uncheck();
  await input.fill("octo");
  await input.press("Enter");
  await expect(picker.getByRole("status")).toHaveText("Loading GitHub user...");
  await expect.poll(() => calls.length).toBe(1);
  finish({ people: [{ id: "42", login: "octocat" }] });
  await expect(picker.getByRole("status")).toContainText("1 users found");
  const person = picker.getByRole("checkbox", {
    name: "@octocat",
    exact: true,
  });
  await person.focus();
  await person.press("Space");
  await expect(person).toBeChecked();
  await input.fill("nobody");
  await input.press("Enter");
  await expect.poll(() => calls.length).toBe(2);
  finish({ people: [] });
  await expect(picker.getByRole("status")).toContainText(
    "No matching GitHub users",
  );
  await expect(
    picker.getByRole("checkbox", { name: "@octocat", exact: true }),
  ).toBeChecked();
  await input.fill("failure");
  await input.press("Enter");
  await expect.poll(() => calls.length).toBe(3);
  finish({ error: "network" });
  await expect(picker.getByRole("alert")).toBeVisible();
  await expect(picker.getByRole("status")).toContainText(
    "selections are unchanged",
  );
  await page.screenshot({ path: testInfo.outputPath("user-picker-error.png") });
  await page.setViewportSize({ width: 320, height: 300 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await picker
    .getByRole("button", { name: "Use selected users", exact: true })
    .focus();
  await page.screenshot({
    path: testInfo.outputPath("user-picker-error-selected.png"),
  });
  await page.keyboard.press("Enter");
  await expect(
    editor.getByRole("button", { name: "Edit Users", exact: true }),
  ).toBeFocused();
  await expect(editor.locator(".watchlist")).toContainText("@octocat");
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].watched_authors).toEqual([
    { id: "42", login: "octocat" },
  ]);
  expect(saved.repositories[0].overrides.watched_authors).toEqual([]);
  expect(saved.repositories[1]).toEqual(initial.repositories[1]);
  expect(saved.defaults).toEqual(initial.defaults);
  expect(calls).toEqual(
    ["octo", "nobody", "failure"].map((query) => ({
      provider: "github",
      accountId: "22",
      query,
    })),
  );
});

test("legacy effective choices remain visible without rewriting on open and All selects every category", async ({
  page,
  store,
}) => {
  const initial = await seedBoundRepositories(store, ["example/repo"]);
  initial.repositories[0].enabled = false;
  initial.defaults.reviewer_assignment = false;
  await store("seed_settings", initial);
  await page.goto("/?view=settings");
  const editor = await repositorySettings(page, "example/repo");
  const all = editor.getByRole("checkbox", {
    name: "All Pull Requests",
    exact: true,
  });
  await expect(all).toBeChecked({ indeterminate: true });
  await expect(editor.locator("[data-watch-summary]")).toContainText(
    "All authors",
  );
  expect((await store("snapshot")).settings).toEqual(initial);
  await all.check();
  await expect(all).toBeChecked();
  await expect(
    editor.getByRole("checkbox", {
      name: "Pull requests where my review is requested",
      exact: true,
    }),
  ).toBeChecked();
  await expect(
    editor.getByRole("checkbox", {
      name: "Reply to @Mentions",
      exact: true,
    }),
  ).toBeChecked();
});

test("dismissed user search cannot mutate a replacement editor or borrow another account", async ({
  page,
  store,
}) => {
  const initial = await seedBoundRepositories(store, ["example/repo"]);
  initial.repositories[0].enabled = false;
  await store("seed_settings", initial);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__hasSelectedAccount = true;
    window.__peopleRequests = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "github_auth_state")
        return {
          accounts: [
            ...(window.__hasSelectedAccount
              ? [
                  {
                    provider: "github",
                    account_id: "22",
                    login: "selected",
                    state: "connected",
                  },
                ]
              : []),
            {
              provider: "github",
              account_id: "33",
              login: "other",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        };
      if (command === "search_provider_people") {
        window.__peopleRequests.push(args);
        return new Promise((resolve) => {
          window.__completePeople = resolve;
        });
      }
      return original(command, args);
    };
  });
  await page.goto("/?view=settings");
  const editor = await repositorySettings(page, "example/repo");
  await editor.getByRole("button", { name: "Edit Users", exact: true }).click();
  const picker = page.getByRole("dialog", { name: "Edit Users", exact: true });
  await picker.getByRole("searchbox", { name: "GitHub login" }).fill("octo");
  await picker.getByRole("button", { name: "Search", exact: true }).click();
  await expect(picker.getByRole("status")).toHaveText("Loading GitHub user...");
  await picker.getByRole("button", { name: "Cancel", exact: true }).click();
  await page.evaluate(() => {
    window.__hasSelectedAccount = false;
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts"));
  });
  await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("github_auth_state"),
  );
  await editor.getByRole("button", { name: "Edit Users", exact: true }).click();
  await expect(
    picker.getByRole("searchbox", { name: "GitHub login" }),
  ).toBeDisabled();
  await expect(picker).toContainText("another account cannot be substituted");
  await page.evaluate(() =>
    window.__completePeople([{ id: "42", login: "octocat" }]),
  );
  await expect(picker.getByRole("checkbox")).toHaveCount(0);
  expect(await page.evaluate(() => window.__peopleRequests)).toEqual([
    { provider: "github", accountId: "22", query: "octo" },
  ]);
  expect((await store("snapshot")).settings).toEqual(initial);
});
