import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const NOW = Math.floor(Date.now() / 1000);
const secondAgentId = "00000000-0000-4000-8000-000000000013";
const secondAssignmentId = "00000000-0000-4000-8000-000000000012";
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

const record = {
  state_id: "3:5f1c2e9a",
  hash: "f".repeat(64),
  schema: 1,
  tool_set: ["read_changes", "search_code", "leave_comments", "panic_exit"],
  exits_offered: ["leave_comments", "ready_for_human", "panic_exit"],
  exits_denied: [{ exit: "approve_and_merge", reason: "CI is red." }],
};
const titles = {
  21: "Approve exit",
  22: "Reply exit",
  23: "Tossed exit",
  24: "Panic exit",
  25: "Ready exit",
  26: "Failed write",
};

// A completed packet run of PR `number` that ended on `exit`.
function packetRun(fixture, number, exit) {
  const run = fixture.review(number);
  run.job.title = titles[number] ?? run.job.title;
  run.mode = "packet";
  run.packet = record;
  run.result.output = {
    synopsis: "",
    files: [],
    findings: [],
    decision: "human_input_required",
    exit,
  };
  run.result.coverage = { read: 3, changed: 5 };
  return run;
}

const replies = {
  exit: "comments",
  new_comments: [],
  reply_comments: [{ thread_id: "T1", body: "Fixed in this head." }],
  resolve_own_threads: ["T2"],
};

function write(run, phase, patch = {}) {
  const step = (kind, thread_id) => ({
    kind,
    thread_id,
    body: kind === "reply" ? "Fixed in this head." : "",
    root_id: null,
    revision: null,
    state: "planned",
    receipt: null,
  });
  return {
    id: `write-${run.job.number}`,
    review_operation_id: run.operation.id,
    job: run.job,
    assignment_id: run.assignment_id,
    comments_first: false,
    published: false,
    steps: [step("reply", "T1"), step("resolve", "T2")],
    phase,
    operation: {
      ...run.operation,
      id: `write-op-${run.job.number}`,
      operation_type: "exit_writes",
      state: "queued",
    },
    uncertain: false,
    error: null,
    consumed: false,
    ...patch,
  };
}

const binding = (job) => ({
  provider: job.provider,
  configuration_id: job.configuration_id,
  account_id: job.account_id,
  repository_id: job.repository_id,
  pull_request_id: job.pull_request_id,
});

async function seedPackets(store, { consumed = false } = {}) {
  const fixture = await queueFixture(store);
  const approve = packetRun(fixture, 21, {
    exit: "approve_and_merge",
    comment: "Reviewed the diff. Ship it.",
  });
  const reply = packetRun(fixture, 22, replies);
  const tossed = packetRun(fixture, 23, replies);
  const panic = packetRun(fixture, 24, null);
  panic.result = null;
  panic.error = "Agent panic exit: tool broken";
  Object.assign(panic.operation, {
    state: "failed",
    failure: "panic",
    attempt_count: 1,
    initial_attempt_at: NOW - 5 * 60,
    resume_cycle: 1,
    resume_at: NOW + 30 * 60,
  });
  const ready = packetRun(fixture, 25, {
    exit: "ready_for_human",
    summary: "Small rename; behaviour unchanged.",
  });
  const failed = packetRun(fixture, 26, replies);
  const reviews = [approve, reply, tossed, panic, ready, failed];
  await store("seed_queue_state", {
    jobs: reviews.map((run) => run.job),
    reviews,
    publications: [],
    follow_ups: [],
    feedback: {
      records: [],
      mentions: [],
      exit_writes: [
        consumed
          ? write(reply, "completed", {
              consumed: true,
              steps: write(reply, "completed").steps.map((s) => ({
                ...s,
                state: "done",
                receipt: "C1",
              })),
            })
          : write(reply, "verified"),
        write(tossed, "tossed", {
          error: "PR state moved. The exit was discarded.",
        }),
        write(failed, "failed", {
          steps: [
            { ...write(failed, "failed").steps[0], state: "done" },
            write(failed, "failed").steps[1],
          ],
          error: "A thread root changed; nothing further was written.",
          operation: {
            ...failed.operation,
            id: "write-op-26",
            operation_type: "exit_writes",
            state: "manual_retry",
            failure: "permanent",
          },
        }),
      ],
    },
    pr_states: {
      records: [
        { binding: binding(tossed.job), iteration_id: "i", error_count: 3 },
      ],
    },
  });
  const items = (await store("monitoring_snapshot")).items;
  return Object.fromEntries(items.map((item) => [item.job.number, item]));
}

