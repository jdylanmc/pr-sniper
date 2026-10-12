import { expect, test, captureInspector, nativeCapacity } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import {
  closeDialog,
  repositorySettings,
  withPermissions,
} from "./navigation.mjs";
import { createHash } from "node:crypto";

const kv = (root, label) => root.locator(`dt:text-is("${label}") + dd`);

async function actionFixture(store, observe = true, intelligence) {
  const fixture = await queueFixture(store);
  if (intelligence) {
    const agent = fixture.settings.agents[0];
    const value = { ...agent, intelligence };
    await store("save_resource", {
      edit: { kind: "agent", id: agent.id, expected: agent, value },
    });
    Object.assign(agent, value);
  }
  const repository = fixture.settings.repositories[0];
  repository.assignments[0].comment = false;
  repository.assignments[0].actions = {
    reply: false,
    approve: true,
    merge: true,
  };
  const review = fixture.review(9);
  review.trust_confirmed = false;
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

test("confirmed peer approval is shown without fabricating a merging author's vote", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store, false);
  fixture.observation.author_id = "22";
  fixture.observation.reviews = [
    {
      id: "144",
      actor_id: "44",
      head: fixture.review.job.head_sha,
      state: "APPROVED",
      body: "A confirmed peer approval.",
      submitted_at: "2026-09-30T00:00:00Z",
    },
  ];
  await store("seed_action_observation", {
    itemId: "action-iteration",
    observation: fixture.observation,
  });
  await page.goto("/?view=queue");
  const queue = page.locator("#handoff-queue");
  await expect(kv(queue, "Approval")).toHaveText("144");
  await expect(kv(queue, "Actor")).toHaveText("44");
  const snapshot = await store("monitoring_snapshot");
  expect(snapshot.items[0].action_status.provider_approval).toMatchObject({
    id: "144",
    actor_id: "44",
  });
  expect(snapshot.items[0].action_status.effects).toEqual([]);
  expect(snapshot.items[0].action_status.blockers).not.toContain(
    "GitHub does not allow the PR author to approve their own PR.",
  );
});

test("retired global and repository publication flags cannot change assignment readiness", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store, false);
  for (const publish of [false, true]) {
    for (const global of [false, true]) {
      for (const override of [undefined, false, true]) {
        const settings = structuredClone(fixture.settings);
        settings.defaults.automatic_comment_publication = global;
        settings.repositories[0].assignments[0].comment = publish;
        settings.repositories[0].assignments[0].actions.reply = !publish;
        settings.repositories[0].overrides = {
          automatic_comment_publication: override,
        };
        await store("seed_settings", settings);
        const snapshot = await store("monitoring_snapshot");
        expect(snapshot.items[0].state).toBe(
          publish ? "awaiting_publication" : "machine_signed_off",
        );
        expect(snapshot.publications[0].automatic).toBe(publish);
        expect(snapshot.publications[0].local_only).toBe(!publish);
        await page.goto("/?view=queue");
        if (publish)
          await expect(page.locator("#agent-reviews")).toContainText(
            "Publication: automatic",
          );
        else
          await expect(page.locator("#agent-reviews")).toContainText(
            "Local only",
          );
      }
    }
  }
});

