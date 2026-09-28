import { createHash } from "node:crypto";
import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

async function captureDestinations(page) {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__destinations = [];
    window.__queueActions = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "open_queue_destination") {
        const url = await original("queue_destination", args);
        window.__destinations.push({ args, url });
        return;
      }
      if (
        ["open_settings", "open_diagnostics", "publish_review"].includes(
          command,
        )
      ) {
        window.__queueActions.push({ command, args });
        return;
      }
      return original(command, args);
    };
  });
}

test("persisted queue prioritizes operator handoffs, not author waits or local-only sign-off", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await page.goto("/?view=queue");
  const inbox = page.locator("#handoff-queue");
  await expect(inbox.locator("article h3")).toHaveText([
    "example/repo #2: Review change",
    "example/repo #9: Fix reconnect race",
    "example/repo #3: Review change",
    "example/repo #1: Review change",
  ]);
  await expect(
    inbox.getByRole("article", { name: "example/repo #9", exact: true }),
  ).toContainText("Ready for your final review");
  await expect(
    inbox.getByRole("article", { name: "example/repo #1", exact: true }),
  ).toContainText("The PR author needs to respond");
  const failed = inbox.getByRole("article", {
    name: "example/repo #3",
    exact: true,
  });
  await expect(failed).toContainText("Failed / recovery required");
  await expect(failed).not.toContainText("Ready for your final review");
  await expect(
    page.getByRole("button", { name: /^(Approve|Merge)/ }),
  ).toHaveCount(0);
});

test("exact account and revision selection survives reload with complete ordered guide and native-validated links", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const ready = fixture.state.reviews.find((r) => r.job.number === 9);
  ready.result.output.files = Array.from({ length: 301 }, (_, index) => ({
    path:
      index === 300
        ? "<img src=x onerror=alert(1)>.rs"
        : `ordered-${300 - index}.rs`,
    order: index + 1,
    explanation: `Read step ${index + 1}.`,
  }));
  await store("seed_queue_state", fixture.state);
  await captureDestinations(page);
  await page.goto("/?view=queue");
  const item = (await store("monitoring_snapshot")).items.find(
    (i) => i.job.number === 9,
  );
  await page
    .locator("#handoff-queue")
    .getByRole("article", { name: "example/repo #9", exact: true })
    .getByRole("button", { name: "Evidence and actions" })
    .click();
  await expect(page.locator("#evidence-heading")).toBeFocused();
  expect(new URL(page.url()).hash).toBe(`#item=${item.id}`);
  await page.reload();
  const evidence = page.locator("#agent-reviews");
  await expect(evidence.locator("article")).toHaveCount(1);
  await expect(evidence).toContainText("example/repo #9");
  await evidence
    .getByText("Complete file guide (301 files)", { exact: true })
    .click();
  await expect(evidence.locator("ol li")).toHaveCount(301);
  await expect(evidence.locator("ol li").first()).toHaveText(
    "ordered-300.rs: Read step 1.",
  );
  await expect(evidence.locator("ol li").last()).toHaveText(
    "<img src=x onerror=alert(1)>.rs: Read step 301.",
  );
  await expect(evidence.locator("img")).toHaveCount(0);
  await evidence
    .getByRole("link", { name: "<img src=x onerror=alert(1)>.rs", exact: true })
    .click();
  const hash = createHash("sha256")
    .update("<img src=x onerror=alert(1)>.rs")
    .digest("hex");
  await expect
    .poll(() => page.evaluate(() => window.__destinations))
    .toEqual([
      {
        args: { itemId: item.id, file: "<img src=x onerror=alert(1)>.rs" },
        url: `https://github.com/example/repo/pull/9/files#diff-${hash}`,
      },
    ]);
  await expect(page.locator("#content")).toContainText(
    "GitHub links open the live PR or current diff",
  );
});

