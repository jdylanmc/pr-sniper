import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

// Synthetic device flow mirroring the native GithubAuthView contract: start
// answers with the provider user code, and a completed sign-in for a saved
// identity becomes a pending confirmation that renews that same record.
// Only Queue persistence uses the production Store bridge.
const ACCOUNT = { account_id: "120949562", login: "fixture-reconnect" };

const reconnectRequired = (account = ACCOUNT) => ({
  provider: "github",
  state: "reconnect_required",
  reason: "expired",
  ...account,
});
const connected = (account = ACCOUNT) => ({
  provider: "github",
  state: "connected",
  connection_generation: 2,
  ...account,
});

async function deviceFlow(page, initial) {
  const native = {
    state: structuredClone(initial),
    signedInAs: ACCOUNT,
    completed: false,
    calls: [],
  };
  await page.exposeFunction("__githubReconnect", (command, args) => {
    native.calls.push({ command, args: args ?? null });
    const { state } = native;
    if (command === "start_github_browser_auth") {
      native.completed = false;
      state.flow = {
        state: "connecting",
        ...(args?.expectedAccountId
          ? { expected_account_id: args.expectedAccountId }
          : {}),
        user_code: "WXYZ-1234",
        verification_uri: "https://github.com/login/device",
      };
    }
    if (
      command === "github_auth_state" &&
      state.flow.state === "connecting" &&
      native.completed
    ) {
      const expected = state.flow.expected_account_id;
      state.flow =
        expected && expected !== native.signedInAs.account_id
          ? { state: "failed", reason: "wrong_identity" }
          : { state: "pending_account_confirmation", ...native.signedInAs };
    }
    if (command === "confirm_github_account") {
      if (state.flow.state !== "pending_account_confirmation")
        throw "No GitHub account is awaiting confirmation.";
      const renewed = {
        account_id: state.flow.account_id,
        login: state.flow.login,
      };
      state.accounts = [
        ...state.accounts.filter((a) => a.account_id !== renewed.account_id),
        connected(renewed),
      ];
      state.flow = { state: "idle" };
    }
    if (command === "cancel_github_auth") state.flow = { state: "idle" };
    return structuredClone(state);
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      [
        "github_auth_state",
        "start_github_browser_auth",
        "confirm_github_account",
        "cancel_github_auth",
      ].includes(command)
        ? window.__githubReconnect(command, args)
        : original(command, args);
  });
  return native;
}

const effects = (native) =>
  native.calls.filter(({ command }) => command !== "github_auth_state");

async function expectRenewedInPlace(card, native) {
  const row = card.locator(".github-account");
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-account-state", "connected");
  await expect(row).toContainText(`${ACCOUNT.login} (${ACCOUNT.account_id})`);
  await expect(
    card.getByRole("button", { name: `Reconnect ${ACCOUNT.login}` }),
  ).toHaveCount(0);
  await expect(
    card.getByRole("button", { name: "Try GitHub sign-in again" }),
  ).toHaveCount(0);
  await expect(
    card.getByRole("button", { name: `Disconnect ${ACCOUNT.login}` }),
  ).toBeEnabled();
  expect(
    effects(native).filter(
      ({ command }) => command === "confirm_github_account",
    ),
  ).toHaveLength(1);
}

test("Reconnect shows the device code from its own response and renews the same account in place", async ({
  page,
}, testInfo) => {
  const native = await deviceFlow(page, {
    accounts: [reconnectRequired()],
    flow: { state: "failed", reason: "timeout" },
  });
  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  await expect(card.locator(".github-account")).toHaveAttribute(
    "data-account-state",
    "reconnect_required",
  );
  await expect(card).toContainText("Reconnect required");

  await card
    .getByRole("button", { name: `Reconnect ${ACCOUNT.login}` })
    .click();
  // The code arrives with the start response; no state poll is needed.
  await expect(card.locator(".github-device-code")).toHaveText("WXYZ-1234");
  await expect(card).toContainText("github.com/login/device");
  await expect(card.getByRole("button", { name: "Copy code" })).toBeVisible();
  expect(effects(native)[0]).toEqual({
    command: "start_github_browser_auth",
    args: { expectedAccountId: ACCOUNT.account_id },
  });
  await page.screenshot({
    path: testInfo.outputPath("reconnect-code.png"),
  });

  native.completed = true;
  await expectRenewedInPlace(card, native);
  await expect(card.locator(".github-device-code")).toHaveCount(0);
  await expect(card.getByRole("status")).toHaveText("1 account");
  await page.screenshot({
    path: testInfo.outputPath("reconnect-renewed.png"),
  });
});

test("generic sign-in again for the same identity clears Reconnect required on that account", async ({
  page,
}, testInfo) => {
  const native = await deviceFlow(page, {
    accounts: [reconnectRequired()],
    flow: { state: "failed", reason: "timeout" },
  });
  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  await card.getByRole("button", { name: "Try GitHub sign-in again" }).click();
  await expect(card.locator(".github-device-code")).toHaveText("WXYZ-1234");
  expect(effects(native)[0].command).toBe("start_github_browser_auth");
  expect(effects(native)[0].args?.expectedAccountId).toBeUndefined();

  native.completed = true;
  await expectRenewedInPlace(card, native);
  await page.screenshot({
    path: testInfo.outputPath("sign-in-again-renewed.png"),
  });
});

test("a sign-in completed while the panel was hidden shows on return without reopening Settings", async ({
  page,
}) => {
  const native = await deviceFlow(page, {
    accounts: [reconnectRequired()],
    flow: { state: "idle" },
  });
  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  await expect(card).toContainText("Reconnect required");
  // Native state changes with no in-page action (e.g. completed and
  // confirmed while the panel was hidden by the browser hand-off).
  native.state = { accounts: [connected()], flow: { state: "idle" } };
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(card.locator(".github-account")).toHaveAttribute(
    "data-account-state",
    "connected",
  );
  await expect(
    card.getByRole("button", { name: `Reconnect ${ACCOUNT.login}` }),
  ).toHaveCount(0);
});

test("reconnecting the bound account resumes its parked credentials failure", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const parked = fixture.state.publications.find(
    (entry) => entry.review.job.number === 3,
  );
  parked.operation.failure = "credentials";
  await store("seed_queue_state", fixture.state);
  const bound = { account_id: "22", login: "local-operator" };
  const native = await deviceFlow(page, {
    accounts: [reconnectRequired(bound)],
    flow: { state: "idle" },
  });
  native.signedInAs = bound;

  await page.goto("/");
  await showQueueGroup(page, "attention");
  const failed = page.getByRole("article", {
    name: "example/repo #3",
    exact: true,
  });
  await expect(failed).toContainText("Credentials");
  await expect(page.locator("[data-queue-attention]")).toHaveText("2");

  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  await card.getByRole("button", { name: `Reconnect ${bound.login}` }).click();
  await expect(card.locator(".github-device-code")).toHaveText("WXYZ-1234");
  native.completed = true;
  await expect(card.locator(".github-account")).toHaveAttribute(
    "data-account-state",
    "connected",
  );

  // Native confirm hook: the parked credentials failure resumes without a
  // human Retry once the same account is reconnected.
  expect(await store("fixture_reconnect_account", { accountId: "22" })).toEqual(
    { before: 0, after: 1 },
  );
  await page.goto("/");
  await showQueueGroup(page, "attention");
  // The credentials park is cleared; this fixture's publication still has an
  // unresolved remote outcome, so it stays visible as a reconcile, not a Fix.
  await expect(failed).not.toContainText("Credentials");
  await expect(failed.locator("[data-fix-account]")).toHaveCount(0);
  await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
});