for (const surface of ["standalone", "panel"]) {
  for (const state of [
    "queued",
    "captured",
    "completed",
    "failed",
    "legacy",
    "absent",
  ]) {
    test(`${surface} ${state} primary-final-only Diagnostics uses stored Intelligence, not today's Agent`, async ({
      page,
      store,
    }) => {
      const requested = {
        reasoning_effort: "high",
        context_tier: "long_context",
      };
      const fixture = await actionFixture(store, true, requested);
      const final = fixture.actions.finals[0];
      expect(final.execution.selection.agent.intelligence).toEqual(requested);
      final.execution.operation.id = "final-operation-128";
      final.execution.operation.attempt_count = state === "queued" ? 0 : 1;
      final.execution.operation.state =
        state === "captured"
          ? "running"
          : state === "failed"
            ? "failed"
            : state === "queued"
              ? "queued"
              : "completed";
      final.execution.error =
        state === "failed" ? "Synthetic final failure." : null;
      final.execution.result = ["completed", "legacy"].includes(state)
        ? {
            ...structuredClone(fixture.review.result),
            session_id: "actual-final-session-128",
            intelligence: requested,
          }
        : null;
      if (state === "legacy") {
        delete final.execution.selection.agent.intelligence;
        delete final.execution.result.intelligence;
      }
      fixture.state.reviews = [];
      fixture.state.actions = fixture.actions;
      if (state === "absent") fixture.state.actions.finals = [];
      await store("seed_queue_state", fixture.state);
      const originalSnapshot = await store("monitoring_snapshot");
      const capturedExecution =
        originalSnapshot.items[0].action_status.final_review?.execution;
      const settings = (await store("snapshot")).settings;
      const value = {
        ...settings.agents[0],
        model: "later-final-model",
        intelligence: { reasoning_effort: "low", context_tier: "default" },
      };
      await store("save_resource", {
        edit: {
          kind: "agent",
          id: value.id,
          expected: settings.agents[0],
          value,
        },
      });
      const snapshot = await store("monitoring_snapshot");
      expect(snapshot.reviews.every((candidate) => !candidate.run)).toBe(true);
      expect(snapshot.follow_ups).toEqual([]);
      const stored = snapshot.items[0].action_status.final_review;
      if (state !== "absent") {
        expect(stored.execution).toEqual(capturedExecution);
        expect(stored.execution.selection).toEqual(final.execution.selection);
      } else expect(stored).toBeNull();
      if (surface === "panel")
        await store("panel_navigate", {
          route: { tab: "running", detail: { type: "diagnostics" } },
        });
      await page.goto(surface === "panel" ? "/" : "/?view=diagnostics");
      const diagnostics = page.locator("[data-intelligence-diagnostics]");
      await diagnostics.locator("summary").click();
      await expect(diagnostics).not.toContainText("later-final-model");
      if (state === "absent") {
        await expect(diagnostics).toHaveText("Recorded job IntelligenceNone");
        await expect(diagnostics.locator("h4")).toHaveCount(0);
        return;
      }
      await expect(diagnostics).not.toHaveText("Recorded job IntelligenceNone");
      await expect(diagnostics.locator("h4")).toHaveText([
        "Job final-operation-128",
      ]);
      await expect(diagnostics).toContainText(
        "Requested modelconfigured-model",
      );
      if (state === "legacy") {
        await expect(diagnostics).toContainText(
          "Requested reasoning effortNot recorded",
        );
        await expect(diagnostics).toContainText(
          "Requested context windowNot recorded",
        );
        await expect(diagnostics).toContainText(
          "Actual IntelligenceNot recorded",
        );
      } else {
        await expect(diagnostics).toContainText(
          "Requested reasoning efforthigh",
        );
        await expect(diagnostics).toContainText(
          "Requested context windowlong_context",
        );
      }
      if (final.execution.result) {
        await expect(diagnostics).toContainText(
          "Sessionactual-final-session-128",
        );
        await expect(diagnostics).toContainText("Actual modelconfigured-model");
        if (state === "completed")
          await expect(diagnostics).toContainText(
            "Actual IntelligenceEffort: high; context: long_context",
          );
      } else {
        await expect(diagnostics).toContainText("Actual modelNot recorded");
        await expect(diagnostics).toContainText("SessionNot recorded");
        await expect(diagnostics).toContainText(
          "Actual IntelligenceNot recorded",
        );
      }
      if (state === "completed") {
        await store("panel_navigate", {
          route: {
            tab: "running",
            detail: { type: "job", kind: "primary_final", id: final.id },
          },
        });
        await page.goto("/");
        const captured = page.locator(".work-configuration").filter({
          hasText: "Assigned Agent configuration",
        });
        await expect(captured).toContainText("long_context");
        await expect(captured).not.toContainText("later-final-model");
        const raw = page.getByText("JSON", { exact: true });
        await raw.click();
        const finalEvidence = raw.locator("..");
        await expect(finalEvidence).toContainText("actual-final-session-128");
        await expect(finalEvidence).toContainText('"reasoning_effort": "high"');
      }
    });
  }
}

test("Diagnostics preserves normal, reply and mention order and deduplicates recorded execution projections", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store, true, {
    reasoning_effort: "high",
    context_tier: "long_context",
  });
  const final = fixture.actions.finals[0];
  final.execution.operation.id = "final-operation-order";
  final.execution.operation.state = "completed";
  final.execution.result = {
    ...structuredClone(fixture.review.result),
    session_id: "final-session-order",
    intelligence: { reasoning_effort: "high", context_tier: "long_context" },
  };
  fixture.state.actions = fixture.actions;
  await store("seed_queue_state", fixture.state);
  const snapshot = await store("monitoring_snapshot");
  const normalId = snapshot.reviews[0].run.operation.id;
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const value = await original(command, args);
      if (command !== "monitoring_snapshot") return value;
      const execution = value.items[0].action_status.final_review.execution;
      // Exercise repeated read projections, without altering stored evidence.
      value.reviews.push(value.reviews[0]);
      value.items.push(value.items[0]);
      value.follow_ups = ["reply", "mention"].map((kind) => ({
        run: {
          id: `${kind}-order`,
          context: { job: execution.job, selection: execution.selection },
          result: { ...execution.result, session_id: `${kind}-session-order` },
        },
      }));
      return value;
    };
  });
  await page.goto("/?view=diagnostics");
  const diagnostics = page.locator("[data-intelligence-diagnostics]");
  await diagnostics.locator("summary").click();
  await expect(diagnostics.locator("h4")).toHaveText([
    `Job ${normalId}`,
    "Job reply-order",
    "Job mention-order",
    "Job final-operation-order",
  ]);
  for (const session of [
    "reply-session-order",
    "mention-session-order",
    "final-session-order",
  ])
    await expect(diagnostics).toContainText(session);
  await expect((await store("monitoring_snapshot")).reviews).toHaveLength(
    snapshot.reviews.length,
  );
});