async function openDetail(page, store, item) {
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  await expect(page.locator("[data-queue-status]")).toHaveCount(1);
}

const activity = (page, kind) =>
  page.locator(`[data-queue-activity] > li[data-activity="${kind}"]`);

async function expand(row) {
  await row.locator(".activity-toggle").click();
  return row.locator(".activity-facts");
}
const fact = (facts, label) => facts.locator(`dt:text-is("${label}") + dd`);

test("packet exits map to typed queue states", async ({ store }) => {
  const items = await seedPackets(store);
  expect(items[21]).toMatchObject({
    state: "machine_signed_off",
    exit: "approve_and_merge",
    progress: "approving",
  });
  expect(items[22]).toMatchObject({
    state: "awaiting_publication",
    exit: "leave_comments",
    progress: "publishing",
  });
  expect(items[23]).toMatchObject({
    state: "failed",
    failure_cause: "tossed",
    resume_at: null,
  });
  expect(items[24]).toMatchObject({
    state: "failed",
    failure_cause: "panic",
    exit: "panic_exit",
  });
  expect(items[24].resume_at).toBeGreaterThan(NOW);
  expect(items[25]).toMatchObject({
    state: "machine_signed_off",
    exit: "ready_for_human",
    progress: null,
  });
  expect(items[26]).toMatchObject({
    state: "failed",
    failure_cause: "exit_failed",
  });
  const consumed = await seedPackets(store, { consumed: true });
  expect(consumed[22]).toMatchObject({
    state: "waiting_for_author",
    progress: null,
  });
});

test("approve exit is Ready for you with Approving; detail shows exit, packet and read", async ({
  page,
  store,
}) => {
  const items = await seedPackets(store);
  await page.goto("/");
  await tab(page, "Queue").click();
  await showQueueGroup(page, "ready");
  const ready = page.locator("[data-queue-group='ready'] .queue-item");
  const row = ready.filter({ hasText: "Approve exit" });
  await expect(row).toHaveCount(1);
  const chip = row.locator("[data-queue-progress]");
  await expect(chip).toHaveAttribute("data-queue-progress", "approving");
  await expect(chip).toHaveText("Approving");
  await expect(chip.locator("svg")).toHaveCount(1);
  // Publishing work is in flight, not a handoff.
  await expect(
    page.locator(".queue-item").filter({ hasText: "Reply exit" }),
  ).toHaveCount(0);
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("packet-approving-row.png"),
  });

  await openDetail(page, store, items[21]);
  const label = page.locator(".queue-status-label");
  await expect(label).toHaveAttribute("data-progress", "approving");
  await expect(label).toHaveText("Approving");
  const read = activity(page, "review");
  await expect(read.locator(".activity-verb")).toHaveText("Read");
  await expect(read.locator(".activity-value")).toHaveText("3 of 5");
  const exit = activity(page, "exit");
  await expect(exit).toHaveAttribute("data-exit", "approve_and_merge");
  await expect(exit.locator(".activity-verb")).toHaveText("Approve + merge");
  await expect(exit.locator(".activity-value")).toHaveText("Approving");
  let facts = await expand(exit);
  await expect(fact(facts, "Exit")).toHaveText("approve_and_merge");
  await expect(fact(facts, "Approve")).toHaveText("1");
  await expect(fact(facts, "Merge")).toHaveText("1");
  const packet = activity(page, "packet");
  await expect(packet.locator(".activity-value")).toHaveText("v1");
  facts = await expand(packet);
  await expect(fact(facts, "State")).toHaveText("3:5f1c2e9a");
  await expect(fact(facts, "Schema")).toHaveText("1");
  await expect(fact(facts, "Tools")).toHaveText("4");
  await expect(fact(facts, "Offered")).toHaveText("3");
  await expect(fact(facts, "Denied")).toHaveText("1");
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("packet-approving-detail.png"),
    fullPage: true,
  });
});

