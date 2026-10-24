import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { writeFile } from "node:fs/promises";

const geometryObservations = new WeakMap();

test.afterEach(async ({ page }, testInfo) => {
  try {
    if (testInfo.status !== testInfo.expectedStatus)
      await page.screenshot({
        path: testInfo.outputPath("failed-attempt.png"),
      });
  } finally {
    const observations = geometryObservations.get(testInfo);
    if (observations)
      await writeFile(
        testInfo.outputPath("geometry.json"),
        JSON.stringify(observations, null, 2) + "\n",
      );
  }
});

function accountTabKey(browserName, direction) {
  // Native WebKit keyboard mode is independent of spoofed consent user agents.
  return browserName === "webkit" && process.platform === "darwin"
    ? `Alt+${direction}`
    : direction;
}

test("account keyboard mode traverses neutral buttons and summary in both directions", async ({
  page,
  browserName,
}) => {
  await page.setContent(`
    <input aria-label="Start">
    <button>First</button>
    <details><summary>Consent</summary>Neutral consent text</details>
    <button>Last</button>
    <input aria-label="End">
  `);
  const controls = [
    page.getByRole("textbox", { name: "Start", exact: true }),
    page.getByRole("button", { name: "First", exact: true }),
    page.locator("summary"),
    page.getByRole("button", { name: "Last", exact: true }),
    page.getByRole("textbox", { name: "End", exact: true }),
  ];
  await controls[0].focus();
  for (const direction of ["Tab", "Shift+Tab"]) {
    const ordered =
      direction === "Tab"
        ? controls.slice(1)
        : controls.slice(0, -1).toReversed();
    for (const control of ordered) {
      await page.keyboard.press(accountTabKey(browserName, direction));
      await expect(control).toBeFocused();
    }
  }
});

// Synthetic auth transport using the native GithubAuthView / copilot_view shapes.
// Only Settings persistence uses the production Store. No provider, credential,
// browser handoff or clipboard operation is performed by these tests.
const idle = (accounts = []) => ({ accounts, flow: { state: "idle" } });
const identity = (provider, id = "101") => ({
  provider,
  account_id: id,
  login: `fixture-${provider}-${id}`,
  state: "connected",
});
const roles = {
  github: {
    card: ".github-auth-card",
    start: "start_github_browser_auth",
    confirm: "confirm_github_account",
    cancel: "cancel_github_auth",
    disconnect: "disconnect_github_auth",
    connectLabel: "Add GitHub account",
    confirmLabel: "Confirm",
    cancelLabel: "Cancel",
    reconnectLabel: "Reconnect fixture-github-101",
    disconnectLabel: "Disconnect fixture-github-101",
    role: "Repository access and publication identity",
  },
  copilot: {
    card: ".copilot-auth-card",
    start: "start_copilot_auth",
    confirm: "confirm_copilot_account",
    cancel: "cancel_copilot_auth",
    disconnect: "disconnect_copilot_account",
    connectLabel: "Connect Copilot account",
    confirmLabel: "Confirm Copilot account",
    cancelLabel: "Cancel Copilot sign-in",
    reconnectLabel: "Reconnect Copilot fixture-copilot-101",
    disconnectLabel: "Disconnect Copilot fixture-copilot-101",
    role: "AI identity for Agent reviews",
  },
};
const pending = (provider) => ({
  state: "pending_account_confirmation",
  account_id: "101",
  login: identity(provider).login,
});
// Browser sign-in auto-confirms a returned identity once. A confirmation error
// keeps the manual Confirm and Cancel controls available for these tests.
const manualPending = (provider) => ({
  ...pending(provider),
  confirmation_error: "credentials_unavailable",
});
const connecting = {
  state: "connecting",
  user_code: "TEST-CODE",
  verification_uri: "https://github.com/login/device",
};

