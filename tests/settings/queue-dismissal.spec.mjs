import { readFile, readdir, mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

async function staleFixture(store, published = false) {
  const fixture = await queueFixture(store);
  const review = fixture.review(9);
  review.job.waiting = "superseded";
  const binding = Object.fromEntries(
    [
      "provider",
      "configuration_id",
      "account_id",
      "repository_id",
      "pull_request_id",
    ].map((key) => [key, review.job[key]]),
  );
  fixture.state = {
    jobs: [review.job],
    reviews: [review],
    publications: published ? [fixture.published(review)] : [],
    follow_ups: [],
    pr_states: {
      records: [
        {
          binding,
          iteration_id: "iteration-1",
          hash: "state-1",
          baseline_hash: "state-1",
          state_seq: 1,
        },
      ],
    },
  };
  await store("seed_queue_state", fixture.state);
  return fixture;
}

async function item(store) {
  return (await store("monitoring_snapshot")).items[0];
}

function dismiss(store, entry) {
  return store("dismiss_stale_review", {
    itemId: entry.id,
    expectedToken: entry.dismiss_token,
  });
}

test("dismiss stale review persists across Store restart and reload without deleting history", async ({
  page,
  store,
  dataRoot,
}, testInfo) => {
  await staleFixture(store, true);
  const before = await item(store);
  const stateDir = join(dataRoot, "state");
  const preserved = new Map(
    await Promise.all(
      (await readdir(stateDir))
        .filter((name) => name.endsWith(".json"))
        .map(async (name) => [
          name,
          await readFile(join(stateDir, name), "utf8"),
        ]),
    ),
  );
  await page.goto("/?panel=1");
  await showQueueGroup(page, "attention");
  const button = page.getByRole("button", {
    name: "Dismiss stale review",
    exact: true,
  });
  await expect(page.locator("[data-queue-attention]")).toHaveText("1");
  await page.screenshot({
    path: testInfo.outputPath("stale-dismiss-control.png"),
  });
  await button.focus();
  await button.press("Enter");
  await expect(page.locator("[data-queue-attention]")).toHaveText("0");
  await expect(
    page.getByRole("heading", { name: "Your queue", exact: true }),
  ).toBeFocused();
  await expect(button).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("stale-dismissed.png") });
  await page.reload();
  await expect(page.locator("[data-queue-attention]")).toHaveText("0");
  // Each bridge call opens a fresh Store in a separate process.
  const after = await item(store);
  expect(after.dismissed).toBe(true);
  expect(after.state).toBe("stale_after_publication");
  expect(after.review_keys).toEqual(before.review_keys);
  for (const [name, bytes] of preserved) {
    // Navigation alone updates the panel/selection files.
    if (["panel.json", "queue-selection.json"].includes(name)) continue;
    expect(await readFile(join(stateDir, name), "utf8"), name).toBe(bytes);
  }
  expect(await store("queue_destination", { itemId: before.id })).toContain(
    "/pull/9",
  );
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Reviewed", exact: true })
    .click();
  await expect(page.getByRole("list", { name: "Reviewed work" })).toContainText(
    "Fix reconnect race",
  );
});

test("later meaningful PR state and head changes are not dismissed by an earlier click", async ({
  store,
}) => {
  const fixture = await staleFixture(store);
  const old = await item(store);
  await dismiss(store, old);
  expect((await item(store)).dismissed).toBe(true);
  fixture.state.pr_states.records[0].hash = "state-2";
  fixture.state.pr_states.records[0].state_seq = 2;
  await store("seed_queue_state", fixture.state);
  const changed = await item(store);
  expect(changed.dismissed).toBe(false);
  expect(changed.dismiss_token).not.toBe(old.dismiss_token);
  await expect(dismiss(store, old)).rejects.toContain("changed");
  await dismiss(store, changed);
  fixture.state.jobs[0].head_sha = "c".repeat(40);
  const key = JSON.parse(fixture.state.reviews[0].key);
  key[5] = fixture.state.jobs[0].head_sha;
  fixture.state.reviews[0].key = JSON.stringify(key);
  await store("seed_queue_state", fixture.state);
  const newHead = await item(store);
  expect(newHead.dismissed).toBe(false);
  expect(newHead.id).not.toBe(old.id);
});

test("dismissal cannot cross an account boundary or dismiss current non-stale work", async ({
  store,
}) => {
  const fixture = await staleFixture(store);
  const old = await item(store);
  const other = fixture.review(9);
  other.job.account_id = "44";
  other.job.waiting = "superseded";
  const key = JSON.parse(other.key);
  key[1] = "44";
  other.key = JSON.stringify(key);
  fixture.state.jobs.push(other.job);
  fixture.state.reviews.push(other);
  await store("seed_queue_state", fixture.state);
  await dismiss(store, old);
  const entries = (await store("monitoring_snapshot")).items;
  const separate = entries.find((entry) => entry.job.account_id === "44");
  expect(separate.dismissed).toBe(false);
  await expect(
    store("dismiss_stale_review", {
      itemId: separate.id,
      expectedToken: old.dismiss_token,
    }),
  ).rejects.toContain("changed");
  fixture.state.jobs[0].waiting = "human_start";
  await store("seed_queue_state", fixture.state);
  await expect(dismiss(store, old)).rejects.toContain("changed");
});

