import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { target } from "./paths.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const screenshots = join(target, "visual-77-79");
test.use({ viewport: { width: 408, height: 744 } });

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

async function nativeCapacity(page, activeIds) {
  await page.addInitScript((activeIds) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__activeIds = activeIds;
    window.__stoppingIds = [];
    window.__capacityUnavailable = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "automation_snapshot") {
        if (window.__capacityUnavailable)
          return Promise.reject("Synthetic capacity failure");
        return original("fixture_capacity_snapshot", {
          activeIds: window.__activeIds,
          stoppingIds: window.__stoppingIds,
        });
      }
      return original(command, args);
    };
  }, activeIds);
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
    "Purpose: Normal pass",
  );
  await expect(page.locator("[data-work-context]")).toContainText(
    "Captured role: Primary Agent",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("[data-work-context]")).toContainText(
    "normal pass ordinal 1",
  );
  await expect(page.locator("[data-work-context]")).toContainText(
    "Retry attempt for this job: 1",
  );
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
  await expect(page.locator(".work-spin")).toHaveCount(3);
  await expect(
    page.locator('[data-job-id="normal:visual-work-1"] .work-state'),
  ).toHaveText("Stopping");
  await expect(
    page.locator('[data-job-id="normal:visual-work-1"] .work-spin'),
  ).toHaveCount(0);
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
  const captured = page.locator(".work-configuration").filter({
    has: page.getByText("Captured execution configuration", {
      exact: true,
    }),
  });
  await captured.locator("summary").click();
  await expect(captured).toContainText("Review correctness.");
  await expect(captured).toContainText(
    "Captured doctrine: read every changed file.",
  );
  await expect(captured).toContainText('"account_id": "33"');
  await expect(captured).toContainText('"primary": true');
  await expect(captured).not.toContainText("Today's changed");
  await page.locator(".panel-content").evaluate((e) => {
    e.scrollTop = 0;
  });
  await page.screenshot({
    path: join(screenshots, "captured-job-detail.png"),
  });
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