async function optOutInSettings(page) {
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  const parent = await repositorySettings(page, "example/repo");
  await parent
    .locator(".assignment-row")
    .locator("[data-edit]")
    .click({ force: true });
  const modal = page.getByRole("dialog", {
    name: "Edit assignment",
    exact: true,
  });
  await withPermissions(page, modal, () =>
    modal.getByRole("radio", { name: "Neither", exact: true }).check(),
  );
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
    await expect(page.locator("#handoff-queue")).toContainText("Failed");
    await optOutInSettings(page);
    const settings = (await store("snapshot")).settings;
    expect(settings.repositories[0].assignments[0].actions).toEqual({
      reply: false,
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
      destination === "panel" ? "Ready for you" : "Ready for your final review",
    );
    if (destination === "panel") {
      await page
        .getByRole("button", { name: "Evidence and actions", exact: true })
        .click();
      await expect(page.locator("[data-item-more]")).not.toBeEmpty();
      await page
        .locator("[data-queue-details]")
        .evaluate((details) => (details.open = true));
    }
    const evidence =
      destination === "panel"
        ? page.locator("[data-queue-details]")
        : page.locator("#handoff-queue");
    await expect(kv(evidence, "Personal review")).toHaveText("Required");
    await expect(kv(evidence, "Blocked")).toHaveText(
      "Optional action evidence unavailable.",
    );
    await expect(kv(evidence, "Refresh")).toContainText(
      "No enabled provider action",
    );
    await expect(
      evidence.getByRole("button", {
        name: "Retry provider evidence",
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
  await expect(page.locator("[data-item-more]")).not.toBeEmpty();
  await page
    .locator("[data-queue-details]")
    .evaluate((details) => (details.open = true));
  const evidence = page.locator("[data-queue-details]");
  const retry = evidence.getByRole("button", {
    name: "Retry provider evidence",
  });
  await expect(retry).toBeEnabled();
  await retry.click();
  await expect(page.locator("[data-panel-error]")).toHaveText("Failed");
  await expect(page.locator("[data-panel-error]")).toHaveAttribute(
    "title",
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
  expect(snapshot.items[0].state).toBe("failed");
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
  await expect(
    kv(page.locator('[data-action-group="Agent"]'), "State"),
  ).toHaveText("completed");
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
    "Primary final review",
  );
  await expect(page.locator(".work-configuration")).toContainText(
    "Review correctness.",
  );
  await captureInspector(page, "primary-final-done");
  await page.getByText("Files (3)", { exact: true }).click();
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
      "Primary final review",
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
  await expect(
    kv(page.locator("#handoff-queue"), "Personal review"),
  ).toHaveText("Required");
  await expect(
    kv(page.locator('#handoff-queue [data-action-group="Agent"]'), "State"),
  ).toHaveText("queued");
  await expect(page.getByRole("checkbox", { name: /trust/i })).toHaveCount(0);
  expect(fixture.actions.finals[0].execution.trust_confirmed).toBe(false);
  await expect(
    page.getByRole("button", { name: /Start .*final full review/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Cancel queued final review" }),
  ).toBeEnabled();
  expect(
    (await store("automation_snapshot")).work.find(
      (work) => work.key.id === fixture.actions.finals[0].id,
    ),
  ).toMatchObject({
    state: "blocked",
    reason: "Waiting for final-review retry backoff.",
  });
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

test("approval receipt leaves the Queue for Reviewed yet keeps personal review required, whereas confirmed merge is terminal without a human-review claim", async ({
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
  const queue = page.locator("#handoff-queue");
  await expect(kv(queue, "Approve")).toHaveText("confirmed");
  await expect(kv(queue, "Approve receipt")).toHaveText("401");
  await expect(kv(queue, "Approve actor")).toHaveText("22");
  await expect(kv(queue, "Personal review")).toHaveText("Required");
  await store("set_notifications_enabled", { enabled: true });
  const ledger = await store("observe_notifications");
  expect(ledger.notices.some((n) => n.event.category === "approved")).toBe(
    true,
  );
  const approvedItem = (await store("monitoring_snapshot")).items[0];
  expect(approvedItem.state).toBe("approved");
  await page.goto("/");
  await expect(page.locator("#handoff-queue article")).toHaveCount(0);
  await expect(page.locator("[data-queue-ready]")).toHaveText("0");
  await expect(page.locator("[data-queue-attention]")).toHaveText("0");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Reviewed", exact: true })
    .click();
  await expect(
    page.locator('[data-panel-view="reviewed"] [data-work-state="approved"]'),
  ).toContainText("Approved");
  await page.goto("/?view=queue");
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
  await expect(
    kv(page.locator("#handoff-queue"), "Personal review"),
  ).toHaveText("Not inferred");
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
  // Every parked failure is one human Needs-attention row with Retry; Running
  // never lists it.
  await expect(page.locator("#handoff-queue article")).toHaveCount(1);
  await expect(
    page.locator('#handoff-queue [data-queue-group="attention"] article'),
  ).toHaveCount(1);
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "Ready for you",
  );
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Running", exact: true })
    .click();
  await expect(page.locator('[data-work-group="failures"]')).toHaveCount(0);
  await expect(page.locator("[data-running-list] article")).toHaveCount(0);
  await store("panel_navigate", {
    route: {
      tab: "queue",
      detail: { type: "item", item_id: "action-iteration" },
    },
  });
  await page.reload();
  await expect(page.locator("[data-queue-status]")).toHaveAttribute(
    "data-queue-status",
    "failed",
  );
  await expect(page.locator("[data-item-more]")).not.toBeEmpty();
  await page
    .locator("[data-queue-details]")
    .evaluate((details) => (details.open = true));
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "Action unresolved",
  );
  await expect(
    kv(page.locator("[data-queue-details]"), "Approve receipt"),
  ).toHaveText("None");
  await expect(
    kv(page.locator("[data-queue-details]"), "Personal review"),
  ).toHaveText("Not inferred");
  const before = (await store("monitoring_snapshot")).items[0].action_status
    .effects[0];
  await page.getByRole("button", { name: "Reconcile original action" }).click();
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
  await expect(page.locator("#agent-reviews")).toContainText("Local only");
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
  await page
    .locator("#handoff-queue")
    .getByRole("button", { name: "Evidence and actions" })
    .click();
  await page.locator("[data-queue-details]").evaluate((d) => (d.open = true));
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

test("panel detail shows terse Activity rows, expandable facts and no evidence prose", async ({
  page,
  store,
}) => {
  const fixture = await actionFixture(store);
  const final = fixture.actions.finals[0];
  final.execution.result = fixture.review.result;
  final.execution.operation.state = "completed";
  final.execution.operation.attempt_count = 2;
  const head = fixture.review.job.head_sha;
  fixture.actions.effects = [
    {
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
        id: "5493796731",
        actor_id: "22",
        head,
        action: "approve",
        merge_commit: null,
      },
    },
  ];
  fixture.state.actions = fixture.actions;
  await store("seed_queue_state", fixture.state);
  await store("panel_navigate", {
    route: {
      tab: "queue",
      detail: { type: "item", item_id: "action-iteration" },
    },
  });
  await page.goto("/");
  const activity = page.locator("[data-queue-activity]");
  await expect(activity).toBeVisible();
  const kinds = await activity
    .locator("[data-activity]")
    .evaluateAll((rows) => rows.map((row) => row.dataset.activity));
  expect(kinds).toEqual(["review", "findings", "final", "retried", "approve"]);
  const approved = activity.locator('[data-activity="approve"]');
  await expect(approved.locator(".activity-verb")).toHaveText("Approved");
  await expect(approved.locator(".activity-value")).toHaveText(
    head.slice(0, 7),
  );
  await expect(
    approved.getByRole("button", { name: /Open approved/ }),
  ).toBeVisible();
  await expect(approved.locator(".queue-facts")).toBeHidden();
  await approved.locator(".activity-toggle").click();
  await expect(kv(approved, "Receipt")).toHaveText("5493796731");
  await expect(
    activity.locator('[data-activity="retried"] .activity-value'),
  ).toHaveText("1x");
  const status = page.locator("[data-item-evidence]");
  for (const prose of [
    "Machine clearance",
    "Final evidence",
    "verified green",
    "review this unmerged PR",
    "provider actions and personal review are separate",
  ])
    await expect(page.locator("body")).not.toContainText(prose);
  await expect(
    status.getByRole("button", { name: "Retry provider evidence" }),
  ).toHaveCount(0);
  const details = page.locator("[data-queue-details]");
  await expect(details).not.toHaveAttribute("open");
  await details.evaluate((element) => (element.open = true));
  for (const title of ["Agent", "Result", "Provider"])
    await expect(details.locator(`[data-action-group="${title}"]`)).toHaveCount(
      1,
    );
  await expect(kv(details, "Approve receipt")).toHaveText("5493796731");
});
