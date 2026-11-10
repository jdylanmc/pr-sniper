import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { repositorySettings } from "./navigation.mjs";

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

test("a reopened panel shows Ready rows once the chosen Needs attention card emptied", async ({
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
  // While open, the explicit choice holds.
  await stateChanged(page);
  await expect(page.locator("[data-queue-attention]")).toHaveText("0");
  await expect(page.locator("[data-queue-filter=attention]")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await page.evaluate(async () => {
    await window.__TAURI_INTERNALS__.invoke("hide_panel", {});
    await window.__TAURI_INTERNALS__.invoke("fixture_show_panel", {});
    await window.__settingsIdle();
  });
  await expect(page.locator("[data-queue-filter=ready]")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(
    page.getByRole("article", { name: "example/repo #9", exact: true }),
  ).toBeVisible();
});

test("a card chosen right after a reopen holds before its first snapshot lands", async ({
  page,
  store,
  ipc,
}) => {
  const { state } = await queueFixture(store);
  await page.goto("/");
  await expect(
    page.locator('[data-queue-group="attention"] .queue-item').first(),
  ).toBeAttached();
  const kept = (entry) =>
    (entry.job ?? entry.review?.job ?? entry).number === 9;
  for (const key of ["jobs", "reviews", "publications"])
    state[key] = state[key].filter(kept);
  await store("seed_queue_state", state);
  const hold = ipc.holdNext("monitoring_snapshot");
  await page.evaluate(async () => {
    await window.__TAURI_INTERNALS__.invoke("hide_panel", {});
    await window.__TAURI_INTERNALS__.invoke("fixture_show_panel", {});
  });
  await hold.arrived;
  await page.locator("[data-queue-filter=attention]").click();
  hold.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("[data-queue-attention]")).toHaveText("0");
  await stateChanged(page);
  await expect(page.locator("[data-queue-filter=attention]")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("a card chosen before the first panel state lands holds on a fresh panel", async ({
  page,
  store,
  ipc,
}) => {
  const { state } = await queueFixture(store);
  const hold = ipc.holdNext("panel_snapshot");
  await page.goto("/");
  await hold.arrived;
  await page.locator("[data-queue-filter=attention]").click();
  const kept = (entry) =>
    (entry.job ?? entry.review?.job ?? entry).number === 9;
  for (const key of ["jobs", "reviews", "publications"])
    state[key] = state[key].filter(kept);
  await store("seed_queue_state", state);
  hold.release();
  await page.evaluate(() => window.__settingsIdle());
  await stateChanged(page);
  await expect(page.locator("[data-queue-attention]")).toHaveText("0");
  await expect(page.locator("[data-queue-filter=attention]")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("a notification navigation error never renders on Settings, even after repository saves", async ({
  page,
  store,
}) => {
  const legacy = "Panel opening was replaced by a newer dismissal or Quit.";
  await store("set_notifications_enabled", { enabled: true });
  await queueFixture(store);
  const ledger = await store("observe_notifications");
  const [upgraded, missing] = ledger.notices;
  // Saved by an older build; superseded opens are no longer errors.
  upgraded.navigation_error = legacy;
  missing.event.destination = { kind: "queue_item", item_id: "missing-item" };
  await store("seed_notifications", ledger);
  const name = "fixture/repo-0";
  await store("save_repository", { repository: name });
  const { settings } = await store("snapshot");
  Object.assign(
    settings.repositories.find((r) => r.name === name),
    {
      provider_account_id: "22",
      provider_repository_id: "200",
      enabled: false,
      cron: "*/30 * * * *",
    },
  );
  await store("seed_settings", settings);
  await page.goto("/");
  await page.evaluate(async (noticeId) => {
    await window.__TAURI_INTERNALS__.invoke("fixture_notification_click", {
      itemId: "missing-item",
      noticeId,
      race: "none",
    });
    await window.__settingsIdle();
  }, missing.id);
  const notices = new Map(
    (await store("notification_snapshot")).notices.map((n) => [
      n.id,
      n.navigation_error,
    ]),
  );
  expect(notices.get(upgraded.id)).toBeNull();
  const failure = notices.get(missing.id);
  expect(failure).toContain("not available");
  expect((await store("snapshot")).error).toBeNull();
  const nav = (label) =>
    page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: label, exact: true });
  await nav("Settings").click();
  const noError = async () => {
    await page.evaluate(() => window.__settingsIdle());
    for (const text of [legacy, failure])
      await expect(
        page.locator("[role=alert]:visible", { hasText: text }),
      ).toHaveCount(0);
    await expect(page.locator("#error")).toBeHidden();
  };
  for (const every of ["15", "60"]) {
    const editor = await repositorySettings(page, name);
    await expect(editor.locator("[data-schedule]")).not.toHaveAttribute(
      "aria-busy",
      "true",
    );
    await editor.locator("#schedule-every").selectOption(every);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor).toHaveCount(0);
    await noError();
  }
  expect(
    (await store("snapshot")).settings.repositories.find((r) => r.name === name)
      .cron,
  ).not.toBe("*/30 * * * *");
  await page.reload();
  await nav("Settings").click();
  await noError();
});
