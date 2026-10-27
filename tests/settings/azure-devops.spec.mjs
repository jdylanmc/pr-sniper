import { test, expect } from "./fixtures.mjs";

const TENANT = "00000000-0000-0000-0000-00000000000a";
const avery = {
  id: "00000000-0000-0000-0000-000000000001",
  display_name: "Avery Example",
  unique_name: "avery@contoso.example",
};
const blake = {
  id: "00000000-0000-0000-0000-000000000002",
  display_name: "Blake Example",
  unique_name: "blake@contoso.example",
};
const contoso = { state: "tenant_known", name: "contoso", tenant: TENANT };
const ready = { state: "ready", version: "2.90.0" };

function setup(overrides = {}) {
  return {
    cli: ready,
    minimum: "2.54.0",
    installer: "homebrew",
    organization: contoso,
    account: { state: "signed_out" },
    checking: false,
    ...overrides,
  };
}

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const overviewRow = (page, name) =>
  page.locator(".settings-overview").getByRole("button", { name, exact: true });
const back = (page) => page.getByRole("button", { name: "Back", exact: true });
const setupRow = (page, key) => page.locator(`[data-azure-row="${key}"]`);
const value = (page, key) => page.locator(`[data-azure-value="${key}"]`);
const actions = (page, key) =>
  setupRow(page, key)
    .locator(".azure-devops-actions button")
    .evaluateAll((buttons) =>
      buttons.map((button) => button.getAttribute("aria-label")),
    );

/** In-page fake of the native Azure DevOps commands. */
async function fakeBackend(page, initial) {
  await page.addInitScript((initial) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__ado = { view: initial, calls: [], fail: {} };
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (!command.startsWith("azure_devops_")) return original(command, args);
      const ado = window.__ado;
      ado.calls.push(command);
      if (ado.fail[command]) return Promise.reject(ado.fail[command]);
      const next = ado.next?.[command];
      if (next) ado.view = next(args ?? {}, ado.view);
      return window.__settingsTrack(
        Promise.resolve(JSON.parse(JSON.stringify(ado.view))),
      );
    };
  }, initial);
}

async function openSetup(page) {
  await page.goto("/");
  await tab(page, "Settings").click();
  await overviewRow(page, "Accounts").click();
  await overviewRow(page, "Git Repository").click();
  await overviewRow(page, "Azure DevOps").click();
  await expect(page.locator(".settings-heading h1")).toHaveText("Azure DevOps");
  await expect(setupRow(page, "account")).toBeVisible();
}

const setView = (page, view) =>
  page.evaluate((view) => {
    window.__ado.view = view;
  }, view);
const calls = (page) => page.evaluate(() => window.__ado.calls);

test.use({ viewport: { width: 408, height: 744 } });

