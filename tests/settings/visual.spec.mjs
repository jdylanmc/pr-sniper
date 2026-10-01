import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { captureInspector, nativeCapacity, expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { target } from "./paths.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const screenshots = join(target, "visual-correction-1");
test.use({ viewport: { width: 408, height: 744 } });

for (const failUtility of [false, true]) {
  test(`monitor polling survives detached recovery controls and delayed ${failUtility ? "failed" : "successful"} Diagnostics`, async ({
    page,
    store,
    ipc,
  }) => {
    const fixture = await queueFixture(store);
    await page.clock.install();
    await page.addInitScript((failUtility) => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__monitorReads = 0;
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command === "monitoring_snapshot") window.__monitorReads++;
        if (command === "diagnostics" && failUtility)
          return Promise.reject("Synthetic utility read failure");
        return original(command, args);
      };
    }, failUtility);
    await page.goto("/");
    await page.getByRole("button", { name: "Status", exact: true }).click();
    await expect(page.getByRole("button", { name: "Check Now" })).toBeVisible();
    const held = ipc.holdNext("snapshot");
    await page
      .getByRole("button", { name: "Diagnostics", exact: true })
      .click();
    await held.arrived;
    await expect(page.locator("#check-now")).toHaveCount(0);
    await page.clock.runFor(5100);
    held.release();
    if (failUtility)
      await expect(page.locator("[data-panel-error]")).toHaveText(
        "Synthetic utility read failure",
      );
    else
      await expect(page.locator('[data-panel-view="utility"]')).toContainText(
        "No host events recorded",
      );
    await tab(page, "Queue").click();
    await page.evaluate(() => window.__settingsIdle());
    const before = await page.evaluate(() => window.__monitorReads);
    for (const [index, title] of [
      "First automatic update",
      "Second automatic update",
    ].entries()) {
      fixture.state.jobs.find((job) => job.number === 9).title = title;
      await store("seed_queue_state", fixture.state);
      await page.clock.runFor(5100);
      await expect(page.locator("#handoff-queue")).toContainText(title);
      expect(await page.evaluate(() => window.__monitorReads)).toBe(
        before + index + 1,
      );
      await expect(page.locator("[data-panel-heading]")).toHaveText(
        "Your queue",
      );
    }
  });
}

test("compact human cards retain all ordered files and exact external links", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const ready = fixture.state.reviews.find((r) => r.job.number === 9);
  ready.result.output.files = Array.from({ length: 301 }, (_, index) => ({
    path: `file-${index + 1}.rs`,
    order: index + 1,
    explanation: `Step ${index + 1}.`,
  })).reverse();
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__destinations = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "open_queue_destination") {
        window.__destinations.push({
          args,
          url: await original("queue_destination", args),
        });
        return;
      }
      return original(command, args);
    };
  });
  await page.goto("/");
  const cards = page.locator("#handoff-queue article");
  await expect(cards).toHaveCount(3);
  await expect(page.locator("[data-summary-main]")).toHaveText("3 for you");
  await expect(page.locator("[data-summary-detail]")).toHaveText(
    "1 ready / 2 need attention",
  );
  await expect(page.locator("#handoff-queue")).not.toContainText("For agents");
  await mkdir(screenshots, { recursive: true });
  await page.locator(".panel-header strong").click();
  await page.screenshot({ path: join(screenshots, "queue.png") });
  await cards
    .filter({ hasText: "Fix reconnect race" })
    .getByRole("button", { name: "Evidence and actions" })
    .click();
  const evidence = page.locator("[data-item-evidence]");
  await expect(evidence).toContainText(
    "Personal review, comments and approval happen on GitHub",
  );
  await expect(evidence.getByRole("textbox")).toHaveCount(0);
  await page.screenshot({ path: join(screenshots, "queue-detail.png") });
  await evidence.getByRole("button", { name: "Open PR on GitHub" }).click();
  const guide = page.locator("#agent-reviews details").filter({
    has: page.getByText("Complete file guide (301 files)", { exact: true }),
  });
  await guide.locator("summary").click();
  await expect(guide.locator("li")).toHaveCount(301);
  await expect(guide.locator("li").first()).toHaveText("file-1.rs: Step 1.");
  await expect(guide.locator("li").last()).toHaveText("file-301.rs: Step 301.");
  await guide.getByRole("link", { name: "file-301.rs", exact: true }).click();
  const item = (await store("monitoring_snapshot")).items.find(
    (i) => i.job.number === 9,
  );
  expect(await page.evaluate(() => window.__destinations)).toEqual([
    {
      args: { itemId: item.id, file: null },
      url: "https://github.com/example/repo/pull/9",
    },
    {
      args: { itemId: item.id, file: "file-301.rs" },
      url: `https://github.com/example/repo/pull/9/files#diff-${createHash("sha256").update("file-301.rs").digest("hex")}`,
    },
  ]);
  await tab(page, "Reviewed").click();
  await expect(
    page.locator('[data-panel-view="reviewed"] article'),
  ).toHaveCount(4);
  await page.screenshot({ path: join(screenshots, "reviewed-shell.png") });
  await tab(page, "Settings").click();
  await expect(
    page.getByLabel("Settings section", { exact: true }),
  ).toBeVisible();
  await page.screenshot({ path: join(screenshots, "settings-shell.png") });
});

