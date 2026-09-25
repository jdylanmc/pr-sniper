import { test, expect } from "./fixtures.mjs";
import { section, closeDialog, saveChanges } from "./navigation.mjs";

const account = (id, login, state = "connected") => ({
  provider: "copilot",
  account_id: id,
  login,
  state,
  ...(state === "connected" ? {} : { reason: "disconnected" }),
});
const idle = (accounts = []) => ({ accounts, flow: { state: "idle" } });
const model = (id, name = id) => ({ id, name });
const agentId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const first = account("101", "fixture-ai-one");
const second = account("202", "fixture-ai-two");

async function bridge(page, handler) {
  await page.exposeFunction("__copilotFixture", handler);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command.includes("copilot"))
        return window.__copilotFixture(command, args ?? {});
      if (command === "github_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "github",
              account_id: "303",
              login: "fixture-repo",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        });
      return original(command, args);
    };
  });
}

test("Copilot connect, cancel, confirm, reconnect and disconnect stay separate from repository accounts", async ({
  page,
}) => {
  let state = idle();
  let next = first;
  await bridge(page, (command, args) => {
    if (command === "start_copilot_auth")
      state = {
        accounts: state.accounts,
        flow: {
          state: "connecting",
          user_code: "TEST-CODE",
          verification_uri: "https://github.com/login/device",
          expected_account_id: args.expectedAccountId,
        },
      };
    if (command === "copilot_auth_state" && state.flow.state === "connecting")
      state.flow = {
        state: "pending_account_confirmation",
        account_id: next.account_id,
        login: next.login,
      };
    if (command === "cancel_copilot_auth") state = idle(state.accounts);
    if (command === "confirm_copilot_account") {
      state = idle([
        ...state.accounts.filter((a) => a.account_id !== next.account_id),
        next,
      ]);
      next = second;
    }
    if (command === "disconnect_copilot_account") {
      state = idle(
        state.accounts.map((a) =>
          a.account_id === args.accountId
            ? account(a.account_id, a.login, "reconnect_required")
            : a,
        ),
      );
      next = first;
    }
    return state;
  });
  await page.goto("/?view=settings");
  const card = page.locator(".copilot-auth-card");
  await expect(card).toContainText("No Copilot accounts connected");
  await card
    .getByRole("button", { name: "Connect Copilot account", exact: true })
    .click();
  await expect(card).toContainText("TEST-CODE");
  await expect(card).toContainText("Confirm fixture-ai-one (101)");
  await card.getByRole("button", { name: "Cancel Copilot sign-in" }).click();
  await expect(card).not.toContainText("Signed in as");
  for (const login of [first.login, second.login]) {
    await card
      .getByRole("button", { name: "Connect Copilot account", exact: true })
      .click();
    await expect(card).toContainText(`Confirm ${login}`);
    await card
      .getByRole("button", { name: "Confirm Copilot account", exact: true })
      .click();
    await expect(card).toContainText(`Signed in as ${login}`);
  }
  await expect(page.locator(".github-auth-card")).toContainText(
    "fixture-repo (303)",
  );
  await card
    .getByRole("button", {
      name: "Disconnect Copilot fixture-ai-one",
      exact: true,
    })
    .click();
  await expect(card).toContainText("Disconnected. Reconnect");
  await expect(card).toContainText("Signed in as fixture-ai-two");
  await expect(page.locator(".github-auth-card")).toContainText(
    "Connected through",
  );
  await card
    .getByRole("button", {
      name: "Reconnect Copilot fixture-ai-one",
      exact: true,
    })
    .click();
  await expect(card).toContainText("Confirm fixture-ai-one");
  await card
    .getByRole("button", { name: "Confirm Copilot account", exact: true })
    .click();
  await page.reload();
  await expect(card).toContainText("Signed in as fixture-ai-one");
  await expect(card).toContainText("not a subscription, seat or model test");
  await expect(page.locator("body")).not.toContainText("access_token");
});

