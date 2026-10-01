import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

async function actionFixture(store) {
  const fixture = await queueFixture(store);
  const review = fixture.review(9);
  const repository = fixture.settings.repositories[0];
  repository.assignments[0].actions = { approve: true, merge: true };
  review.job.work = {
    id: review.key,
    item_id: "action-iteration",
    iteration_id: "iteration-1",
    iteration: 1,
    agent_id: fixture.settings.agents[0].id,
    enqueue_order: 1,
    pass_ordinal: 1,
    trigger: "admission",
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
  };
  const state = {
    jobs: [review.job],
    reviews: [review],
    publications: [],
    follow_ups: [],
    tracked: [
      {
        provider: "github",
        configuration_id: repository.id,
        account_id: "22",
        repository_id: "100",
        pull_request_id: "9",
        number: 9,
        head_sha: review.job.head_sha,
        lifecycle: "open",
        iteration_id: "iteration-1",
        item_id: "action-iteration",
        iteration: 1,
        admission: review.job.work.admission,
        admitted_at: 100,
        observed_at: 100,
      },
    ],
    monitoring: {
      activations: {
        [repository.id]: {
          version: "fixture-scope",
          repository_id: repository.id,
          name: repository.name,
          account_id: "22",
          provider_repository_id: "100",
          trigger_policy: review.job.trigger_policy,
          creation_watermark: 0,
          mode: "new_only",
          selected_existing: 0,
          baseline: {},
          confirmed_at: 100,
        },
      },
    },
  };
  await store("seed_settings", fixture.settings);
  await store("seed_queue_state", state);
  const observation = {
    write_capability: true,
    node_id: "PR_node",
    repository_id: "100",
    pull_request_id: "9",
    account_id: "22",
    author_id: "11",
    head_repository_id: "100",
    head: review.job.head_sha,
    base: review.result.reviewed_base_sha,
    state: "OPEN",
    draft: false,
    permission: "WRITE",
    base_name: "main",
    merge_rules: [],
    merge_rules_error: null,
    mergeable: "MERGEABLE",
    merge_state: "CLEAN",
    review_decision: "APPROVED",
    checks: "SUCCESS",
    check_contexts: [],
    in_merge_queue: false,
    method: "SQUASH",
    protection: null,
    threads: [],
    reviews: [],
    comments: [],
    merged_by: null,
    merged_at: null,
    merge_commit: null,
  };
  const actions = await store("seed_action_observation", {
    itemId: "action-iteration",
    observation,
  });
  return { ...fixture, state, observation, actions, review };
}

test("panel primary-final route exposes only that final and returns to its exact Reviewed row", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  const final = fixture.actions.finals[0];
  final.execution.result = fixture.review.result;
  final.execution.operation.state = "completed";
  final.execution.phase = "Primary final full review complete";
  fixture.state.actions = fixture.actions;
  await store("seed_queue_state", fixture.state);
  const before = await store("monitoring_snapshot");
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Reviewed", exact: true })
    .click();
  const row = page
    .locator('[data-panel-view="reviewed"] article')
    .filter({ hasText: "Primary final review" });
  await row.getByRole("button", { name: "Open job", exact: true }).click();
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "Primary final full review complete",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  await expect(page.locator("#thread-follow-ups article")).toHaveCount(0);
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(
    row.getByRole("button", { name: "Open job", exact: true }),
  ).toBeFocused();
  expect((await store("monitoring_snapshot")).items[0].action_status).toEqual(
    before.items[0].action_status,
  );
});

test("current normal clearance hands off personally and exposes a distinct durable final request", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  expect(fixture.actions.finals).toHaveLength(1);
  expect(fixture.actions.effects).toHaveLength(0);
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue")).toContainText(
    "Ready for your final review",
  );
  await expect(page.locator("#handoff-queue")).toContainText(
    "Personal review: Required",
  );
  await expect(page.locator("#handoff-queue")).toContainText(
    "Primary final full review",
  );
  await page
    .getByRole("button", { name: "Start / retry final full review" })
    .click();
  await expect
    .poll(
      async () =>
        (await store("monitoring_snapshot")).items[0].action_status.final_review
          .execution.manual_start,
    )
    .toBe(true);
  const snapshot = await store("monitoring_snapshot");
  expect(snapshot.reviews).toHaveLength(1);
  expect(snapshot.publications).toHaveLength(1);
  expect(snapshot.publications[0]).toMatchObject({
    local_only: true,
    publication: null,
  });
  expect(
    snapshot.items[0].action_status.final_review.execution.operation
      .operation_type,
  ).toBe("primary_final_review");
  await expect(
    page.getByRole("button", { name: /^(Approve|Merge)$/ }),
  ).toHaveCount(0);
});