test("reply-only exit shows Publishing with its counts outside the Queue", async ({
  page,
  store,
}) => {
  const items = await seedPackets(store);
  await openDetail(page, store, items[22]);
  const label = page.locator(".queue-status-label");
  await expect(label).toHaveAttribute("data-progress", "publishing");
  await expect(label).toHaveText("Publishing");
  const exit = activity(page, "exit");
  await expect(exit).toHaveAttribute("data-exit", "leave_comments");
  await expect(exit.locator(".activity-verb")).toHaveText("Comments");
  await expect(exit.locator(".activity-value")).toHaveText("Publishing");
  const facts = await expand(exit);
  await expect(fact(facts, "New")).toHaveText("0");
  await expect(fact(facts, "Replies")).toHaveText("1");
  await expect(fact(facts, "Resolved")).toHaveText("1");
  await expect(fact(facts, "Write")).toHaveText("verified");
  await expect(fact(facts, "Done")).toHaveText("0 of 2");
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("packet-publishing-detail.png"),
    fullPage: true,
  });

  // Consumed: the item leaves the Queue; the counts stay in Activity.
  const consumed = await seedPackets(store, { consumed: true });
  await openDetail(page, store, consumed[22]);
  const done = activity(page, "exit");
  await expect(done.locator(".activity-value")).toHaveText(
    "New 0 \u00b7 Replies 1 \u00b7 Resolved 1",
  );
  await expect(page.locator(".queue-status-label[data-progress]")).toHaveCount(
    0,
  );
});

test("parked toss, panic and a failed write are typed Needs attention rows", async ({
  page,
  store,
}) => {
  await seedPackets(store);
  await page.goto("/");
  await tab(page, "Queue").click();
  await showQueueGroup(page, "attention");
  const rows = page.locator("[data-queue-group='attention'] .queue-item");
  await expect(rows).toHaveCount(3);
  const tossed = rows.filter({ hasText: "Tossed exit" });
  await expect(tossed).toHaveCount(1);
  await expect(tossed.locator(".queue-state")).toHaveText("Tossed");
  await expect(tossed.locator("[data-queue-resume]")).toHaveCount(0);
  await expect(tossed.locator("[data-retry-item]")).toHaveCount(1);
  const panic = rows.filter({ hasText: "Panic exit" });
  await expect(panic.locator(".queue-state")).toContainText("Panic");
  const clock = panic.locator("[data-queue-resume]");
  await expect(clock).toHaveText(/^(29|30)m$/);
  await expect(clock.locator("svg")).toHaveCount(1);
  const failed = rows.filter({ hasText: "Failed write" });
  await expect(failed.locator(".queue-state")).toHaveText("Write failed");
  await expect(failed.locator("[data-retry-item]")).toHaveCount(1);
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("packet-attention-rows.png"),
  });

  // Retry restarts the toss budget and queues the review again.
  await tossed.locator("[data-retry-item]").click();
  await expect(rows.filter({ hasText: "Tossed exit" })).toHaveCount(0);
  const snapshot = await store("monitoring_snapshot");
  expect(snapshot.items.find((item) => item.job.number === 23).state).toBe(
    "queued",
  );
  const states = await store("pr_states");
  expect(
    states.records.find((r) => r.binding.pull_request_id === "23").error_count,
  ).toBe(0);
});

test("panic detail shows cause Panic, the cooldown clock and the panic exit", async ({
  page,
  store,
}) => {
  const items = await seedPackets(store);
  await openDetail(page, store, items[24]);
  const status = page.locator("[data-queue-status]");
  await expect(status.locator(".queue-status-label")).toHaveAttribute(
    "data-failure-cause",
    "panic",
  );
  await expect(status.locator(".queue-status-label")).toHaveText("Panic");
  const facts = status.locator(".queue-facts");
  await expect(fact(facts, "Cause")).toHaveText("Failed / panic");
  await expect(fact(facts, "Next retry")).toHaveText(/^(29|30)m$/);
  await expect(fact(facts, "Next retry").locator("svg")).toHaveCount(1);
  await expect(page.locator("[data-queue-primary]")).toHaveText("Retry");
  const exit = activity(page, "exit");
  await expect(exit).toHaveAttribute("data-exit", "panic_exit");
  await expect(exit.locator(".activity-verb")).toHaveText("Panic");
  await expect(exit.locator(".activity-value")).toHaveText(/^(29|30)m$/);
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("packet-panic-detail.png"),
    fullPage: true,
  });
});

