import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test("bounded native pages retain stable cursors across fresh Store processes", async ({
  store,
}) => {
  const fixture = await queueFixture(store);
  const reviews = Array.from({ length: 23 }, (_, index) =>
    fixture.review(index + 1),
  );
  const state = {
    jobs: reviews.map((r) => r.job),
    reviews,
    publications: [],
    follow_ups: [],
  };
  await store("seed_queue_state", state);
  const first = await store("result_page", {
    request: { limit: 7, cursor: null },
  });
  expect(first.results).toHaveLength(7);
  expect(first.next_cursor).not.toBeNull();
  await store("seed_queue_state", state);
  expect(
    await store("result_page", { request: { limit: 7, cursor: null } }),
  ).toEqual(first);
  const ids = first.results.map((r) => r.item_id);
  let cursor = first.next_cursor;
  while (cursor) {
    const page = await store("result_page", { request: { limit: 7, cursor } });
    expect(page.results.length).toBeLessThanOrEqual(7);
    ids.push(...page.results.map((r) => r.item_id));
    cursor = page.next_cursor;
  }
  expect(ids).toHaveLength(23);
  expect(new Set(ids).size).toBe(23);
  const destination = { type: "item", item_id: ids[0] };
  const detail = await store("result_detail", { destination });
  expect(detail.status).toBe("available");
  expect(detail.destination).toEqual(destination);
  expect(detail.evidence.jobs).toHaveLength(1);
  expect(detail.evidence.reviews).toHaveLength(1);
  await expect(
    store("result_page", { request: { limit: 0, cursor: null } }),
  ).rejects.toContain("page size");
});

test("cleaned and missing destinations stay exact without changing settings", async ({
  store,
  dataRoot,
}) => {
  const fixture = await queueFixture(store);
  const review = fixture.review(1);
  const state = {
    jobs: [review.job],
    reviews: [review],
    publications: [fixture.published(review)],
    follow_ups: [],
  };
  await store("seed_queue_state", state);
  const item = (
    await store("result_page", { request: { limit: 1, cursor: null } })
  ).results[0].item_id;
  const settingsPath = join(dataRoot, "config/settings.json");
  const settings = await readFile(settingsPath);
  const job = review.job;
  const tracked = {
    provider: job.provider,
    configuration_id: job.configuration_id,
    account_id: job.account_id,
    repository_id: job.repository_id,
    pull_request_id: job.pull_request_id,
    number: job.number,
    head_sha: job.head_sha,
    lifecycle: "closed",
    terminal_observed: true,
    iteration_id: "fixture-iteration",
    item_id: item,
    iteration: 1,
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
    admitted_at: 100,
    observed_at: 101,
  };
  await store("seed_queue_state", { ...state, tracked: [tracked] });
  expect(await store("fixture_retention")).toEqual({ cleaned: 1 });
  expect(await store("fixture_retention")).toEqual({ cleaned: 0 });
  const destination = { type: "item", item_id: item };
  expect(await store("result_detail", { destination })).toMatchObject({
    status: "cleaned",
    destination,
  });
  const route = { tab: "reviewed", detail: destination };
  expect(await store("panel_navigate", { route })).toMatchObject({
    route,
    missing: expect.stringContaining("cleaned"),
  });
  const missing = { type: "item", item_id: "absent-exact-item" };
  expect(await store("result_detail", { destination: missing })).toMatchObject({
    status: "missing",
    destination: missing,
  });
  expect(await readFile(settingsPath)).toEqual(settings);
  expect(
    (await store("result_page", { request: { limit: 7, cursor: null } }))
      .results,
  ).toEqual([]);
});