async function syntheticAuth(page, initial = {}) {
  const states = { github: idle(), copilot: idle(), ...initial };
  const calls = [];
  const failures = new Map();
  const replies = new Map();
  const commands = [
    "github_auth_state",
    "copilot_auth_state",
    ...Object.values(roles).flatMap((role) => [
      role.start,
      role.confirm,
      role.cancel,
      role.disconnect,
    ]),
    "verify_copilot_account",
  ];
  await page.exposeFunction("__accountNativeFixture", (command, args) => {
    calls.push({ command, args: args ?? null });
    if (replies.has(command)) return replies.get(command)();
    if (failures.has(command)) throw failures.get(command);
    const provider = command.includes("copilot") ? "copilot" : "github";
    const role = roles[provider];
    const state = states[provider];
    if (command === role.start)
      state.flow = {
        ...connecting,
        ...(args?.expectedAccountId
          ? { expected_account_id: args.expectedAccountId }
          : {}),
      };
    if (command === role.cancel) state.flow = { state: "idle" };
    if (command === role.confirm) {
      if (state.flow.state !== "pending_account_confirmation")
        throw "Synthetic transport: no returned identity to confirm.";
      state.accounts = [
        ...state.accounts.filter((a) => a.account_id !== state.flow.account_id),
        identity(provider, state.flow.account_id),
      ];
      state.flow = { state: "idle" };
    }
    if (command === role.disconnect)
      state.accounts = state.accounts.filter(
        (a) => a.account_id !== args.accountId,
      );
    return structuredClone(state);
  });
  await page.addInitScript((commands) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    const pending = new Set();
    window.__accountFixtureIdle = async () => {
      while (pending.size) await Promise.allSettled([...pending]);
    };
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (!commands.includes(command)) return original(command, args);
      const request = window.__accountNativeFixture(command, args);
      const settled = request.finally(() => pending.delete(settled));
      pending.add(settled);
      return settled;
    };
    Object.defineProperty(navigator, "clipboard", {
      value: {
        writeText() {
          throw new Error("Clipboard is not authorized in this fixture.");
        },
      },
    });
  }, commands);
  return { states, calls, failures, replies };
}

async function refresh(page, provider) {
  await page.evaluate((provider) => {
    window.dispatchEvent(
      new Event(
        provider === "github" ? "pr-sniper:refresh-provider-accounts" : "focus",
      ),
    );
    return window.__accountFixtureIdle();
  }, provider);
}

function effects(calls) {
  return calls.filter(({ command }) => !command.endsWith("_auth_state"));
}

