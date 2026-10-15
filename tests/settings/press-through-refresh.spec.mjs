import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

// WebKit drops a click whose pressed node is replaced before release. Each
// test presses a control, lets a scheduled refresh land, then releases: the
// click must still reach the control.
test.use({ viewport: { width: 408, height: 744 } });

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

// The page clock is paused at `start`, so 5 s pollers fire exactly at
// start + 5 s, 10 s, ...; a press spans the next tick and stays well inside
// the bounded deferral.
const start = new Date("2026-10-14T12:00:00Z");
let elapsed = 0;
async function advanceTo(page, ms) {
  await page.clock.fastForward(ms - elapsed);
  elapsed = ms;
}

async function pressThroughRefresh(page, target, during) {
  await expect(target).toBeVisible();
  const box = await target.boundingBox();
  const tick = (Math.floor(elapsed / 5000) + 1) * 5000;
  await advanceTo(page, tick - 100);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await during?.();
  await advanceTo(page, tick + 100);
  await page.evaluate(() => window.__settingsIdle());
  await page.mouse.up();
  await page.evaluate(() => window.__settingsIdle());
}

async function open(page, store) {
  elapsed = 0;
  await page.clock.install({ time: start });
  await page.clock.pauseAt(start);
  await store("panel_snapshot");
  await page.goto("/");
  await page.evaluate(() => window.__settingsIdle());
  await tab(page, "Queue").click();
  await page.evaluate(() => window.__settingsIdle());
}

// A refresh that changes another PR forces the queue list to redraw.
const retitled = "Retitled during the press";
const retitle = (store, fixture, number) => async () => {
  fixture.state.jobs.find((job) => job.number === number).title = retitled;
  await store("seed_queue_state", fixture.state);
};

test("a tab button keeps its click across a running-count refresh", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const value = await original(command, args);
      return command === "automation_snapshot"
        ? { ...value, active: 2 }
        : value;
    };
  });
  await open(page, store);
  await pressThroughRefresh(page, page.locator("[data-running-count]"));
  await expect(page.locator("[data-panel-heading]")).toHaveText("Work queue");
});

test("a queue card keeps its click across a count refresh", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await open(page, store);
  const card = page.locator("[data-queue-attention-card]");
  await expect(card).toHaveAttribute("aria-pressed", "false");
  await pressThroughRefresh(page, page.locator("[data-queue-attention]"));
  await expect(card).toHaveAttribute("aria-pressed", "true");
});

test("a queue row keeps its click when another row changes", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  await open(page, store);
  await showQueueGroup(page, "ready");
  const row = page
    .locator('[data-queue-group="ready"] .queue-card-open')
    .first();
  await pressThroughRefresh(page, row, retitle(store, fixture, 2));
  await expect
    .poll(async () => (await store("panel_snapshot")).route.detail?.type)
    .toBe("item");
  await expect(page.locator("#handoff-queue")).toContainText(retitled);
});

test("a Retry button keeps its click when another row changes", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__recoveries = [];
    const recoveries = [
      "check_now",
      "publish_review",
      "reconcile_provider_action",
      "retry_monitoring_operation",
      "retry_packet_state",
      "start_review",
    ];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (recoveries.includes(command)) {
        window.__recoveries.push(command);
        return Promise.resolve(null);
      }
      return original(command, args);
    };
  });
  await open(page, store);
  await showQueueGroup(page, "attention");
  const retry = page
    .locator('[data-queue-group="attention"] .queue-retry')
    .first();
  await pressThroughRefresh(page, retry, retitle(store, fixture, 9));
  await expect
    .poll(() => page.evaluate(() => window.__recoveries.length))
    .toBeGreaterThan(0);
  // The deferred redraw still lands once the press ends.
  await expect(page.locator("#handoff-queue")).toContainText(retitled);
});

test("a secondary-button press never defers background redraws", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  await open(page, store);
  await showQueueGroup(page, "ready");
  const card = page
    .locator('[data-queue-group="ready"] .queue-card-open')
    .first();
  const box = await card.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  // A context menu can swallow the release; the redraw must not wait for it.
  await page.mouse.down({ button: "right" });
  await retitle(store, fixture, 2)();
  await advanceTo(page, 5100);
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#handoff-queue")).toContainText(retitled);
  await page.mouse.up({ button: "right" });
});

test("a press held past the bound still lets the background redraw land", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  await open(page, store);
  const heading = await page.locator("[data-panel-heading]").boundingBox();
  await advanceTo(page, 4900);
  await page.mouse.move(heading.x + 4, heading.y + 4);
  await page.mouse.down();
  await retitle(store, fixture, 2)();
  await advanceTo(page, 5100);
  await page.evaluate(() => window.__settingsIdle());
  // Deferred while pressed...
  await expect(page.locator("#handoff-queue")).not.toContainText(retitled);
  // ...but never past the bound (1.5 s), even without a release.
  await advanceTo(page, 6500);
  await expect(page.locator("#handoff-queue")).toContainText(retitled);
  await page.mouse.up();
});