async function workFixture(store) {
  const fixture = await queueFixture(store);
  const base = fixture.review(9);
  const repository = fixture.settings.repositories[0];
  const agent = fixture.settings.agents[0];
  fixture.settings.doctrines = [
    {
      title: "Visual fixture",
      body: "Captured doctrine: read every changed file.",
    },
  ];
  fixture.settings.agents = Array.from({ length: 7 }, (_, index) => ({
    ...agent,
    id: `aaaaaaaa-aaaa-4aaa-8aaa-${String(index + 1).padStart(12, "0")}`,
    name: [
      "Scout",
      "Pathfinder",
      "Sentinel",
      "Sentry",
      "Clarity",
      "Compass",
      "Focus",
    ][index],
    doctrines: ["Visual fixture"],
  }));
  repository.assignments = fixture.settings.agents.map((agent, index) => ({
    id: `cccccccc-cccc-4ccc-8ccc-${String(index + 1).padStart(12, "0")}`,
    agent_id: agent.id,
    comment: false,
    schedule: { kind: "interval", minutes: 5, timezone: "UTC" },
  }));
  repository.primary_assignment_id = repository.assignments[0].id;
  const jobs = fixture.settings.agents.map((agent, index) => ({
    ...base.job,
    title: "Keep recent searches across sessions",
    assignment_id: repository.assignments[index].id,
    work: {
      id: `visual-work-${index + 1}`,
      item_id: "visual-item",
      iteration_id: "visual-iteration",
      iteration: 1,
      agent_id: agent.id,
      enqueue_order: index + 1,
      pass_ordinal: 1,
      trigger: "admission",
      admission: {
        watched_author: true,
        requested_reviewer: false,
        all_authors: false,
      },
    },
  }));
  const state = {
    jobs,
    publications: [],
    follow_ups: [],
    reviews: jobs.map((job, index) => ({
      ...structuredClone(base),
      key: job.work.id,
      assignment_id: job.assignment_id,
      job,
      selection: { ...base.selection, agent: fixture.settings.agents[index] },
      operation: {
        ...base.operation,
        id: `visual-operation-${index + 1}`,
        state: index < 4 ? "running" : "queued",
        attempt_count: index < 4 ? 1 : 0,
        next_attempt_at: 1_800_000_000,
      },
      phase: index < 4 ? "Reviewing" : "Waiting for capacity",
      result: null,
    })),
  };
  await store("seed_settings", fixture.settings);
  await store("seed_queue_state", state);
  const snapshot = await store("monitoring_snapshot");
  for (const run of state.reviews)
    run.selection = snapshot.reviews.find(
      (r) => r.key === run.key,
    ).planned_selection;
  await store("seed_queue_state", state);
  return { ...fixture, state, base };
}

