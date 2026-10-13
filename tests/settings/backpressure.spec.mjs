import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const NOW = 1_800_000_000;
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

async function seedWaiting(store, patch = {}) {
  const fixture = await queueFixture(store);
  const waiting = fixture.state.reviews.find((r) => r.job.number === 3);
  Object.assign(waiting.operation, {
    state: "queued",
    attempt_count: 0,
    failure: null,
    initial_attempt_at: NOW,
    retry_deadline: NOW + 900,
    next_attempt_at: NOW,
    ...patch,
  });
  waiting.result = null;
  waiting.error = null;
  fixture.state.publications = fixture.state.publications.filter(
    (p) => p.review.job.number !== 3,
  );
  await store("seed_queue_state", fixture.state);
  return waiting;
}

async function expectQuietClockRow(page, store, until) {
  const work = (await store("automation_snapshot")).work.find((w) =>
    w.key.id.includes('"3"'),
  );
  expect(work).toMatchObject({
    state: "waiting",
    reason: null,
    failure: null,
    delayed_until: until,
  });
  await page.goto("/");
  await tab(page, "Running").click();
  const row = page
    .locator("[data-running-list] article")
    .filter({ hasText: "example/repo #3" });
  await expect(row).toHaveCount(1);
  const button = row.locator("button.work-row");
  await expect(button).toHaveAttribute("data-display", "waiting");
  await expect(button.locator(".work-marker svg")).toHaveCount(1);
  await expect(button.locator(".work-state")).toHaveText(/^\d{1,2}\D\d{2}/);
  await expect(row.locator(".review-failure, .work-retry-all")).toHaveCount(0);
  await tab(page, "Queue").click();
  await showQueueGroup(page, "attention");
  await expect(
    page
      .locator('[data-queue-group="attention"]')
      .getByRole("article", { name: "example/repo #3", exact: true }),
  ).toHaveCount(0);
}

test("a rate-limited run waits as a quiet clock row, never a failure", async ({
  page,
  store,
}) => {
  await seedWaiting(store, {
    next_attempt_at: NOW + 900,
    backpressure_until: NOW + 900,
    retry_deadline: NOW + 1800,
  });
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 3,
  );
  expect(item.state).not.toBe("failed");
  await expectQuietClockRow(page, store, NOW + 900);
});

test("a persisted account gate delays queued work without any failure", async ({
  page,
  store,
}) => {
  await seedWaiting(store);
  await store("seed_backpressure", {
    gates: [
      {
        scope: { provider: "github", account_id: "22" },
        until: NOW + 600,
        step: 2,
      },
    ],
  });
  await expectQuietClockRow(page, store, NOW + 600);
});

test("a gate on another account leaves the work runnable", async ({
  store,
}) => {
  await seedWaiting(store);
  await store("seed_backpressure", {
    gates: [
      {
        scope: { provider: "github", account_id: "999" },
        until: NOW + 600,
        step: 0,
      },
      {
        scope: { provider: "copilot", account_id: "999" },
        until: NOW + 600,
        step: 0,
      },
    ],
  });
  const work = (await store("automation_snapshot")).work.find((w) =>
    w.key.id.includes('"3"'),
  );
  expect(work.delayed_until ?? null).toBeNull();
});
