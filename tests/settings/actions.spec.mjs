import { expect, test, captureInspector, nativeCapacity } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { closeDialog, repositorySettings } from "./navigation.mjs";
import { createHash } from "node:crypto";

async function actionFixture(store, observe = true) {
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
  const actions = observe
    ? await store("seed_action_observation", {
        itemId: "action-iteration",
        observation,
      })
    : { finals: [], effects: [], observations: [] };
  return { ...fixture, state, observation, actions, review };
}

async function optOutInSettings(page) {
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  const parent = await repositorySettings(page, "example/repo");
  await parent
    .locator(".assignment-row")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  const modal = page.getByRole("dialog", {
    name: "Edit assignment",
    exact: true,
  });
  await modal.getByRole("checkbox", { name: /^Approve/ }).uncheck();
  await modal.getByRole("checkbox", { name: /^Merge/ }).uncheck();
  await modal
    .getByRole("button", { name: "Save assignment", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  await closeDialog(page);
}

function failedObservation() {
  return {
    item_id: "action-iteration",
    at: 1_800_000_100,
    observation: null,
    error: "Optional action evidence unavailable.",
    failures: 1,
    retry_at: null,
  };
}

for (const destination of ["panel", "legacy queue"]) {
  test(`action read failure followed by real settings opt-out restores personal handoff after restart in ${destination}`, async ({
    page,
    store,
  }) => {
    const fixture = await actionFixture(store, false);
    fixture.actions.observations = [failedObservation()];
    fixture.state.actions = fixture.actions;
    await store("seed_queue_state", fixture.state);
    await page.goto("/");
    await expect(page.locator("#handoff-queue")).toContainText("Blocked");
    await optOutInSettings(page);
    const settings = (await store("snapshot")).settings;
    expect(settings.repositories[0].assignments[0].actions).toEqual({
      approve: false,
      merge: false,
    });
    // Every bridge command creates a new Store; navigation also remounts the UI.
    await page.goto(destination === "panel" ? "/" : "/?view=queue");
    if (destination === "panel") {
      await page
        .getByRole("navigation", { name: "Application destinations" })
        .getByRole("button", { name: "Queue", exact: true })
        .click();
    }
    await expect(page.locator("#handoff-queue")).toContainText(
      "Ready for your final review",
    );
    if (destination === "panel")
      await page
        .getByRole("button", { name: "Evidence and actions", exact: true })
        .click();
    const evidence =
      destination === "panel"
        ? page.locator("[data-item-evidence]")
        : page.locator("#handoff-queue");
    await expect(evidence).toContainText("Personal review: Required");
    await expect(evidence).toContainText(
      "Optional action evidence unavailable.",
    );
    await expect(evidence).toContainText(
      "Refresh unavailable: No enabled provider action",
    );
    await expect(
      evidence.getByRole("button", {
        name: "Refresh / retry provider evidence",
      }),
    ).toBeDisabled();
    await expect(
      page.getByRole("button", {
        name: /^(Approve|Merge|Start \/ retry final)/,
      }),
    ).toHaveCount(0);
    const snapshot = await store("monitoring_snapshot");
    expect(snapshot.items[0].action_status).toMatchObject({
      machine_clear: true,
      final_review: null,
      effects: [],
      permissions: { approve: false, merge: false },
      blockers: ["Optional action evidence unavailable."],
      provider_observed_at: 1_800_000_100,
    });
    await expect(
      store("retry_action_observation", { itemId: "action-iteration" }),
    ).rejects.toContain("No enabled provider action");
    expect((await store("automation_snapshot")).work).toEqual([]);
    expect((await store("monitoring_snapshot")).items[0].action_status).toEqual(
      snapshot.items[0].action_status,
    );
  });
}

test("retained panel detail reports refresh failure and allows a real native retry without remounting", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store, false);
  fixture.actions.observations = [failedObservation()];
  fixture.state.actions = fixture.actions;
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    let rejectOnce = true;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "retry_action_observation" && rejectOnce) {
        rejectOnce = false;
        return Promise.reject(
          "Fixture transport interrupted before native retry.",
        );
      }
      return original(command, args);
    };
  });
  await page.goto("/");
  await page
    .getByRole("button", { name: "Evidence and actions", exact: true })
    .click();
  const evidence = page.locator("[data-item-evidence]");
  const retry = evidence.getByRole("button", {
    name: "Refresh / retry provider evidence",
  });
  await expect(retry).toBeEnabled();
  await retry.click();
  await expect(page.locator("[data-panel-error]")).toHaveText(
    "Fixture transport interrupted before native retry.",
  );
  await expect(retry).toBeEnabled();
  await retry.click();
  await expect
    .poll(
      async () =>
        (await store("monitoring_snapshot")).items[0].action_status
          .observation_retry_blocker,
    )
    .toContain("backoff");
  await expect(retry).toBeDisabled();
  await expect(evidence).toContainText("Optional action evidence unavailable.");
  const snapshot = await store("monitoring_snapshot");
  expect(snapshot.items[0].state).toBe("blocked");
  expect(snapshot.items[0].action_status.effects).toEqual([]);
  expect(snapshot.items[0].action_status.final_review).toBeNull();
});

