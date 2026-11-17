import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

// Queue and Reviewed filter by repository through a drill-in checklist;
// the filter composes with the Ready / Needs attention cards and outcome chips.

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const chip = (page, list) =>
  page.locator(`[data-repository-filter-open="${list}"]`);
const picker = (page) => page.locator('[data-panel-view="repositories"]');
const option = (page, name) =>
  picker(page).getByRole("button", { name, exact: true });
const queueRows = (page) =>
  page
    .locator("#handoff-queue article[data-item-id]")
    .evaluateAll((rows) =>
      rows.map((row) => row.getAttribute("aria-label")).sort(),
    );
const reviewedRows = (page) =>
  page
    .locator('[data-panel-view="reviewed"] [data-job-id] .work-reference')
    .evaluateAll((rows) =>
      rows.map((row) => row.firstChild.textContent).sort(),
    );
const cards = async (page) => [
  await page.locator("[data-queue-ready]").textContent(),
  await page.locator("[data-queue-attention]").textContent(),
];

const portal = "contoso/Project/portal";
const mobile = "contoso/Project/mobile";

/** Five repositories on both providers: acme/web (#1 #2 #3 #9 from the queue
 * fixture), acme/api, and two Azure DevOps repositories. */
async function seed(store) {
  const fixture = await queueFixture(store);
  const { state, review, published } = fixture;
  for (const job of state.jobs) job.repository_name = "acme/web";
  const add = (number, decision, job) => {
    const run = review(number, decision);
    Object.assign(run.job, job);
    Object.assign(run.operation, {
      provider: run.job.provider,
      account_id: run.job.account_id,
      repository_id: run.job.repository_id,
    });
    const key = JSON.parse(run.key);
    key[0] = run.job.provider;
    key[1] = run.job.account_id;
    key[3] = run.job.repository_id;
    run.key = JSON.stringify(key);
    state.jobs.push(run.job);
    state.reviews.push(run);
    const publication = published(run);
    state.publications.push(publication);
    return publication;
  };
  const api = { repository_id: "101", repository_name: "acme/api" };
  const ado = (repository, name) => ({
    provider: "azure_devops",
    account_id: "00000000-0000-4000-8000-0000000000a1",
    account_login: "ado-operator",
    repository_id: `contoso/00000000-0000-0000-0000-0000000000b1/${repository}`,
    repository_name: name,
  });
  add(5, "machine_sign_off", api);
  add(
    6,
    "machine_sign_off",
    ado("00000000-0000-0000-0000-0000000000c1", portal),
  );
  // Needs attention: a publication whose outcome was lost.
  Object.assign(
    add(
      7,
      "machine_sign_off",
      ado("00000000-0000-0000-0000-0000000000c1", portal),
    ),
    {
      phase: "unresolved",
      uncertain: true,
      error: "Provider response was lost.",
      receipts: [],
    },
  ).operation.state = "manual_retry";
  add(
    8,
    "machine_sign_off",
    ado("00000000-0000-0000-0000-0000000000c2", mobile),
  );
  await store("seed_queue_state", state);
  return fixture;
}

async function open(page, list) {
  await chip(page, list).click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Repositories");
}

async function back(page, list) {
  await page.getByRole("button", { name: "Back" }).click();
  await expect(chip(page, list)).toBeFocused();
}