test("approval receipt retains personal handoff, whereas confirmed merge is terminal without a human-review claim", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  const final = fixture.actions.finals[0];
  final.execution.result = fixture.review.result;
  final.execution.operation.state = "completed";
  final.execution.phase = "Primary final full review complete";
  const effect = {
    cancelled: false,
    reconcile_attempts: 0,
    reconcile_requested: false,
    id: "approval-intent",
    final_id: final.id,
    item_id: "action-iteration",
    action: "approve",
    operation: {
      ...fixture.review.operation,
      id: "approval-op",
      operation_type: "github_approve",
    },
    observation: fixture.observation,
    body: "Frozen fixture approval.",
    state: "confirmed",
    error: null,
    receipt: {
      id: "401",
      actor_id: "22",
      head: fixture.review.job.head_sha,
      action: "approve",
      merge_commit: null,
    },
  };
  fixture.actions.effects = [effect];
  fixture.state.actions = fixture.actions;
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue")).toContainText(
    "GitHub approval: confirmed",
  );
  await expect(page.locator("#handoff-queue")).toContainText(
    "Confirmed receipt 401; acting account 22",
  );
  await expect(page.locator("#handoff-queue")).toContainText(
    "Personal review: Required",
  );
  await store("set_notifications_enabled", { enabled: true });
  const ledger = await store("observe_notifications");
  expect(ledger.notices.some((n) => n.event.category === "approved")).toBe(
    true,
  );
  fixture.actions.effects.push({
    ...effect,
    id: "merge-intent",
    action: "merge",
    operation: {
      ...effect.operation,
      id: "merge-op",
      operation_type: "github_merge",
    },
    receipt: {
      id: "PR_node",
      actor_id: "22",
      head: fixture.review.job.head_sha,
      action: "merge",
      merge_commit: "c".repeat(40),
    },
  });
  await store("seed_queue_state", fixture.state);
  await page.reload();
  await expect(page.locator("#handoff-queue")).toContainText(
    "Merged on GitHub",
  );
  await expect(page.locator("#handoff-queue")).toContainText(
    "Personal human review is not inferred",
  );
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "Ready for your final review",
  );
});

test("unknown effects expose original reconciliation rather than another approve or merge request", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  const final = fixture.actions.finals[0];
  final.execution.result = fixture.review.result;
  final.execution.operation.state = "completed";
  fixture.actions.effects = [
    {
      cancelled: false,
      reconcile_attempts: 3,
      reconcile_requested: false,
      id: "lost-intent",
      final_id: final.id,
      item_id: "action-iteration",
      action: "approve",
      operation: {
        ...fixture.review.operation,
        id: "lost-op",
        operation_type: "github_approve",
        state: "manual_retry",
        attempted_mutation: "Approve",
      },
      observation: fixture.observation,
      body: "<script>window.executed=true</script>",
      state: "uncertain",
      error:
        "Approval outcome remains unresolved; no replacement vote will be submitted.",
      receipt: null,
    },
  ];
  fixture.state.actions = fixture.actions;
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__actionRequests = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "reconcile_provider_action") {
        window.__actionRequests.push({ command, args });
        return;
      }
      return original(command, args);
    };
  });
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue")).toContainText(
    "Failed / recovery required",
  );
  await expect(page.locator("#handoff-queue")).toContainText(
    "No action receipt",
  );
  await page
    .getByRole("button", { name: "Reconcile original action (no resend)" })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.__actionRequests))
    .toEqual([
      { command: "reconcile_provider_action", args: { id: "lost-intent" } },
    ]);
  expect(await page.evaluate(() => window.executed)).toBeUndefined();
  expect(
    (await store("monitoring_snapshot")).items[0].action_status.effects,
  ).toHaveLength(1);
});

test("publication off shows local-only evidence while an existing pending batch retains recovery", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  await page.goto("/?view=queue");
  await expect(page.locator("#agent-reviews")).toContainText(
    "Local-only evidence",
  );
  await expect(
    page.getByRole("button", { name: "Publish review", exact: true }),
  ).toHaveCount(0);
  const pending = fixture.published(fixture.review);
  pending.phase = "unresolved";
  pending.uncertain = true;
  pending.receipts = [];
  pending.operation.state = "manual_retry";
  pending.error = "Lost original batch response.";
  fixture.state.publications = [pending];
  await store("seed_queue_state", fixture.state);
  await page.reload();
  await expect(page.locator("#agent-reviews")).toContainText("original batch");
  await expect(
    page.getByRole("button", {
      name: "Reconcile / retry publication",
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "Ready for your final review",
  );
});