test("legacy Agent becomes explicitly account/model bound and survives a real storage restart", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.agents = [
    {
      id: agentId,
      name: "Legacy reviewer",
      model: "copilot",
      prompt: "Keep my prompt.",
      signature: "My signature",
    },
  ];
  settings.repositories = [
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      provider: "github",
      name: "fixture/repository",
      enabled: true,
      assignments: [
        {
          id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          agent_id: agentId,
          schedule: { kind: "interval", minutes: 30, timezone: "UTC" },
          comment: false,
          approve: false,
        },
      ],
    },
  ];
  await store("seed_settings", settings);
  let state = idle([first, second]);
  await bridge(page, (command, args) => {
    if (command === "list_copilot_models")
      return [model(`model-${args.accountId}`, "Claude through Copilot")];
    if (command === "disconnect_copilot_account")
      state = idle([account("101", first.login, "reconnect_required"), second]);
    return state;
  });
  await page.goto("/?view=settings");
  await section(page, "Agents");
  await expect(page.locator(".agent-card")).toContainText("Unconfigured");
  await page
    .locator(".agent-card")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Edit agent", exact: true });
  await expect(dialog.getByLabel("AI account", { exact: true })).toHaveValue(
    "",
  );
  await expect(dialog.getByLabel("Model", { exact: true })).toHaveValue(
    "copilot",
  );
  await dialog.getByLabel("AI account", { exact: true }).selectOption("101");
  await expect(dialog.getByLabel("Model", { exact: true })).toBeEnabled();
  await expect(dialog.getByLabel("Model", { exact: true })).toHaveValue("");
  await dialog.getByLabel("Model", { exact: true }).selectOption("model-101");
  await dialog.getByRole("button", { name: "Save agent", exact: true }).click();
  await saveChanges(page);
  const persisted = (await store("snapshot")).settings;
  expect(persisted.agents[0]).toMatchObject({
    ai_account: { provider: "copilot", account_id: "101" },
    model: "model-101",
    prompt: "Keep my prompt.",
    signature: "My signature",
  });
  expect(persisted.repositories).toEqual(settings.repositories);
  await section(page, "Integrations");
  await page
    .getByRole("button", {
      name: "Disconnect Copilot fixture-ai-one",
      exact: true,
    })
    .click();
  await page.reload();
  await section(page, "Agents");
  await expect(page.locator(".agent-card")).toContainText(
    "Reconnect required; Agent blocked",
  );
  expect((await store("snapshot")).settings.agents).toEqual(persisted.agents);
  expect((await store("snapshot")).settings.repositories).toEqual(
    settings.repositories,
  );
});

test("model failures, empty catalogs and policy denial stay honest and retryable without losing sign-in", async ({
  page,
}) => {
  let calls = 0;
  await bridge(page, (command) => {
    if (command === "list_copilot_models") {
      calls++;
      if (calls === 1)
        throw "Copilot model listing failed. Check network or organization policy.";
      if (calls === 2) return [];
      return [
        model("available", "Claude via Copilot"),
        { ...model("blocked"), policy: { state: "disabled" } },
      ];
    }
    return idle([first]);
  });
  await page.goto("/?view=settings");
  await section(page, "Agents");
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "New agent", exact: true });
  await dialog.getByLabel("Name", { exact: true }).fill("Explicit reviewer");
  await dialog.getByLabel("AI account", { exact: true }).selectOption("101");
  await expect(dialog.getByRole("status")).toContainText(
    "model listing failed",
  );
  await dialog.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText(
    "Choose a verified AI account",
  );
  await dialog.getByRole("button", { name: "Retry model list" }).click();
  await expect(dialog.getByRole("status")).toContainText("returned no models");
  await dialog.getByRole("button", { name: "Retry model list" }).click();
  await expect(dialog.getByLabel("Model", { exact: true })).toBeEnabled();
  await expect(dialog.locator('option[value="blocked"]')).toHaveJSProperty(
    "disabled",
    true,
  );
  await expect(dialog.getByLabel("Model", { exact: true })).toHaveValue("");
  await dialog.getByLabel("Model", { exact: true }).selectOption("available");
  await dialog.getByRole("button", { name: "Save agent", exact: true }).click();
  await section(page, "Integrations");
  await expect(page.locator(".copilot-auth-card")).toContainText(
    "Signed in as fixture-ai-one",
  );
  await expect(
    page.locator(".integration-card").filter({ hasText: "Direct Claude" }),
  ).toContainText("Coming soon");
});