test("panel primary-final route exposes only that final and returns to its exact Reviewed row", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  const final = fixture.actions.finals[0];
  final.execution.result = structuredClone(fixture.review.result);
  final.execution.result.output.files.push({
    path: "final-only.rs",
    order: 3,
    explanation: "Retained final guide evidence.",
  });
  final.execution.operation.state = "completed";
  final.execution.operation.attempt_count = 1;
  final.execution.phase = "Primary final full review complete";
  fixture.state.actions = fixture.actions;
  await store("seed_queue_state", fixture.state);
  const before = await store("monitoring_snapshot");
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__finalDestination = null;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "open_queue_destination") {
        window.__finalDestination = await original("queue_destination", args);
        return;
      }
      return original(command, args);
    };
  });
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
  await expect(page.locator("[data-work-context]")).toContainText(
    "Primary final review",
  );
  await expect(page.locator("[data-work-context]")).toContainText(
    "Iteration 1 (iteration-1)",
  );
  await expect(page.locator("[data-work-context]")).toContainText(
    "Captured rolePrimary Agent",
  );
  await expect(page.locator("[data-work-context]")).toContainText(
    "Primary final review (separate from normal passes)",
  );
  await expect(page.locator(".work-configuration")).toContainText(
    "Captured for this execution.",
  );
  await expect(page.locator(".work-configuration")).toContainText(
    "Review correctness.",
  );
  await captureInspector(page, "primary-final-done");
  await page
    .getByText("Complete final file guide (3 files)", { exact: true })
    .click();
  await expect(page.locator("[data-item-evidence] ol li")).toHaveText([
    "z-first.rs: Read this first.",
    "a-second.rs: Then read this.",
    "final-only.rs: Retained final guide evidence.",
  ]);
  await page.getByRole("link", { name: "final-only.rs", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => window.__finalDestination))
    .toBe(
      `https://github.com/example/repo/pull/9/files#diff-${createHash("sha256").update("final-only.rs").digest("hex")}`,
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

test("primary final inspector distinguishes waiting, active, stopping, failed and superseded from normal clearance", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  const final = fixture.actions.finals[0];
  final.execution.manual_start = true;
  fixture.state.actions = fixture.actions;
  await nativeCapacity(page, []);
  for (const [state, label] of [
    ["queued", "Waiting"],
    ["running", "Running"],
    ["stopping", "Stopping"],
    ["failed", "Failed"],
    ["superseded", "Superseded"],
  ]) {
    final.execution.operation.state =
      state === "stopping"
        ? "running"
        : state === "superseded"
          ? "failed"
          : state;
    final.execution.operation.attempt_count = state === "queued" ? 0 : 2;
    final.execution.operation.next_attempt_at = 1_800_000_000;
    final.execution.job.waiting =
      state === "superseded" ? "superseded" : "human_start";
    await store("seed_queue_state", fixture.state);
    await page.goto("/");
    await page.evaluate(
      ({ id, state }) => {
        window.__activeIds = ["running", "stopping"].includes(state)
          ? [id]
          : [];
        window.__stoppingIds = state === "stopping" ? [id] : [];
      },
      { id: final.id, state },
    );
    await page.evaluate(
      (id) =>
        window.__TAURI_INTERNALS__.invoke("panel_navigate", {
          route: {
            tab: "running",
            detail: { type: "job", kind: "primary_final", id },
          },
        }),
      final.id,
    );
    await expect(page.locator(".job-status strong")).toHaveText(label);
    await expect(page.locator(".job-hero .work-spin")).toHaveCount(
      state === "running" ? 1 : 0,
    );
    await expect(page.locator(".job-facts")).toContainText(
      "Primary final review (separate from normal passes)",
    );
    await expect(page.locator(".job-facts")).not.toContainText("Review count");
    await expect(page.locator("#agent-reviews article")).toHaveCount(0);
    if (state === "running" || state === "failed")
      await captureInspector(page, `primary-final-${state}`);
  }
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
  await page.goto("/");
  await optOutInSettings(page);
  await page.reload();
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Queue", exact: true })
    .click();
  await expect(page.locator("#handoff-queue")).toContainText(
    "Failed / recovery required",
  );
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "Ready for your final review",
  );
  await page
    .getByRole("button", { name: "Evidence and actions", exact: true })
    .click();
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "No action receipt",
  );
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "Personal review: Not inferred.",
  );
  const before = (await store("monitoring_snapshot")).items[0].action_status
    .effects[0];
  await page
    .getByRole("button", { name: "Reconcile original action (no resend)" })
    .click();
  await expect
    .poll(
      async () =>
        (await store("monitoring_snapshot")).items[0].action_status.effects,
    )
    .toEqual([{ ...before, reconcile_requested: true }]);
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