test("tossed and failed-write details carry typed causes", async ({
  page,
  store,
}) => {
  const items = await seedPackets(store);
  await openDetail(page, store, items[23]);
  let status = page.locator("[data-queue-status]");
  await expect(status.locator(".queue-status-label")).toHaveText("Tossed");
  await expect(fact(status.locator(".queue-facts"), "Next retry")).toHaveText(
    "Manual",
  );
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("packet-tossed-detail.png"),
    fullPage: true,
  });
  await openDetail(page, store, items[26]);
  status = page.locator("[data-queue-status]");
  await expect(status.locator(".queue-status-label")).toHaveAttribute(
    "data-failure-cause",
    "exit_failed",
  );
  await expect(status.locator(".queue-status-label")).toHaveText(
    "Write failed",
  );
  const facts = await expand(activity(page, "exit"));
  await expect(fact(facts, "Write")).toHaveText("failed");
  await expect(fact(facts, "Done")).toHaveText("1 of 2");
});

// A primary packet run plus one teammate report on PR 25; returns its queue item.
async function seedTeam(
  store,
  {
    leadExit = {
      exit: "ready_for_human",
      summary: "Small rename; behaviour unchanged.",
    },
    mateName = "Security reviewer",
    findings,
    consumedWrite = false,
  },
) {
  const fixture = await queueFixture(store);
  const settings = (await store("snapshot")).settings;
  const [first] = settings.agents;
  settings.agents.push({
    ...first,
    id: secondAgentId,
    name: mateName,
  });
  const repository = settings.repositories[0];
  const [primary] = repository.assignments;
  primary.tools = ["read_changes", "ready_for_human"];
  repository.primary_assignment_id = primary.id;
  repository.assignments.push({
    ...primary,
    id: secondAssignmentId,
    agent_id: secondAgentId,
    tools: ["read_changes", "submit_report"],
  });
  await store("seed_settings", settings);
  const work = (id, agent, order) => ({
    id,
    item_id: "item-team",
    iteration_id: "iteration-team",
    iteration: 1,
    agent_id: agent,
    enqueue_order: order,
    pass_ordinal: 1,
    trigger: "admission",
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
    state_seq: 0,
  });
  const lead = packetRun(fixture, 25, leadExit);
  lead.key = "work-primary";
  lead.job = { ...lead.job, work: work("work-primary", first.id, 1) };
  const mate = packetRun(fixture, 25, { exit: "submit_report" });
  mate.key = "work-teammate";
  mate.assignment_id = secondAssignmentId;
  mate.job = {
    ...mate.job,
    assignment_id: secondAssignmentId,
    work: work("work-teammate", secondAgentId, 2),
  };
  mate.selection = {
    ...mate.selection,
    agent: settings.agents.find((a) => a.id === secondAgentId),
  };
  mate.operation = { ...mate.operation, id: "review-teammate" };
  mate.result.output.report = { findings };
  const done = () =>
    write(lead, "completed", {
      consumed: true,
      steps: write(lead, "completed").steps.map((s) => ({
        ...s,
        state: "done",
        receipt: "C1",
      })),
    });
  await store("seed_queue_state", {
    jobs: [lead.job, mate.job],
    reviews: [lead, mate],
    publications: [],
    follow_ups: [],
    ...(consumedWrite && {
      feedback: { records: [], mentions: [], exit_writes: [done()] },
    }),
  });
  return (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 25,
  );
}

