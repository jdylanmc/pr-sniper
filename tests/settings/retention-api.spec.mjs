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

test("scoped unavailable mentions cannot hide or starve healthy result pages", async ({
  store,
}) => {
  const fixture = await queueFixture(store);
  function review(number, other = false) {
    const value = fixture.review(number);
    if (other) {
      for (const identity of [value.job, value.operation]) {
        identity.account_id = "23";
        identity.configuration_id = "00000000-0000-4000-8000-000000000004";
        identity.repository_id = "200";
      }
      value.job.repository_name = "example/other";
      value.job.account_login = "other-operator";
      value.key = JSON.stringify([
        value.job.provider,
        value.job.account_id,
        value.job.configuration_id,
        value.job.repository_id,
        value.job.pull_request_id,
        value.job.head_sha,
        value.job.trigger_policy,
        value.assignment_id,
      ]);
    }
    return value;
  }
  function itemId(job) {
    return Buffer.from(
      JSON.stringify([
        job.provider,
        job.account_id,
        job.configuration_id,
        job.repository_id,
        job.pull_request_id,
        job.head_sha,
        job.trigger_policy,
      ]),
    ).toString("base64url");
  }
  const reviews = Array.from({ length: 23 }, (_, i) =>
    review(i + 1, i % 2 === 1),
  );
  const tracked = [];
  const mentions = Array.from({ length: 31 }, (_, i) => {
    const job = review(i === 1 ? 2 : 100 + i, i % 2 === 1).job;
    const binding = {
      configuration_id: job.configuration_id,
      account_id: job.account_id,
      account_login: job.account_login,
      repository_id: job.repository_id,
      repository_name: job.repository_name,
      pull_request_id: job.pull_request_id,
      number: job.number,
    };
    tracked.push({
      provider: job.provider,
      ...binding,
      head_sha: job.head_sha,
      lifecycle: "open",
      terminal_observed: false,
      iteration_id: `tracked-${i}`,
      item_id: itemId(job),
      iteration: i % 2 === 0 ? 1 : 2,
      admission: {
        watched_author: true,
        all_authors: false,
        requested_reviewer: false,
      },
      admitted_at: 100,
      observed_at: 120,
    });
    // Tracking has no display login/name fields.
    delete tracked[i].account_login;
    delete tracked[i].repository_name;
    return {
      ...(i % 2 === 0 ? { item_id: itemId(job) } : {}),
      key: JSON.stringify([
        "mention",
        "github",
        binding.account_id,
        binding.configuration_id,
        binding.repository_id,
        binding.pull_request_id,
        `comment-${i}`,
      ]),
      work_id: `unavailable-${String(i).padStart(2, "0")}`,
      enqueue_order: 100 + i,
      enqueued_at: 110,
      binding,
      comment: {
        id: `comment-${i}`,
        body: "@operator explain",
        author_id: "11",
        author_login: "author",
        created_at: "2026-10-02T00:00:00Z",
        updated_at: "2026-10-02T00:00:00Z",
      },
      follow_up_id: null,
      blocked: "No execution available.",
    };
  });
  const state = {
    jobs: reviews.map((r) => r.job),
    reviews,
    tracked,
    publications: [],
    follow_ups: [],
    feedback: { records: [], mentions },
  };
  await store("seed_queue_state", state);
  const request = { limit: 3, cursor: null, unavailable_cursor: null };
  const first = await store("result_page", { request });
  expect(first.results).toHaveLength(3);
  expect(first.unavailable).toHaveLength(3);
  expect(first.unavailable_count).toBe(31);
  expect(first.unavailable[0].message).toContain("no review job exists");
  expect(first.unavailable[1].message).toContain(
    "Legacy mention iteration is unavailable",
  );
  expect(first.unavailable[1].message).toContain(
    "GitHub account 23, repository example/other (200)",
  );
  expect(await store("result_page", { request })).toEqual(first);
  await store("seed_queue_state", state);
  expect(await store("result_page", { request })).toEqual(first);

  // Each invocation is a fresh native Store process. Traverse independently so
  // exhausting either stream never restarts it or starves the other.
  const ids = first.results.map((r) => r.item_id);
  let cursor = first.next_cursor;
  while (cursor) {
    const page = await store("result_page", {
      request: { ...request, cursor },
    });
    expect(page.results.length).toBeLessThanOrEqual(3);
    expect(page.unavailable.length).toBeLessThanOrEqual(3);
    expect(page.unavailable_count).toBe(31);
    ids.push(...page.results.map((r) => r.item_id));
    cursor = page.next_cursor;
  }
  expect(new Set(ids)).toEqual(new Set(reviews.map((r) => itemId(r.job))));
  expect(ids).toHaveLength(23);
  const unavailable = [...first.unavailable];
  let unavailableCursor = first.next_unavailable_cursor;
  while (unavailableCursor) {
    const page = await store("result_page", {
      request: { ...request, unavailable_cursor: unavailableCursor },
    });
    expect(page.results).toHaveLength(3);
    expect(page.unavailable.length).toBeLessThanOrEqual(3);
    unavailable.push(...page.unavailable);
    unavailableCursor = page.next_unavailable_cursor;
  }
  expect(unavailable.map((u) => u.destination.id)).toEqual(
    mentions.map((m) => m.work_id),
  );
  for (const entry of unavailable) {
    expect(
      await store("result_detail", { destination: entry.destination }),
    ).toMatchObject({
      status: "available",
      destination: entry.destination,
      evidence: { work_id: entry.destination.id, follow_up_id: null },
    });
  }

  // A real row now backs one intent; new uncertainty on either side of an
  // existing keyset position must not invalidate or reshuffle healthy pages.
  const nowAvailable = review(100);
  state.jobs.push(nowAvailable.job);
  state.reviews.push(nowAvailable);
  for (const suffix of ["00a", "99"]) {
    const added = structuredClone(mentions[2]);
    added.work_id = `unavailable-${suffix}`;
    added.comment.id += suffix;
    const key = JSON.parse(added.key);
    key[key.length - 1] = added.comment.id;
    added.key = JSON.stringify(key);
    mentions.push(added);
  }
  await store("seed_queue_state", state);
  const evolved = await store("result_page", { request });
  expect(evolved.unavailable_count).toBe(32);
  expect(evolved.results[0].item_id).toBe(itemId(nowAvailable.job));
  expect(evolved.results[0].conversations).toBe(1);
  expect(evolved.unavailable[0].destination.id).toBe("unavailable-00a");
  const tail = await store("result_page", {
    request: {
      ...request,
      cursor: first.next_cursor,
      unavailable_cursor: first.next_unavailable_cursor,
    },
  });
  expect(tail.results.map((r) => r.item_id)).toEqual(ids.slice(3, 6));
  expect(tail.unavailable.map((u) => u.destination.id)).toEqual([
    "unavailable-03",
    "unavailable-04",
    "unavailable-05",
  ]);
  expect(await store("result_page", { request })).toEqual(evolved);
  for (const invalid of [
    { version: 2, work_id: "unavailable-01" },
    { version: 1, work_id: "" },
    { version: 1, work_id: "x".repeat(16_385) },
  ]) {
    await expect(
      store("result_page", {
        request: { ...request, unavailable_cursor: invalid },
      }),
    ).rejects.toContain("Unavailable cursor is invalid");
  }
});
