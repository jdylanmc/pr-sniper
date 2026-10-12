import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test("queue item detail leads with status and one primary action; prose is gone", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const items = (await store("monitoring_snapshot")).items;
  const item = items.find((entry) => entry.job.number === 1);
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  const status = page.locator("[data-queue-status]");
  await expect(status).toHaveCount(1);
  await expect(status.locator("[data-queue-primary]")).toHaveCount(1);
  await expect(
    status.getByRole("button", { name: "Open PR on GitHub" }),
  ).toBeVisible();
  const evidence = page.locator("[data-item-evidence]");
  for (const prose of [
    "Personal review, comments and approval happen on GitHub",
    "Review eligibility",
    "Check schedule health",
    "PR identity and provenance",
  ])
    await expect(page.locator("body")).not.toContainText(prose);
  await expect(evidence.locator("details")).toHaveCount(0);
  await expect(page.locator("[data-queue-details]")).not.toHaveAttribute(
    "open",
  );
});

test("details disclosure is collapsed by default in the panel", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 1,
  );
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  const details = page.locator("[data-queue-details]");
  await expect(details).toHaveCount(1);
  await expect(details).not.toHaveAttribute("open");
});

async function parkedReview(store, patch) {
  const fixture = await queueFixture(store);
  const run = fixture.state.reviews.find((r) => r.job.number === 1);
  const now = Math.floor(Date.now() / 1000);
  Object.assign(run.operation, {
    state: "manual_retry",
    failure: "provider",
    attempt_count: 3,
    initial_attempt_at: now - 12 * 60,
    next_attempt_at: null,
    resume_cycle: 2,
    resume_at: now + 25 * 60,
    ...patch,
  });
  run.result = null;
  run.error =
    "Copilot did not read every changed file; no review result was accepted.";
  fixture.state.publications = fixture.state.publications.filter(
    (p) => p.review.job.number !== 1,
  );
  await store("seed_queue_state", fixture.state);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 1,
  );
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  return item;
}

test("failed detail lists terse facts, the raw error and the retry hint", async ({
  page,
  store,
}) => {
  const item = await parkedReview(store, {});
  expect(item.state).toBe("failed");
  expect(item.resume_at).toBeGreaterThan(Date.now() / 1000);
  await page.goto("/");
  const facts = page.locator("[data-queue-status] .queue-facts");
  await expect(facts).toBeVisible();
  const row = (label) => facts.locator(`dt:text-is("${label}") + dd`);
  await expect(row("Cause")).toHaveText("Failed / provider");
  await expect(row("Attempts")).toHaveText("3 of 3 \u00b7 cycle 2 of 5");
  await expect(row("Last attempt")).toHaveText(/^1[12]m ago$/);
  await expect(row("Next retry")).toHaveText(/^2[45]m$/);
  await expect(row("Next retry").locator("svg")).toHaveCount(1);
  await expect(row("Agent")).not.toHaveText("");
  await expect(row("Revision")).toHaveText(/^[0-9a-f]{7}$/);
  await expect(row("Account")).not.toHaveText("");
  const error = page.locator("[data-queue-error]");
  await expect(error).toContainText("did not read every changed file");
  await expect(error).toHaveAttribute(
    "title",
    /did not read every changed file/,
  );
  await expect(page.locator("[data-queue-details]")).not.toHaveAttribute(
    "open",
  );
  await expect(
    page.locator("[data-queue-details] > summary[aria-label=Details] svg"),
  ).toHaveCount(1);
  await expect(page.locator("[data-queue-primary]")).toHaveText("Retry");
});

test("failed detail shows Manual when no automatic retry is scheduled", async ({
  page,
  store,
}) => {
  await parkedReview(store, {
    failure: "permanent",
    state: "failed",
    resume_cycle: 0,
    resume_at: null,
    attempt_count: 1,
  });
  await page.goto("/");
  const facts = page.locator("[data-queue-status] .queue-facts");
  await expect(facts.locator('dt:text-is("Next retry") + dd')).toHaveText(
    "Manual",
  );
  await expect(facts.locator('dt:text-is("Attempts") + dd')).toHaveText(
    "1 of 3",
  );
});

