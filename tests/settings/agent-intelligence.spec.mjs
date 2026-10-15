import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { copyDiagnostics, recordClipboard } from "./diagnostics-copy.mjs";

const capable = {
  id: "deep-model",
  name: "Deep model",
  capabilities: { limits: { max_output_tokens: 8000 } },
  supportedReasoningEfforts: ["low", "high"],
  defaultReasoningEffort: "low",
  supportedContextTiers: ["future-tier"],
  billing: {
    tokenPrices: {
      maxPromptTokens: 152000,
      longContext: { maxPromptTokens: 992000 },
    },
  },
};
const plain = {
  id: "plain-model",
  name: "Plain model",
  capabilities: { limits: { max_context_window_tokens: 64000 } },
};

async function provider(page, models = () => [capable, plain]) {
  await page.exposeFunction("__intelligenceModels", models);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "copilot_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "copilot",
              account_id: "101",
              login: "fixture-101",
              state: "connected",
            },
            {
              provider: "copilot",
              account_id: "202",
              login: "fixture-202",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        });
      if (command === "list_copilot_models")
        return window.__intelligenceModels(args.accountId);
      if (command === "cancel_copilot_models") return Promise.resolve();
      return original(command, args);
    };
  });
}

async function editor(page, existing = false) {
  await section(page, "Agents");
  const name = existing === true ? "Intelligence fixture" : existing;
  if (existing)
    await page
      .locator(".agent-card")
      .filter({ has: page.getByRole("heading", { name, exact: true }) })
      .locator("[data-edit]")
      .click({ force: true });
  else
    await page.getByRole("button", { name: "New agent", exact: true }).click();
  return page.getByRole("dialog", {
    name: existing ? `Editing ${name}` : "New agent",
    exact: true,
  });
}

async function create(page, model = "deep-model") {
  const modal = await editor(page);
  await modal.getByLabel("Name", { exact: true }).fill("Intelligence fixture");
  await modal.getByLabel("AI account", { exact: true }).selectOption("101");
  await expect(modal.getByLabel("Model", { exact: true })).toBeEnabled();
  await modal.getByLabel("Model", { exact: true }).selectOption(model);
  return modal;
}

test("advertised efforts and priced context capacities save through the real Agent resource and restart", async ({
  page,
  store,
}, testInfo) => {
  await provider(page);
  await page.setViewportSize({ width: 408, height: 744 });
  await page.goto("/?view=settings");
  let modal = await create(page);
  await expect(modal.getByLabel("Reasoning effort")).toContainText(
    "Provider default (low)",
  );
  await expect(modal.getByLabel("Context window")).toContainText(
    "Standard (160,000 tokens)",
  );
  await expect(modal.getByLabel("Context window")).toContainText(
    "Long context (1,000,000 tokens)",
  );
  await expect(
    modal.locator("[name=context-tier] option[value=future-tier]"),
  ).toHaveJSProperty("disabled", true);
  await expect(
    modal.locator("[name=context-tier] option[value=future-tier]"),
  ).toHaveText("future-tier - unsupported");
  await modal.getByLabel("Reasoning effort").selectOption("high");
  await modal.getByLabel("Context window").selectOption("long_context");
  await page.screenshot({
    path: testInfo.outputPath("agent-intelligence.png"),
  });
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].intelligence).toEqual({
    reasoning_effort: "high",
    context_tier: "long_context",
  });
  await page.reload();
  modal = await editor(page, true);
  await expect(modal.getByLabel("Reasoning effort")).toHaveValue("high");
  await expect(modal.getByLabel("Context window")).toHaveValue("long_context");
  await modal.getByLabel("Reasoning effort").selectOption("");
  await modal.getByLabel("Context window").selectOption("");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  expect((await store("snapshot")).settings.agents[0].intelligence).toEqual({
    reasoning_effort: null,
    context_tier: null,
  });
});

