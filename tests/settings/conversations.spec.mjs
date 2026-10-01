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
});
