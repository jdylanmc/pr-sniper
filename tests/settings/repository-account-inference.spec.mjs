import { test, expect } from "./fixtures.mjs";
import {
  actor,
  actor2,
  providerFixture,
} from "./repository-provider-fixture.mjs";
import { section } from "./navigation.mjs";

async function intake(page, store, accounts = [actor]) {
  const fixture = await providerFixture(page, store);
  fixture.accounts = accounts;
  await page.goto("/?view=settings");
  await section(page, "Repositories");
  await page.getByRole("button", { name: "Add repository by URL" }).click();
  const dialog = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await dialog
    .getByLabel("Repository URL")
    .fill("https://github.com/orbit/one");
  return { fixture, dialog };
}

test("sole Git Repository connection is inferred and its exact identity is saved", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store);
  await expect(dialog.getByLabel("Acting GitHub account")).toBeHidden();
  await expect(dialog.getByRole("status")).toContainText("GitHub / fixture");
  await expect(
    dialog.getByRole("button", { name: "Change", exact: true }),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(
    page.getByRole("dialog", { name: "Settings for orbit/one" }),
  ).toBeVisible();
  expect(
    fixture.calls.filter((c) => c.command === "resolve_provider_repository"),
  ).toEqual([
    {
      command: "resolve_provider_repository",
      args: {
        provider: "github",
        accountId: "22",
        repository: "https://github.com/orbit/one",
      },
    },
  ]);
  expect((await store("snapshot")).settings.repositories[0]).toMatchObject({
    provider: "github",
    provider_account_id: "22",
    provider_repository_id: "100",
    name: "orbit/one",
    enabled: false,
  });
});

test("multiple compatible connections require an explicit keyboard choice", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store, [actor, actor2]);
  const choice = dialog.getByLabel("Acting GitHub account");
  await expect(choice).toBeVisible();
  await expect(choice).toHaveValue("");
  await expect(choice.locator("option")).toHaveText([
    "Choose a GitHub account",
    "fixture",
    "other",
  ]);
  await expect(
    dialog.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
  expect(
    fixture.calls.filter((c) => c.command === "resolve_provider_repository"),
  ).toHaveLength(0);
  await choice.focus();
  await page.keyboard.press("o");
  await page.keyboard.press("Tab");
  await expect(choice).toHaveValue("44");
  await expect(dialog.getByRole("status")).toContainText("GitHub / other");
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(
    page.getByRole("dialog", { name: "Settings for orbit/one" }),
  ).toBeVisible();
  expect(
    (await store("snapshot")).settings.repositories[0].provider_account_id,
  ).toBe("44");
});

for (const [name, accounts] of [
  ["zero accounts (AI access is separate)", []],
  [
    "AI-only connection with matching login",
    [{ ...actor, provider: "copilot" }],
  ],
  ["unsupported provider connection", [{ ...actor, provider: "azure_devops" }]],
  [
    "unconfirmed connection",
    [{ ...actor, state: "pending_account_confirmation" }],
  ],
  [
    "missing scope",
    [{ ...actor, state: "reconnect_required", reason: "missing_scope" }],
  ],
]) {
  test(`${name} cannot supply a repository actor`, async ({ page, store }) => {
    const { fixture, dialog } = await intake(page, store, accounts);
    await expect(
      dialog.getByRole("button", { name: "Add & configure" }),
    ).toBeDisabled();
    await expect(dialog.getByRole("status")).toContainText(
      name === "missing scope" ? "Reconnect" : "Connect",
    );
    await dialog
      .getByRole("button", {
        name:
          name === "missing scope"
            ? "Reconnect GitHub account"
            : "Connect GitHub account",
        exact: true,
      })
      .click();
    await expect(
      page.getByRole("dialog", { name: "GitHub repository accounts" }),
    ).toBeVisible();
    expect(
      fixture.calls.filter((c) => c.command === "resolve_provider_repository"),
    ).toHaveLength(0);
    expect((await store("snapshot")).settings.repositories).toBeUndefined();
  });
}

