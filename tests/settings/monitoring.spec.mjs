import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

// Schedule health and detected pull requests lived only on the removed Status
// screen; the queue surfaces never render them or a Check Now control.
test("Review Queue surfaces render no schedule recovery or Check Now controls", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  expect((await store("monitoring_snapshot")).items.length).toBeGreaterThan(0);
  for (const path of ["/?view=queue", "/"]) {
    await page.goto(path);
    await page.evaluate(() => window.__settingsIdle?.());
    for (const selector of [
      "#schedule-health",
      "#review-jobs",
      "#check-now",
      "[data-monitor-recovery]",
    ])
      await expect(page.locator(selector)).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Check Now", exact: true }),
    ).toHaveCount(0);
  }
});