test("seven assigned Agents use four actual reservations and three waiting rows, not one trigger or duplicate IDs", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  const ids = fixture.state.jobs.map((j) => j.work.id);
  const capacity = await store("fixture_capacity_snapshot", {
    activeIds: [ids[0], ...ids],
  });
  expect(capacity).toMatchObject({
    capacity: 4,
    active: 4,
    waiting: 3,
    blocked: 0,
  });
  expect(new Set(capacity.work.map((w) => w.key.id)).size).toBe(7);
  await nativeCapacity(page, [ids[0], ...ids]);
  await page.goto("/");
  await expect(page.locator("#handoff-queue article")).toHaveCount(0);
  await tab(page, "Running").click();
  const rows = page.locator("[data-running-list] article");
  await expect(rows).toHaveCount(7);
  await expect(rows.locator(".work-state")).toHaveText([
    "Running",
    "Running",
    "Running",
    "Running",
    "Queued",
    "Queued",
    "Queued",
  ]);
  await expect(page.locator("[data-summary-main]")).toHaveText("4 / 4 running");
  await expect(page.locator("[data-summary-detail]")).toHaveText("3 waiting");
  await expect(page.locator("[data-ai-work]")).toHaveCount(0);
  await expect(page.locator(".work-spin")).toHaveCount(4);
  expect(
    await rows
      .first()
      .locator(".work-spin")
      .evaluate((e) => getComputedStyle(e).animationName),
  ).toBe("work-turn");
  await mkdir(screenshots, { recursive: true });
  await page.screenshot({ path: join(screenshots, "running.png") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  expect(
    await rows
      .first()
      .locator(".work-spin")
      .evaluate((e) => getComputedStyle(e).animationName),
  ).toBe("none");
  await rows.first().getByRole("button", { name: "Open job" }).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-work-context]")).toHaveCount(1);
  await expect(page.locator("[data-work-context]")).toContainText(
    "Normal pass",
  );
  await expect(page.locator("[data-work-context]")).toContainText(
    "Captured rolePrimary Agent",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("[data-work-context]")).toContainText(
    "Review count1 for this Agent on this PR",
  );
  await expect(page.locator("[data-work-context]")).toContainText(
    "Retry attempt1",
  );
  await expect(page.locator(".job-status strong")).toHaveText("Running");
  await captureInspector(page, "normal-running");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(
    rows.first().getByRole("button", { name: "Open job" }),
  ).toBeFocused();
  await page.setViewportSize({ width: 320, height: 300 });
  await rows.last().getByRole("button").focus();
  await expect(rows.last().getByRole("button")).toBeInViewport();
  await expect(tab(page, "Settings")).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: join(screenshots, "running-narrow-reduced-motion.png"),
  });
  await page.evaluate(() => {
    window.__stoppingIds = ["visual-work-1"];
  });
  await tab(page, "Queue").click();
  await tab(page, "Running").click();
  await expect(page.locator("[data-summary-main]")).toHaveText("3 / 4 running");
  await expect(page.locator("[data-summary-detail]")).toHaveText(
    "3 waiting / 1 stopping",
  );
  await expect(page.locator("[data-running-count]")).toHaveText("4");
  await expect(rows.locator(".work-spin")).toHaveCount(3);
  await expect(
    page.locator('[data-job-id="normal:visual-work-1"] .work-state'),
  ).toHaveText("Stopping");
  await expect(
    page.locator('[data-job-id="normal:visual-work-1"] .work-spin'),
  ).toHaveCount(0);
});

