import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

async function iterationFixture(store, count = 7) {
  const fixture = await queueFixture(store);
  const settings = fixture.settings;
  const base = fixture.review(9);
  const repository = settings.repositories[0];
  const agent = settings.agents[0];
  settings.agents = [];
  repository.assignments = [];
  const jobs = [];
  const admission = {
    watched_author: true,
    all_authors: false,
    requested_reviewer: true,
  };
  for (let index = 1; index <= count; index++) {
    const id = `aaaaaaaa-aaaa-4aaa-8aaa-${String(index).padStart(12, "0")}`;
    const assignment = `cccccccc-cccc-4ccc-8ccc-${String(index).padStart(12, "0")}`;
    settings.agents.push({ ...agent, id, name: `Agent ${index}` });
    repository.assignments.push({
      id: assignment,
      agent_id: id,
      schedule: { kind: "interval", minutes: index, timezone: "UTC" },
      comment: false,
    });
    jobs.push({
      ...base.job,
      assignment_id: assignment,
      work: {
        id: `work-${index}`,
        item_id: "iteration-first",
        iteration_id: "first",
        iteration: 1,
        agent_id: id,
        enqueue_order: index,
        pass_ordinal: 1,
        trigger: "admission",
        admission,
      },
    });
  }
  const state = {
    jobs,
    reviews: count
      ? [
          {
            ...base,
            key: jobs[0].work.id,
            assignment_id: jobs[0].assignment_id,
            job: structuredClone(jobs[0]),
            selection: { ...base.selection, agent: settings.agents[0] },
            operation: { ...base.operation, attempt_count: 3 },
          },
        ]
      : [],
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
        head_sha: base.job.head_sha,
        lifecycle: "open",
        iteration_id: "first",
        item_id: "iteration-first",
        iteration: 1,
        admission,
        admitted_at: 100,
        observed_at: 1_800_000_000,
      },
    ],
    monitoring: {
      global_scan: {
        schedule_key: "cron:*/15 * * * *:UTC",
        next_run: 1_800_000_900,
        pending: [],
        requested: false,
      },
      health: {
        [repository.id]: {
          repository_id: repository.id,
          name: repository.name,
          provider_account_id: "22",
          account_login: "local-operator",
          provider_repository_id: "100",
          schedule_key: "cron:*/15 * * * *:UTC",
          assignment_id: null,
          enabled: true,
          last_attempt: 1_800_000_000,
          last_success: 1_800_000_001,
          next_run: 1_800_000_900,
          schedule_available: true,
          last_failure: null,
          in_flight: false,
        },
      },
    },
  };
  await store("seed_settings", settings);
  await store("seed_queue_state", state);
  return { state, settings, base };
}

test("one global repository read exposes seven ordered independent jobs and immutable admission", async ({
  page,
  store,
}) => {
  await iterationFixture(store);
  const before = await store("monitoring_snapshot");
  expect(before.reviews).toHaveLength(7);
  expect(before.health).toHaveLength(1);
  await page.goto("/?view=queue");
  await expect(page.locator("#agent-reviews article")).toHaveCount(7);
  await expect(page.locator("#agent-reviews article h3")).toHaveText(
    Array.from(
      { length: 7 },
      (_, index) => `example/repo #9 / Agent ${index + 1}`,
    ),
  );
  for (let index = 1; index <= 7; index++) {
    const row = page.locator("#agent-reviews article").nth(index - 1);
    await expect(row).toContainText(
      `Normal pass 1; iteration 1 (first); queue order ${index}; work ID work-${index}`,
    );
  }
  await expect(page.locator("#agent-reviews article").first()).toContainText(
    "attempt 3",
  );
  await expect(page.locator("#schedule-health")).toContainText(
    "Global scan: cron:*/15 * * * *:UTC",
  );
  await expect(page.locator("#schedule-health")).toContainText(
    "repository read; fans out to the scan's assignments",
  );
  await expect(page.locator("#review-jobs article")).toHaveCount(7);
  await expect(page.locator("#review-jobs article").first()).toContainText(
    "Trigger: watched author and requested reviewer",
  );
  await page.reload();
  await expect(page.locator("#agent-reviews article")).toHaveCount(7);
  expect((await store("monitoring_snapshot")).jobs).toEqual(before.jobs);
});

for (const terminal of ["closed", "merged"]) {
  test(`verified ${terminal} history and a same-head reopen remain different selectable iterations`, async ({
    page,
    store,
  }) => {
    const { state } = await iterationFixture(store, 1);
    state.jobs[0].waiting = terminal;
    const next = structuredClone(state.jobs[0]);
    next.waiting = "human_start";
    next.work = {
      ...next.work,
      id: "reopened-work",
      item_id: "iteration-second",
      iteration_id: "second",
      iteration: 2,
      enqueue_order: 2,
      pass_ordinal: 2,
      trigger: "reopened",
    };
    state.jobs.push(next);
    Object.assign(state.tracked[0], {
      iteration_id: "second",
      iteration: 2,
      item_id: "iteration-second",
    });
    await store("seed_queue_state", state);
    const snapshot = await store("monitoring_snapshot");
    expect(snapshot.items).toHaveLength(2);
    expect(
      snapshot.items.find((item) => item.id === "iteration-first").state,
    ).toBe(terminal);
    await store("select_queue_item", { itemId: "iteration-first" });
    await page.goto("/?view=queue");
    await expect(page.locator("#agent-reviews article")).toHaveCount(1);
    await expect(page.locator("#agent-reviews")).toContainText(
      "work ID work-1",
    );
    await expect(page.locator("#handoff-queue")).toContainText(
      terminal === "closed" ? "Closed on GitHub" : "Merged on GitHub",
    );
    const reopened = page
      .locator("#handoff-queue article")
      .filter({ hasText: "Iteration 2 (second)" });
    await reopened
      .getByRole("button", { name: "Evidence and actions" })
      .click();
    await expect(page.locator("#agent-reviews")).toContainText(
      "Normal pass 2; iteration 2 (second); queue order 2",
    );
    await expect(page.locator("#agent-reviews")).toContainText(
      "cause: reopened",
    );
    expect((await store("monitoring_snapshot")).jobs[0].head_sha).toBe(
      next.head_sha,
    );
  });
}

test("a retained legacy destination alias selects its adopted iteration without retargeting", async ({
  page,
  store,
}) => {
  const { state } = await iterationFixture(store, 1);
  state.jobs[0].work.legacy_item_id = "legacy-exact-destination";
  await store("seed_queue_state", state);
  await store("select_queue_item", { itemId: "legacy-exact-destination" });
  await page.goto("/?view=queue");
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#handoff-queue")).toContainText(
    "Showing evidence and actions for the selected account, repository and revision.",
  );
  await expect(
    page.locator('#handoff-queue article[data-selected="true"]'),
  ).toHaveCount(1);
  expect(
    await store("queue_destination", { itemId: "legacy-exact-destination" }),
  ).toBe("https://github.com/example/repo/pull/9");
});

test("sticky tracking without assignments is visible without fabricating Agent jobs", async ({
  page,
  store,
}) => {
  await iterationFixture(store, 0);
  await page.goto("/?view=queue");
  await expect(page.locator("#review-jobs")).toContainText(
    "tracked; no Agent jobs yet",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  expect((await store("monitoring_snapshot")).tracked).toHaveLength(1);
});
