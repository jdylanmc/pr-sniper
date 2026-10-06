import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { test, expect, captureInspector, nativeCapacity } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

async function resetConversationInspector(page, kind, id, state) {
  await page.goto("/");
  // The shell initially says Queue before the native snapshot has been applied.
  await expect(page.locator("[data-panel-heading]")).toHaveText("Job details");
  await page.evaluate(() => window.__settingsIdle());
  await page.evaluate(
    ({ id, state }) => {
      window.__activeIds = ["running", "stopping"].includes(state) ? [id] : [];
      window.__stoppingIds = state === "stopping" ? [id] : [];
    },
    { id, state },
  );
  // This UI command is queued; idle alone can miss a not-yet-dispatched command.
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Queue", exact: true })
    .click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
  await page.evaluate(() => window.__settingsIdle());
  // Reuse the retained monitor and read native capacity; no worker is launched.
  await page.evaluate(
    ({ kind, id }) =>
      window.__TAURI_INTERNALS__.invoke("panel_navigate", {
        route: { tab: "running", detail: { type: "job", kind, id } },
      }),
    { kind, id },
  );
}

async function feedbackFixture(store) {
  const fixture = await queueFixture(store);
  const review = fixture.review(1, "human_input_required");
  review.result.output.findings = [
    {
      path: "source.rs",
      side: "head",
      line: 1,
      severity: "high",
      title: "Wrong value",
      explanation: "Check the returned value.",
      confidence: 90,
    },
  ];
  const origin = fixture.published(review);
  origin.batch.comments = [
    {
      path: "source.rs",
      line: 1,
      side: "RIGHT",
      body: "Wrong value <script>window.injected=true</script>",
    },
  ];
  origin.receipts[0].comment_ids = ["100"];
  const thread = {
    id: "thread-1",
    resolved: false,
    can_reply: true,
    comments: [
      {
        id: "100",
        body: origin.batch.comments[0].body,
        author_id: "22",
        author_login: "local-operator",
        reply_to: null,
        review_id: origin.receipts[0].review_id,
        original_commit: review.job.head_sha,
        created_at: "2026-09-30T00:00:00Z",
        published_at: "2026-09-30T00:00:00Z",
      },
      {
        id: "101",
        body: "The caller intentionally expects 42.",
        author_id: "11",
        author_login: "pr-author",
        reply_to: "100",
        review_id: origin.receipts[0].review_id,
        original_commit: review.job.head_sha,
        created_at: "2026-09-30T00:01:00Z",
        published_at: "2026-09-30T00:01:00Z",
      },
    ],
  };
  const context = {
    id: JSON.stringify(["github", "22", "100", "1", "100"]),
    publication_id: origin.id,
    owner_agent_id: fixture.settings.agents[0].id,
    owner_assignment_id: review.assignment_id,
    original_head: review.job.head_sha,
    root_id: "100",
    path: "source.rs",
    title: "Wrong value",
    body: thread.comments[0].body,
    thread,
    closed: false,
    unavailable: null,
  };
  const state = {
    jobs: [review.job],
    reviews: [review],
    publications: [origin],
    follow_ups: [],
    feedback: {
      records: [
        { context, job: review.job, observed_head: review.job.head_sha },
      ],
      mentions: [],
    },
  };
  await store("seed_queue_state", state);
  return { ...fixture, review, origin, thread, context, state };
}

function reply(fixture, head = "a".repeat(40)) {
  return {
    id: "conversation-1",
    key: "owner-comment-101",
    trigger_id: "101",
    target: {
      kind: "owned",
      publication_id: fixture.origin.id,
      review: structuredClone(fixture.review),
      thread: fixture.thread,
    },
    context: {
      assignment_id: fixture.review.assignment_id,
      job: { ...fixture.review.job, head_sha: head },
      selection: fixture.review.selection,
      trust_confirmed: true,
      feedback: [fixture.context],
      feedback_checked: true,
    },
    phase: "waiting_start",
    analysis: null,
    publication: null,
    history: [],
    manual_start: true,
    confirmed: false,
    automatic_publication: false,
    cancelled: false,
    error: null,
    result: null,
    body: null,
    uncertain: false,
    receipt: null,
    reply_ordinal: 1,
    enqueue_order: 2,
    enqueued_at: 1_800_000_010,
  };
}

