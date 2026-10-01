import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { target } from "./paths.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const screenshots = join(target, "visual-77-79");
test.use({ viewport: { width: 408, height: 744 } });

test("compact human cards retain all ordered files and exact external links", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const ready = fixture.state.reviews.find((r) => r.job.number === 9);
  ready.result.output.files = Array.from({ length: 301 }, (_, index) => ({
    path: `file-${index + 1}.rs`,
    order: index + 1,
    explanation: `Step ${index + 1}.`,
  })).reverse();
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__destinations = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "open_queue_destination") {
        window.__destinations.push({
          args,
          url: await original("queue_destination", args),
        });
        return;
      }
      return original(command, args);
    };
  });
  await page.goto("/");
  const cards = page.locator("#handoff-queue article");
  await expect(cards).toHaveCount(3);
  await expect(page.locator("[data-summary-main]")).toHaveText("3 for you");
  await expect(page.locator("[data-summary-detail]")).toHaveText(
    "1 ready / 2 need attention",
  );
  await expect(page.locator("#handoff-queue")).not.toContainText("For agents");
  await mkdir(screenshots, { recursive: true });
  await page.screenshot({ path: join(screenshots, "queue.png") });
  await cards
    .filter({ hasText: "Fix reconnect race" })
    .getByRole("button", { name: "Evidence and actions" })
    .click();
  const evidence = page.locator("[data-item-evidence]");
  await expect(evidence).toContainText(
    "Personal review, comments and approval happen on GitHub",
  );
  await expect(evidence.getByRole("textbox")).toHaveCount(0);
  await evidence.getByRole("button", { name: "Open PR on GitHub" }).click();
  const guide = page
    .locator("#agent-reviews details")
    .filter({
      has: page.getByText("Complete file guide (301 files)", { exact: true }),
    });
  await guide.locator("summary").click();
  await expect(guide.locator("li")).toHaveCount(301);
  await expect(guide.locator("li").first()).toHaveText("file-1.rs: Step 1.");
  await expect(guide.locator("li").last()).toHaveText("file-301.rs: Step 301.");
  await guide.getByRole("link", { name: "file-301.rs", exact: true }).click();
  const item = (await store("monitoring_snapshot")).items.find(
    (i) => i.job.number === 9,
  );
  expect(await page.evaluate(() => window.__destinations)).toEqual([
    {
      args: { itemId: item.id, file: null },
      url: "https://github.com/example/repo/pull/9",
    },
    {
      args: { itemId: item.id, file: "file-301.rs" },
      url: `https://github.com/example/repo/pull/9/files#diff-${createHash("sha256").update("file-301.rs").digest("hex")}`,
    },
  ]);
});

test("human Queue excludes running and author-wait jobs and recovers an unavailable snapshot", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const waiting = fixture.state.reviews.find((r) => r.job.number === 3);
  waiting.operation.state = "queued";
  waiting.result = null;
  fixture.state.publications = fixture.state.publications.filter(
    (p) => p.review.job.number !== 3,
  );
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__failMonitoring = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "monitoring_snapshot" && window.__failMonitoring)
        return Promise.reject("Synthetic monitoring failure");
      return original(command, args);
    };
  });
  await page.goto("/");
  await expect(page.locator("#handoff-queue article")).toHaveCount(2);
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "example/repo #3",
  );
  await expect(page.locator("#handoff-queue")).not.toContainText(
    "example/repo #1",
  );
  await page.evaluate(() => {
    window.__failMonitoring = true;
  });
  await tab(page, "Running").click();
  await tab(page, "Queue").click();
  await expect(page.locator("[data-summary-main]")).toHaveText(
    "Queue unavailable",
  );
  await expect(page.locator("#handoff-queue article")).toHaveCount(0);
  await expect(page.locator("[data-panel-error]")).toContainText(
    "not an empty successful check",
  );
  await page.evaluate(() => {
    window.__failMonitoring = false;
  });
  await tab(page, "Running").click();
  await tab(page, "Queue").click();
  await expect(page.locator("#handoff-queue article")).toHaveCount(2);
  await expect(page.locator("[data-summary-main]")).toHaveText("2 for you");
});