test("unsupported URL never borrows a GitHub identity and PR intake waits for configuration Save", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store);
  await dialog
    .getByLabel("Repository URL")
    .fill("https://dev.azure.com/org/project/_git/repository");
  await expect(dialog.getByRole("status")).toContainText("Azure DevOps");
  await expect(dialog.getByRole("status")).toContainText("coming soon");
  await expect(dialog.getByRole("status")).not.toContainText("fixture");
  await expect(dialog.getByLabel("Acting GitHub account")).toBeHidden();
  await expect(
    dialog.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
  await dialog.getByLabel("Repository URL").fill("https://[broken");
  await expect(dialog.getByRole("status")).toContainText("valid HTTPS");
  await expect(dialog.getByRole("status")).not.toContainText("fixture");
  await expect(
    dialog.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
  await dialog
    .getByLabel("Repository URL")
    .fill("https://github.com.example.org/orbit/one");
  await expect(dialog.getByRole("status")).toContainText("not supported");
  expect(
    fixture.calls.filter((c) => c.command === "resolve_provider_repository"),
  ).toHaveLength(0);
  await dialog
    .getByLabel("Repository URL")
    .fill("https://github.com/orbit/one/pull/7");
  await expect(dialog.getByRole("status")).toContainText("GitHub / fixture");
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(
    page.getByRole("dialog", { name: "Settings for orbit/one" }),
  ).toBeVisible();
  expect(
    fixture.calls.filter((c) => c.command === "resolve_provider_repository"),
  ).toHaveLength(1);
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  expect((await store("monitoring_snapshot")).jobs).toEqual([]);
});

test("explicit Change uses the named repository account, not the AI connection", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store);
  fixture.accounts = [actor, { ...actor2, login: "fixture_corp" }];
  await page.evaluate(() =>
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
  );
  await expect(dialog.getByRole("status")).toContainText("GitHub / fixture");
  await dialog.getByRole("button", { name: "Change", exact: true }).click();
  const choice = dialog.getByLabel("Acting GitHub account");
  await expect(choice).toBeFocused();
  await expect(choice.locator("option")).toHaveText([
    "Choose a GitHub account",
    "fixture",
    "fixture_corp",
  ]);
  await choice.selectOption("44");
  await expect(dialog.getByRole("status")).toContainText(
    "GitHub / fixture_corp",
  );
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(
    page.getByRole("dialog", { name: "Settings for orbit/one" }),
  ).toBeVisible();
  expect(
    fixture.calls.find((c) => c.command === "resolve_provider_repository").args
      .accountId,
  ).toBe("44");
  expect(
    (await store("snapshot")).settings.repositories[0].provider_account_id,
  ).toBe("44");
});

test("account read failure and recovery keep the URL, focus and original actor", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store);
  const input = dialog.getByLabel("Repository URL");
  await input.focus();
  fixture.handler = (command) => {
    if (command === "github_auth_state") throw new Error("Synthetic outage");
  };
  await page.evaluate(() =>
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
  );
  await expect(dialog.getByRole("status")).toContainText("unavailable");
  await expect(input).toHaveValue("https://github.com/orbit/one");
  await expect(input).toBeFocused();
  await expect(
    dialog.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
  fixture.handler = undefined;
  fixture.accounts = [actor2];
  await dialog.getByRole("button", { name: "Retry reading accounts" }).click();
  await expect(dialog.getByRole("status")).toContainText("Reconnect");
  await expect(dialog.getByRole("status")).toContainText("fixture");
  await expect(
    dialog.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
  await dialog.getByRole("button", { name: "Change", exact: true }).click();
  await dialog.getByLabel("Acting GitHub account").selectOption("44");
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(
    page.getByRole("dialog", { name: "Settings for orbit/one" }),
  ).toBeVisible();
  expect(
    (await store("snapshot")).settings.repositories[0].provider_account_id,
  ).toBe("44");
});

test("ambiguous discovery does not become a last-account default after disconnect", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store, [actor, actor2]);
  fixture.accounts = [actor2];
  await page.evaluate(() =>
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
  );
  await expect(
    dialog.getByLabel("Acting GitHub account").locator("option"),
  ).toHaveCount(2);
  await expect(dialog.getByLabel("Acting GitHub account")).toHaveValue("");
  await expect(
    dialog.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
});