test("normal inspector distinguishes waiting, running, stopping, failed, superseded and unknown capacity", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  await nativeCapacity(page, []);
  const run = fixture.state.reviews[0];
  for (const [state, label] of [
    ["queued", "Waiting"],
    ["running", "Running"],
    ["stopping", "Stopping"],
    ["failed", "Failed"],
    ["superseded", "Superseded"],
    ["unknown", "Activity unavailable"],
  ]) {
    run.operation.state = ["stopping", "unknown"].includes(state)
      ? "running"
      : state === "superseded"
        ? "failed"
        : state;
    run.operation.attempt_count = state === "queued" ? 0 : 2;
    fixture.state.jobs[0].waiting =
      state === "superseded" ? "superseded" : "human_start";
    await store("seed_queue_state", fixture.state);
    await page.goto("/");
    await page.evaluate(
      ({ id, state }) => {
        window.__activeIds = ["running", "stopping"].includes(state)
          ? [id]
          : [];
        window.__stoppingIds = state === "stopping" ? [id] : [];
        window.__capacityUnavailable = state === "unknown";
      },
      { id: run.key, state },
    );
    await page.evaluate(
      (id) =>
        window.__TAURI_INTERNALS__.invoke("panel_navigate", {
          route: {
            tab: "running",
            detail: { type: "job", kind: "normal", id },
          },
        }),
      run.key,
    );
    await expect(page.locator(".job-status strong")).toHaveText(label);
    await expect(page.locator(".job-hero .work-spin")).toHaveCount(
      state === "running" ? 1 : 0,
    );
    if (state === "superseded" || state === "queued")
      await captureInspector(page, `normal-${state}`);
  }
});

test("completion removes its row, capacity refills immediately and new blocked work appends at the tail", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  await nativeCapacity(
    page,
    fixture.state.jobs.slice(0, 4).map((j) => j.work.id),
  );
  await page.goto("/");
  await tab(page, "Running").click();
  await expect(page.locator(".work-spin")).toHaveCount(4);
  fixture.state.reviews[0].operation.state = "completed";
  fixture.state.reviews[0].result = fixture.base.result;
  fixture.state.reviews[0].phase = "Automated review complete";
  fixture.state.reviews[4].operation.state = "running";
  fixture.state.reviews[4].operation.attempt_count = 1;
  const added = structuredClone(fixture.state.reviews[6]);
  added.key = "visual-new-work";
  added.operation.id = "visual-new-operation";
  added.operation.state = "failed";
  added.operation.attempt_count = 2;
  added.error =
    "Synthetic provider unavailable; correct the account before retrying.";
  added.job = {
    ...added.job,
    pull_request_id: "10",
    number: 10,
    work: {
      ...added.job.work,
      id: added.key,
      item_id: "visual-new-item",
      iteration_id: "visual-new-iteration",
      enqueue_order: 8,
    },
  };
  fixture.state.jobs.push(added.job);
  fixture.state.reviews.push(added);
  await store("seed_queue_state", fixture.state);
  await page.evaluate(() => {
    window.__activeIds = [2, 3, 4, 5].map((n) => `visual-work-${n}`);
  });
  await tab(page, "Queue").click();
  await tab(page, "Running").click();
  const rows = page.locator("[data-running-list] article");
  await expect(rows).toHaveCount(7);
  await expect
    .poll(() => rows.evaluateAll((rows) => rows.map((r) => r.dataset.jobId)))
    .toEqual([
      ...[2, 3, 4, 5, 6, 7].map((n) => `normal:visual-work-${n}`),
      "normal:visual-new-work",
    ]);
  await expect(page.locator(".work-spin")).toHaveCount(4);
  await expect(rows.last()).toContainText(added.error);
  await expect(rows.last().locator(".work-state")).toHaveText("Failed");
  await expect(page.locator("[data-summary-detail]")).toHaveText(
    "2 waiting / 1 blocked",
  );
  await tab(page, "Queue").click();
  await expect(page.locator("#handoff-queue article")).toHaveCount(1);
  await expect(page.locator("#handoff-queue article")).toContainText(
    "example/repo #10",
  );
  await tab(page, "Running").click();
  await page.evaluate(() => {
    window.__capacityUnavailable = true;
  });
  await tab(page, "Queue").click();
  await tab(page, "Running").click();
  await expect(page.locator("[data-running-count]")).toHaveText("?");
  await expect(rows).toHaveCount(0);
  await expect(page.locator("[data-running-list]")).toContainText(
    "Work state unavailable",
  );
});