async function reopenedConversationFixture(store, kind, legacy = false) {
  const fixture = await feedbackFixture(store);
  const original = fixture.review;
  if (!legacy) {
    original.job.work = {
      id: "original-work",
      item_id: "original-item",
      iteration_id: "original-iteration",
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
    original.key = original.job.work.id;
  }
  const run = reply(fixture);
  if (kind === "mention") {
    run.id = "historical-mention";
    run.trigger_id = "501";
    run.target = {
      kind: "mention",
      comment: {
        id: "501",
        body: "@local-operator explain the original iteration.",
        author_id: "11",
        author_login: "pr-author",
        created_at: "2026-09-30T00:01:00Z",
        updated_at: "2026-09-30T00:01:00Z",
      },
    };
  }
  const reopened = structuredClone(original);
  reopened.key = "reopened-work";
  reopened.job.work = {
    ...original.job.work,
    id: reopened.key,
    item_id: "reopened-item",
    iteration_id: "reopened-iteration",
    iteration: 2,
    agent_id: fixture.settings.agents[0].id,
    enqueue_order: 2,
    pass_ordinal: 2,
    trigger: "reopened",
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
  };
  reopened.operation.id = "reopened-review";
  reopened.result.output.decision = "machine_sign_off";
  reopened.result.output.findings = [];
  const publication = fixture.published(reopened);
  publication.id = "reopened-publication";
  publication.operation.id = "reopened-publication-op";
  publication.receipts[0].review_id = "201";
  fixture.context.closed = true;
  fixture.context.thread.resolved = true;
  fixture.state.jobs = [{ ...original.job, waiting: "closed" }, reopened.job];
  fixture.state.reviews.push(reopened);
  fixture.state.publications.push(publication);
  fixture.state.follow_ups = [run];
  await store("seed_queue_state", fixture.state);
  return { ...fixture, run };
}

for (const kind of ["reply", "mention"]) {
  test(`panel historical ${kind} keeps its closed canonical iteration after same-head reopen and Back`, async ({
    page,
    store,
  }) => {
    const fixture = await reopenedConversationFixture(store, kind);
    const before = await store("monitoring_snapshot");
    const original = before.items.find((item) => item.id === "original-item");
    const reopened = before.items.find((item) => item.id === "reopened-item");
    expect(before.items[0].id).toBe(reopened.id);
    expect(original.state).toBe("closed");
    expect(reopened.state).toBe("machine_signed_off");
    expect(original.job.head_sha).toBe(reopened.job.head_sha);
    expect(original.follow_up_ids).toContain(fixture.run.id);
    expect(reopened.follow_up_ids).toContain(fixture.run.id);
    await page.goto("/");
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Running", exact: true })
      .click();
    const origin = page.locator(`[data-job-id="${kind}:${fixture.run.id}"]`);
    const button = origin.getByRole("button", {
      name: "Open job",
      exact: true,
    });
    await button.click();
    await expect(page.locator(".job-facts")).toContainText("#1 / iteration 1");
    await expect(page.locator("[data-work-context]")).not.toContainText(
      original.summary,
    );
    await expect(page.locator("[data-item-evidence]")).not.toContainText(
      reopened.summary,
    );
    const conversation = page.locator("#thread-follow-ups");
    await expect(conversation.locator("article")).toHaveCount(1);
    await conversation.getByText(/^Conversation \(/).click();
    if (kind === "reply") {
      await expect(conversation).toContainText(fixture.thread.comments[0].body);
      await expect(conversation).toContainText(fixture.thread.comments[1].body);
      await expect(conversation).toContainText(
        `Original root head ${original.job.head_sha}; publication ${fixture.origin.id}`,
      );
    } else {
      await expect(conversation).toContainText(fixture.run.target.comment.body);
      await expect(conversation).toContainText("Primary conversation");
    }
    await conversation
      .getByText("Original target and captured analysis context", {
        exact: true,
      })
      .click();
    const captured = before.follow_ups.find(
      (candidate) => candidate.run.id === fixture.run.id,
    ).run;
    await expect(
      conversation
        .locator("details")
        .filter({
          has: page.getByText("Original target and captured analysis context", {
            exact: true,
          }),
        })
        .locator("pre"),
    ).toHaveText(
      JSON.stringify(
        { target: captured.target, context: captured.context },
        null,
        2,
      ),
    );
    await expect(page.locator("[data-work-context]")).toContainText(
      "Iteration 1 (original-iteration)",
    );
    await expect(page.locator("[data-work-context]")).toContainText(
      kind === "reply" ? "Targeted reply" : "Primary conversation",
    );
    await expect(page.locator("#agent-reviews article")).toHaveCount(0);
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(button).toBeFocused();
    expect((await store("panel_snapshot")).route).toEqual({ tab: "running" });
    expect(await store("monitoring_snapshot")).toEqual(before);
  });
}

for (const kind of ["reply", "mention"]) {
  test(`selected ${kind} keeps historical count and separate retry across later same-SHA work`, async ({
    page,
    store,
  }) => {
    const fixture = await reopenedConversationFixture(store, kind);
    const run = fixture.run;
    run.analysis = {
      ...fixture.review.operation,
      id: `${kind}-analysis-1`,
      operation_type: "thread_analysis",
      state: "queued",
      attempt_count: 3,
    };
    const later = structuredClone(run);
    later.id = `${kind}-later`;
    later.key = `${kind}-later-key`;
    later.trigger_id = "later-comment";
    later.reply_ordinal = 2;
    later.enqueue_order = 3;
    later.analysis.id = `${kind}-analysis-2`;
    later.context.job = fixture.state.reviews[1].job;
    fixture.state.follow_ups.push(later);
    await store("seed_queue_state", fixture.state);
    await store("panel_navigate", {
      route: { tab: "running", detail: { type: "job", kind, id: run.id } },
    });
    await page.goto("/");
    await expect(page.locator(".job-facts")).toContainText(
      "Reply / mention count1 for this Agent on this PR",
    );
    await expect(page.locator(".job-facts")).toContainText("Retry attempt3");
    await expect(page.locator(".job-facts")).toContainText("#1 / iteration 1");
    await expect(page.locator(".job-facts")).not.toContainText(
      "2 for this Agent",
    );
    await captureInspector(page, `${kind}-historical`);
    run.reply_ordinal = null;
    await store("seed_queue_state", fixture.state);
    await page.reload();
    await expect(page.locator(".job-facts")).toContainText(
      "Reply / mention countNot recorded",
    );
  });

  for (const [index, [state, label]] of [
    ["queued", "Waiting"],
    ["running", "Running"],
    ["stopping", "Stopping"],
    ["failed", "Failed"],
    ["manual_retry", "Failed"],
    ["backoff", "Retry queued"],
    ["superseded", "Superseded"],
    ["cancelled", "Stopped"],
    ["completed", "Done"],
    ["human_input_required", "Human input required"],
    ["waiting_publication", "Awaiting publication"],
    ["publishing", "Publishing"],
    ["publication_failed", "Publication failed"],
    ["publication_backoff", "Publication retry queued"],
    ["unresolved", "Outcome unknown"],
    ["unresolved_cancelled", "Outcome unknown"],
    ["published", "Published"],
    ["receipt_recovery", "Publication retry queued"],
    ["stale_after_publication", "Published; stale evidence"],
  ].entries()) {
    test(`${kind} inspector presents only its own ${state} state (${label})`, async ({
      page,
      store,
    }) => {
      if (index >= 2) {
        // The former running-state capture set these for every later case.
        await page.setViewportSize({ width: 408, height: 744 });
        await page.emulateMedia({ reducedMotion: "reduce" });
      }
      const fixture = await feedbackFixture(store);
      fixture.review.job.work = {
        id: "conversation-parent",
        item_id: "conversation-item",
        iteration_id: "conversation-iteration",
        iteration: 1,
        pass_ordinal: 1,
        enqueue_order: 1,
        agent_id: fixture.settings.agents[0].id,
        trigger: "admission",
        admission: {
          watched_author: true,
          all_authors: false,
          requested_reviewer: false,
        },
      };
      fixture.review.key = "conversation-parent";
      const run = reply(fixture);
      if (kind === "mention") {
        run.target = {
          kind: "mention",
          comment: {
            id: "101",
            body: "@local-operator clarify this change.",
            author_id: "11",
            author_login: "pr-author",
            created_at: "2026-09-30T00:01:00Z",
            updated_at: "2026-09-30T00:01:00Z",
          },
        };
      }
      fixture.state.follow_ups = [run];
      await store("seed_queue_state", fixture.state);
      run.context.selection = (
        await store("monitoring_snapshot")
      ).reviews[0].planned_selection;
      await nativeCapacity(page, []);
      const publication = [
        "publishing",
        "publication_failed",
        "publication_backoff",
        "unresolved",
        "unresolved_cancelled",
        "published",
        "receipt_recovery",
        "stale_after_publication",
      ].includes(state);
      const analyzed =
        publication ||
        ["completed", "human_input_required", "waiting_publication"].includes(
          state,
        );
      run.context.job.waiting =
        state === "superseded" ? "superseded" : "human_start";
      run.phase = [
        "failed",
        "manual_retry",
        "backoff",
        "cancelled",
        "superseded",
        "publication_failed",
        "publication_backoff",
      ].includes(state)
        ? "stopped"
        : state === "completed"
          ? "quiet"
          : ["running", "stopping"].includes(state)
            ? "analyzing"
            : state === "queued"
              ? "waiting_start"
              : state === "unresolved_cancelled"
                ? "unresolved"
                : state === "receipt_recovery"
                  ? "published"
                  : state;
      run.cancelled = ["cancelled", "unresolved_cancelled"].includes(state);
      run.uncertain = ["unresolved", "unresolved_cancelled"].includes(state);
      run.receipt = [
        "published",
        "receipt_recovery",
        "stale_after_publication",
      ].includes(state)
        ? `${kind}-confirmed-reply`
        : null;
      run.error = [
        "failed",
        "manual_retry",
        "backoff",
        "publication_failed",
        "publication_backoff",
        "unresolved",
        "unresolved_cancelled",
        "receipt_recovery",
      ].includes(state)
        ? "Synthetic operation failure; original intent retained."
        : run.cancelled
          ? "Thread follow-up cancelled."
          : null;
      run.analysis = {
        ...fixture.review.operation,
        id: `${kind}-own-analysis`,
        operation_type:
          kind === "mention" ? "mention_analysis" : "thread_analysis",
        next_attempt_at: ["queued", "backoff"].includes(state)
          ? 1_800_000_000
          : null,
        failure: ["failed", "cancelled", "superseded"].includes(state)
          ? "permanent"
          : ["backoff", "manual_retry"].includes(state)
            ? "network"
            : null,
        state: analyzed
          ? "completed"
          : state === "stopping"
            ? "running"
            : ["superseded", "cancelled"].includes(state)
              ? "failed"
              : state === "backoff"
                ? "queued"
                : state,
        attempt_count:
          state === "queued" ? 0 : state === "manual_retry" ? 4 : 2,
      };
      run.result = analyzed
        ? {
            ...fixture.review.result,
            output: {
              decision:
                state === "completed"
                  ? "quiet"
                  : state === "human_input_required"
                    ? state
                    : "reply",
              body:
                state === "completed" || state === "human_input_required"
                  ? ""
                  : "Saved synthetic reply.",
              new_information: "",
              reason: "No new response needed.",
              evidence: [],
              feedback_assessments: [],
            },
            session_id: `${kind}-session`,
            model: "configured-model",
          }
        : null;
      run.publication = publication
        ? {
            ...fixture.review.operation,
            id: `${kind}-original-publication`,
            operation_type:
              kind === "mention" ? "mention_reply" : "thread_reply",
            state: ["published", "stale_after_publication"].includes(state)
              ? "completed"
              : state === "publishing"
                ? "running"
                : state === "publication_failed"
                  ? "failed"
                  : "queued",
            failure:
              state === "publication_failed"
                ? "permanent"
                : run.error
                  ? "network"
                  : null,
            attempt_count: 1,
            next_attempt_at: [
              "publication_backoff",
              "unresolved",
              "unresolved_cancelled",
              "receipt_recovery",
            ].includes(state)
              ? 1_800_000_000
              : null,
            attempted_mutation: "thread_reply",
            confirmed_receipt: run.receipt,
          }
        : null;
      run.body = publication ? run.result.output.body : null;
      await store("seed_queue_state", fixture.state);
      const saved = (await store("monitoring_snapshot")).follow_ups[0].run;
      expect(saved).toMatchObject({
        phase: run.phase,
        cancelled: run.cancelled,
        uncertain: run.uncertain,
        analysis: { state: run.analysis.state },
        publication: run.publication ? { state: run.publication.state } : null,
      });
      if (state === "queued")
        expect(
          (await store("monitoring_snapshot")).follow_ups[0].blocked,
        ).toBeNull();
      await store("panel_navigate", {
        route: { tab: "running", detail: { type: "job", kind, id: run.id } },
      });
      await resetConversationInspector(page, kind, run.id, state);
      await expect(page.locator(".job-status strong")).toHaveText(label);
      await expect(page.locator(".job-hero .work-spin")).toHaveCount(
        state === "running" ? 1 : 0,
      );
      await expect(page.locator("#thread-follow-ups article")).toHaveCount(1);
      await expect(page.locator("#agent-reviews article")).toHaveCount(0);
      await expect(page.locator(".job-facts")).toContainText(
        `Analysis${run.analysis.state.replaceAll("_", " ")}`,
      );
      if (run.receipt)
        await expect(page.locator(".job-facts")).toContainText(
          `Confirmed GitHub reply ${run.receipt}`,
        );
      else {
        await expect(page.locator("[data-monitor-detail]")).not.toContainText(
          "Confirmed GitHub reply",
        );
        if (publication)
          await expect(page.locator(".job-facts")).toContainText(
            "no confirmed reply receipt",
          );
      }
      if (run.uncertain) {
        await expect(page.locator("#thread-follow-ups")).toContainText(
          "Reconciliation checks the original reply and never blindly posts a replacement.",
        );
        await expect(
          page.getByRole("button", { name: "Reconcile / retry reply" }),
        ).toBeEnabled();
        await expect(
          page.getByRole("checkbox", {
            name: /Publish or reconcile this thread/,
          }),
        ).toHaveCount(0);
      }
      await expect(page.locator(".job-facts")).toContainText(
        "Reply / mention count1 for this Agent on this PR",
      );
      if (["running", "failed", "unresolved"].includes(state))
        await captureInspector(page, `${kind}-${state}`);
    });
  }
}

for (const stage of ["before-dispatch", "after-response"]) {
  test(`conversation setup awaits initial native Job and Queue ${stage}`, async ({
    page,
    store,
    ipc,
    dataRoot,
  }, testInfo) => {
    const fixture = await reopenedConversationFixture(store, "mention");
    const route = {
      tab: "running",
      detail: { type: "job", kind: "mention", id: fixture.run.id },
    };
    const seeded = await store("panel_navigate", { route });
    expect(seeded.missing).toBeNull();
    await nativeCapacity(page, []);
    await page.addInitScript((stage) => {
      const original = window.__TAURI_INTERNALS__.invoke;
      const gate = Promise.withResolvers();
      window.__releaseQueueSetup = () => gate.resolve();
      window.__setupRoutes = [];
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        if (command === "panel_navigate" && args.route.tab === "queue") {
          window.__setupRoutes.push({ phase: "queue-entered" });
          if (stage === "before-dispatch") await gate.promise;
        }
        const panel = ["panel_snapshot", "panel_navigate"].includes(command);
        if (panel)
          window.__setupRoutes.push({ phase: "dispatch", command, args });
        const snapshot = await original(command, args);
        if (panel)
          window.__setupRoutes.push({ phase: "response", command, snapshot });
        return snapshot;
      };
    }, stage);
    const initial = ipc.holdNext("panel_snapshot");
    const queue =
      stage === "after-response" ? ipc.holdNext("panel_navigate") : null;
    const evidence = [];
    const record = async (phase) => {
      const persisted = JSON.parse(
        await readFile(join(dataRoot, "fixture-panel-session.json"), "utf8"),
      );
      const snapshot = await store("panel_snapshot");
      const browser = await page.evaluate(() => ({
        heading: document.querySelector("[data-panel-heading]").textContent,
        routes: window.__setupRoutes,
      }));
      expect(persisted.route).toEqual(snapshot.route);
      expect(persisted.revision).toBe(snapshot.revision);
      evidence.push({ phase, persisted, snapshot, browser });
      return { snapshot, browser };
    };
    const opening = resetConversationInspector(
      page,
      "mention",
      fixture.run.id,
      "queued",
    );
    try {
      await initial.arrived;
      const boot = await record("initial-native-read-held");
      expect(boot.snapshot).toEqual(seeded);
      expect(boot.browser.heading).toBe("Your queue");
      expect(boot.browser.routes).toEqual([
        { phase: "dispatch", command: "panel_snapshot", args: {} },
      ]);
      initial.release();
      await expect
        .poll(() =>
          page.evaluate(() =>
            window.__setupRoutes.some(
              (entry) => entry.phase === "queue-entered",
            ),
          ),
        )
        .toBe(true);
      if (queue) await queue.arrived;
      else await page.evaluate(() => window.__settingsIdle());
      const held = await record("queue-held");
      expect(held.snapshot.route).toEqual(queue ? { tab: "queue" } : route);
      expect(held.snapshot.revision).toBe(seeded.revision + (queue ? 2 : 0));
      expect(held.browser.heading).toBe("Job details");
      expect(
        held.browser.routes.filter(
          (entry) =>
            entry.phase === "dispatch" && entry.command === "panel_navigate",
        ),
      ).toEqual(
        queue
          ? [
              {
                phase: "dispatch",
                command: "panel_navigate",
                args: { route: { tab: "queue" } },
              },
            ]
          : [],
      );
      queue?.release();
      await page.evaluate(() => window.__releaseQueueSetup());
      await opening;
      await page.evaluate(() => window.__settingsIdle());
      const final = await record("setup-complete");
      expect(final.snapshot).toEqual({
        ...seeded,
        revision: seeded.revision + 4,
      });
      expect(final.browser.heading).toBe("Job details");
      expect(
        final.browser.routes
          .filter((entry) => entry.phase === "response")
          .map(({ command, snapshot }) => ({
            command,
            route: snapshot.route,
            revision: snapshot.revision,
          })),
      ).toEqual([
        { command: "panel_snapshot", route, revision: seeded.revision },
        {
          command: "panel_navigate",
          route: { tab: "queue" },
          revision: seeded.revision + 2,
        },
        { command: "panel_navigate", route, revision: seeded.revision + 4 },
      ]);
      expect(final.browser.routes.map((entry) => entry.phase)).toEqual([
        "dispatch",
        "response",
        "queue-entered",
        "dispatch",
        "response",
        "dispatch",
        "response",
      ]);
      await expect(page.locator("[data-work-context]")).toContainText(
        "Primary conversation",
      );
      await expect(page.locator("#thread-follow-ups article")).toHaveCount(1);
      await expect(page.locator("#agent-reviews article")).toHaveCount(0);
    } finally {
      initial.release();
      queue?.release();
      await page.evaluate(() => window.__releaseQueueSetup());
      try {
        await opening;
      } finally {
        await testInfo.attach("conversation-route-sequence", {
          body: JSON.stringify(evidence, null, 2),
          contentType: "application/json",
        });
      }
    }
  });
}

test("panel legacy conversation rejects ambiguous same-head iterations without losing its body", async ({
  page,
  store,
}) => {
  const fixture = await reopenedConversationFixture(store, "reply", true);
  const before = await store("monitoring_snapshot");
  expect(before.items).toHaveLength(2);
  expect(
    before.items.every((item) => item.follow_up_ids.includes(fixture.run.id)),
  ).toBe(true);
  await store("panel_navigate", {
    route: {
      tab: "running",
      detail: { type: "job", kind: "reply", id: fixture.run.id },
    },
  });
  await page.goto("/");
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "Parent PR iteration context unavailable",
  );
  for (const item of before.items)
    await expect(page.locator("[data-item-evidence]")).not.toContainText(
      item.summary,
    );
  await page.getByText("Conversation (2 comments)", { exact: true }).click();
  await expect(page.locator("#thread-follow-ups")).toContainText(
    fixture.thread.comments[1].body,
  );
  expect(await store("monitoring_snapshot")).toEqual(before);
});

