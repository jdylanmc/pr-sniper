import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test("notification opt-in is immediate, independent and tests the explicitly selected queue item", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const before = (await store("snapshot")).settings;
  const item = (await store("monitoring_snapshot")).items.find(
    (item) => item.job.number === 9,
  );
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  const enabled = page.getByRole("switch", {
    name: /^Notify me when my attention/,
  });
  await expect(enabled).not.toBeChecked();
  await expect(
    page.getByRole("button", { name: "Send test notification" }),
  ).toBeDisabled();
  await enabled.check();
  await expect
    .poll(async () => (await store("notification_snapshot")).enabled)
    .toBe(true);
  expect((await store("snapshot")).settings).toEqual(before);
  await page.getByLabel("Test destination").selectOption(item.id);
  await page.getByRole("button", { name: "Send test notification" }).click();
  await expect
    .poll(async () => (await store("notification_snapshot")).notices.length)
    .toBe(1);
  const notice = (await store("notification_snapshot")).notices[0];
  expect(notice.event.destination).toEqual({
    kind: "queue_item",
    item_id: item.id,
  });
  expect(notice.title).toBe("PR Sniper test notification");
  expect(notice.body).not.toContain("example/repo");
  await page.reload();
  await section(page, "Preferences");
  await expect(enabled).toBeChecked();
});

test("native projection ignores author waits and deduplicates every repeated poll and restart", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await store("set_notifications_enabled", { enabled: true });
  const first = await store("observe_notifications");
  expect(first.notices.map((notice) => notice.event.category).sort()).toEqual([
    "failure",
    "human_input",
    "ready",
  ]);
  const second = await store("observe_notifications");
  expect(second.notices.map((n) => n.id)).toEqual(
    first.notices.map((n) => n.id),
  );
  await page.goto("/?view=queue");
  await page
    .locator("#notification-history")
    .getByText("Notification history (3)", { exact: true })
    .click();
  await expect(page.locator("#notification-history article")).toHaveCount(3);
  await page.reload();
  expect(
    (await store("observe_notifications")).notices.map((n) => n.id),
  ).toEqual(first.notices.map((n) => n.id));
});

test("history distinguishes OS acceptance, uncertainty and saved destinations without acknowledgment claims", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await store("set_notifications_enabled", { enabled: true });
  const ledger = await store("observe_notifications");
  ledger.notices[0].phase = "accepted_unconfirmed";
  ledger.notices[1].phase = "outcome_unknown";
  ledger.notices[1].error = "No callback; do not repeat.";
  ledger.notices[2].phase = "permission_denied";
  ledger.notices[2].navigation_error = "<img src=x onerror=alert(1)>";
  await store("seed_notifications", ledger);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__notificationOpens = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "open_notification") {
        const destination = await original("notification_destination", args);
        window.__notificationOpens.push({ args, destination });
        return;
      }
      return original(command, args);
    };
  });
  await page.goto("/?view=queue");
  const history = page.locator("#notification-history");
  await history.getByText("Notification history (3)", { exact: true }).click();
  await expect(history).toContainText(
    "Accepted by macOS; banner visibility is unconfirmed",
  );
  await expect(history).toContainText("Delivery outcome unknown");
  await expect(history).toContainText(
    "macOS permission or alert settings prevent delivery",
  );
  await expect(history.locator("img")).toHaveCount(0);
  await expect(history).not.toContainText("Destination opened");
  await history
    .getByRole("button", { name: "Open saved destination" })
    .first()
    .click();
  await expect
    .poll(() => page.evaluate(() => window.__notificationOpens))
    .toEqual([
      {
        args: { id: ledger.notices[2].id },
        destination: ledger.notices[2].event.destination,
      },
    ]);
});

test("a delayed permission refresh cannot overwrite a newly saved opt-in", async ({
  page,
  store,
  ipc,
}) => {
  await page.clock.install();
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  const enabled = page.getByRole("switch", {
    name: /^Notify me when my attention/,
  });
  await expect(enabled).toBeEnabled();
  await expect(enabled).not.toBeChecked();
  const held = ipc.holdNext("notification_snapshot");
  await page.clock.fastForward(5000);
  await held.arrived;
  await enabled.check();
  await expect
    .poll(async () => (await store("notification_snapshot")).enabled)
    .toBe(true);
  held.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(enabled).toBeChecked();
});

test("denied permission leaves opt-in off and visible rather than claiming successful delivery", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__denyNotifications = true;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (
        command === "set_notifications_enabled" &&
        args.enabled &&
        window.__denyNotifications
      ) {
        throw "macOS denied notifications. Enable them in System Settings before opting in.";
      }
      const result = await original(command, args);
      if (command === "notification_snapshot" && window.__denyNotifications) {
        result.permission = {
          authorization: "denied",
          alerts_enabled: false,
          center_enabled: false,
        };
      }
      return result;
    };
  });
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  const enabled = page.getByRole("switch", {
    name: /^Notify me when my attention/,
  });
  await enabled.check();
  await expect(page.locator("#notification-error")).toContainText(
    "macOS denied notifications",
  );
  await expect(enabled).not.toBeChecked();
  expect((await store("notification_snapshot")).enabled).toBe(false);
  await expect(page.locator("#notification-permission")).toContainText(
    "permission: denied",
  );
  await page.evaluate(() => {
    window.__denyNotifications = false;
  });
  await enabled.check();
  await expect
    .poll(async () => (await store("notification_snapshot")).enabled)
    .toBe(true);
  await expect(page.locator("#notification-error")).toBeHidden();
});

test("late native status cannot restore a notification panel after navigating away", async ({
  page,
  ipc,
}) => {
  const held = ipc.holdNext("notification_snapshot");
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  await held.arrived;
  await section(page, "Doctrines");
  held.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(
    page.getByRole("heading", { name: "Doctrines", exact: true }),
  ).toBeVisible();
  await expect(page.locator("#notification-settings")).toHaveCount(0);
});

test("persisted notification history survives unavailable queue data without an invented destination", async ({
  page,
  store,
  dataRoot,
}) => {
  await queueFixture(store);
  await store("set_notifications_enabled", { enabled: true });
  await store("observe_notifications");
  await writeFile(join(dataRoot, "state/queue.json"), "invalid");
  await page.goto("/?view=queue");
  const history = page.locator("#notification-history");
  await history.getByText("Notification history (3)", { exact: true }).click();
  await expect(history.getByRole("alert")).toContainText(
    "Review queue is invalid",
  );
  await expect(history.locator("article")).toHaveCount(3);
  await expect(page.locator("#error")).toContainText(
    "Could not read monitoring state",
  );
});