test("captured full configuration survives current Agent and doctrine edits without relabeling history", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  await nativeCapacity(page, ["visual-work-1"]);
  fixture.settings.agents[0].name = "Today's changed Agent";
  fixture.settings.agents[0].prompt = "Today's changed prompt";
  fixture.settings.doctrines[0].body = "Today's changed doctrine";
  await store("seed_settings", fixture.settings);
  await page.goto("/");
  await tab(page, "Running").click();
  const row = page.locator('[data-job-id="normal:visual-work-1"]');
  await expect(row).toContainText("Scout");
  await expect(row).not.toContainText("Today's changed Agent");
  await row.getByRole("button", { name: "Open job" }).click();
  const captured = page.locator("[data-work-context] > .work-configuration");
  await expect(captured).toContainText("Captured for this execution.");
  await expect(captured.locator("details")).not.toHaveAttribute("open");
  await expect(captured).toContainText("Review correctness.");
  await expect(captured).toContainText(
    "Captured doctrine: read every changed file.",
  );
  await expect(captured.locator("dl").first()).toContainText("AI account33");
  await expect(captured.locator("dl").first()).toContainText(
    "RolePrimary Agent",
  );
  await expect(captured).not.toContainText("Today's changed");
  await page.locator(".panel-content").evaluate((e) => {
    e.scrollTop = 0;
  });
  await page.screenshot({
    path: join(screenshots, "captured-job-detail.png"),
  });
});

test("selected historical normal pass keeps its ordinal and retry attempt after same-head reopening", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  const original = fixture.state.reviews[0];
  original.operation.state = "completed";
  original.operation.attempt_count = 3;
  original.result = fixture.base.result;
  const later = structuredClone(original);
  later.key = "reopened-normal";
  later.operation.id = "reopened-operation";
  later.job.work = {
    ...later.job.work,
    id: later.key,
    item_id: "reopened-item",
    iteration_id: "reopened-iteration",
    iteration: 2,
    pass_ordinal: 2,
    enqueue_order: 8,
    trigger: "reopened",
  };
  fixture.state.jobs.push(later.job);
  fixture.state.reviews.push(later);
  await store("seed_queue_state", fixture.state);
  await store("panel_navigate", {
    route: {
      tab: "reviewed",
      detail: { type: "job", kind: "normal", id: original.key },
    },
  });
  await page.goto("/");
  await expect(page.locator(".job-facts")).toContainText(
    "Review count1 for this Agent on this PR",
  );
  await expect(page.locator(".job-facts")).toContainText("Retry attempt3");
  await expect(page.locator(".job-facts")).toContainText("#9 / iteration 1");
  await expect(page.locator(".job-facts")).not.toContainText(
    "2 for this Agent",
  );
  await captureInspector(page, "normal-historical");
});