for (const field of ["item_id", "iteration_id", "configuration_id"]) {
  test(`panel canonical conversation rejects mismatched ${field} without legacy substitution`, async ({
    page,
    store,
  }) => {
    const fixture = await reopenedConversationFixture(store, "reply");
    if (field === "configuration_id") {
      // Native same-PR legacy compatibility treats an empty configuration as a wildcard.
      fixture.state.jobs[0].configuration_id = "";
    } else {
      fixture.state.jobs[0].work = {
        ...fixture.state.jobs[0].work,
        [field]: `different-${field}`,
      };
    }
    await store("seed_queue_state", fixture.state);
    await store("panel_navigate", {
      route: {
        tab: "running",
        detail: { type: "job", kind: "reply", id: fixture.run.id },
      },
    });
    await page.goto("/");
    await expect(page.locator("[data-item-evidence]")).toContainText(
      "Parent PR iteration context unavailable",
    );
    await expect(page.locator("#thread-follow-ups article")).toHaveCount(1);
    await page.getByText("Conversation (2 comments)", { exact: true }).click();
    await expect(page.locator("#thread-follow-ups")).toContainText(
      fixture.thread.comments[1].body,
    );
    expect(
      (await store("monitoring_snapshot")).follow_ups[0].run.analysis,
    ).toBeNull();
  });
}

