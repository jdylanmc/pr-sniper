import { test, expect } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

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
    .filter({ hasText: "Primary mention" })
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
