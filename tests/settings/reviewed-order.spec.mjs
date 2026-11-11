import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test.use({ viewport: { width: 400, height: 680 } });

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const reviewed = (page) => page.locator('[data-panel-view="reviewed"]');

/** A completed pass of PR `number`; `completed` absent = a legacy record. */
function pass(fixture, number, ordinal, started, completed) {
  const run = fixture.review(number);
  const id = `work-${number}-${ordinal}`;
  run.key = id;
  run.operation.id = `review-${number}-${ordinal}`;
  run.operation.initial_attempt_at = started;
  if (completed !== undefined) run.operation.completed_at = completed;
  run.job.work = {
    id,
    item_id: `item-${number}-${ordinal}`,
    iteration_id: `iteration-${number}`,
    iteration: 1,
    agent_id: fixture.settings.agents[0].id,
    enqueue_order: number * 10 + ordinal,
    pass_ordinal: ordinal,
    trigger: "admission",
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
  };
  return run;
}

test("Reviewed lists newest completions first, newest pass first, whatever the queue order", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const old = 1_790_000_000;
  const recent = 1_800_000_000;
  const runs = [
    // Weeks-old legacy passes (no completion time) of a re-enabled repository,
    // saved first in the queue.
    pass(fixture, 7, 1, old),
    pass(fixture, 7, 2, old + 600),
    pass(fixture, 7, 3, old + 1200),
    // Today's passes: started before the old ones' queue rows, completed later.
    pass(fixture, 8, 1, recent - 900, recent + 100),
    pass(fixture, 8, 2, recent, recent + 200),
    // Ties on completion time: newest pass, then a stable id.
    pass(fixture, 11, 2, recent, recent + 200),
    pass(fixture, 10, 1, recent, recent + 200),
  ];
  const seed = (jobs) =>
    store("seed_queue_state", {
      ...fixture.state,
      jobs,
      reviews: runs,
      publications: [],
      follow_ups: [],
    });
  const expected = [
    "normal:work-11-2",
    "normal:work-8-2",
    "normal:work-10-1",
    "normal:work-8-1",
    "normal:work-7-3",
    "normal:work-7-2",
    "normal:work-7-1",
  ];
  const order = () =>
    reviewed(page)
      .locator("[data-job-id]")
      .evaluateAll((rows) => rows.map((row) => row.dataset.jobId));

  await seed(runs.map((run) => run.job));
  await page.goto("/");
  await tab(page, "Reviewed").click();
  await expect.poll(order).toEqual(expected);

  // Re-enabling or reactivating rewrites queue rows (order, waiting state,
  // detection time); completed reviews keep their places.
  await seed(
    runs
      .map((run) => ({ ...run.job, waiting: "human_start", detected_at: 999 }))
      .reverse(),
  );
  await page.reload();
  await tab(page, "Reviewed").click();
  await expect.poll(order).toEqual(expected);
});