test("panel legacy conversation accepts only one exact binding despite same-head same-number neighbors", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  const run = reply(fixture);
  const original = (await store("monitoring_snapshot")).items[0];
  for (const binding of [
    { account_id: "different-account" },
    { repository_id: "different-repository" },
    { configuration_id: "" },
  ]) {
    fixture.state.jobs.push({ ...fixture.review.job, ...binding });
  }
  fixture.state.follow_ups = [run];
  await store("seed_queue_state", fixture.state);
  const before = await store("monitoring_snapshot");
  expect(before.items).toHaveLength(4);
  const bound = before.items.find((item) => item.id === original.id);
  expect(before.items[0].id).not.toBe(bound.id);
  expect(
    before.items.every(
      (item) =>
        item.job.head_sha === run.context.job.head_sha &&
        item.job.number === run.context.job.number,
    ),
  ).toBe(true);
  await store("panel_navigate", {
    route: {
      tab: "running",
      detail: { type: "job", kind: "reply", id: run.id },
    },
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "open_queue_destination") {
        window.__boundItem = args.itemId;
        return original("queue_destination", args);
      }
      return original(command, args);
    };
  });
  await page.goto("/");
  await expect(page.locator(".job-facts")).toContainText(
    "Repositoryexample/repo",
  );
  await page
    .getByRole("button", { name: "View pull request on GitHub" })
    .click();
  expect(await page.evaluate(() => window.__boundItem)).toBe(bound.id);
  await expect(page.locator("[data-item-evidence]")).not.toContainText(
    "unavailable",
  );
  await expect(page.locator("#thread-follow-ups article")).toHaveCount(1);
  expect(await store("monitoring_snapshot")).toEqual(before);
});