for (const platform of ["Macintosh", "Windows"]) {
  test.describe(platform, () => {
    test.use({ userAgent: `Mozilla/5.0 (${platform})` });

    test("named provider actions and disabled future providers never select identities or models", async ({
      page,
      store,
    }) => {
      const fixture = await syntheticAuth(page);
      await page.goto("/?view=settings");
      await page.evaluate(() => window.__settingsIdle());
      const before = (await store("snapshot")).settings;
      for (const [provider, role] of Object.entries(roles)) {
        const card = page.locator(role.card);
        await expect(card).toContainText(role.role);
        await expect(
          card.getByRole("button", { name: role.connectLabel, exact: true }),
        ).toBeEnabled();
        await expect(
          card.getByRole("button", { name: role.confirmLabel, exact: true }),
        ).toHaveCount(0);
        await expect(card.locator("input, select")).toHaveCount(0);
        await expect(card.locator("summary, details")).toHaveCount(0);
      }
      for (const name of ["Claude", "Codex", "Grok"]) {
        const button = page.getByRole("button", {
          name: `Direct ${name} Coming soon`,
          exact: true,
        });
        await expect(button).toBeDisabled();
        await expect(button).toHaveAttribute("aria-disabled", "true");
        await button.evaluate((element) => element.click());
      }
      expect(effects(fixture.calls)).toEqual([]);
      expect((await store("snapshot")).settings).toEqual(before);
      await section(page, "Agents");
      await expect(
        page.getByRole("button", { name: "New agent", exact: true }),
      ).toBeDisabled();
    });

    for (const [provider, role] of Object.entries(roles)) {
      test(`${provider}: exact command boundary, cancel, confirm and disconnect preserve saved references`, async ({
        page,
        store,
      }) => {
        const settings = (await store("snapshot")).settings;
        settings.agents = [
          {
            id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            name: "Pinned reviewer",
            model: "retained-model",
            ai_account: { provider: "copilot", account_id: "202" },
            prompt: "Retain this exact prompt.",
            signature: "Fixture",
          },
        ];
        settings.repositories = [
          {
            id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            provider: "github",
            name: "fixture/repository",
            provider_account_id: "202",
            provider_repository_id: "303",
            enabled: false,
            assignments: [
              {
                id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                agent_id: settings.agents[0].id,
                schedule: settings.defaults.schedule,
                comment: false,
                approve: false,
              },
            ],
          },
        ];
        await store("seed_settings", settings);
        const fixture = await syntheticAuth(page, {
          github: idle([identity("github", "202")]),
          copilot: idle([identity("copilot", "202")]),
        });
        await page.goto("/?view=settings");
        await page.evaluate(() => window.__settingsIdle());
        const before = (await store("snapshot")).settings;
        const card = page.locator(role.card);
        const click = (name) =>
          card.getByRole("button", { name, exact: true }).click();
        await click(role.connectLabel);
        await expect(card).toContainText("TEST-CODE");
        expect(effects(fixture.calls)).toEqual([
          { command: role.start, args: {} },
        ]);
        expect(fixture.states[provider].accounts).toHaveLength(1);
        fixture.states[provider].flow = manualPending(provider);
        await refresh(page, provider);
        await expect(card.getByRole("status")).toContainText(
          provider === "github"
            ? "Storage failed: fixture-github-101 (101)"
            : "Connecting fixture-copilot-101",
        );
        expect(effects(fixture.calls)).toHaveLength(1);
        await click(role.cancelLabel);
        expect(fixture.states[provider].accounts).toHaveLength(1);
        expect((await store("snapshot")).settings).toEqual(before);
        await click(role.connectLabel);
        fixture.states[provider].flow = pending(provider);
        await refresh(page, provider);
        // The returned identity is confirmed automatically exactly once.
        await expect(card.locator(".github-account")).toHaveCount(2);
        expect(effects(fixture.calls)).toEqual([
          { command: role.start, args: {} },
          { command: role.cancel, args: {} },
          { command: role.start, args: {} },
          { command: role.confirm, args: {} },
        ]);
        expect((await store("snapshot")).settings).toEqual(before);
        await click(role.disconnectLabel);
        expect(effects(fixture.calls).at(-1)).toEqual({
          command: role.disconnect,
          args: { accountId: "101" },
        });
        expect(fixture.states[provider].accounts).toContainEqual(
          identity(provider, "202"),
        );
        const other = provider === "github" ? "copilot" : "github";
        expect(fixture.states[other]).toEqual(idle([identity(other, "202")]));
        await page.reload();
        await expect(page.locator(role.card)).toContainText(
          `fixture-${provider}-202 (202)`,
        );
        expect((await store("snapshot")).settings).toEqual(before);
      });
    }
  });
}