test("uncertain publication resurfaces a dismissed stale entry and cannot be dismissed", async ({
  store,
}) => {
  const fixture = await staleFixture(store, true);
  const old = await item(store);
  await dismiss(store, old);
  fixture.state.publications[0].uncertain = true;
  await store("seed_queue_state", fixture.state);
  const unsafe = await item(store);
  expect(unsafe.dismissed).toBe(false);
  expect(unsafe.dismiss_token).toBeNull();
  await expect(dismiss(store, old)).rejects.toContain("unsettled");
  const snapshot = await store("monitoring_snapshot");
  expect(snapshot.publications[0].publication.uncertain).toBe(true);
  expect(snapshot.publications[0].publication.receipts).toEqual(
    fixture.state.publications[0].receipts,
  );
});

test("an original uncertain action remains intact and prevents stale dismissal", async ({
  store,
}) => {
  const fixture = await staleFixture(store);
  const old = await item(store);
  await dismiss(store, old);
  const review = fixture.state.reviews[0];
  const effect = {
    id: "uncertain-action",
    final_id: "original-final",
    item_id: old.id,
    action: "approve",
    state: "uncertain",
    error: null,
    receipt: null,
    body: "Original request",
    operation: {
      ...review.operation,
      id: "original-action",
      operation_type: "github_approve",
      state: "manual_retry",
    },
    observation: {
      node_id: "PR_fixture",
      repository_id: "100",
      pull_request_id: "9",
      account_id: "22",
      author_id: "11",
      head_repository_id: "100",
      head: review.job.head_sha,
      base: review.job.observed_base_sha,
      base_name: "main",
      merge_rules: [],
      merge_rules_error: null,
      state: "OPEN",
      draft: false,
      permission: "WRITE",
      write_capability: true,
      mergeable: "MERGEABLE",
      merge_state: "CLEAN",
      review_decision: null,
      checks: "SUCCESS",
      check_contexts: [],
      in_merge_queue: false,
      method: "SQUASH",
      protection: null,
      threads: [],
      reviews: [],
      comments: [],
      merged_by: null,
      merged_at: null,
      merge_commit: null,
    },
  };
  fixture.state.actions = { finals: [], observations: [], effects: [effect] };
  await store("seed_queue_state", fixture.state);
  const unsafe = await item(store);
  expect(unsafe.dismissed).toBe(false);
  expect(unsafe.dismiss_token).toBeNull();
  await expect(dismiss(store, old)).rejects.toContain("unsettled");
  expect(unsafe.action_status.effects[0]).toMatchObject(effect);
});

test("in-flight review cannot be dismissed and failed persistence does not acknowledge it", async ({
  store,
  dataRoot,
}) => {
  const fixture = await staleFixture(store);
  const old = await item(store);
  fixture.state.reviews[0].operation.state = "running";
  await store("seed_queue_state", fixture.state);
  expect((await item(store)).dismiss_token).toBeNull();
  await expect(dismiss(store, old)).rejects.toContain("unsettled");
  fixture.state.reviews[0].operation.state = "completed";
  await store("seed_queue_state", fixture.state);
  const path = join(dataRoot, "state", "queue-dismissals.json");
  await mkdir(path);
  await expect(dismiss(store, old)).rejects.toContain("dismissals");
  await rm(path, { recursive: true });
  expect((await item(store)).dismissed).toBe(false);
});

test("dismissal errors keep the card visible", async ({ page, store }) => {
  await staleFixture(store);
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "dismiss_stale_review"
        ? Promise.reject("Dismiss failed")
        : invoke(command, args);
  });
  await page.goto("/?panel=1");
  await showQueueGroup(page, "attention");
  const button = page.getByRole("button", {
    name: "Dismiss stale review",
    exact: true,
  });
  await button.click();
  await expect(button).toBeEnabled();
  await expect(page.locator("[data-queue-attention]")).toHaveText("1");
  expect((await item(store)).dismissed).toBe(false);
});

test("dismissal completion does not steal focus after navigation", async ({
  page,
  store,
  ipc,
}) => {
  await staleFixture(store);
  await page.goto("/?panel=1");
  await showQueueGroup(page, "attention");
  const held = ipc.holdNext("dismiss_stale_review");
  const button = page.getByRole("button", {
    name: "Dismiss stale review",
    exact: true,
  });
  await button.focus();
  await button.press("Enter");
  await held.arrived;
  const reviewed = page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Reviewed", exact: true });
  await reviewed.click();
  const heading = page.getByRole("heading", { name: "Reviewed", exact: true });
  await expect(heading).toBeFocused();
  held.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(heading).toBeFocused();
});