test("panel exact reply route retains provenance and never substitutes a normal or mention job", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  const run = reply(fixture);
  fixture.state.follow_ups = [run];
  await store("seed_queue_state", fixture.state);
  await store("panel_navigate", {
    route: {
      tab: "running",
      detail: { type: "job", kind: "reply", id: run.id },
    },
  });
  await page.goto("/");
  await expect(page.locator("#thread-follow-ups article")).toHaveCount(1);
  await expect(page.locator("#thread-follow-ups")).toContainText("Wrong value");
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  await page.evaluate(
    (id) =>
      window.__TAURI_INTERNALS__.invoke("panel_navigate", {
        route: { tab: "running", detail: { type: "job", kind: "mention", id } },
      }),
    run.id,
  );
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "No other PR",
  );
  await expect(page.locator("#thread-follow-ups article")).toHaveCount(0);
  expect(
    (await store("monitoring_snapshot")).follow_ups[0].run.analysis,
  ).toBeNull();
});

test("panel exact primary mention route shows its captured comment without authorizing work", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  const run = reply(fixture);
  run.id = "mention-exact";
  run.key = "mention-comment-501";
  run.trigger_id = "501";
  run.target = {
    kind: "mention",
    comment: {
      id: "501",
      body: "@local-operator explain <script>window.injected=true</script>",
      author_id: "11",
      author_login: "pr-author",
      created_at: "2026-09-30T00:01:00Z",
      updated_at: "2026-09-30T00:01:00Z",
    },
  };
  fixture.state.follow_ups = [run, reply(fixture)];
  await store("seed_queue_state", fixture.state);
  await store("panel_navigate", {
    route: {
      tab: "running",
      detail: { type: "job", kind: "mention", id: run.id },
    },
  });
  await page.goto("/");
  await expect(page.locator("#thread-follow-ups article")).toHaveCount(1);
  await expect(page.locator("#thread-follow-ups")).toContainText(
    "@local-operator explain",
  );
  await expect(page.locator("#thread-follow-ups script")).toHaveCount(0);
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  expect(
    (await store("monitoring_snapshot")).follow_ups.every(
      (f) => f.run.analysis === null,
    ),
  ).toBe(true);
});