for (const [provider, role] of Object.entries(roles)) {
  for (const [reason, message] of [
    ["expired", "Reconnect required"],
    ["timeout", "Timed out"],
    ["wrong_identity", "Wrong account"],
    ["denied", "Denied"],
    ["missing_scope", "Reconnect required"],
    ["network", "Offline"],
    ["browser_open", "Browser failed"],
    ["device_flow_disabled", "Device sign-in disabled"],
    ["cancelled", "Cancelled"],
    ["provider", "Verification failed"],
    ["invalid_response", "Invalid response"],
    ["rate_limited", "Rate limited"],
  ]) {
    test(`${provider}: native ${reason} failure remains visible with explicit retry and cancellation`, async ({
      page,
      store,
    }) => {
      const fixture = await syntheticAuth(page, {
        [provider]: {
          accounts: [identity(provider, "202")],
          flow: { state: "failed", reason },
        },
      });
      await page.goto("/?view=settings");
      await page.evaluate(() => window.__settingsIdle());
      const before = (await store("snapshot")).settings;
      const card = page.locator(role.card);
      await expect(card).toHaveAttribute("data-flow-state", "failed");
      const status =
        provider === "github"
          ? card.getByRole("status")
          : card.getByRole("alert");
      await expect(status).toBeVisible();
      await expect(status).toContainText(message);
      await expect(
        card.getByRole("button", { name: role.confirmLabel, exact: true }),
      ).toHaveCount(0);
      expect(effects(fixture.calls)).toEqual([]);
      const retry =
        provider === "github" ? "Try GitHub sign-in again" : role.connectLabel;
      await card.getByRole("button", { name: retry, exact: true }).click();
      await expect(card).toContainText("TEST-CODE");
      await card
        .getByRole("button", { name: role.cancelLabel, exact: true })
        .click();
      await expect(card).not.toContainText("TEST-CODE");
      expect(effects(fixture.calls)).toEqual([
        { command: role.start, args: {} },
        { command: role.cancel, args: {} },
      ]);
      expect(fixture.states[provider].accounts).toEqual([
        identity(provider, "202"),
      ]);
      expect((await store("snapshot")).settings).toEqual(before);
    });
  }

  test(`${provider}: secure-storage confirmation rejection cannot connect and cancellation preserves another identity`, async ({
    page,
    store,
  }) => {
    const fixture = await syntheticAuth(page, {
      [provider]: {
        accounts: [identity(provider, "202")],
        flow: pending(provider),
      },
    });
    const failure =
      "Credentials could not be saved securely. Synthetic storage rejection.";
    fixture.failures.set(role.confirm, failure);
    await page.goto("/?view=settings");
    await page.evaluate(() => window.__settingsIdle());
    const before = (await store("snapshot")).settings;
    const card = page.locator(role.card);
    // The automatic confirmation attempt is the rejected one.
    await expect(card).toContainText(
      provider === "github" ? "Storage failed" : "could not be saved securely",
    );
    await expect(
      card.getByRole("button", { name: role.confirmLabel, exact: true }),
    ).toBeEnabled();
    expect(fixture.states[provider].accounts).toEqual([
      identity(provider, "202"),
    ]);
    await card
      .getByRole("button", { name: role.cancelLabel, exact: true })
      .click();
    expect(effects(fixture.calls)).toEqual([
      { command: role.confirm, args: {} },
      { command: role.cancel, args: {} },
    ]);
    expect((await store("snapshot")).settings).toEqual(before);
  });

  test(`${provider}: reconnect uses the retained exact account ID and native wrong-identity state never substitutes it`, async ({
    page,
  }) => {
    const retained = {
      ...identity(provider),
      state: "reconnect_required",
      reason: "expired",
    };
    const fixture = await syntheticAuth(page, { [provider]: idle([retained]) });
    await page.goto("/?view=settings");
    const card = page.locator(role.card);
    await card
      .getByRole("button", { name: role.reconnectLabel, exact: true })
      .click();
    expect(effects(fixture.calls)).toEqual([
      { command: role.start, args: { expectedAccountId: "101" } },
    ]);
    fixture.states[provider].flow = {
      state: "failed",
      reason: "wrong_identity",
    };
    await refresh(page, provider);
    await expect(card).toHaveAttribute("data-flow-state", "failed");
    await expect(card).toContainText("Wrong account");
    expect(fixture.states[provider].accounts).toEqual([retained]);
  });

  test(`${provider}: state-read failure exposes a real retry command, never an assumed connection`, async ({
    page,
  }) => {
    const fixture = await syntheticAuth(page);
    fixture.failures.set(
      `${provider}_auth_state`,
      "Synthetic state read failure.",
    );
    await page.goto("/?view=settings");
    const card = page.locator(role.card);
    await expect(card).toContainText("Unavailable");
    const retry = card.getByRole("button", {
      name: `Retry reading ${provider === "github" ? "GitHub" : "Copilot"} accounts`,
    });
    await expect(retry).toBeEnabled();
    const reads = fixture.calls.length;
    fixture.failures.clear();
    await retry.click();
    await expect(
      card.getByRole("button", { name: role.connectLabel, exact: true }),
    ).toBeEnabled();
    expect(fixture.calls.slice(reads)).toContainEqual({
      command: `${provider}_auth_state`,
      args: {},
    });
    expect(effects(fixture.calls)).toEqual([]);
  });

  test(`${provider}: a late state read cannot revive cancelled sign-in or take newer focus`, async ({
    page,
  }) => {
    const fixture = await syntheticAuth(page, {
      [provider]: { accounts: [], flow: connecting },
    });
    await page.goto("/?view=settings");
    const card = page.locator(role.card);
    await expect(card).toContainText("TEST-CODE");
    const held = Promise.withResolvers();
    const started = Promise.withResolvers();
    fixture.replies.set(`${provider}_auth_state`, () => {
      fixture.replies.delete(`${provider}_auth_state`);
      started.resolve();
      return held.promise;
    });
    await page.evaluate((provider) => {
      window.dispatchEvent(
        new Event(
          provider === "github"
            ? "pr-sniper:refresh-provider-accounts"
            : "focus",
        ),
      );
    }, provider);
    await started.promise;
    await card
      .getByRole("button", { name: role.cancelLabel, exact: true })
      .click();
    await expect(card).not.toContainText("TEST-CODE");
    const search = page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("button", { name: "Agents", exact: true });
    await search.focus();
    held.resolve({ accounts: [], flow: connecting });
    await page.evaluate(() => window.__accountFixtureIdle());
    await expect(card).not.toContainText("TEST-CODE");
    await expect(search).toBeFocused();
    expect(effects(fixture.calls)).toEqual([
      { command: role.cancel, args: {} },
    ]);
  });

  test(`${provider}: native request and confirmation failure states preserve keyboard ownership and require a real retry`, async ({
    page,
  }) => {
    const fixture = await syntheticAuth(page, {
      [provider]: { accounts: [], flow: { state: "connecting" } },
    });
    await page.goto("/?view=settings");
    const card = page.locator(role.card);
    await expect(card.getByRole("status")).toContainText("Requesting code");
    await expect(card.locator(".github-device-code")).toHaveCount(0);
    await expect(
      card.getByRole("button", { name: role.confirmLabel, exact: true }),
    ).toHaveCount(0);
    const cancel = card.getByRole("button", {
      name: role.cancelLabel,
      exact: true,
    });
    await cancel.focus();
    fixture.states[provider].flow = {
      ...pending(provider),
      confirmation_error: "credentials_unavailable",
    };
    await refresh(page, provider);
    await expect(cancel).toBeFocused();
    await expect(card).toContainText(
      provider === "github" ? "Storage failed" : "Storage unavailable",
    );
    expect(effects(fixture.calls)).toEqual([]);
    await card
      .getByRole("button", { name: role.confirmLabel, exact: true })
      .focus();
    await page.keyboard.press("Enter");
    await expect(card.getByRole("status")).toBeFocused();
    await expect(card.locator(".github-account")).toHaveCount(1);
    expect(effects(fixture.calls)).toEqual([
      { command: role.confirm, args: {} },
    ]);
  });

  test(`${provider}: background account changes retain focus by ID, never by list position`, async ({
    page,
  }) => {
    const first = {
      ...identity(provider),
      state: "reconnect_required",
      reason: "expired",
    };
    const second = identity(provider, "202");
    const fixture = await syntheticAuth(page, {
      [provider]: idle([first, second]),
    });
    await page.goto("/?view=settings");
    const card = page.locator(role.card);
    const reconnect = card.getByRole("button", {
      name: role.reconnectLabel,
      exact: true,
    });
    await reconnect.focus();
    fixture.states[provider].accounts = [
      second,
      { ...first, reason: "denied" },
    ];
    await refresh(page, provider);
    await expect(reconnect).toBeFocused();
    expect(effects(fixture.calls)).toEqual([]);
  });

  test(`${provider}: returning from native sign-in retains the mounted account controls when Settings is unchanged`, async ({
    page,
  }) => {
    await syntheticAuth(page, {
      [provider]: { accounts: [], flow: manualPending(provider) },
    });
    await page.goto("/?view=settings");
    const card = page.locator(role.card);
    const confirm = card.getByRole("button", {
      name: role.confirmLabel,
      exact: true,
    });
    await confirm.focus();
    await card.evaluate((element) => {
      window.__originalAccountCard = element;
    });
    await page.evaluate(async () => {
      window.dispatchEvent(new Event("focus"));
      await window.__settingsIdle();
      await window.__accountFixtureIdle();
    });
    await expect(confirm).toBeFocused();
    expect(
      await card.evaluate(
        (element) => element === window.__originalAccountCard,
      ),
    ).toBe(true);
  });

  for (const viewport of [
    { width: 408, height: 744 },
    { width: 408, height: 500 },
  ]) {
    test(`${provider}: compact native states and keyboard fit at ${viewport.width}x${viewport.height}`, async ({
      page,
    }, testInfo) => {
      await page.setViewportSize(viewport);
      await page.emulateMedia({ reducedMotion: "reduce" });
      const fixture = await syntheticAuth(page);
      await page.goto("/");
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      await expect(page.locator(".settings-overview")).toBeVisible();
      await section(page, provider === "github" ? "GitHub" : "GitHub Copilot");
      const card = page.locator(role.card);
      const action = card.getByRole("button", {
        name: role.connectLabel,
        exact: true,
      });
      await action.scrollIntoViewIfNeeded();
      await expect(action).toBeInViewport({ ratio: 1 });
      await page.screenshot({
        path: testInfo.outputPath(`${provider}-empty.png`),
      });
      await action.focus();
      await page.keyboard.press("Enter");
      await expect(card.getByRole("status")).toBeFocused();
      await expect(card).toContainText("TEST-CODE");
      const copy = card.getByRole("button", { name: "Copy code", exact: true });
      await copy.focus();
      await expect(copy).toBeInViewport({ ratio: 1 });
      await page.screenshot({
        path: testInfo.outputPath(`${provider}-waiting.png`),
      });
      const reads = fixture.calls.length;
      await expect.poll(() => fixture.calls.length).toBeGreaterThan(reads + 2);
      await expect(copy).toBeFocused();
      fixture.states[provider].flow = manualPending(provider);
      await refresh(page, provider);
      const confirm = card.getByRole("button", {
        name: role.confirmLabel,
        exact: true,
      });
      await confirm.focus();
      await expect(confirm).toBeInViewport({ ratio: 1 });
      await page.screenshot({
        path: testInfo.outputPath(`${provider}-confirmation.png`),
      });
      const bounds = await card.evaluate((element) => ({
        overflow: element.scrollWidth - element.clientWidth,
        font: getComputedStyle(element).fontFamily,
        background: getComputedStyle(element).backgroundColor,
        clipped: [...element.querySelectorAll("button, code")].some(
          (control) => {
            if (!control.getClientRects().length) return false;
            const card = element.getBoundingClientRect();
            const box = control.getBoundingClientRect();
            return (
              box.left < card.left ||
              box.right > card.right ||
              control.scrollWidth > control.clientWidth + 1
            );
          },
        ),
      }));
      expect(bounds).toMatchObject({
        overflow: 0,
        clipped: false,
        background: "rgb(255, 255, 255)",
      });
      expect(bounds.font).toContain("Segoe UI");
      const cancel = card.getByRole("button", {
        name: role.cancelLabel,
        exact: true,
      });
      await cancel.focus();
      await page.keyboard.press("Enter");
      await expect(action).toBeEnabled();
      await expect(card).toHaveAttribute("data-flow-state", "idle");
      // The empty idle status prose is CSS-hidden in the panel, which cannot
      // hold focus; when it is shown it must own focus after Cancel.
      const idleStatus = card.locator("[role=status]");
      if (await idleStatus.isVisible()) await expect(idleStatus).toBeFocused();
      await expect(card).not.toContainText("TEST-CODE");
      expect(effects(fixture.calls)).toEqual([
        { command: role.start, args: {} },
        { command: role.cancel, args: {} },
      ]);
      fixture.states[provider] = {
        accounts: [
          {
            ...identity(provider),
            state: "reconnect_required",
            reason: "expired",
          },
        ],
        flow: { state: "failed", reason: "wrong_identity" },
      };
      await refresh(page, provider);
      await card
        .getByRole("button", { name: role.reconnectLabel, exact: true })
        .focus();
      await page.screenshot({
        path: testInfo.outputPath(`${provider}-disconnected.png`),
      });
      // The sign-in state and the account row share one row inset.
      const insets = await card.evaluate((element) =>
        [".account-connection-state", ".github-account"].map((selector) => {
          const block = element.querySelector(selector);
          return (
            block.getBoundingClientRect().left +
            parseFloat(getComputedStyle(block).paddingLeft)
          );
        }),
      );
      expect(Math.abs(insets[0] - insets[1])).toBeLessThanOrEqual(1);
      const retryButton = card.locator(
        ".account-connection-state button:not(.primary):visible",
      );
      for (const button of await retryButton.all()) {
        const box = await button.boundingBox();
        expect(Math.abs(box.x - insets[1])).toBeLessThanOrEqual(1);
      }
    });
  }
}