test("models without advertised choices explain disabled selectors and save explicit Provider defaults", async ({
  page,
  store,
}) => {
  await provider(page);
  await page.goto("/?view=settings");
  const modal = await create(page, "plain-model");
  await expect(modal.getByLabel("Reasoning effort")).toBeDisabled();
  await expect(modal.getByLabel("Context window")).toBeDisabled();
  await expect(modal.locator("[name=context-tier] option")).toHaveCount(1);
  await expect(modal.locator("[data-intelligence-status]")).toContainText(
    "Max 64,000 tokens",
  );
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  expect((await store("snapshot")).settings.agents[0].intelligence).toEqual({
    reasoning_effort: null,
    context_tier: null,
  });
});

test("retained compact Intelligence controls support keyboard focus, reduced motion and high contrast", async ({
  page,
  store,
}, testInfo) => {
  await provider(page);
  await page.setViewportSize({ width: 408, height: 744 });
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  const modal = await create(page);
  await modal.getByLabel("Reasoning effort").selectOption("high");
  await modal.getByLabel("Context window").selectOption("long_context");
  await modal.getByLabel("Context window").focus();
  await page.keyboard.press("Tab");
  await expect(modal.getByLabel("Reasoning effort")).toBeFocused();
  await page.screenshot({
    path: testInfo.outputPath("retained-agent-intelligence.png"),
  });
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });
  await page.setViewportSize({ width: 320, height: 300 });
  const save = modal.getByRole("button", { name: "Save agent", exact: true });
  await save.scrollIntoViewIfNeeded();
  await save.focus();
  await expect(save).toBeFocused();
  await page.screenshot({
    path: testInfo.outputPath("intelligence-high-contrast-tiny.png"),
  });
  await page.keyboard.press("Enter");
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].intelligence).toEqual({
    reasoning_effort: "high",
    context_tier: "long_context",
  });
});

test("account and model changes retain deliberate choices and require explicit repair, not silent resetting", async ({
  page,
  store,
}) => {
  await provider(page, (id) => (id === "101" ? [capable, plain] : [plain]));
  await page.goto("/?view=settings");
  const modal = await create(page);
  await modal.getByLabel("Reasoning effort").selectOption("high");
  await modal.getByLabel("Context window").selectOption("long_context");
  await modal.getByLabel("Model", { exact: true }).selectOption("plain-model");
  await expect(modal.getByLabel("Reasoning effort")).toHaveValue("high");
  await expect(modal.getByLabel("Context window")).toHaveValue("long_context");
  await expect(modal.locator("[data-intelligence-status]")).toContainText(
    "Effort unsupported",
  );
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toContainText("Effort unsupported");
  expect((await store("snapshot")).settings.agents ?? []).toEqual([]);
  await modal.getByLabel("Model", { exact: true }).selectOption("deep-model");
  await expect(modal.getByLabel("Reasoning effort")).toHaveValue("high");
  await modal.getByLabel("AI account", { exact: true }).selectOption("202");
  await expect(modal.getByLabel("Model", { exact: true })).toBeEnabled();
  await modal.getByLabel("Model", { exact: true }).selectOption("plain-model");
  await expect(modal.getByLabel("Context window")).toHaveValue("long_context");
  await expect(modal.locator("[data-intelligence-status]")).toContainText(
    "Effort unsupported",
  );
  await modal.getByLabel("Reasoning effort").selectOption("");
  await modal.getByLabel("Context window").selectOption("");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  const saved = (await store("snapshot")).settings.agents[0];
  expect(saved.ai_account.account_id).toBe("202");
  expect(saved.intelligence).toEqual({
    reasoning_effort: null,
    context_tier: null,
  });
});