test("owned conversation exposes original provenance separately from current iteration analysis", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  const run = reply(fixture, "c".repeat(40));
  const current = fixture.review;
  const newJob = { ...current.job, head_sha: "c".repeat(40) };
  fixture.state.jobs.push(newJob);
  fixture.state.feedback.records[0].observed_head = newJob.head_sha;
  fixture.state.follow_ups = [run];
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  const conversations = page.locator("#thread-follow-ups");
  await expect(conversations).toContainText(`analysis head ${"c".repeat(40)}`);
  await expect(conversations).toContainText(
    `Original root head ${"a".repeat(40)}`,
  );
  await expect(conversations).toContainText(fixture.origin.id);
  await conversations
    .getByText("Conversation (2 comments)", { exact: true })
    .click();
  await expect(conversations).toContainText(
    "The caller intentionally expects 42.",
  );
  await expect(conversations.locator("script")).toHaveCount(0);
  expect(await page.evaluate(() => window.injected)).toBeUndefined();
  await page.reload();
  const saved = (await store("monitoring_snapshot")).follow_ups[0].run;
  expect(saved.target.review.job.head_sha).toBe("a".repeat(40));
  expect(saved.context.job.head_sha).toBe("c".repeat(40));
});

test("retained compact provenance renders its actual conversation without a fabricated historical review", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  const run = reply(fixture);
  const job = run.context.job;
  run.target = {
    kind: "retained",
    proof: {
      publication_id: fixture.origin.id,
      configuration_id: job.configuration_id,
      account_id: job.account_id,
      repository_id: job.repository_id,
      repository_name: job.repository_name,
      pull_request_id: job.pull_request_id,
      number: job.number,
      head_sha: job.head_sha,
      assignment_id: run.context.assignment_id,
      agent_id: run.context.selection.agent.id,
      review_id: fixture.origin.receipts[0].review_id,
      root_ids: ["100"],
      body_hashes: [
        createHash("sha256")
          .update(fixture.thread.comments[0].body)
          .digest("hex"),
      ],
    },
    thread: fixture.thread,
  };
  run.context.feedback = [];
  run.context.feedback_checked = false;
  fixture.state.reviews = [];
  fixture.state.publications = [];
  fixture.state.feedback.records = [];
  fixture.state.follow_ups = [run];
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  const conversations = page.locator("#thread-follow-ups");
  await expect(conversations).toContainText("Thread thread-1");
  await conversations
    .getByText("Conversation (2 comments)", { exact: true })
    .click();
  await expect(conversations).toContainText(
    "The caller intentionally expects 42.",
  );
  await expect(conversations.locator("script")).toHaveCount(0);
  expect(await page.evaluate(() => window.injected)).toBeUndefined();
  const saved = (await store("monitoring_snapshot")).follow_ups[0].run;
  expect(saved.target.kind).toBe("retained");
  expect(saved.target.review).toBeUndefined();
  expect(saved.thread).toEqual(fixture.thread);
  expect(saved.context).toMatchObject(run.context);
  expect(saved.analysis).toBeNull();
});