test("each setup state shows its values and actions", async ({ page }) => {
  await fakeBackend(page, setup());
  await openSetup(page);
  const cases = [
    [{ cli: { state: "unknown" } }, "cli", "Checking", ["Re-check"]],
    [
      { cli: { state: "missing" } },
      "cli",
      "Missing",
      ["Install Azure CLI", "Re-check"],
    ],
    [{ cli: { state: "failed" } }, "cli", "Failed", ["Re-check"]],
    [
      { cli: { state: "too_old", version: "2.53.1" } },
      "cli",
      "Too old",
      ["Update Azure CLI", "Re-check"],
    ],
    [
      { cli: { state: "installing", version: null } },
      "cli",
      "Installing",
      ["Cancel install"],
    ],
    [{}, "cli", "Ready", ["Re-check"]],
    [{ organization: { state: "unset" } }, "organization", "", []],
    [
      { organization: { state: "not_found", name: "contoso" } },
      "organization",
      "Not found",
      [],
    ],
    [
      { organization: { state: "tenant_unknown", name: "contoso" } },
      "organization",
      "No tenant",
      [],
    ],
    [
      { organization: { state: "offline", name: "contoso" } },
      "organization",
      "Offline",
      ["Retry organization"],
    ],
    [{}, "organization", "", []],
    [{ account: { state: "unavailable" } }, "account", "", []],
    [{}, "account", "Signed out", ["Sign in to Azure DevOps"]],
    [
      { account: { state: "interaction_required" } },
      "account",
      "Sign-in required",
      ["Sign in to Azure DevOps"],
    ],
    [
      { account: { state: "tenant_access_blocked" } },
      "account",
      "Access denied",
      ["Retry account"],
    ],
    [
      { account: { state: "offline" } },
      "account",
      "Offline",
      ["Retry account"],
    ],
    [{ account: { state: "failed" } }, "account", "Failed", ["Retry account"]],
    [
      { account: { state: "signing_in" } },
      "account",
      "Signing in",
      ["Cancel sign-in"],
    ],
    [
      { account: { state: "connected", identity: avery } },
      "account",
      "Connected",
      ["Forget avery@contoso.example"],
    ],
    [
      {
        account: { state: "wrong_identity", identity: blake, expected: avery },
      },
      "account",
      "Wrong account",
      ["Sign in to Azure DevOps", "Forget blake@contoso.example"],
    ],
  ];
  for (const [overrides, key, expected, expectedActions] of cases) {
    await setView(page, setup(overrides));
    await back(page).click();
    await expect(overviewRow(page, "Azure DevOps")).toBeFocused();
    await overviewRow(page, "Azure DevOps").click();
    await expect(setupRow(page, key)).toHaveAttribute(
      "data-state",
      setup(overrides)[key].state,
    );
    await expect(value(page, key)).toHaveText(expected);
    expect(await actions(page, key), JSON.stringify(overrides)).toEqual(
      expectedActions,
    );
  }
  // Values only: version, minimum, tenant and identities.
  await setView(
    page,
    setup({
      cli: { state: "too_old", version: "2.53.1" },
      account: { state: "wrong_identity", identity: blake, expected: avery },
    }),
  );
  await back(page).click();
  await overviewRow(page, "Azure DevOps").click();
  await expect(page.locator("[data-azure-version]")).toHaveText("2.53.1");
  await expect(page.locator("[data-azure-minimum]")).toHaveText(
    "\u2265 2.54.0",
  );
  await expect(page.locator("[data-azure-tenant]")).toHaveText(TENANT);
  await expect(page.getByLabel("Organization", { exact: true })).toHaveValue(
    "contoso",
  );
  await expect(setupRow(page, "account").locator("strong")).toHaveText(
    "Blake Example",
  );
  await expect(page.locator("[data-azure-identity]")).toHaveText(
    "blake@contoso.example",
  );
  await expect(page.locator("[data-azure-expected]")).toHaveText(
    "\u2260 avery@contoso.example",
  );
  expect(await calls(page)).not.toContain("azure_devops_sign_in");
  expect(await calls(page)).not.toContain("azure_devops_install");
});

test("install runs natively, shows Installing, cancels and re-detects", async ({
  page,
}) => {
  await fakeBackend(page, setup({ cli: { state: "missing" } }));
  await page.addInitScript(() => {
    window.__ado.next = {
      azure_devops_install: (_args, view) => ({
        ...view,
        cli: { state: "installing", version: null },
      }),
      azure_devops_cancel: (_args, view) => ({
        ...view,
        cli: { state: "missing" },
      }),
    };
  });
  await openSetup(page);
  const install = page.getByRole("button", { name: "Install Azure CLI" });
  await install.click();
  await expect(value(page, "cli")).toHaveText("Installing");
  const cancel = page.getByRole("button", { name: "Cancel install" });
  await expect(cancel).toBeFocused();
  await expect(setupRow(page, "account")).toHaveAttribute(
    "data-state",
    "signed_out",
  );
  await cancel.click();
  await expect(value(page, "cli")).toHaveText("Missing");
  await expect(install).toBeFocused();
  await install.click();
  await expect(value(page, "cli")).toHaveText("Installing");
  // Completion arrives through polling and re-detects Azure CLI.
  await page.evaluate(() => {
    window.__ado.view = {
      ...window.__ado.view,
      cli: { state: "ready", version: "2.90.0" },
    };
  });
  await expect(value(page, "cli")).toHaveText("Ready");
  await expect(page.locator("[data-azure-version]")).toHaveText("2.90.0");
  await expect(page.getByRole("button", { name: "Re-check" })).toBeFocused();
  const made = await calls(page);
  expect(made.filter((c) => c === "azure_devops_install")).toHaveLength(2);
  expect(made).toContain("azure_devops_cancel");
  expect(made).not.toContain("azure_devops_sign_in");
});

