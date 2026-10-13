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

async function seed(store) {
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
  const state = {
    ...fixture.state,
    jobs: [merged, closed, open],
    reviews: fixture.state.reviews.filter((r) =>
      [1, 2, 9].includes(r.job.number),
    ),
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
