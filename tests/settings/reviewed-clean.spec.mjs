import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test.use({ viewport: { width: 400, height: 680 } });

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const itemId = (job) =>
  Buffer.from(
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
const bindingOf = (job) => ({
  provider: job.provider,
  configuration_id: job.configuration_id,
  account_id: job.account_id,
  repository_id: job.repository_id,
  pull_request_id: job.pull_request_id,
});

async function seed(store, extra = 0) {
  const fixture = await queueFixture(store);
  const [merged, closed, open] = [
    fixture.state.reviews.find((r) => r.job.number === 1).job,
    fixture.state.reviews.find((r) => r.job.number === 2).job,
    fixture.state.reviews.find((r) => r.job.number === 9).job,
  ];
  merged.waiting = "merged";
  closed.waiting = "closed";
  const tracked = [merged, closed, open].map((job, index) => ({
    provider: job.provider,
    configuration_id: job.configuration_id,
    account_id: job.account_id,
    repository_id: job.repository_id,
    pull_request_id: job.pull_request_id,
    number: job.number,
    head_sha: job.head_sha,
    lifecycle: ["merged", "closed", "open"][index],
    terminal_observed: index < 2,
    iteration_id: `iteration-${job.number}`,
    item_id: itemId(job),
    iteration: 1,
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
    admitted_at: 100,
    observed_at: 120,
  }));
  const extras = Array.from({ length: extra }, (_, n) =>
    fixture.review(1000 + n),
  );
  const state = {
    ...fixture.state,
    jobs: [merged, closed, open, ...extras.map((r) => r.job)],
    reviews: [
      ...fixture.state.reviews.filter((r) => [1, 2, 9].includes(r.job.number)),
      ...extras,
    ],
    publications: [],
    follow_ups: [],
    tracked,
    pr_states: {
      records: [merged, closed, open].map((job) => ({
        binding: bindingOf(job),
        iteration_id: `iteration-${job.number}`,
        state_seq: 1,
        baseline_hash: "baseline",
      })),
    },
  };
  await store("seed_queue_state", state);
  return { merged, closed, open };
}

const reviewed = (page) => page.locator('[data-panel-view="reviewed"]');
const records = async (store) =>
  (await store("pr_states")).records.map((r) => r.binding.pull_request_id);

test("Reviewed cleans one terminal PR and leaves open PRs and other terminal PRs", async ({
  page,
  store,
}) => {
  const { merged, closed, open } = await seed(store);
  await page.goto("/");
  await tab(page, "Reviewed").click();
  // Only terminal rows carry a Clean action.
  await expect(reviewed(page).locator("[data-clean-item]")).toHaveCount(2);
  await expect(
    reviewed(page).getByRole("button", { name: "Clean all" }),
  ).toContainText("2");
  await page.screenshot({ path: test.info().outputPath("reviewed-clean.png") });
  await reviewed(page)
    .locator(`[data-clean-item="${itemId(merged)}"]`)
    .click();
  await expect(reviewed(page).locator("[data-clean-item]")).toHaveCount(1);
  await expect(
    reviewed(page).locator(`[data-clean-item="${itemId(closed)}"]`),
  ).toBeVisible();
  await expect(reviewed(page)).toContainText("example/repo #9");
  expect(await records(store)).toEqual(
    expect.arrayContaining([closed.pull_request_id, open.pull_request_id]),
  );
  expect(await records(store)).not.toContain(merged.pull_request_id);
});

test("Reviewed Clean all removes every terminal PR and keeps the open one", async ({
  page,
  store,
}) => {
  const { open } = await seed(store);
  await page.goto("/");
  await tab(page, "Reviewed").click();
  await reviewed(page).getByRole("button", { name: "Clean all" }).click();
  await expect(reviewed(page).locator("[data-clean-item]")).toHaveCount(0);
  await expect(
    reviewed(page).getByRole("button", { name: "Clean all" }),
  ).toHaveCount(0);
  await expect(reviewed(page)).toContainText("example/repo #9");
  await expect(reviewed(page)).not.toContainText("example/repo #1");
  await expect(reviewed(page)).not.toContainText("example/repo #2");
  expect(await records(store)).toEqual([open.pull_request_id]);
  // The open PR's job and review survive.
  expect((await store("monitoring_snapshot")).jobs).toHaveLength(1);
});

/** The seeded terminal/open PRs plus `extra` completed reviews. */
const seedMany = (store, extra) => seed(store, extra);

const band = async (page) =>
  (await page.locator(".panel-context").boundingBox()).y +
  (await page.locator(".panel-context").boundingBox()).height;

test("Reviewed shows 50 rows, appends 50 per Show more, keeps Clean all last and clips under the band", async ({
  page,
  store,
}, testInfo) => {
  await page.setViewportSize({ width: 408, height: 744 });
  const { merged } = await seedMany(store, 127);
  await page.goto("/");
  await tab(page, "Reviewed").click();
  const rows = reviewed(page).locator("[data-job-id]");
  await expect(rows).toHaveCount(50);
  const total = 132;
  const more = reviewed(page).getByRole("button", { name: "Show 50 more" });
  await expect(more).toHaveText(`+${total - 50}`);
  // The list card overlaps the band like Queue; Clean all sits after Show more.
  const list = await reviewed(page).locator(".work-list").boundingBox();
  expect(list.y).toBeCloseTo((await band(page)) - 23, 0);
  const clean = reviewed(page).getByRole("button", { name: "Clean all" });
  await expect(clean).toContainText("2");
  const order = await reviewed(page).evaluate((root) =>
    [...root.children].map((child) => child.className),
  );
  expect(order).toEqual([
    "work-list",
    "work-more",
    "work-retry-all work-clean-all",
  ]);
  await page.screenshot({ path: testInfo.outputPath("reviewed-top.png") });

  await more.click();
  await expect(rows).toHaveCount(100);
  await expect(rows.nth(50).locator(".work-row")).toBeFocused();
  const rest = reviewed(page).getByRole("button", {
    name: `Show ${total - 100} more`,
  });
  await expect(rest).toHaveText(`+${total - 100}`);
  // A background redraw (merged PR and its review cleaned elsewhere) keeps the expanded count.
  await store("clean_reviewed", { itemId: itemId(merged) });
  const after = reviewed(page).getByRole("button", {
    name: `Show ${total - 102} more`,
  });
  await expect(after).toHaveText(`+${total - 102}`, { timeout: 12_000 });
  await expect(rows).toHaveCount(100);
  await after.click();
  await expect(rows).toHaveCount(total - 2);
  await expect(rows.nth(100).locator(".work-row")).toBeFocused();
  await expect(reviewed(page).locator(".work-more")).toHaveCount(0);

  // Scrolled rows never draw over the title band.
  const scroller = page.locator(".panel-content");
  await scroller.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await page.waitForTimeout(100);
  const edge = await band(page);
  const covered = await page.evaluate((y) => {
    const box = document
      .querySelector(".panel-context")
      .getBoundingClientRect();
    const hits = [];
    for (let x = box.left + 20; x < box.right - 20; x += 40)
      for (const dy of [1, 6, 12, 18, 22])
        hits.push(
          document.elementFromPoint(x, y - dy)?.closest(".panel-context") !==
            null,
        );
    return hits.every(Boolean);
  }, edge);
  expect(covered).toBe(true);
  await expect(clean).toContainText("1");
  await expect(clean).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("reviewed-scrolled.png") });

  // Reopening the tab starts at one page again.
  await tab(page, "Queue").click();
  await tab(page, "Reviewed").click();
  await expect(rows).toHaveCount(50);
});
