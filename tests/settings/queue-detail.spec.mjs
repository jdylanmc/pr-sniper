import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test("queue item detail leads with status and one primary action; prose is gone", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const items = (await store("monitoring_snapshot")).items;
  const item = items.find((entry) => entry.job.number === 1);
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  const status = page.locator("[data-queue-status]");
  await expect(status).toHaveCount(1);
  await expect(status.locator("[data-queue-primary]")).toHaveCount(1);
  await expect(
    status.getByRole("button", { name: "Open PR on GitHub" }),
  ).toBeVisible();
  const evidence = page.locator("[data-item-evidence]");
  for (const prose of [
    "Personal review, comments and approval happen on GitHub",
    "Review eligibility",
    "Check schedule health",
    "PR identity and provenance",
  ])
    await expect(page.locator("body")).not.toContainText(prose);
  await expect(evidence.locator("details")).toHaveCount(0);
  await expect(page.locator("[data-queue-details]")).not.toHaveAttribute(
    "open",
  );
});

test("details disclosure is collapsed by default in the panel", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const item = (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 1,
  );
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  const details = page.locator("[data-queue-details]");
  await expect(details).toHaveCount(1);
  await expect(details).not.toHaveAttribute("open");
});