test("failed before first attempt retains readable planned configuration after queued edits, while executed jobs retain captured data", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  await nativeCapacity(
    page,
    fixture.state.jobs.slice(0, 4).map((job) => job.work.id),
  );
  await page.goto("/");
  await tab(page, "Running").click();
  await expect(page.locator("[data-summary-main]")).toHaveText("4 / 4 running");
  await expect(
    page.locator('[data-job-id="normal:visual-work-5"] .work-state'),
  ).toHaveText("Queued");
  fixture.settings.agents[4].prompt = "Changed while waiting; never executed.";
  fixture.settings.agents[4].model = "changed-model";
  fixture.settings.doctrines[0].body = "Updated planned doctrine.";
  await store("seed_settings", fixture.settings);
  const failed = fixture.state.reviews[4];
  failed.operation.state = "failed";
  failed.phase = "Failed before execution";
  failed.error = "Configuration changed before dispatch.";
  await store("seed_queue_state", fixture.state);
  await page
    .locator('[data-job-id="normal:visual-work-5"]')
    .getByRole("button")
    .click();
  await expect(page.locator(".job-status strong")).toHaveText("Failed");
  const planned = page.locator("[data-work-context] > .work-configuration");
  await expect(planned).toContainText(
    "Planned configuration; revalidated at start. No execution is claimed.",
  );
  await expect(planned).toContainText("Changed while waiting; never executed.");
  await expect(planned).toContainText("Updated planned doctrine.");
  await expect(planned).toContainText("changed-model");
  await expect(planned).toContainText("AI account33");
  await expect(planned).not.toContainText("Captured for this execution");
  await expect(page.locator(".job-facts")).toContainText(
    "Retry attemptNot started (0)",
  );
  await captureInspector(page, "normal-failed-zero");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(
    page.locator('[data-job-id="normal:visual-work-5"]').getByRole("button"),
  ).toBeFocused();
  await page
    .locator('[data-job-id="normal:visual-work-1"]')
    .getByRole("button")
    .click();
  await expect(planned).toContainText("Captured for this execution.");
  await expect(planned).toContainText(
    "Captured doctrine: read every changed file.",
  );
  await expect(planned).not.toContainText("Updated planned doctrine.");
  fixture.state.reviews[0].operation.state = "completed";
  fixture.state.reviews[0].result = fixture.base.result;
  await store("seed_queue_state", fixture.state);
  await page.evaluate(() => {
    window.__activeIds = ["visual-work-2"];
  });
  await tab(page, "Queue").click();
  await tab(page, "Reviewed").click();
  await page
    .locator(
      '[data-panel-view="reviewed"] [data-job-id="normal:visual-work-1"]',
    )
    .getByRole("button")
    .click();
  await expect(page.locator(".job-status strong")).toHaveText("Done");
  await expect(planned).toContainText(
    "Captured doctrine: read every changed file.",
  );
  const snapshot = await store("monitoring_snapshot");
  const parent = snapshot.items.find((item) =>
    item.review_keys.includes("visual-work-1"),
  );
  await expect(page.locator("[data-work-context]")).not.toContainText(
    parent.summary,
  );
  await captureInspector(page, "normal-done-active-sibling");
});

test("completed jobs do not adopt aggregate author-wait or ready-for-personal-review states", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const snapshot = await store("monitoring_snapshot");
  for (const number of [1, 9]) {
    const item = snapshot.items.find((item) => item.job.number === number);
    expect(item.state).toBe(
      number === 1 ? "waiting_for_author" : "machine_signed_off",
    );
    await store("panel_navigate", {
      route: {
        tab: "reviewed",
        detail: { type: "job", kind: "normal", id: item.review_keys[0] },
      },
    });
    await page.goto("/");
    await expect(page.locator(".job-status strong")).toHaveText("Done");
    await expect(page.locator("[data-work-context]")).not.toContainText(
      item.summary,
    );
    await expect(page.locator(".job-facts")).toContainText(
      "Review countNot recorded",
    );
    await expect(page.locator(".job-facts")).toContainText("Retry attempt1");
  }
});

test("polling preserves selected inspector controls on unrelated work and keyed focus and disclosures on its own transition", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  await nativeCapacity(page, ["visual-work-1"]);
  await page.clock.install();
  await page.goto("/");
  await tab(page, "Running").click();
  await page
    .locator('[data-job-id="normal:visual-work-1"]')
    .getByRole("button")
    .click();
  const context = page.locator("[data-work-context]");
  const link = context.getByRole("button", {
    name: "View pull request on GitHub",
  });
  const configuration = context.locator('[data-disclosure="configuration"]');
  const raw = context.locator('[data-disclosure="raw-configuration"]');
  const provenance = context.locator('[data-disclosure="provenance"]');
  await raw.locator("summary").click();
  await provenance.locator("summary").click();
  await link.focus();
  await context.evaluate((element) => {
    window.__selectedInspector = element;
    window.__selectedControls = [...element.querySelectorAll("button,summary")];
  });
  fixture.state.reviews[1].operation.attempt_count++;
  await store("seed_queue_state", fixture.state);
  await page.clock.runFor(5100);
  await page.evaluate(() => window.__settingsIdle());
  expect(
    await context.evaluate(
      (element) =>
        element === window.__selectedInspector &&
        [...element.querySelectorAll("button,summary")].every(
          (control, index) => control === window.__selectedControls[index],
        ),
    ),
  ).toBe(true);
  await expect(link).toBeFocused();
  for (const detail of [configuration, raw, provenance])
    await expect(detail).toHaveAttribute("open", "");
  fixture.state.reviews[0].operation.state = "completed";
  fixture.state.reviews[0].result = fixture.base.result;
  await store("seed_queue_state", fixture.state);
  await page.evaluate(() => {
    window.__activeIds = [];
  });
  await page.clock.runFor(5100);
  await expect(page.locator(".job-status strong")).toHaveText("Done");
  await expect(link).toBeFocused();
  for (const detail of [configuration, raw, provenance])
    await expect(detail).toHaveAttribute("open", "");
  await provenance.locator("summary").focus();
  fixture.state.reviews[0].operation.attempt_count++;
  await store("seed_queue_state", fixture.state);
  await page.clock.runFor(5100);
  await expect(page.locator(".job-facts")).toContainText("Retry attempt2");
  await expect(provenance.locator("summary")).toBeFocused();
  await expect(raw).toHaveAttribute("open", "");
});