test("late catalogs cannot overwrite another account selection and close cancels only its own lookup", async ({
  page,
}) => {
  const slow = Promise.withResolvers();
  const cancellations = [];
  const lookups = [];
  await bridge(page, (command, args) => {
    if (command === "list_copilot_models") {
      lookups.push(args);
      return args.accountId === "101" ? slow.promise : [model("second-model")];
    }
    if (command === "cancel_copilot_models") cancellations.push(args);
    return idle([first, second]);
  });
  await page.goto("/?view=settings");
  await section(page, "Agents");
  await page.getByRole("button", { name: "New agent", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "New agent", exact: true });
  await dialog.getByLabel("AI account", { exact: true }).selectOption("101");
  await expect.poll(() => lookups.length).toBe(1);
  await dialog.getByLabel("AI account", { exact: true }).selectOption("202");
  await expect(dialog.getByLabel("Model", { exact: true })).toBeEnabled();
  await dialog
    .getByLabel("Model", { exact: true })
    .selectOption("second-model");
  slow.resolve([model("late-first-model")]);
  await expect(dialog.getByLabel("Model", { exact: true })).toHaveValue(
    "second-model",
  );
  expect(cancellations[0]).toEqual(lookups[0]);
  await dialog.getByLabel("AI account", { exact: true }).selectOption("101");
  await closeDialog(page);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator(".agent-card")).toHaveCount(0);
});

test("no-account and secure-storage errors provide a recoverable connection route at narrow widths", async ({
  page,
}) => {
  let reads = 0;
  await bridge(page, (command) => {
    if (command === "copilot_auth_state" && ++reads === 1)
      throw "Secure storage unavailable. Retry.";
    return idle();
  });
  await page.setViewportSize({ width: 420, height: 820 });
  await page.goto("/?view=settings");
  const card = page.locator(".copilot-auth-card");
  await expect(card.getByRole("alert")).toContainText(
    "Secure storage unavailable",
  );
  await card
    .getByRole("button", { name: "Retry reading Copilot accounts" })
    .click();
  await expect(card).toContainText("No Copilot accounts connected");
  expect(
    await card.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await page
    .getByLabel("Settings section", { exact: true })
    .selectOption("agents");
  await expect(
    page.getByRole("button", { name: "New agent", exact: true }),
  ).toBeDisabled();
  await expect(page.locator("[data-copilot-status]")).toContainText(
    "Connect an account in Integrations",
  );
});

test("unavailable saved models and legacy unconfigured Agents survive unrelated edits without fallback", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.agents = [
    {
      id: agentId,
      name: "Pinned",
      model: "retired-model",
      ai_account: { provider: "copilot", account_id: "101" },
      prompt: "Retain prompt.",
      signature: "Fixture",
    },
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      name: "Legacy",
      model: "copilot",
      prompt: "Legacy prompt.",
      signature: "Fixture",
    },
  ];
  await store("seed_settings", settings);
  await bridge(page, (command) =>
    command === "list_copilot_models"
      ? [model("different-model")]
      : idle([first]),
  );
  await page.goto("/?view=settings");
  await section(page, "Agents");
  const pinned = page
    .locator(".agent-card")
    .filter({
      has: page.getByRole("heading", { name: "Pinned", exact: true }),
    });
  await pinned.getByRole("button", { name: "Edit", exact: true }).click();
  let dialog = page.getByRole("dialog", { name: "Edit agent", exact: true });
  await expect(dialog.getByRole("status")).toContainText(
    "saved model is unavailable",
  );
  await expect(dialog.getByLabel("Model", { exact: true })).toHaveValue(
    "retired-model",
  );
  await dialog.getByLabel("Name", { exact: true }).fill("Renamed pinned");
  await dialog.getByRole("button", { name: "Save agent", exact: true }).click();
  const legacy = page
    .locator(".agent-card")
    .filter({
      has: page.getByRole("heading", { name: "Legacy", exact: true }),
    });
  await legacy.getByRole("button", { name: "Edit", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "Edit agent", exact: true });
  await dialog.getByLabel("Signature", { exact: true }).fill("New signature");
  await dialog.getByRole("button", { name: "Save agent", exact: true }).click();
  await saveChanges(page);
  const actual = (await store("snapshot")).settings.agents;
  expect(actual[0]).toEqual({ ...settings.agents[0], name: "Renamed pinned" });
  expect(actual[1]).toEqual({
    ...settings.agents[1],
    signature: "New signature",
  });
});