test("quiet analysis does not clear feedback without an explicit assessment and never rewrites findings", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  const run = reply(fixture);
  run.phase = "quiet";
  run.analysis = {
    ...fixture.review.operation,
    id: "reply-analysis",
    operation_type: "thread_analysis",
    state: "completed",
    initial_attempt_at: 1_800_000_100,
  };
  run.result = {
    ...fixture.review.result,
    output: {
      decision: "quiet",
      body: "",
      new_information: "",
      reason: "No response needed.",
      evidence: [],
      feedback_assessments: [],
    },
  };
  fixture.state.follow_ups = [run];
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue")).toContainText(
    "Waiting for PR author",
  );
  run.result.output.feedback_assessments = [
    {
      feedback_id: fixture.context.id,
      disposition: "cleared",
      reason: "The explanation and source agree.",
      evidence: [
        { path: "source.rs", side: "head", line: 1, quote: "return 42;" },
      ],
    },
  ];
  await store("seed_queue_state", fixture.state);
  await page.reload();
  await expect(page.locator("#handoff-queue")).toContainText(
    "Ready for your final review",
  );
  await page.getByText("Current owned feedback (1)", { exact: true }).click();
  await expect(page.locator("#handoff-queue")).toContainText(
    "Agent reassessment, not provider thread closure",
  );
  const saved = await store("monitoring_snapshot");
  expect(saved.reviews[0].run.result.output.findings).toHaveLength(1);
  expect(saved.publications[0].publication.receipts[0].comment_ids).toEqual([
    "100",
  ]);
  expect(saved.follow_ups[0].run.receipt).toBeNull();
});

test("provider-closed tombstones remain visible and missing observations cannot become clearance", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  fixture.context.closed = true;
  fixture.context.thread.resolved = true;
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue")).toContainText(
    "Ready for your final review",
  );
  await page.getByText("Current owned feedback (1)", { exact: true }).click();
  await expect(page.locator("#handoff-queue")).toContainText(
    "closed: Wrong value",
  );
  fixture.context.unavailable =
    "The original root is unavailable; no resolution inferred.";
  await store("seed_queue_state", fixture.state);
  await page.reload();
  await expect(page.locator("#handoff-queue")).toContainText("Blocked");
  await page.getByText("Current owned feedback (1)", { exact: true }).click();
  await expect(page.locator("#handoff-queue")).toContainText(
    "no resolution inferred",
  );
  expect(
    (await store("monitoring_snapshot")).feedback[
      Object.keys((await store("monitoring_snapshot")).feedback)[0]
    ][0].context.closed,
  ).toBe(true);
});