test("the install page leaves Re-check to detect an outside install", async ({
  page,
}) => {
  await fakeBackend(
    page,
    setup({ cli: { state: "missing" }, installer: "page" }),
  );
  await page.addInitScript(() => {
    window.__ado.next = {
      azure_devops_check: (_args, view) =>
        window.__ado.installedOutside
          ? { ...view, cli: { state: "ready", version: "2.90.0" } }
          : view,
    };
  });
  await openSetup(page);
  await page.getByRole("button", { name: "Install Azure CLI" }).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(value(page, "cli")).toHaveText("Missing");
  const recheck = page.getByRole("button", { name: "Re-check" });
  await expect(recheck).toBeVisible();
  await page.evaluate(() => {
    window.__ado.installedOutside = true;
    window.__ado.calls = [];
  });
  await recheck.click();
  await expect(value(page, "cli")).toHaveText("Ready");
  expect(await calls(page)).toEqual(["azure_devops_check"]);
});

test("install failures keep focus on the action and show a value", async ({
  page,
}) => {
  await fakeBackend(page, setup({ cli: { state: "missing" } }));
  await page.addInitScript(() => {
    window.__ado.fail.azure_devops_install = "browser_open";
  });
  await openSetup(page);
  const install = page.getByRole("button", { name: "Install Azure CLI" });
  await install.click();
  await expect(page.locator("#error")).toHaveText("Browser failed");
  await expect(install).toBeFocused();
  await expect(value(page, "cli")).toHaveText("Missing");
});

