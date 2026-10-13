import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const NOW = 1_800_000_000;
const secondAgentId = "00000000-0000-4000-8000-000000000013";
const secondAssignmentId = "00000000-0000-4000-8000-000000000012";
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

// One PR, a packet-mode primary and one teammate for the same state.
async function seedTeam(store, teammate) {
  const fixture = await queueFixture(store);
  const settings = (await store("snapshot")).settings;
  const [first] = settings.agents;
  settings.agents.push({
    ...first,
    id: secondAgentId,
    name: "Security reviewer",
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
  const base = fixture.review(5);
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
  const primaryJob = {
    ...base.job,
    work: work("work-primary", first.id, 1),
  };
  const teammateJob = {
    ...base.job,
    assignment_id: secondAssignmentId,
    work: work("work-teammate", secondAgentId, 2),
  };
  const run = {
    ...base,
    key: "work-teammate",
    assignment_id: secondAssignmentId,
    job: teammateJob,
    selection: {
      ...base.selection,
      agent: settings.agents.find((a) => a.id === secondAgentId),
    },
    operation: {
      ...base.operation,
      id: "review-teammate",
      initial_attempt_at: NOW,
      retry_deadline: NOW + 900,
      ...teammate.operation,
    },
    result: null,
    phase: "Waiting",
    error: teammate.error ?? null,
  };
  await store("seed_queue_state", {
    jobs: [primaryJob, teammateJob],
    reviews: [run],
    publications: [],
    follow_ups: [],
  });
  return (await store("monitoring_snapshot")).items.find(
    (item) => item.job.number === 5,
  );
}

const failedOperation = {
  state: "failed",
  failure: "permanent",
  attempt_count: 1,
  next_attempt_at: null,
};

test("a parked teammate is one Needs attention row naming the agent and cause", async ({
  page,
  store,
}) => {
  const item = await seedTeam(store, {
    operation: failedOperation,
    error: "Agent configuration is invalid.",
  });
  expect(item.state).toBe("failed");
  expect(item.failed_teammate).toEqual({
    assignment_id: secondAssignmentId,
    agent_name: "Security reviewer",
    cause: "permanent",
  });
  const items = (await store("monitoring_snapshot")).items.filter(
    (entry) => entry.job.number === 5,
  );
  expect(items).toHaveLength(1);

  await page.goto("/");
  await tab(page, "Queue").click();
  await showQueueGroup(page, "attention");
  const rows = page.locator("[data-queue-group='attention'] .queue-item");
  await expect(rows).toHaveCount(1);
  const chip = rows.first().locator("[data-queue-teammate]");
  await expect(chip).toHaveAttribute("data-teammate-cause", "permanent");
  await expect(chip).toContainText("Security reviewer");
  await expect(chip).toContainText("Permanent");
  await expect(chip.locator("svg")).toHaveCount(1);
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("teammate-failed-row.png"),
  });

  await rows
    .first()
    .getByRole("button", { name: "Evidence and actions" })
    .click();
  const facts = page.locator("[data-queue-status] .queue-facts");
  await expect(facts.locator('dt:text-is("Teammate") + dd')).toHaveText(
    "Security reviewer",
  );
  await expect(facts.locator('dt:text-is("Cause") + dd')).toHaveText(
    "Failed / permanent",
  );
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("teammate-failed-detail.png"),
  });
});

test("the primary shows blocked on the teammate in Running until its report exists", async ({
  page,
  store,
}) => {
  const item = await seedTeam(store, {
    operation: {
      state: "queued",
      failure: null,
      attempt_count: 0,
      next_attempt_at: NOW,
    },
  });
  expect(item.failed_teammate).toBeNull();
  expect(item.blocked_by).toEqual({
    teammate: {
      assignment_id: secondAssignmentId,
      agent_name: "Security reviewer",
    },
  });
  const work = (await store("automation_snapshot")).work.find(
    (entry) => entry.key.id === "work-primary",
  );
  expect(work).toMatchObject({ state: "blocked" });
  expect(work.blocked_by.teammate.agent_name).toBe("Security reviewer");

  await page.goto("/");
  await tab(page, "Running").click();
  const chip = page.locator("[data-queue-teammate]");
  await expect(chip).toHaveCount(1);
  await expect(chip).toHaveAttribute("data-teammate-state", "waiting");
  await expect(chip).toContainText("Security reviewer");
  await page.evaluate(() => window.__settingsIdle());
  await page.screenshot({
    path: test.info().outputPath("teammate-blocked-primary.png"),
  });
});