test("Needs attention rows show a compact retry clock for parked failures", async ({
  page,
  store,
}) => {
  await parkedReview(store, {});
  await page.goto("/");
  await expect(async () => {
    await page
      .locator("[data-queue-filter=attention]")
      .evaluate((element) => element.click());
    await expect(page.locator('[data-queue-group="attention"]')).toBeVisible({
      timeout: 1000,
    });
  }).toPass();
  const hint = page.locator("[data-queue-resume]");
  await expect(hint).toHaveCount(1);
  await expect(hint).toHaveText(/^2[45]m$/);
  await expect(hint.locator("svg")).toHaveCount(1);
});

test("monitoring failures show the stored error line and a readable cause", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const run = fixture.state.reviews.find((r) => r.job.number === 1);
  const now = Math.floor(Date.now() / 1000);
  fixture.state.monitoring = {
    health: {
      [run.job.configuration_id]: {
        repository_id: run.job.configuration_id,
        name: "example/repo",
        schedule_key: "fixture",
        provider_account_id: "22",
        account_login: "local-operator",
        provider_repository_id: "100",
        enabled: true,
        last_attempt: now - 5 * 60,
        last_success: null,
        next_run: 0,
        schedule_available: true,
        last_failure: "conversation_admission",
        last_error: "Could not write conversation state; retry the check.",
        in_flight: false,
        operation: {
          id: "health-op",
          provider: "github",
          account_id: "22",
          configuration_id: run.job.configuration_id,
          repository_id: "100",
          pull_request_id: null,
          head_sha: null,
          trigger_policy: "fixture",
          operation_type: "repository_poll",
          state: "manual_retry",
          attempt_count: 3,
          initial_attempt_at: now - 20 * 60,
          retry_deadline: now - 5 * 60,
          next_attempt_at: null,
          failure: "provider",
          attempted_mutation: null,
          pending_review_id: null,
          owned_thread_id: null,
          triggering_external_comment_id: null,
          confirmed_receipt: null,
          resume_at: now + 30 * 60,
        },
      },
    },
  };
  await store("seed_queue_state", fixture.state);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 1,
  );
  expect(item.state).toBe("failed");
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  await expect(page.locator(".queue-status-label strong")).toHaveText(
    "Reply admission failed",
  );
  await expect(page.locator("[data-queue-error]")).toHaveText(
    "Could not write conversation state; retry the check.",
  );
  await expect(
    page.locator('.queue-facts dt:text-is("Last attempt") + dd'),
  ).toHaveText("5m ago");
});

test("Retry on a repository poll failure retries the poll, never the published review", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__commands = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      window.__commands.push(command);
      return original(command, args);
    };
  });
  const fixture = await queueFixture(store);
  const run = fixture.state.reviews.find((r) => r.job.number === 1);
  const now = Math.floor(Date.now() / 1000);
  fixture.state.monitoring = {
    health: {
      [run.job.configuration_id]: {
        repository_id: run.job.configuration_id,
        name: "example/repo",
        schedule_key: "fixture",
        provider_account_id: "22",
        account_login: "local-operator",
        provider_repository_id: "100",
        enabled: true,
        last_attempt: now - 5 * 60,
        last_success: null,
        next_run: 0,
        schedule_available: true,
        last_failure: "conversation_admission",
        last_error: "Could not write conversation state; retry the check.",
        in_flight: false,
        operation: {
          id: "health-op",
          provider: "github",
          account_id: "22",
          configuration_id: run.job.configuration_id,
          repository_id: "100",
          pull_request_id: null,
          head_sha: null,
          trigger_policy: "fixture",
          operation_type: "repository_poll",
          state: "manual_retry",
          attempt_count: 3,
          initial_attempt_at: now - 20 * 60,
          retry_deadline: now - 5 * 60,
          next_attempt_at: null,
          failure: "provider",
          attempted_mutation: null,
          pending_review_id: null,
          owned_thread_id: null,
          triggering_external_comment_id: null,
          confirmed_receipt: null,
          resume_at: now + 30 * 60,
        },
      },
    },
  };
  await store("seed_queue_state", fixture.state);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 1,
  );
  expect(item.state).toBe("failed");
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  await page.locator('[data-queue-primary="retry"]').click();
  await expect
    .poll(() => page.evaluate(() => window.__commands))
    .toContain("retry_monitoring_operation");
  const commands = await page.evaluate(() => window.__commands);
  expect(commands).not.toContain("start_review");
  await expect(page.locator("[data-panel-error]")).toBeHidden();
});