test("failed discovery keeps saved choices and validity distinct; retry exposes actual incompatibility", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.agents = [
    {
      id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      name: "Retained",
      ai_account: { provider: "copilot", account_id: "101" },
      model: "deep-model",
      intelligence: { reasoning_effort: "high", context_tier: "long_context" },
      prompt: "Review correctness.",
      signature: "Fixture",
    },
  ];
  await store("seed_settings", settings);
  let fail = true;
  let models = [{ ...plain, id: "deep-model" }];
  await provider(page, () => {
    if (fail) throw "Fixture model transport is unavailable.";
    return models;
  });
  await page.goto("/?view=settings");
  let modal = await editor(page, "Retained");
  await expect(modal.locator("[data-model-status]")).toContainText(
    "Fixture model transport is unavailable",
  );
  await expect(modal.locator("[data-intelligence-status]")).toContainText(
    "Unavailable",
  );
  await expect(modal.getByLabel("Reasoning effort")).toHaveValue("high");
  await modal.getByLabel("Name", { exact: true }).fill("Retained renamed");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  expect((await store("snapshot")).settings.agents[0].intelligence).toEqual(
    settings.agents[0].intelligence,
  );
  modal = await editor(page, "Retained renamed");
  fail = false;
  await modal
    .getByRole("button", { name: "Refresh model list", exact: true })
    .click();
  await expect(modal.locator("[data-intelligence-status]")).toContainText(
    "Effort unsupported",
  );
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toContainText("Effort unsupported");
  models = [];
  await modal
    .getByRole("button", { name: "Refresh model list", exact: true })
    .click();
  await expect(modal.locator("[data-model-status]")).toContainText("No models");
  await expect(modal.locator("[data-intelligence-status]")).toContainText(
    "Choose a model",
  );
  await modal
    .getByLabel("Name", { exact: true })
    .fill("Unavailable model retained");
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].intelligence).toEqual(
    settings.agents[0].intelligence,
  );
});

test("job details retain captured requested and actual intelligence after later Agent edits; legacy remains unavailable", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const review = fixture.review(9);
  review.selection.agent.intelligence = {
    reasoning_effort: "high",
    context_tier: "long_context",
  };
  review.result.intelligence = {
    reasoning_effort: "high",
    context_tier: "long_context",
  };
  await store("seed_queue_state", {
    jobs: [review.job],
    reviews: [review],
    publications: [],
    follow_ups: [],
  });
  const settings = (await store("snapshot")).settings;
  const changed = {
    ...settings.agents[0],
    model: "later-model",
    intelligence: { reasoning_effort: "low", context_tier: "default" },
  };
  await store("save_resource", {
    edit: {
      kind: "agent",
      id: changed.id,
      expected: settings.agents[0],
      value: changed,
    },
  });
  await store("panel_navigate", {
    route: {
      tab: "running",
      detail: { type: "job", kind: "normal", id: review.key },
    },
  });
  await page.goto("/");
  await expect(page.getByText(/Session session-9/)).toContainText(
    "Effort: high; context: long_context",
  );
  const captured = page
    .locator(".work-configuration")
    .filter({ hasText: "high" })
    .first();
  await captured.locator("summary").first().click();
  await expect(captured).toContainText("Requested reasoning effort");
  await expect(captured).toContainText("long_context");
  await expect(captured).not.toContainText("later-model");
  await recordClipboard(page);
  await page.goto("/?view=settings");
  let diagnostics = await copyDiagnostics(page);
  expect(diagnostics).toContain(
    "Actual Intelligence: Effort: high; context: long_context",
  );
  expect(diagnostics).not.toContain("later-model");
  await store("panel_navigate", { route: { tab: "settings" } });
  await page.goto("/");
  diagnostics = await copyDiagnostics(page, { panel: true });
  expect(diagnostics).toContain(
    "Actual Intelligence: Effort: high; context: long_context",
  );
  expect(diagnostics).not.toContain("later-model");
  delete review.selection.agent.intelligence;
  delete review.result.intelligence;
  await store("seed_queue_state", {
    jobs: [review.job],
    reviews: [review],
    publications: [],
    follow_ups: [],
  });
  await store("panel_navigate", {
    route: {
      tab: "running",
      detail: { type: "job", kind: "normal", id: review.key },
    },
  });
  await page.reload();
  await expect(page.getByText(/Session session-9/)).toContainText(
    "Not recorded",
  );
  const legacy = page
    .locator(".work-configuration")
    .filter({ hasText: "Legacy snapshot" })
    .first();
  await legacy.locator("summary").first().click();
  await expect(legacy).toContainText(
    /Requested reasoning effort\s*Not recorded.*Requested context window\s*Not recorded/s,
  );
});
