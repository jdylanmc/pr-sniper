const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium, webkit } = require("playwright");
const base = "http://127.0.0.1:4178/";
const key = "pr-sniper.vnext.v2";
const evidence = process.env.PROTOTYPE_SCREENSHOTS;

async function check(engine, name) {
  const browser = await engine.launch({ headless: true });
  try {
    const context = await browser.newContext({
      viewport: { width: 1440, height: 960 },
    });
    const page = await context.newPage();
    const errors = [];
    const external = [];
    const dialogs = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (!request.url().startsWith(base)) external.push(request.url());
    });
    page.on("dialog", async (dialog) => {
      dialogs.push(dialog.message());
      await dialog.accept();
    });
    const settle = () => page.waitForTimeout(270);
    const state = () =>
      page.evaluate(
        (storageKey) => JSON.parse(localStorage.getItem(storageKey)),
        key,
      );
    const snapshot = async (label) => {
      if (!evidence) return;
      fs.mkdirSync(evidence, { recursive: true });
      await page.screenshot({
        path: path.join(evidence, `vnext-onboarding-${name}-${label}.png`),
      });
    };
    const click = async (selector) => {
      await page.locator(selector).click();
      await settle();
    };
    const connect = async (kind, handle) => {
      await click(
        `.genie-card [data-step="${kind === "copilot" ? "ai" : "code"}"]`,
      );
      assert.equal(await page.locator('input[name="accountName"]').count(), 0);
      await click(`[data-provider="${kind}"]`);
      await page.locator('input[name="accountName"]').fill(handle);
      await click('[data-form="account-signin"] button[type="submit"]');
      await page.locator('input[name="confirmIdentity"]').check();
      await click('[data-form="account-confirm"] button[type="submit"]');
      assert.equal(
        await page.locator('.app-view[data-page="genie"]').count(),
        1,
      );
    };

    await page.goto(base);
    assert.equal(await page.locator("[data-scenario]").count(), 2);
    await snapshot("desktop-icons");
    await click('[data-scenario="fresh"]');
    let current = await state();
    assert.equal(current.scenario, "fresh");
    for (const field of ["accounts", "agents", "repos", "reviews"])
      assert.equal(current[field].length, 0);
    assert.equal(current.limit, 4);
    assert.equal(current.monitoring, false);
    assert.equal(await page.locator("#monitoring-toggle").isDisabled(), true);
    assert.equal(
      await page.locator('[role="progressbar"]').getAttribute("aria-valuenow"),
      "0",
    );
    assert.equal(await page.locator(".review-open").count(), 0);
    await snapshot("welcome");
    await page.setViewportSize({ width: 390, height: 844 });
    await snapshot("welcome-compact");
    assert.equal(
      await page
        .locator('[data-scroll="welcome"]')
        .evaluate((element) => element.scrollWidth > element.clientWidth),
      false,
    );
    await page.setViewportSize({ width: 1440, height: 960 });
    await click('[data-nav="running"]');
    assert(
      (await page.locator(".empty-state").textContent()).includes(
        "No reviews yet",
      ),
    );
    await click('[data-nav="reviewed"]');
    assert.equal(await page.locator(".review-open").count(), 0);
    await click('[data-nav="queue"]');
    await click('.welcome-card [data-page="genie"]');
    await snapshot("genie");
    await click('.genie-card [data-step="ai"]');
    assert.equal(await page.locator(".provider-option").count(), 4);
    assert.equal(await page.locator('input[name="accountName"]').count(), 0);
    assert.equal(
      await page.locator('[data-provider="copilot"]').isDisabled(),
      false,
    );
    for (const provider of ["claude", "codex", "grok"])
      assert.equal(
        await page.locator(`[data-provider="${provider}"]`).isDisabled(),
        true,
      );
    await snapshot("choose-ai-provider");
    await click('[data-provider="copilot"]');
    await page.locator('input[name="accountName"]').fill("not-confirmed");
    await click('[data-form="account-signin"] button[type="submit"]');
    await click('[data-action="cancel-account"]');
    assert.equal((await state()).accounts.length, 0);
    assert.equal(await page.locator('.app-view[data-page="genie"]').count(), 1);

    await connect("copilot", "first-ai-demo");
    assert.equal(
      await page.locator('[role="progressbar"]').getAttribute("aria-valuenow"),
      "1",
    );
    await page.reload();
    await click("#tray-trigger");
    assert.equal(
      await page.locator('.app-view[data-page="welcome"]').count(),
      1,
    );
    assert.equal(
      await page.locator('[role="progressbar"]').getAttribute("aria-valuenow"),
      "1",
    );
    await click('.welcome-card [data-page="genie"]');
    await connect("github", "first-code-demo");
    assert.equal(
      await page.locator('[role="progressbar"]').getAttribute("aria-valuenow"),
      "2",
    );

    await click('.genie-card [data-step="agent"]');
    assert.equal(
      await page.locator('select[name="accountId"]').inputValue(),
      "",
    );
    current = await state();
    await page.locator('input[name="name"]').fill("My first reviewer");
    await page
      .locator('select[name="accountId"]')
      .selectOption(
        current.accounts.find((account) => account.kind === "copilot").id,
      );
    await page.locator('select[name="model"]').selectOption("Balanced (demo)");
    await page
      .locator('textarea[name="prompt"]')
      .fill("Look for concrete bugs and explain their failure paths.");
    await click('[data-form="agent"] button[type="submit"]');
    assert.equal(
      await page.locator('[role="progressbar"]').getAttribute("aria-valuenow"),
      "3",
    );
    assert.equal((await state()).monitoring, false);

    await click('.genie-card [data-step="repo"]');
    current = await state();
    await page.locator('input[name="name"]').fill("onboard/new-demo");
    await page
      .locator('select[name="accountId"]')
      .selectOption(
        current.accounts.find((account) => account.kind === "github").id,
      );
    await page.locator('input[name="scope"]').check();
    await page
      .locator(`input[name="agentIds"][value="${current.agents[0].id}"]`)
      .check();
    await page.locator("details.check-group summary").click();
    await page.locator('input[name="watched"]').first().check();
    await page.locator('input[name="minutes"]').fill("15");
    await page.locator('select[name="start"]').selectOption("on");
    await page.locator('select[name="comments"]').selectOption("off");
    await click('[data-form="repo"] button[type="submit"]');
    assert.equal(
      await page.locator('[role="progressbar"]').getAttribute("aria-valuenow"),
      "4",
    );
    assert.equal((await state()).reviews.length, 0);
    assert.equal((await state()).monitoring, false);
    await snapshot("ready");
    await click('.genie-card [data-page="setup-review"]');
    assert.equal(
      await page
        .locator('[data-form="setup-finish"] button[type="submit"]')
        .isDisabled(),
      true,
    );
    const summary = await page
      .locator('[data-scroll="setup-review"]')
      .textContent();
    for (const phrase of [
      "first-code-demo",
      "first-ai-demo",
      "Every 15 minutes",
      "Automatic when eligible",
      "Manual publication gate",
    ])
      assert(summary.includes(phrase));
    await snapshot("review-setup");
    await click("#back-button");
    assert.equal((await state()).monitoring, false);
    assert.equal((await state()).accounts.length, 2);
    await click('.genie-card [data-page="setup-review"]');
    await page.locator('input[name="confirmSetup"]').check();
    await click('[data-form="setup-finish"] button[type="submit"]');
    current = await state();
    assert.equal(current.onboarded, true);
    assert.equal(current.monitoring, true);
    assert.equal(current.accounts.length, 2);
    assert.equal(current.agents.length, 1);
    assert.equal(current.repos.length, 1);
    assert.equal(current.reviews.length, 0);
    assert.equal(await page.locator("#human-count").textContent(), "0");
    assert.equal(await page.locator("#agent-count").textContent(), "0");
    assert.equal(await page.locator("#monitoring-toggle").isDisabled(), false);
    await snapshot("into-app");
    await click("#agent-tab");
    await click('[data-action="detect"]');
    assert.equal((await state()).reviews.length, 1);
    assert.equal(await page.locator("#running-nav-count").textContent(), "1");

    await page.reload();
    await click("#tray-trigger");
    assert.equal(await page.locator('.app-view[data-page="queue"]').count(), 1);
    await click('[data-scenario="fresh"]');
    assert.equal((await state()).accounts.length, 0);
    assert.equal((await state()).reviews.length, 0);
    assert.equal(
      await page.locator('.app-view[data-page="welcome"]').count(),
      1,
    );
    await click('[data-scenario="configured"]');
    current = await state();
    assert.equal(current.accounts.length, 3);
    assert.equal(current.agents.length, 6);
    assert.equal(current.reviews.length, 28);
    assert.equal(await page.locator("#human-count").textContent(), "3");
    assert.equal(await page.locator("#agent-count").textContent(), "18");
    assert.equal(await page.locator("#running-nav-count").textContent(), "4");
    await click("#human-list .review-open:first-child");
    assert.equal(
      await page.locator(".platform-link").getAttribute("href"),
      "https://github.com/orbit/console/pull/248",
    );
    assert.equal(
      await page.locator(".platform-link").getAttribute("target"),
      "_blank",
    );
    assert(
      (await page.locator(".platform-link").getAttribute("rel")).includes(
        "noopener",
      ),
    );
    assert.equal(
      await page
        .locator('[data-form="human"], [data-action="publish"], textarea')
        .count(),
      0,
    );
    assert.equal(await page.locator(".file-guide li").count(), 3);
    assert(
      !(await page.locator("#sniper-popover").textContent()).includes(
        "Your next move",
      ),
    );
    await snapshot("platform-handoff");
    await page.locator(".file-guide").scrollIntoViewIfNeeded();
    await snapshot("changed-file-guide");
    await page.emulateMedia({ reducedMotion: "reduce" });
    await click('[data-scenario="fresh"]');
    await page.setViewportSize({ width: 320, height: 568 });
    await click('.welcome-card [data-page="genie"]');
    const bounds = await page.locator("#sniper-popover").boundingBox();
    assert(bounds.x >= 0 && bounds.x + bounds.width <= 321);
    assert(bounds.y + bounds.height <= 569);
    assert.equal(
      await page
        .locator('[data-scroll="genie"]')
        .evaluate((element) => element.scrollWidth > element.clientWidth),
      false,
    );
    await snapshot("genie-small");
    assert.deepEqual(dialogs, []);
    assert.deepEqual(errors, []);
    assert.deepEqual(external, []);
    console.log(
      `${name}: reset launchers, empty first install, Genie account/editor reuse, cancellation, persistence, review-before-start, entry into the app, platform-only human handoff, file guide, compact layout, and no automatic external requests passed.`,
    );
  } finally {
    await browser.close();
  }
}

(async () => {
  await check(chromium, "chromium");
  await check(webkit, "webkit");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
