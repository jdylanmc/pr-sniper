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

async function pressThroughRefresh(page, target, during) {
  await expect(target).toBeVisible();
  const box = await target.boundingBox();
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  const pressed = await page.evaluateHandle(
    ([x, y]) => document.elementFromPoint(x, y),
    [x, y],
  );
  await during?.();
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await page.mouse.up();
  await page.evaluate(() => window.__settingsIdle());
  await pressed.dispose();
}

async function open(page, store) {
  await page.clock.install();
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
