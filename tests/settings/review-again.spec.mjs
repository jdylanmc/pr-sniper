import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

// One packet-mode PR whose only pass completed with a machine sign-off.
async function reviewedFixture(
  store,
  operationState = "completed",
  removed = null,
) {
  const fixture = await queueFixture(store);
  const run = fixture.review(9);
  run.job.work = {
    id: "work-9",
    item_id: "item-9",
    iteration_id: "first",
    iteration: 1,
    agent_id: run.selection.agent.id,
    enqueue_order: 1,
    pass_ordinal: 1,
    trigger: "admission",
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
    state_seq: 1,
  };
  run.key = run.job.work.id;
  run.mode = "packet";
  run.packet = {
    state_id: "1:5f1c2e9a",
    hash: "f".repeat(64),
    schema: 1,
    tool_set: ["read_changes", "ready_for_human"],
    exits_offered: ["ready_for_human"],
    exits_denied: [],
  };
  run.result.output = {
    synopsis: "",
    files: [],
    findings: [],
    decision: "human_input_required",
    exit: { exit: "ready_for_human", summary: "Small rename." },
  };
  run.result.coverage = { read: 3, changed: 5 };
  run.operation.state = operationState;
  if (operationState !== "completed") run.result = null;
  const binding = Object.fromEntries(
    [
      "provider",
      "configuration_id",
      "account_id",
      "repository_id",
      "pull_request_id",
    ].map((key) => [key, run.job[key]]),
  );
  const jobs = [structuredClone(run.job)];
  const reviews = [run];
  if (removed) {
    // A teammate whose assignment was removed while its run was unfinished.
    const teammate = structuredClone(run);
    teammate.assignment_id = "00000000-0000-4000-8000-0000000000ee";
    teammate.job.assignment_id = teammate.assignment_id;
    teammate.job.waiting = "assignment_removed";
    teammate.job.work.id = "work-9-removed";
    teammate.job.work.enqueue_order = 2;
    teammate.key = teammate.job.work.id;
    teammate.operation = {
      ...teammate.operation,
      id: "review-9-removed",
      state: removed,
    };
    teammate.result = null;
    jobs.push(structuredClone(teammate.job));
    reviews.push(teammate);
  }
  await store("seed_queue_state", {
    jobs,
    reviews,
    publications: [],
    follow_ups: [],
    pr_states: {
      records: [
        {
          binding,
          iteration_id: "first",
          hash: "state-1",
          baseline_hash: "state-1",
          state_seq: 1,
        },
      ],
    },
  });
  const item = (await store("monitoring_snapshot")).items[0];
  if (operationState === "completed")
    expect(item.state).toBe("machine_signed_off");
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  return item;
}

test("Review again starts a new pass for a reviewed PR and keeps its evidence", async ({
  page,
  store,
}, testInfo) => {
  const item = await reviewedFixture(store);
  await page.goto("/");
  const status = page.locator("[data-queue-status]");
  const again = status.getByRole("button", {
    name: "Review again",
    exact: true,
  });
  await expect(again).toBeVisible();
  await expect(again).not.toContainText(/\S/);
  await page.screenshot({ path: testInfo.outputPath("review-again.png") });
  await again.focus();
  await again.press("Enter");
  await expect(again).toHaveCount(0);
  await expect(page.locator("[data-panel-heading]")).toBeFocused();
  const state = await store("monitoring_snapshot");
  // The item now tracks the new, not yet started pass of the same PR.
  const passes = state.reviews.filter((candidate) =>
    state.items[0].review_keys.includes(candidate.key),
  );
  expect(passes).toHaveLength(1);
  expect(passes[0].key).not.toBe("work-9");
  expect(passes[0].run ?? null).toBeNull();
  expect(state.items[0].id).toBe(item.id);
  const record = (await store("pr_states")).records[0];
  expect(record.review_again_seq).toBe(2);
});

test("Review again is absent while a pass is pending or failed", async ({
  page,
  store,
}) => {
  for (const operationState of ["queued", "running", "failed"]) {
    await reviewedFixture(store, operationState);
    await page.goto("/");
    await expect(page.locator("[data-queue-status]")).toHaveCount(1);
    await expect(
      page.getByRole("button", { name: "Review again", exact: true }),
    ).toHaveCount(0);
  }
});

test("Review again is absent for a legacy run without packet work", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 9,
  );
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  await expect(page.locator("[data-queue-status]")).toHaveCount(1);
  await expect(
    page.getByRole("button", { name: "Review again", exact: true }),
  ).toHaveCount(0);
});

test("a removed teammate's unfinished run does not hide Review again", async ({
  page,
  store,
}) => {
  for (const removed of ["queued", "failed"]) {
    const item = await reviewedFixture(store, "completed", removed);
    expect(item.review_keys).toContain("work-9-removed");
    await page.goto("/");
    await expect(
      page
        .locator("[data-queue-status]")
        .getByRole("button", { name: "Review again", exact: true }),
    ).toBeVisible();
  }
});
