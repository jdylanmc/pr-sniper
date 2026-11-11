import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test.use({ viewport: { width: 400, height: 680 } });

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const reviewed = (page) => page.locator('[data-panel-view="reviewed"]');
const chip = (page, outcome) =>
  reviewed(page).locator(`[data-reviewed-outcome="${outcome}"]`);
const states = (page) =>
  reviewed(page)
    .locator("[data-job-id]")
    .evaluateAll((items) =>
      Object.fromEntries(
        items.map((item) => [
          item.dataset.jobId,
          item.querySelector(".work-state").textContent,
        ]),
      ),
    );
const rows = (page) =>
  reviewed(page)
    .locator("[data-job-id]")
    .evaluateAll((items) => items.map((item) => item.dataset.jobId).sort());

async function seed(store) {
  const fixture = await queueFixture(store);
  const exits = {
    7: { exit: "wait", reason: "ci" },
    8: { exit: "ready_for_human", summary: "Over to you." },
    10: { exit: "submit_report" },
    11: { exit: "ready_for_human", summary: "Over to you." },
  };
  const runs = Object.entries(exits).map(([number, exit]) => {
    const run = fixture.review(Number(number));
    run.result.output.exit = exit;
    return run;
  });
  await store("seed_queue_state", {
    ...fixture.state,
    jobs: runs.map((run) => run.job),
    reviews: runs,
    publications: [],
    follow_ups: [],
  });
  return Object.fromEntries(
    runs.map((run) => [run.job.number, `normal:${run.key}`]),
  );
}

test("Reviewed rows show their outcome and outcome chips filter the list", async ({
  page,
  store,
}) => {
  const id = await seed(store);
  await page.goto("/");
  await tab(page, "Reviewed").click();
  await expect.poll(() => rows(page)).toHaveLength(4);
  const group = reviewed(page).getByRole("group", { name: "Outcome" });
  await expect(group.getByRole("button")).toHaveCount(3);
  await expect(chip(page, "ready_for_human")).toHaveAccessibleName(
    "Ready for human 2",
  );
  await expect(chip(page, "waiting")).toHaveAccessibleName("Waiting 1");
  await expect(chip(page, "report")).toHaveAccessibleName("Report 1");
  await expect
    .poll(() => states(page))
    .toEqual({
      [id[7]]: "Waiting",
      [id[8]]: "Ready for human",
      [id[10]]: "Report",
      [id[11]]: "Ready for human",
    });

  await chip(page, "ready_for_human").click();
  await expect(chip(page, "ready_for_human")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect.poll(() => rows(page)).toEqual([id[8], id[11]].sort());
  // Counts stay whole-list while filtered.
  await expect(chip(page, "waiting")).toHaveAccessibleName("Waiting 1");

  // The selection survives leaving and reopening the tab.
  await tab(page, "Queue").click();
  await tab(page, "Reviewed").click();
  await expect(chip(page, "ready_for_human")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect.poll(() => rows(page)).toEqual([id[8], id[11]].sort());
});

/** Focuses an outcome chip and toggles it with `key`. A window focus event
 * from a busy test host can restore the heading first; a key that lands
 * there changes nothing, so the press is retried. */
async function toggle(page, outcome, key, pressed) {
  await expect(async () => {
    if (
      (await chip(page, outcome).getAttribute("aria-pressed")) !==
      String(pressed)
    ) {
      await chip(page, outcome).focus();
      await page.keyboard.press(key);
    }
    await expect(chip(page, outcome)).toHaveAttribute(
      "aria-pressed",
      String(pressed),
      { timeout: 1000 },
    );
  }).toPass();
  // The toggle redraws the chips and keeps focus on the one pressed.
  await expect(chip(page, outcome)).toBeFocused();
}

test("Outcome chips toggle from the keyboard and keep focus", async ({
  page,
  store,
}) => {
  const id = await seed(store);
  await page.goto("/");
  await tab(page, "Reviewed").click();
  await expect.poll(() => rows(page)).toHaveLength(4);

  await toggle(page, "waiting", "Space", true);
  await expect.poll(() => rows(page)).toEqual([id[7]]);

  // Multi-select: a second outcome adds its rows.
  await toggle(page, "ready_for_human", "Enter", true);
  await expect.poll(() => rows(page)).toEqual([id[7], id[8], id[11]].sort());

  // Clearing every chip shows every row again.
  await toggle(page, "ready_for_human", "Enter", false);
  await toggle(page, "waiting", "Space", false);
  await expect.poll(() => rows(page)).toHaveLength(4);
});