test("seven failed Agents produce one human PR card; superseded and completed jobs stay distinct", async ({
  page,
  store,
}) => {
  const fixture = await workFixture(store);
  for (const run of fixture.state.reviews) {
    run.operation.state = "failed";
    run.error = `Synthetic failure for ${run.selection.agent.name}.`;
  }
  await store("seed_queue_state", fixture.state);
  await page.goto("/");
  await expect(page.locator("#handoff-queue article")).toHaveCount(1);
  await expect(page.locator("[data-summary-main]")).toHaveText("1 for you");
  await tab(page, "Running").click();
  await expect(page.locator("[data-running-list] article")).toHaveCount(7);
  await expect(page.locator(".work-state")).toHaveText(Array(7).fill("Failed"));
  await expect(page.locator(".work-spin")).toHaveCount(0);
  fixture.state.jobs[6].waiting = "superseded";
  fixture.state.reviews[0].operation.state = "completed";
  fixture.state.reviews[0].error = null;
  fixture.state.reviews[0].result = fixture.base.result;
  fixture.state.reviews[0].phase = "Automated review complete";
  await store("seed_queue_state", fixture.state);
  await tab(page, "Queue").click();
  await tab(page, "Running").click();
  await expect(page.locator("[data-running-list] article")).toHaveCount(6);
  await expect(
    page.locator('[data-job-id="normal:visual-work-7"] .work-state'),
  ).toHaveText("Superseded");
  await tab(page, "Reviewed").click();
  const completed = page.locator(
    '[data-panel-view="reviewed"] [data-job-id="normal:visual-work-1"]',
  );
  await expect(completed).toContainText("Automated review complete");
  await completed.getByRole("button", { name: "Open job" }).click();
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#agent-reviews")).toContainText("Scout");
  await expect(page.locator("#agent-reviews")).not.toContainText(
    "Synthetic failure for Pathfinder",
  );
});

test("human Queue excludes running and author-wait jobs and recovers an unavailable snapshot", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const waiting = fixture.state.reviews.find((r) => r.job.number === 3);
  waiting.operation.state = "queued";
  waiting.result = null;
  fixture.state.publications = fixture.state.publications.filter(
    (p) => p.review.job.number !== 3,
  );
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__failMonitoring = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "monitoring_snapshot" && window.__failMonitoring)
        return Promise.reject("Synthetic monitoring failure");
      return original(command, args);
    };
  });
  await page.goto("/");
  await expect(page.locator("#handoff-queue article")).toHaveCount(2);
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "example/repo #3",
  );
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "example/repo #1",
  );
  await page.evaluate(() => {
    window.__failMonitoring = true;
  });
  await tab(page, "Running").click();
  await tab(page, "Queue").click();
  await expect(page.locator("[data-summary-main]")).toHaveText(
    "Queue unavailable",
  );
  await expect(page.locator("#handoff-queue article")).toHaveCount(0);
  await expect(page.locator("[data-panel-error]")).toContainText(
    "not an empty successful check",
  );
  await page.evaluate(() => {
    window.__failMonitoring = false;
  });
  await tab(page, "Running").click();
  await tab(page, "Queue").click();
  await expect(page.locator("#handoff-queue article")).toHaveCount(2);
  await expect(page.locator("[data-summary-main]")).toHaveText("2 for you");
});