test("Queue repository filter narrows rows and card counts, multi-selects, persists and clears", async ({
  page,
  store,
}) => {
  await page.setViewportSize({ width: 408, height: 744 });
  await seed(store);
  await page.goto("/");
  await expect.poll(() => queueRows(page)).toHaveLength(7);
  const all = await queueRows(page);
  const whole = await cards(page);
  await expect(chip(page, "queue")).toHaveAccessibleName(
    "Repositories: All repos",
  );
  await expect(chip(page, "queue")).toHaveAttribute("aria-pressed", "false");

  // The drill-in lists repositories with rows, grouped by provider account.
  await open(page, "queue");
  const groups = picker(page).locator("section.repository-filter-group");
  await expect(groups).toHaveCount(2);
  await expect(groups.nth(0).locator("h2")).toHaveText("GHlocal-operator");
  await expect(groups.nth(1).locator("h2")).toHaveText("AZado-operator");
  await expect(option(page, "All repos 7")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(option(page, "web 3")).toBeVisible();
  await expect(option(page, "api 1")).toBeVisible();
  await expect(option(page, "portal 2")).toBeVisible();
  await expect(option(page, "mobile 1")).toBeVisible();

  await option(page, "portal 2").click();
  await expect(option(page, "portal 2")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(option(page, "All repos 7")).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  await back(page, "queue");
  await expect(chip(page, "queue")).toHaveAccessibleName(
    "Repositories: portal",
  );
  await expect
    .poll(() => queueRows(page))
    .toEqual([`${portal} #6`, `${portal} #7`]);
  await expect.poll(() => cards(page)).toEqual(["1", "1"]);

  // Multi-select: a second repository adds its rows.
  await open(page, "queue");
  await option(page, "mobile 1").click();
  await back(page, "queue");
  await expect(chip(page, "queue")).toHaveAccessibleName(
    "Repositories: 2 repos",
  );
  await expect
    .poll(() => queueRows(page))
    .toEqual([`${mobile} #8`, `${portal} #6`, `${portal} #7`]);
  await expect.poll(() => cards(page)).toEqual(["2", "1"]);

  // The selection survives other tabs.
  await tab(page, "Reviewed").click();
  await tab(page, "Running").click();
  await tab(page, "Queue").click();
  await expect(chip(page, "queue")).toHaveAccessibleName(
    "Repositories: 2 repos",
  );
  await expect.poll(() => queueRows(page)).toHaveLength(3);

  // All clears the selection and restores every row.
  await open(page, "queue");
  await option(page, "All repos 7").click();
  await expect(option(page, "portal 2")).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  await back(page, "queue");
  await expect(chip(page, "queue")).toHaveAccessibleName(
    "Repositories: All repos",
  );
  await expect.poll(() => queueRows(page)).toEqual(all);
  await expect.poll(() => cards(page)).toEqual(whole);
});

test("A filtered repository that leaves the queue drops out of the selection", async ({
  page,
  store,
}) => {
  const fixture = await seed(store);
  await page.goto("/");
  await expect.poll(() => queueRows(page)).toHaveLength(7);
  await open(page, "queue");
  await option(page, "api 1").click();
  await back(page, "queue");
  await expect.poll(() => queueRows(page)).toEqual(["acme/api #5"]);
  const state = fixture.state;
  const keep = (run) => run.job.repository_name !== "acme/api";
  await store("seed_queue_state", {
    ...state,
    jobs: state.jobs.filter((job) => job.repository_name !== "acme/api"),
    reviews: state.reviews.filter(keep),
    publications: state.publications.filter((p) => keep(p.review)),
  });
  await expect(chip(page, "queue")).toHaveAccessibleName(
    "Repositories: All repos",
    { timeout: 15_000 },
  );
  await expect.poll(() => queueRows(page)).toHaveLength(6);
});

test("Repository filter works from the keyboard and returns focus to its chip", async ({
  page,
  store,
}) => {
  await seed(store);
  await page.goto("/");
  await expect.poll(() => queueRows(page)).toHaveLength(7);
  await chip(page, "queue").focus();
  await page.keyboard.press("Enter");
  // The drill-in focuses the pressed choice.
  await expect(option(page, "All repos 7")).toBeFocused();
  await option(page, "web 3").focus();
  await page.keyboard.press("Space");
  await expect(option(page, "web 3")).toHaveAttribute("aria-pressed", "true");
  // The toggle redraws the list and keeps focus on the row pressed.
  await expect(option(page, "web 3")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(option(page, "web 3")).toHaveAttribute("aria-pressed", "false");
  await expect(option(page, "web 3")).toBeFocused();
  await page.keyboard.press("Space");
  await page.getByRole("button", { name: "Back" }).focus();
  await page.keyboard.press("Enter");
  await expect(chip(page, "queue")).toBeFocused();
  await expect
    .poll(() => queueRows(page))
    .toEqual(["acme/web #2", "acme/web #3", "acme/web #9"]);
});

test("Reviewed filters by repository and its outcome counts follow", async ({
  page,
  store,
}) => {
  await seed(store);
  await page.goto("/");
  await tab(page, "Reviewed").click();
  await expect.poll(() => reviewedRows(page)).toHaveLength(8);
  const outcomeTotal = () =>
    page
      .locator('[data-panel-view="reviewed"] [data-reviewed-outcome]')
      .evaluateAll((chips) =>
        chips.reduce((sum, chip) => sum + Number(chip.textContent), 0),
      );
  await expect.poll(outcomeTotal).toBe(8);
  await open(page, "reviewed");
  await option(page, "api 1").click();
  await option(page, "mobile 1").click();
  await back(page, "reviewed");
  await expect(chip(page, "reviewed")).toHaveAccessibleName(
    "Repositories: 2 repos",
  );
  await expect
    .poll(() => reviewedRows(page))
    .toEqual(["acme/api #5", `${mobile} #8`]);
  await expect.poll(outcomeTotal).toBe(2);
  // Queue keeps its own selection.
  await tab(page, "Queue").click();
  await expect(chip(page, "queue")).toHaveAccessibleName(
    "Repositories: All repos",
  );
  await tab(page, "Reviewed").click();
  await expect.poll(() => reviewedRows(page)).toHaveLength(2);
});

for (const width of [408, 320]) {
  test(`Queue repository chip sits between the cards and the list at ${width}px`, async ({
    page,
    store,
  }, testInfo) => {
    await page.setViewportSize({ width, height: 640 });
    await seed(store);
    await page.goto("/");
    await expect.poll(() => queueRows(page)).toHaveLength(7);
    const box = async (selector) =>
      page.locator(selector).first().boundingBox();
    const band = await box(".panel-context");
    const metrics = await box("[data-queue-metrics]");
    const filter = await box("[data-queue-repositories]");
    const list = await box("#handoff-queue");
    // Cards still overlap the band; the chip never does.
    expect(metrics.y).toBeCloseTo(band.y + band.height - 23, 0);
    expect(filter.y).toBeGreaterThanOrEqual(metrics.y + metrics.height);
    expect(list.y).toBeGreaterThanOrEqual(filter.y + filter.height);
    expect(filter.x + filter.width).toBeLessThanOrEqual(width);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);

    await page.screenshot({ path: testInfo.outputPath(`queue-${width}.png`) });

    // The drill-in's first card overlaps the band like other drill-ins.
    await open(page, "queue");
    await option(page, "portal 2").click();
    const first = await box('[data-panel-view="repositories"] > :first-child');
    const drillBand = await box(".panel-context");
    expect(first.y).toBeCloseTo(drillBand.y + drillBand.height - 23, 0);
    expect(first.x + first.width).toBeLessThanOrEqual(width);
    await page.screenshot({ path: testInfo.outputPath(`picker-${width}.png`) });
    await back(page, "queue");
    await page.screenshot({
      path: testInfo.outputPath(`filtered-${width}.png`),
    });
  });
}

test("A repository toggle keeps its click across a background refresh", async ({
  page,
  store,
}) => {
  // The page clock is paused, so the 5 s poller fires exactly at each tick.
  const start = new Date("2026-10-14T12:00:00Z");
  await page.clock.install({ time: start });
  await page.clock.pauseAt(start);
  await seed(store);
  await page.goto("/");
  await expect.poll(() => queueRows(page)).toHaveLength(7);
  await open(page, "queue");
  const target = option(page, "api 1");
  const box = await target.boundingBox();
  await page.clock.fastForward(4900);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.clock.fastForward(200);
  await page.evaluate(() => window.__settingsIdle());
  await page.mouse.up();
  await page.evaluate(() => window.__settingsIdle());
  await expect(target).toHaveAttribute("aria-pressed", "true");
  await back(page, "queue");
  await expect(chip(page, "queue")).toHaveAccessibleName("Repositories: api");
});