for (const width of [320, 400, 408]) {
  for (const height of [300, 400, 439, 440, 441, 460, 499, 500, 501, 744]) {
    const capture =
      (width === 320 && height === 300) ||
      (width === 408 && [441, 744].includes(height));
    test(`all compact account controls fit the scroll viewport at ${width}x${height}`, async ({
      page,
      store,
      browserName,
    }, testInfo) => {
      const observations = [];
      geometryObservations.set(testInfo, observations);
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ reducedMotion: "reduce" });
      const fixture = await syntheticAuth(page);
      await page.goto("/");
      // At <=440px the tabs reflow as startup applies the Queue route, then
      // the empty-store Welcome. A click straddling that reflow is lost, so
      // press Settings only once the startup destination has settled.
      await expect(page.locator(".panel-shell")).toHaveAttribute(
        "data-setup",
        "true",
      );
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      await expect(page.locator(".settings-overview")).toBeVisible();
      const navigation = page.getByRole("button", {
        name: /^Back( to (AI Tooling|Git Repository))?$/,
      });
      await page.evaluate(() => window.__settingsIdle());
      const before = (await store("snapshot")).settings;
      for (const [provider, role] of Object.entries(roles)) {
        await section(
          page,
          provider === "github" ? "GitHub" : "GitHub Copilot",
        );
        const accounts = [
          identity(provider, "202"),
          {
            ...identity(provider),
            state: "reconnect_required",
            reason: "expired",
          },
        ];
        for (const [state, flow] of [
          ["saved", { state: "idle" }],
          ["requesting", { state: "connecting" }],
          ["waiting", connecting],
          ["confirmation", manualPending(provider)],
          ["failed", { state: "failed", reason: "wrong_identity" }],
        ]) {
          fixture.states[provider] = { accounts, flow };
          await refresh(page, provider);
          const card = page.locator(role.card);
          const controls = await card
            .locator("button:enabled, summary")
            .filter({ visible: true })
            .all();
          const tabStops = await page
            .locator("button, input, select, textarea, summary, a[href]")
            .count();
          // Bootstrap once, then use the host/browser's full-control Tab mode.
          await navigation.focus();
          for (const direction of ["Tab", "Shift+Tab"]) {
            const ordered =
              direction === "Tab" ? controls : controls.toReversed();
            for (const control of ordered) {
              for (let step = 0; step <= tabStops; step++) {
                if (
                  await control.evaluate(
                    (element) => element === document.activeElement,
                  )
                )
                  break;
                await page.keyboard.press(
                  accountTabKey(browserName, direction),
                );
              }
              await expect(control).toBeFocused();
              const bounds = await control.evaluate((element) => {
                const box = element.getBoundingClientRect();
                const content = document.querySelector("#content");
                let top = 0;
                let bottom = innerHeight;
                let left = 0;
                let right = innerWidth;
                for (
                  let ancestor = element.parentElement;
                  ancestor;
                  ancestor = ancestor.parentElement
                ) {
                  const style = getComputedStyle(ancestor);
                  const rect = ancestor.getBoundingClientRect();
                  if (style.overflowY !== "visible") {
                    top = Math.max(
                      top,
                      rect.top + parseFloat(style.borderTopWidth),
                    );
                    bottom = Math.min(
                      bottom,
                      rect.bottom - parseFloat(style.borderBottomWidth),
                    );
                  }
                  if (style.overflowX !== "visible") {
                    left = Math.max(
                      left,
                      rect.left + parseFloat(style.borderLeftWidth),
                    );
                    right = Math.min(
                      right,
                      rect.right - parseFloat(style.borderRightWidth),
                    );
                  }
                }
                const style = getComputedStyle(element);
                const card = element.closest(".account-connection");
                const cardBox = card.getBoundingClientRect();
                return {
                  label:
                    element.getAttribute("aria-label") || element.textContent,
                  control: box.toJSON(),
                  viewport: { top, bottom, left, right, height: bottom - top },
                  contentHeight: content.getBoundingClientRect().height,
                  fullyVisible:
                    box.top >= top &&
                    box.bottom <= bottom &&
                    box.left >= left &&
                    box.right <= right,
                  focusVisible: element.matches(":focus-visible"),
                  outlineWidth: style.outlineWidth,
                  outlineStyle: style.outlineStyle,
                  horizontallyClipped:
                    document.documentElement.scrollWidth > innerWidth ||
                    content.scrollWidth > content.clientWidth ||
                    card.scrollWidth > card.clientWidth ||
                    [...card.querySelectorAll("button, summary, code")].some(
                      (control) => {
                        if (!control.getClientRects().length) return false;
                        const rect = control.getBoundingClientRect();
                        return (
                          rect.left < cardBox.left ||
                          rect.right > cardBox.right ||
                          control.scrollWidth > control.clientWidth + 1
                        );
                      },
                    ),
                };
              });
              observations.push({ provider, state, direction, ...bounds });
              if (
                await control.evaluate(
                  (element) => element.tagName === "SUMMARY",
                )
              ) {
                await page.keyboard.press("Enter");
                if (direction === "Tab")
                  await expect(card.locator("details")).toHaveAttribute(
                    "open",
                    "",
                  );
                else
                  await expect(card.locator("details")).not.toHaveAttribute(
                    "open",
                    "",
                  );
              }
              if (
                capture &&
                direction === "Tab" &&
                bounds.label === role.confirmLabel
              )
                await page.screenshot({
                  path: testInfo.outputPath(
                    `${provider}-confirmation-focused.png`,
                  ),
                });
            }
            if (capture)
              await page.screenshot({
                path: testInfo.outputPath(
                  `${provider}-${state}-${direction === "Tab" ? "forward" : "reverse"}.png`,
                ),
              });
          }
          expect(fixture.states[provider].accounts).toEqual(accounts);
        }
      }
      expect(effects(fixture.calls)).toEqual([]);
      expect((await store("snapshot")).settings).toEqual(before);
      expect(
        observations.filter(
          (row) =>
            !row.fullyVisible ||
            !row.focusVisible ||
            row.outlineWidth !== "3px" ||
            row.outlineStyle !== "solid" ||
            row.horizontallyClipped,
        ),
      ).toEqual([]);
    });
  }
}

