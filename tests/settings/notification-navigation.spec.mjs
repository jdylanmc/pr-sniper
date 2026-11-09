import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const stateChanged = (page) =>
  page.evaluate(async () => {
    window.__appEvent("pr-sniper:state-changed", {});
    await new Promise((resolve) => setTimeout(resolve, 100));
    await window.__settingsIdle();
  });

async function azureDevOpsItem(store) {
  const { state, review } = await queueFixture(store);
  const ado = review(4);
  Object.assign(ado.job, {
    provider: "azure_devops",
    repository_id:
      "contoso/00000000-0000-0000-0000-0000000000b1/00000000-0000-0000-0000-0000000000c2",
    repository_name: "contoso/Project/Repo",
  });
  Object.assign(ado.operation, {
    provider: ado.job.provider,
    repository_id: ado.job.repository_id,
  });
  const key = JSON.parse(ado.key);
  key[0] = ado.job.provider;
  key[3] = ado.job.repository_id;
  ado.key = JSON.stringify(key);
  state.jobs.push(ado.job);
  state.reviews.push(ado);
  await store("seed_queue_state", state);
  return (await store("monitoring_snapshot")).items.find(
    (item) => item.job.provider === "azure_devops",
  );
}

const click = (page, itemId, race) =>
  page.evaluate(
    async ([itemId, race]) => {
      await window.__TAURI_INTERNALS__.invoke("fixture_notification_click", {
        itemId,
        race,
      });
      await window.__settingsIdle();
    },
    [itemId, race],
  );

test("a notification click raced by the app reopen opens its exact Azure DevOps item", async ({
  page,
  store,
}) => {
  const item = await azureDevOpsItem(store);
  await page.goto("/");
  await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
  await click(page, item.id, "reopen");
  await expect(page.locator("[data-panel-heading]")).toHaveText(
    "Saved evidence",
  );
  await expect(page.locator("[data-queue-status]")).toContainText(
    "contoso/Project/Repo",
  );
  expect(await store("queue_selection")).toBe(item.id);
  await expect(page.locator("[data-panel-error]")).toBeHidden();
  expect((await store("snapshot")).error).toBeNull();
});

test("a superseded notification navigation never shows an error in Queue or Settings", async ({
  page,
  store,
}) => {
  const item = await azureDevOpsItem(store);
  await page.goto("/");
  await click(page, item.id, "navigation");
  await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
  await expect(page.locator("[data-panel-error]")).toBeHidden();
  expect((await store("snapshot")).error).toBeNull();
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Settings");
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#error")).toBeHidden();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.locator("[data-panel-error]")).toBeHidden();
});

test("Ready rows show once the chosen Needs attention card empties", async ({
  page,
  store,
}) => {
  const { state } = await queueFixture(store);
  await page.goto("/");
  await page.locator("[data-queue-filter=attention]").click();
  await expect(
    page.locator('[data-queue-group="attention"] .queue-item').first(),
  ).toBeVisible();
  const kept = (entry) =>
    (entry.job ?? entry.review?.job ?? entry).number === 9;
  for (const key of ["jobs", "reviews", "publications"])
    state[key] = state[key].filter(kept);
  await store("seed_queue_state", state);
  await stateChanged(page);
  await expect(page.locator("[data-queue-attention]")).toHaveText("0");
  await expect(page.locator("[data-queue-filter=ready]")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(
    page.getByRole("article", { name: "example/repo #9", exact: true }),
  ).toBeVisible();
  // An explicit choice of an already empty card still holds.
  await page.locator("[data-queue-filter=attention]").click();
  await stateChanged(page);
  await expect(page.locator("[data-queue-filter=attention]")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});