test("a missing primary is explicit, durable and does not create a fake review or publication", async ({
  page,
  store,
}) => {
  const fixture = await feedbackFixture(store);
  fixture.settings.repositories[0].assignments = [];
  await store("seed_settings", fixture.settings);
  fixture.state.feedback.mentions = [
    {
      key: "mention-key",
      work_id: "mention-501",
      enqueue_order: 2,
      enqueued_at: 1_800_000_010,
      binding: {
        configuration_id: fixture.review.job.configuration_id,
        account_id: "22",
        account_login: "local-operator",
        repository_id: "100",
        repository_name: "example/repo",
        pull_request_id: "1",
        number: 1,
      },
      comment: {
        id: "501",
        body: "@local-operator <script>window.injected=true</script>",
        author_id: "11",
        author_login: "author",
        created_at: "2026-09-30T00:01:00Z",
        updated_at: "2026-09-30T00:01:00Z",
      },
      follow_up_id: null,
      blocked: "No primary assigned; mention retained without fallback.",
    },
  ];
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  await expect(page.locator("#thread-follow-ups")).toContainText(
    "No primary assigned",
  );
  await expect(page.locator("[data-ai-work]")).toContainText(
    "mention mention-501: blocked",
  );
  await expect(page.locator("#thread-follow-ups script")).toHaveCount(0);
  expect(await page.evaluate(() => window.injected)).toBeUndefined();
  const snapshot = await store("monitoring_snapshot");
  expect(snapshot.follow_ups).toHaveLength(0);
  expect(snapshot.publications).toHaveLength(1);
  await page.reload();
  await expect(page.locator("#thread-follow-ups")).toContainText(
    "without fallback",
  );
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Running", exact: true })
    .click();
  await page
    .locator("[data-running-list] article")
    .filter({ hasText: "Primary conversation" })
    .getByRole("button", { name: "Open job", exact: true })
    .click();
  await expect(page.locator("[data-item-evidence]")).not.toContainText(
    "not available",
  );
  await expect(page.locator("#thread-follow-ups")).toContainText(
    "No primary assigned",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  expect((await store("monitoring_snapshot")).follow_ups).toHaveLength(0);
});

test("completed second publication permits closure and explicit same-head reassessment without rewriting the archive", async ({
  page,
  store,
}, testInfo) => {
  const fixture = await feedbackFixture(store);
  fixture.review.feedback_context = [];
  const current = structuredClone(fixture.review);
  current.job.head_sha = "c".repeat(40);
  current.operation.id = "second-review";
  current.operation.head_sha = current.job.head_sha;
  const key = JSON.parse(current.key);
  key[5] = current.job.head_sha;
  current.key = JSON.stringify(key);
  current.feedback_context = [structuredClone(fixture.context)];
  current.result.output.findings = [];
  current.result.output.decision = "machine_sign_off";
  current.result.output.feedback_assessments = [
    {
      feedback_id: fixture.context.id,
      disposition: "open",
      reason: "The earlier concern remains open.",
      evidence: [],
    },
  ];
  const second = fixture.published(current);
  second.id = "second-publication";
  second.operation.id = "second-publication-operation";
  second.receipts[0].review_id = "102";
  fixture.state.jobs = [
    { ...fixture.review.job, waiting: "superseded" },
    current.job,
  ];
  fixture.state.reviews.push(current);
  fixture.state.publications.push(second);
  fixture.state.feedback.records[0].observed_head = current.job.head_sha;
  await store("seed_queue_state", fixture.state);
  const archive = (await store("monitoring_snapshot")).publications.map(
    (p) => p.publication,
  );
  await page.goto("/?view=queue");
  const row = page
    .locator("#handoff-queue .queue-item")
    .filter({ hasText: current.job.head_sha });
  await expect(row).toHaveAttribute("data-state", "waiting_for_author");

  fixture.context.closed = true;
  fixture.context.thread.resolved = true;
  await store("seed_queue_state", fixture.state);
  await page.reload();
  await expect(row).toHaveAttribute("data-state", "machine_signed_off");
  await page.screenshot({
    path: testInfo.outputPath("second-publication-human-closure.png"),
    fullPage: true,
  });

  // A separate synthetic observation exercises local clearance, not provider closure.
  fixture.context.closed = false;
  fixture.context.thread.resolved = false;
  fixture.context.thread.comments.push({
    ...fixture.thread.comments[1],
    id: "103",
    body: "The current source still intentionally returns 42.",
  });
  const run = reply(fixture, current.job.head_sha);
  run.trigger_id = "103";
  run.context.feedback = [structuredClone(fixture.context)];
  run.phase = "quiet";
  run.analysis = {
    ...current.operation,
    id: "owner-reassessment",
    initial_attempt_at: 1_800_000_100,
  };
  run.result = {
    ...current.result,
    output: {
      decision: "quiet",
      body: "",
      new_information: "",
      reason: "Source checked.",
      evidence: [],
      feedback_assessments: [
        {
          feedback_id: fixture.context.id,
          disposition: "cleared",
          reason: "Source matches the explanation.",
          evidence: [
            { path: "source.rs", side: "head", line: 1, quote: "return 42;" },
          ],
        },
      ],
    },
  };
  fixture.state.follow_ups = [run];
  await store("seed_queue_state", fixture.state);
  await page.reload();
  await expect(row).toHaveAttribute("data-state", "machine_signed_off");
  await row.getByText("Current owned feedback (1)", { exact: true }).click();
  await expect(row).toContainText(
    "Agent reassessment, not provider thread closure",
  );
  await page.screenshot({
    path: testInfo.outputPath("second-publication-local-clearance.png"),
    fullPage: true,
  });
  const saved = await store("monitoring_snapshot");
  expect(saved.publications.map((p) => p.publication)).toEqual(archive);
  expect(
    saved.feedback[
      saved.items.find((i) => i.job.head_sha === current.job.head_sha).id
    ][0].context.closed,
  ).toBe(false);
});

test("unlinked durable mention blocks a cleared PR across reload before any execution exists", async ({
  page,
  store,
}, testInfo) => {
  const fixture = await feedbackFixture(store);
  fixture.context.closed = true;
  fixture.context.thread.resolved = true;
  const intent = {
    key: JSON.stringify([
      "mention",
      "github",
      "22",
      fixture.review.job.configuration_id,
      "100",
      "1",
      "501",
    ]),
    work_id: "retained-mention-501",
    enqueue_order: 17,
    enqueued_at: 1_800_000_010,
    binding: {
      configuration_id: fixture.review.job.configuration_id,
      account_id: "22",
      account_login: "local-operator",
      repository_id: "100",
      repository_name: "example/repo",
      pull_request_id: "1",
      number: 1,
    },
    comment: {
      id: "501",
      body: "@local-operator explain",
      author_id: "22",
      author_login: "local-operator",
      created_at: "2026-09-30T00:01:00Z",
      updated_at: "2026-09-30T00:01:00Z",
    },
    follow_up_id: null,
    blocked: "Mention observed; execution admission is pending.",
  };
  fixture.state.feedback.mentions = [intent];
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue .queue-item")).toHaveAttribute(
    "data-state",
    "blocked",
  );
  await expect(page.locator("#thread-follow-ups")).toContainText(
    "execution admission is pending",
  );
  await expect(page.locator("[data-ai-work]")).toContainText(
    "mention retained-mention-501: blocked",
  );
  await page.reload();
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "Ready for your final review",
  );
  const saved = await store("monitoring_snapshot");
  expect(saved.mentions).toEqual([intent]);
  expect(saved.follow_ups).toEqual([]);
  await page.screenshot({
    path: testInfo.outputPath("durable-mention-pending.png"),
    fullPage: true,
  });
});

test("incomplete observation admission stays visibly blocked after retry clears its transient failure", async ({
  page,
  store,
}, testInfo) => {
  const fixture = await feedbackFixture(store);
  fixture.context.closed = true;
  fixture.context.thread.resolved = true;
  fixture.state.monitoring = {
    health: {
      [fixture.review.job.configuration_id]: {
        repository_id: fixture.review.job.configuration_id,
        name: "example/repo",
        schedule_key: "fixture",
        provider_account_id: "22",
        account_login: "local-operator",
        provider_repository_id: "100",
        enabled: true,
        last_attempt: 1_800_000_020,
        last_success: 1_800_000_001,
        next_run: 1_800_000_030,
        schedule_available: true,
        last_failure: null,
        in_flight: false,
        conversation_admission_pending: true,
      },
    },
  };
  await store("seed_queue_state", fixture.state);
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue .queue-item")).toHaveAttribute(
    "data-state",
    "blocked",
  );
  await expect(page.locator("#handoff-queue")).toContainText(
    "Conversation observations are not fully admitted",
  );
  await expect(
    page.getByText("Conversation admission pending", { exact: false }),
  ).toBeVisible();
  await page.reload();
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "Ready for your final review",
  );
  await page.screenshot({
    path: testInfo.outputPath("observation-admission-blocker.png"),
    fullPage: true,
  });
});