test("ready_for_human shows the local summary and the primary's teammate reports", async ({
  page,
  store,
}) => {
  const item = await seedTeam(store, {
    findings: [
      {
        file: "src/auth.rs",
        line: 42,
        nano: "token logged at debug",
        full: "The token reaches the debug log.",
        fix: "Redact before logging.",
      },
      {
        file: "src/auth.rs",
        line: 77,
        nano: "missing expiry check on refresh path; stale token accepted after logout until process restart",
        full: "Expiry is not checked.",
        fix: "Check exp claim.",
      },
    ],
  });
  expect(item).toMatchObject({
    state: "machine_signed_off",
    exit: "ready_for_human",
  });
  await openDetail(page, store, item);
  await expect(page.locator("[data-queue-synopsis]")).toHaveText(
    "Small rename; behaviour unchanged.",
  );
  const exit = activity(page, "exit");
  await expect(exit).toHaveAttribute("data-exit", "ready_for_human");
  await expect(exit.locator(".activity-value")).toHaveText("Local");
  const report = activity(page, "report");
  await expect(report).toHaveCount(1);
  await expect(report.locator(".activity-agent")).toHaveText(
    "Security reviewer",
  );
  await expect(report.locator(".activity-value")).toHaveText("2 findings");
  const facts = await expand(report);
  const finding = (place) =>
    facts.locator(`[data-finding="${place}"] .activity-finding-nano`);
  await expect(finding("src/auth.rs:42")).toHaveText("token logged at debug");
  await expect(finding("src/auth.rs:77")).toHaveText(
    "missing expiry check on refresh path; stale token accepted after logout until process restart",
  );
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("packet-ready-teammates.png"),
    fullPage: true,
  });
});

test("activity rows stay readable with a long agent, a long path and a long finding", async ({
  page,
  store,
}) => {
  const place =
    "raw/meeting/2026-09-18-gang-squad-agreement-experiment-0.md:20";
  const nano =
    "medium: Roster names become stale when the squad agreement changes; the experiment keeps the old owners and pings the wrong people for a week.";
  const item = await seedTeam(store, {
    leadExit: replies,
    mateName: "XGANG CAVEMAN YO",
    consumedWrite: true,
    findings: [
      {
        file: "raw/meeting/2026-09-18-gang-squad-agreement-experiment-0.md",
        line: 20,
        nano,
        full: "Stale roster.",
        fix: "Re-read the roster.",
      },
    ],
  });
  await openDetail(page, store, item);
  const report = activity(page, "report");
  const facts = await expand(report);
  const finding = facts.locator(`[data-finding="${place}"]`);
  await expect(finding.locator(".activity-finding-nano")).toHaveText(nano);
  await expect(finding.locator(".activity-finding-place")).toHaveAttribute(
    "title",
    place,
  );
  await expect(finding.locator(".activity-finding-place")).toHaveText(place);
  await expect(activity(page, "exit").locator(".activity-value")).toHaveText(
    "New 0 \u00b7 Replies 1 \u00b7 Resolved 1",
  );
  await page
    .locator("[data-queue-activity]")
    .evaluate((list) =>
      list
        .querySelectorAll('.activity-toggle[aria-expanded="false"]')
        .forEach((toggle) => toggle.click()),
    );
  await page.evaluate(() => window.__settingsIdle());
  const layout = await page
    .locator("[data-queue-activity]")
    .evaluate((list) => {
      const box = (element) => element.getBoundingClientRect();
      const problems = [];
      for (const row of list.children) {
        const width = box(row).width;
        for (const part of row.querySelectorAll(
          ".activity-toggle > span, .activity-finding-place, .activity-finding-nano",
        )) {
          const { width: w, height: h } = box(part);
          const line = parseFloat(getComputedStyle(part).lineHeight) || 18;
          const name = `${row.dataset.activity} ${part.className}`;
          // Nothing squeezed: head parts stay on one line and are never clipped
          // except the agent's deliberate ellipsis.
          if (part.closest(".activity-toggle")) {
            if (h > line * 1.6) problems.push(`${name} wraps (${h}px)`);
            if (
              !part.classList.contains("activity-agent") &&
              part.scrollWidth > part.clientWidth + 1
            )
              problems.push(`${name} clipped`);
          } else if (w < 40) problems.push(`${name} ${w}px wide`);
          if (
            part.classList.contains("activity-finding-nano") &&
            w < width * 0.6
          )
            problems.push(`${name} ${w}/${width}`);
        }
        for (const agent of row.querySelectorAll(".activity-agent"))
          if (box(agent).width < 40)
            problems.push(
              `${row.dataset.activity} agent ${box(agent).width}px`,
            );
      }
      return problems;
    });
  expect(layout).toEqual([]);
  await report.scrollIntoViewIfNeeded();
  await page.screenshot({ path: test.info().outputPath("activity-long.png") });
});