for (const replacement of [
  "disconnect",
  "generation",
  "explicit Change",
  "URL",
]) {
  test(`${replacement} invalidates a held URL response before Store binding`, async ({
    page,
    store,
  }) => {
    const held = Promise.withResolvers();
    const entered = Promise.withResolvers();
    const { fixture, dialog } = await intake(page, store, [
      { ...actor, connection_generation: 7 },
    ]);
    let holding = true;
    fixture.handler = async (command, args) => {
      if (command === "resolve_provider_repository") {
        if (holding) {
          entered.resolve();
          await held.promise;
        }
        return {
          identity: { id: args.accountId },
          repository: {
            id: args.repository.includes("two") ? "200" : "100",
            name: await store("canonical_repository_name", {
              repository: args.repository,
            }),
          },
          account_generation: holding || replacement === "URL" ? 7 : 8,
        };
      }
    };
    try {
      await dialog.getByRole("button", { name: "Add & configure" }).click();
      await entered.promise;
      if (replacement === "URL") {
        await dialog.getByLabel("Repository URL").fill("orbit/two");
      } else {
        fixture.accounts =
          replacement === "generation"
            ? [{ ...actor, connection_generation: 8 }]
            : replacement === "disconnect"
              ? [actor2]
              : [actor, { ...actor2, connection_generation: 8 }];
        await page.evaluate(() =>
          window.dispatchEvent(
            new Event("pr-sniper:refresh-provider-accounts"),
          ),
        );
        if (replacement === "explicit Change") {
          await dialog
            .getByRole("button", { name: "Change", exact: true })
            .click();
          await dialog.getByLabel("Acting GitHub account").selectOption("44");
        }
      }
      await expect(dialog.getByRole("alert")).toContainText("changed");
      held.resolve();
      await expect(dialog).toBeVisible();
      expect((await store("snapshot")).settings.repositories).toBeUndefined();
      holding = false;
      if (replacement === "disconnect") {
        await expect(
          dialog.getByRole("button", { name: "Add & configure" }),
        ).toBeDisabled();
        await dialog
          .getByRole("button", { name: "Change", exact: true })
          .click();
        await dialog.getByLabel("Acting GitHub account").selectOption("44");
      }
      await dialog.getByRole("button", { name: "Add & configure" }).click();
      const name = replacement === "URL" ? "orbit/two" : "orbit/one";
      await expect(
        page.getByRole("dialog", { name: `Settings for ${name}` }),
      ).toBeVisible();
      expect((await store("snapshot")).settings.repositories[0]).toMatchObject({
        name,
        provider_account_id: ["disconnect", "explicit Change"].includes(
          replacement,
        )
          ? "44"
          : "22",
      });
    } finally {
      held.resolve();
    }
  });
}

test("saved binding stays pinned when a neighbor is the only usable account", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.repositories = [
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      provider: "github",
      name: "orbit/one",
      enabled: false,
      provider_account_id: "22",
      provider_repository_id: "100",
    },
  ];
  await store("seed_settings", settings);
  const fixture = await providerFixture(page, store);
  fixture.accounts = [
    { ...actor, state: "reconnect_required", reason: "missing_scope" },
    actor2,
  ];
  await page.goto("/?view=settings");
  await section(page, "Repositories");
  await page.locator("[data-repository]").click();
  let editor = page.getByRole("dialog", { name: "Settings for orbit/one" });
  await expect(editor.locator(".repository-identity")).toContainText("fixture");
  await expect(editor.getByText("Account ID", { exact: true })).toBeHidden();
  await editor.getByText("Repository and connection", { exact: true }).click();
  await editor
    .getByRole("button", { name: "Edit repository", exact: true })
    .click();
  editor = page.getByRole("dialog", { name: "Edit repository", exact: true });
  await expect(editor.getByRole("status")).toContainText("Reconnect fixture");
  await expect(
    editor.getByRole("button", { name: "Add & configure" }),
  ).toBeDisabled();
  fixture.accounts = [actor2];
  await page.evaluate(() =>
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
  );
  await expect(editor.getByRole("status")).toContainText("Reconnect fixture");
  expect(
    (await store("snapshot")).settings.repositories[0].provider_account_id,
  ).toBe("22");
  expect(
    fixture.calls.filter((c) => c.command === "resolve_provider_repository"),
  ).toHaveLength(0);
});

test("temporary provider warning keeps the actor, surfaces retry and retains failed URL context", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store, [
    { ...actor, warning: "network" },
  ]);
  await expect(dialog.getByRole("status")).toContainText(
    "Verification needs retry",
  );
  fixture.handler = (command) => {
    if (command === "resolve_provider_repository") throw new Error("network");
  };
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(dialog.getByRole("alert")).toContainText("network");
  await expect(dialog.getByLabel("Repository URL")).toHaveValue(
    "https://github.com/orbit/one",
  );
  await expect(dialog.getByRole("status")).toContainText("GitHub / fixture");
  fixture.handler = undefined;
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(
    page.getByRole("dialog", { name: "Settings for orbit/one" }),
  ).toBeVisible();
  expect(
    (await store("snapshot")).settings.repositories[0].provider_account_id,
  ).toBe("22");
});

test("native connection-generation evidence must match the captured request", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store, [
    { ...actor, connection_generation: 8 },
  ]);
  fixture.handler = (command, args) => {
    if (command === "resolve_provider_repository")
      return {
        identity: { id: args.accountId },
        repository: { id: "100", name: "orbit/one" },
        account_generation: 7,
      };
  };
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(dialog.getByRole("alert")).toContainText("connection changed");
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
});

