import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test("notification opt-in is immediate, independent and confirms once without replaying existing events", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const before = (await store("snapshot")).settings;
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  const enabled = page.getByRole("switch", {
    name: "Notifications",
    exact: true,
  });
  await expect(enabled).not.toBeChecked();
  await expect(page.getByLabel("Test destination")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Send test notification" }),
  ).toHaveCount(0);
  await enabled.check();
  await expect
    .poll(async () => (await store("notification_snapshot")).enabled)
    .toBe(true);
  expect((await store("snapshot")).settings).toEqual(before);
  const [notice] = (await store("notification_snapshot")).notices;
  expect(notice.event.category).toBe("test");
  expect(notice.event.destination).toEqual({ kind: "settings" });
  expect(notice.phase).toBe("queued");
  expect(notice.title).toBe("PR Sniper notifications are on");
  expect(notice.body).not.toContain("example/repo");
  const observed = await store("observe_notifications");
  expect(observed.notices.map((n) => n.id)).toEqual([notice.id]);
  await store("set_notifications_enabled", { enabled: true });
  expect((await store("notification_snapshot")).notices).toHaveLength(1);
  await page.reload();
  await section(page, "Preferences");
  await expect(enabled).toBeChecked();
});

test("re-enabling skips events from while off and never sends stale unsent notices", async ({
  store,
}) => {
  await store("set_notifications_enabled", { enabled: true });
  await queueFixture(store);
  const live = await store("observe_notifications");
  expect(live.notices.filter((n) => n.phase === "queued")).toHaveLength(4);
  await store("set_notifications_enabled", { enabled: false });
  const off = (await store("notification_snapshot")).notices;
  expect(off.every((n) => n.phase === "not_sent")).toBe(true);
  expect(off.every((n) => /turned off/.test(n.error))).toBe(true);
  await store("set_notifications_enabled", { enabled: true });
  const again = await store("observe_notifications");
  expect(again.notices).toHaveLength(5);
  const queued = again.notices.filter((n) => n.phase === "queued");
  expect(queued.map((n) => n.event.category)).toEqual(["test"]);
});

test("native projection ignores author waits and deduplicates every repeated poll and restart", async ({
  page,
  store,
}) => {
  await store("set_notifications_enabled", { enabled: true });
  await queueFixture(store);
  const first = await store("observe_notifications");
  expect(first.notices.map((notice) => notice.event.category).sort()).toEqual([
    "failure",
    "human_input",
    "ready",
    "test",
  ]);
  const second = await store("observe_notifications");
  expect(second.notices.map((n) => n.id)).toEqual(
    first.notices.map((n) => n.id),
  );
  await page.goto("/?view=queue");
  await page
    .locator("#notification-history")
    .getByText("Notification history (4)", { exact: true })
    .click();
  await expect(page.locator("#notification-history article")).toHaveCount(4);
  await page.reload();
  expect(
    (await store("observe_notifications")).notices.map((n) => n.id),
  ).toEqual(first.notices.map((n) => n.id));
});

test("history distinguishes OS acceptance, uncertainty and saved destinations without acknowledgment claims", async ({
  page,
  store,
}) => {
  await store("set_notifications_enabled", { enabled: true });
  await queueFixture(store);
  const ledger = await store("observe_notifications");
  ledger.notices.shift();
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
  await expect(history).toContainText(": Accepted");
  await expect(history).toContainText(": Unknown");
  await expect(history).toContainText(": Permission denied");
  await expect(history).not.toContainText(/delivered|acknowledged|seen/i);
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

test("Windows aggregate permission never invents banner or center authorization", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const result = await original(command, args);
      if (command === "notification_snapshot") {
        result.platform = "windows";
        result.permission = {
          authorization: "authorized_aggregate",
          alerts_enabled: null,
          center_enabled: null,
        };
      }
      return result;
    };
  });
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  const status = page.locator("#notification-permission");
  await expect(status).toHaveText(
    "Off; Windows authorized aggregate; banners unknown; center unknown",
  );
  await expect(
    page.getByRole("switch", { name: "Notifications", exact: true }),
  ).not.toBeChecked();
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
    name: "Notifications",
    exact: true,
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
    name: "Notifications",
    exact: true,
  });
  await enabled.check();
  await expect(page.locator("#notification-error")).toContainText(
    "macOS denied notifications",
  );
  await expect(enabled).not.toBeChecked();
  expect((await store("notification_snapshot")).enabled).toBe(false);
  await expect(page.locator("#notification-permission")).toContainText(
    /(macOS|Windows) denied; banners off; center off/,
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
  await store("set_notifications_enabled", { enabled: true });
  await queueFixture(store);
  await store("observe_notifications");
  await writeFile(join(dataRoot, "state/queue.json"), "invalid");
  await page.goto("/?view=queue");
  const history = page.locator("#notification-history");
  await history.getByText("Notification history (4)", { exact: true }).click();
  await expect(history.getByRole("alert")).toContainText(
    "Review queue is invalid",
  );
  await expect(history.locator("article")).toHaveCount(4);
  await expect(page.locator("#error")).toHaveText("Failed");
});