test("GitHub identity replacement and Copilot verification retain exact role-specific payloads", async ({
  page,
}) => {
  const fixture = await syntheticAuth(page, {
    github: { accounts: [], flow: manualPending("github") },
    copilot: idle([identity("copilot")]),
  });
  await page.goto("/?view=settings");
  await page
    .locator(roles.github.card)
    .getByRole("button", { name: "Use a different account", exact: true })
    .click();
  await page
    .locator(roles.copilot.card)
    .getByRole("button", {
      name: "Verify Copilot sign-in for fixture-copilot-101",
      exact: true,
    })
    .click();
  expect(effects(fixture.calls)).toEqual([
    { command: "start_github_browser_auth", args: { selectAccount: true } },
    { command: "verify_copilot_account", args: { accountId: "101" } },
  ]);
  expect(fixture.states.github.accounts).toEqual([]);
});

test("GitHub failed credential deletion stays visible and retries only the affected account", async ({
  page,
}) => {
  const fixture = await syntheticAuth(page, {
    github: idle([
      {
        ...identity("github"),
        state: "reconnect_required",
        reason: "disconnect_failed",
      },
      identity("github", "202"),
    ]),
  });
  await page.goto("/?view=settings");
  const card = page.locator(roles.github.card);
  await expect(card).toContainText("Disconnect failed");
  await card
    .getByRole("button", {
      name: "Retry disconnect fixture-github-101",
      exact: true,
    })
    .click();
  expect(effects(fixture.calls)).toEqual([
    { command: "disconnect_github_auth", args: { accountId: "101" } },
  ]);
  await expect(card.locator(".github-account")).toHaveCount(1);
  await expect(card).toContainText("fixture-github-202");
});