test("publication recovery and remediation stay findable in selected evidence", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await captureDestinations(page);
  await page.goto("/?view=queue");
  await page
    .locator("#handoff-queue")
    .getByRole("article", { name: "example/repo #3", exact: true })
    .getByRole("button", { name: "Evidence and actions" })
    .click();
  const evidence = page.locator("#agent-reviews");
  const reconcile = evidence.getByRole("button", {
    name: "Reconcile / retry publication",
  });
  await expect(reconcile).toBeDisabled();
  await expect(evidence).toContainText("Local result: machine sign-off");
  await expect(evidence).toContainText("GitHub outcome is unresolved");
  await evidence
    .getByRole("checkbox", { name: /Publish or reconcile this exact revision/ })
    .check();
  await reconcile.click();
  await expect
    .poll(() => page.evaluate(() => window.__queueActions))
    .toEqual([
      {
        command: "publish_review",
        args: { reviewOperationId: "review-3" },
      },
    ]);
  await page
    .getByRole("button", { name: "Open Settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Open Diagnostics", exact: true })
    .click();
  expect(
    (await page.evaluate(() => window.__queueActions)).map((a) => a.command),
  ).toEqual(["publish_review", "open_settings", "open_diagnostics"]);
});

test("cold deep links select only their exact item and missing destinations never select another PR", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await captureDestinations(page);
  const item = (await store("monitoring_snapshot")).items.find(
    (i) => i.job.number === 1,
  );
  await page.goto(`/?view=queue#item=${item.id}`);
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #1");
  await page.goto("about:blank");
  await page.goto("/?view=queue");
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #1");
  await page
    .locator("#handoff-queue")
    .getByRole("article", { name: "example/repo #1", exact: true })
    .getByRole("button", { name: "Open PR on GitHub" })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.__destinations.map((d) => d.url)))
    .toEqual(["https://github.com/example/repo/pull/1"]);
  await page.goto("/?view=queue#item=unavailable");
  await expect(page.locator("#handoff-queue")).toContainText(
    "No different PR or revision was selected",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  await page.getByRole("button", { name: "Show all evidence" }).click();
  await expect(page.locator("#agent-reviews article")).toHaveCount(4);
});

test("new heads preserve old publication warnings instead of carrying forward readiness", async ({
  page,
  store,
}) => {
  const { state } = await queueFixture(store);
  const before = (await store("monitoring_snapshot")).items.find(
    (i) => i.job.number === 9,
  );
  const old = state.jobs.find((j) => j.number === 9);
  const next = { ...old, head_sha: "c".repeat(40) };
  old.waiting = "superseded";
  state.jobs.push(next);
  await store("seed_queue_state", state);
  await page.goto(`/?view=queue#item=${before.id}`);
  const rows = page
    .locator("#handoff-queue")
    .getByRole("article", { name: "example/repo #9", exact: true });
  await expect(rows).toHaveCount(2);
  await expect(rows.filter({ hasText: "Stale after publication" })).toHaveCount(
    1,
  );
  await expect(
    rows.filter({ hasText: "Ready for your final review" }),
  ).toHaveCount(0);
  await expect(page.locator("#agent-reviews")).toContainText(
    "Head " + "a".repeat(40),
  );
  await expect(
    page.getByRole("button", { name: "Start review", exact: true }),
  ).toHaveCount(0);
});

test("queue remains readable in native-sized light, dark and narrow layouts", async ({
  page,
  store,
}, testInfo) => {
  await queueFixture(store);
  await page.setViewportSize({ width: 640, height: 720 });
  await page.goto("/?view=queue");
  await expect(page.locator("#handoff-queue article")).toHaveCount(4);
  for (const colorScheme of ["light", "dark"]) {
    await page.emulateMedia({ colorScheme });
    await testInfo.attach(`queue-${colorScheme}`, {
      body: await page.screenshot({
        path: testInfo.outputPath(`queue-${colorScheme}.png`),
      }),
      contentType: "image/png",
    });
  }
  await page.setViewportSize({ width: 390, height: 600 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.locator("#handoff-queue button").first().focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("#evidence-heading")).toBeFocused();
});