test("sign-in needs confirmation, is cancellable and shows the identity", async ({
  page,
}) => {
  await fakeBackend(page, setup());
  await page.addInitScript(() => {
    window.__ado.next = {
      azure_devops_sign_in: (_args, view) => ({
        ...view,
        account: { state: "signing_in" },
      }),
      azure_devops_cancel: (_args, view) => ({
        ...view,
        account: { state: "signed_out" },
      }),
    };
  });
  await openSetup(page);
  await page.evaluate(() => window.__settingsIdle());
  expect(await calls(page)).not.toContain("azure_devops_sign_in");

  const signIn = page.getByRole("button", { name: "Sign in to Azure DevOps" });
  await signIn.click();
  const dialog = page.getByRole("dialog", { name: "Sign in" });
  await expect(dialog.locator("code")).toHaveText(
    `az login --tenant ${TENANT}`,
  );
  await dialog.getByRole("button", { name: "Close dialog" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(signIn).toBeFocused();
  expect(await calls(page)).not.toContain("azure_devops_sign_in");

  await signIn.click();
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(value(page, "account")).toHaveText("Signing in");
  const cancel = page.getByRole("button", { name: "Cancel sign-in" });
  await expect(cancel).toBeFocused();
  await cancel.click();
  await expect(value(page, "account")).toHaveText("Signed out");
  await expect(signIn).toBeFocused();

  await signIn.click();
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(value(page, "account")).toHaveText("Signing in");
  await page.evaluate((identity) => {
    window.__ado.view = {
      ...window.__ado.view,
      account: { state: "connected", identity },
    };
  }, avery);
  await expect(value(page, "account")).toHaveText("Connected");
  await expect(setupRow(page, "account").locator("strong")).toHaveText(
    "Avery Example",
  );
  await expect(
    page.getByRole("button", { name: "Forget avery@contoso.example" }),
  ).toBeFocused();
  const made = await calls(page);
  expect(made.filter((c) => c === "azure_devops_sign_in")).toHaveLength(2);
});

test("background completion never steals focus from outside the setup", async ({
  page,
}) => {
  await fakeBackend(page, setup({ account: { state: "signing_in" } }));
  await openSetup(page);
  const input = page.getByLabel("Organization", { exact: true });
  await input.focus();
  await input.press("End");
  await input.type("-dev");
  await page.evaluate((identity) => {
    window.__ado.view = {
      ...window.__ado.view,
      account: { state: "connected", identity },
    };
  }, avery);
  await expect(value(page, "account")).toHaveText("Connected");
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("contoso-dev");
  await back(page).click();
  await expect(overviewRow(page, "Azure DevOps")).toBeFocused();
});

test("organization saves validated names and wrong identity can be forgotten", async ({
  page,
}) => {
  await fakeBackend(
    page,
    setup({
      organization: { state: "unset" },
      account: { state: "unavailable" },
    }),
  );
  await page.addInitScript(
    ({ tenant, avery }) => {
      window.__ado.next = {
        azure_devops_set_organization: ({ organization }, view) => ({
          ...view,
          organization: { state: "tenant_known", name: organization, tenant },
          account: {
            state: "wrong_identity",
            identity: {
              ...avery,
              id: "x",
              unique_name: "blake@contoso.example",
            },
            expected: avery,
          },
        }),
        azure_devops_forget_account: (_args, view) => ({
          ...view,
          account: {
            state: "connected",
            identity: { ...avery, unique_name: "blake@contoso.example" },
          },
        }),
      };
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        command === "azure_devops_set_organization" &&
        args.organization.includes("/")
          ? Promise.reject("invalid_organization")
          : original(command, args);
    },
    { tenant: TENANT, avery },
  );
  await openSetup(page);
  const input = page.getByLabel("Organization", { exact: true });
  const save = page.getByRole("button", { name: "Save organization" });
  await save.click();
  await expect(input).toHaveAttribute("aria-invalid", "true");
  await input.fill("contoso/x");
  await expect(input).not.toHaveAttribute("aria-invalid", "true");
  await input.press("Enter");
  await expect(input).toHaveAttribute("aria-invalid", "true");
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("contoso/x");
  expect(await calls(page)).not.toContain("azure_devops_set_organization");

  await input.fill("contoso");
  await save.click();
  await expect(page.locator("[data-azure-tenant]")).toHaveText(TENANT);
  await expect(value(page, "account")).toHaveText("Wrong account");
  await page
    .getByRole("button", { name: "Forget blake@contoso.example" })
    .click();
  await expect(value(page, "account")).toHaveText("Connected");
  await expect(
    page.getByRole("button", { name: "Forget blake@contoso.example" }),
  ).toBeFocused();
  await back(page).click();
  await expect(overviewRow(page, "Azure DevOps")).toBeFocused();
  await expect(page.locator('[data-settings-count="azure-devops"]')).toHaveText(
    "contoso",
  );
});

test("the drill-in fits narrow panels without horizontal scroll", async ({
  page,
}, testInfo) => {
  await fakeBackend(
    page,
    setup({
      account: { state: "wrong_identity", identity: blake, expected: avery },
    }),
  );
  await page.setViewportSize({ width: 320, height: 300 });
  await openSetup(page);
  expect(
    await page
      .locator("#content")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
  await expect(page.locator(".settings-savebar")).toBeHidden();
  await page.screenshot({ path: testInfo.outputPath("azure-devops-320.png") });
  await page.setViewportSize({ width: 408, height: 744 });
  await page.screenshot({ path: testInfo.outputPath("azure-devops-408.png") });
});