test("recovery reuses repository sign-in and requires its existing identity confirmation", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store, []);
  fixture.handler = (command) => {
    if (command === "start_github_browser_auth")
      return {
        accounts: [],
        flow: {
          state: "pending_account_confirmation",
          account_id: "22",
          login: "fixture",
        },
      };
    if (command === "confirm_github_account") {
      fixture.accounts = [actor];
      return { accounts: fixture.accounts, flow: { state: "idle" } };
    }
  };
  await dialog.getByRole("button", { name: "Connect GitHub account" }).click();
  const recovery = page.getByRole("dialog", {
    name: "GitHub repository accounts",
  });
  await expect(page.locator(".github-auth-card")).toHaveCount(1);
  await recovery.getByRole("button", { name: "Add GitHub account" }).click();
  await expect(recovery.getByRole("status")).toContainText("Confirm fixture");
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
  await recovery.getByRole("button", { name: "Confirm", exact: true }).click();
  await recovery.locator(".resource-back").click();
  await expect(dialog.getByRole("status")).toContainText("GitHub / fixture");
  await expect(dialog.getByLabel("Repository URL")).toHaveValue(
    "https://github.com/orbit/one",
  );
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(
    page.getByRole("dialog", { name: "Settings for orbit/one" }),
  ).toBeVisible();
  expect(
    fixture.calls.filter((c) => c.command === "confirm_github_account"),
  ).toHaveLength(1);
  expect(
    (await store("snapshot")).settings.repositories[0].provider_account_id,
  ).toBe("22");
});

test("account catalog refresh keeps keyboard focus inside the explicit choice", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store, [actor, actor2]);
  const choice = dialog.getByLabel("Acting GitHub account");
  await choice.focus();
  fixture.accounts = [actor2];
  await page.evaluate(() =>
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
  );
  await expect(choice.locator("option")).toHaveCount(2);
  await expect(choice).toBeFocused();
  fixture.handler = (command) => {
    if (command === "github_auth_state") throw new Error("Synthetic outage");
  };
  await page.evaluate(() =>
    window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
  );
  await expect(dialog.getByRole("status")).toContainText("unavailable");
  await expect(choice).toBeFocused();
  await expect(dialog.getByLabel("Repository URL")).toHaveValue(
    "https://github.com/orbit/one",
  );
});

test("an inferred caption cannot save a different returned provider identity", async ({
  page,
  store,
}) => {
  const { fixture, dialog } = await intake(page, store);
  fixture.handler = (command) => {
    if (command === "resolve_provider_repository")
      return {
        identity: { id: "44" },
        repository: { id: "100", name: "orbit/one" },
        account_generation: 0,
      };
  };
  await dialog.getByRole("button", { name: "Add & configure" }).click();
  await expect(dialog.getByRole("alert")).toContainText("connection changed");
  await expect(dialog.getByRole("status")).toContainText("GitHub / fixture");
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
});

for (const size of [
  { width: 408, height: 744 },
  { width: 320, height: 300 },
]) {
  test(`compact identity and recovery preserve accessible controls at ${size.width}`, async ({
    page,
    store,
  }, info) => {
    await page.setViewportSize(size);
    await page.emulateMedia({ reducedMotion: "reduce" });
    const { fixture, dialog } = await intake(page, store);
    await expect(dialog.getByLabel("Repository URL")).toBeVisible();
    await expect(dialog.getByLabel("Acting GitHub account")).toBeHidden();
    await page.screenshot({
      path: info.outputPath("compact-inferred-account.png"),
    });
    await dialog.getByRole("button", { name: "Change", exact: true }).click();
    await expect(dialog.getByLabel("Acting GitHub account")).toBeFocused();
    await page.screenshot({
      path: info.outputPath("compact-account-choice.png"),
    });
    fixture.accounts = [];
    await page.evaluate(() =>
      window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
    );
    await dialog
      .getByRole("button", { name: "Reconnect GitHub account" })
      .click();
    const recovery = page.getByRole("dialog", {
      name: "GitHub repository accounts",
    });
    await expect(
      recovery.getByRole("button", { name: "Add GitHub account" }),
    ).toBeVisible();
    await recovery.locator(".resource-back").click();
    await expect(dialog.getByLabel("Repository URL")).toHaveValue(
      "https://github.com/orbit/one",
    );
    await expect(
      dialog.getByRole("button", { name: "Reconnect GitHub account" }),
    ).toBeFocused();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
  });
}