test("Retry restarts the poll and each failed review, once per command", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__commands = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      window.__commands.push(command);
      return original(command, args);
    };
  });
  const fixture = await queueFixture(store);
  const run = fixture.state.reviews.find((r) => r.job.number === 1);
  const now = Math.floor(Date.now() / 1000);
  Object.assign(run.operation, {
    state: "manual_retry",
    failure: "network",
    attempt_count: 3,
    initial_attempt_at: now - 12 * 60,
    next_attempt_at: null,
  });
  run.result = null;
  run.error = "Review retry budget exhausted. Manual retry is required.";
  fixture.state.publications = fixture.state.publications.filter(
    (p) => p.review.job.number !== 1,
  );
  fixture.state.monitoring = {
    health: {
      [run.job.configuration_id]: {
        repository_id: run.job.configuration_id,
        name: "example/repo",
        schedule_key: "fixture",
        provider_account_id: "22",
        account_login: "local-operator",
        provider_repository_id: "100",
        enabled: true,
        last_attempt: now - 5 * 60,
        last_success: null,
        next_run: 0,
        schedule_available: true,
        last_failure: "network",
        in_flight: false,
        operation: {
          id: "health-op",
          provider: "github",
          account_id: "22",
          configuration_id: run.job.configuration_id,
          repository_id: "100",
          pull_request_id: null,
          head_sha: null,
          trigger_policy: "fixture",
          operation_type: "repository_poll",
          state: "manual_retry",
          attempt_count: 3,
          initial_attempt_at: now - 20 * 60,
          retry_deadline: now - 5 * 60,
          next_attempt_at: null,
          failure: "network",
          attempted_mutation: null,
          pending_review_id: null,
          owned_thread_id: null,
          triggering_external_comment_id: null,
          confirmed_receipt: null,
        },
      },
    },
  };
  await store("seed_queue_state", fixture.state);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 1,
  );
  expect(item.state).toBe("failed");
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  await page.locator('[data-queue-primary="retry"]').click();
  await expect
    .poll(() => page.evaluate(() => window.__commands))
    .toContain("start_review");
  const commands = await page.evaluate(() => window.__commands);
  expect(
    commands.filter((c) => c === "retry_monitoring_operation"),
  ).toHaveLength(1);
  expect(commands.indexOf("retry_monitoring_operation")).toBeLessThan(
    commands.indexOf("start_review"),
  );
});

test("action errors are terse, dismissible toasts that never cover the header", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__commands = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "retry_monitoring_operation")
        return Promise.reject(
          "Reconcile the original publication before reviewing this revision again; a confirmed published batch cannot be replaced.",
        );
      return original(command, args);
    };
  });
  const fixture = await queueFixture(store);
  const run = fixture.state.reviews.find((r) => r.job.number === 1);
  const now = Math.floor(Date.now() / 1000);
  fixture.state.monitoring = {
    health: {
      [run.job.configuration_id]: {
        repository_id: run.job.configuration_id,
        name: "example/repo",
        schedule_key: "fixture",
        provider_account_id: "22",
        account_login: "local-operator",
        provider_repository_id: "100",
        enabled: true,
        last_attempt: now - 5 * 60,
        last_success: null,
        next_run: 0,
        schedule_available: true,
        last_failure: "conversation_admission",
        last_error: "Could not write conversation state; retry the check.",
        in_flight: false,
        operation: {
          id: "health-op",
          provider: "github",
          account_id: "22",
          configuration_id: run.job.configuration_id,
          repository_id: "100",
          pull_request_id: null,
          head_sha: null,
          trigger_policy: "fixture",
          operation_type: "repository_poll",
          state: "manual_retry",
          attempt_count: 3,
          initial_attempt_at: now - 20 * 60,
          retry_deadline: now - 5 * 60,
          next_attempt_at: null,
          failure: "provider",
          attempted_mutation: null,
          pending_review_id: null,
          owned_thread_id: null,
          triggering_external_comment_id: null,
          confirmed_receipt: null,
          resume_at: now + 30 * 60,
        },
      },
    },
  };
  await store("seed_queue_state", fixture.state);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 1,
  );
  expect(item.state).toBe("failed");
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  await page.locator('[data-queue-primary="retry"]').click();
  const toast = page.locator("[data-panel-error]");
  await expect(toast).toHaveText("Already reviewed");
  await expect(toast).toHaveAttribute("title", /Reconcile the original/);
  const box = await toast.boundingBox();
  const viewport = page.viewportSize();
  expect(box.y).toBeGreaterThan(viewport.height / 2);
  await toast.click();
  await expect(toast).toBeHidden();
});

async function parkedPublication(page, store, patch) {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__commands = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      window.__commands.push([command, args]);
      return original(command, args);
    };
  });
  const fixture = await queueFixture(store);
  const batch = fixture.state.publications.find(
    (p) => p.review.job.number === 3,
  );
  Object.assign(batch, patch.publication);
  Object.assign(batch.operation, patch.operation);
  await store("seed_queue_state", fixture.state);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 3,
  );
  expect(item.state).toBe("failed");
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  return item;
}

test("Retry on an unverified publication reconciles it, never a plain check", async ({
  page,
  store,
}) => {
  const now = Math.floor(Date.now() / 1000);
  const item = await parkedPublication(page, store, {
    publication: {},
    operation: {
      failure: null,
      attempt_count: 4,
      initial_attempt_at: now - 600,
      resume_cycle: 1,
      resume_at: now + 3600,
    },
  });
  expect(item.failure_cause).toBe("publication_unresolved");
  await expect(page.locator(".queue-status-label strong")).toHaveText(
    "Unverified",
  );
  await expect(page.locator("[data-queue-error]")).toContainText(
    "Provider response was lost.",
  );
  await expect(
    page.locator('.queue-facts dt:text-is("Attempts") + dd'),
  ).toHaveText("4 of 3 \u00b7 cycle 1 of 5");
  await page.locator('[data-queue-primary="retry"]').click();
  await expect
    .poll(() =>
      page.evaluate(() => window.__commands.map(([command]) => command)),
    )
    .toContain("publish_review");
  const commands = await page.evaluate(() => window.__commands);
  expect(commands).toContainEqual([
    "publish_review",
    { reviewOperationId: "review-3" },
  ]);
  const names = commands.map(([command]) => command);
  expect(names).not.toContain("check_now");
  expect(names).not.toContain("start_review");
});

test("a failed publication shows Publish failed, its error line and retries by publishing", async ({
  page,
  store,
}) => {
  await parkedPublication(page, store, {
    publication: {
      phase: "pending",
      uncertain: false,
      error: "GitHub rejected this review batch.",
      receipts: [{ review_id: "7", state: "pending", comment_ids: [] }],
    },
    operation: { state: "failed", failure: "permanent", attempt_count: 1 },
  });
  await expect(page.locator(".queue-status-label strong")).toHaveText(
    "Publish failed",
  );
  await expect(page.locator("[data-queue-error]")).toHaveText(
    "GitHub rejected this review batch.",
  );
  await expect(
    page.locator(".queue-facts dt:text-is('Next retry') + dd"),
  ).toHaveText("Manual");
  await page.locator('[data-queue-primary="retry"]').click();
  await expect
    .poll(() =>
      page.evaluate(() => window.__commands.map(([command]) => command)),
    )
    .toContain("publish_review");
  const names = await page.evaluate(() =>
    window.__commands.map(([command]) => command),
  );
  expect(names).not.toContain("check_now");
});

test("a discarded publication leaves Needs attention", async ({ store }) => {
  const fixture = await queueFixture(store);
  const batch = fixture.state.publications.find(
    (p) => p.review.job.number === 3,
  );
  Object.assign(batch, {
    phase: "discarded",
    uncertain: false,
    error: null,
    receipts: [{ review_id: "7", state: "deleted", comment_ids: [] }],
  });
  batch.operation.state = "completed";
  await store("seed_queue_state", fixture.state);
  const items = (await store("monitoring_snapshot")).items;
  const item = items.find((entry) => entry.job.number === 3);
  expect(item.state).toBe("machine_signed_off");
  expect(item.failure_cause ?? null).toBeNull();
});
