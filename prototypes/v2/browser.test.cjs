const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
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
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (!request.url().startsWith(base) && !request.url().startsWith("file:"))
        external.push(request.url());
    });
    page.on("dialog", (dialog) => dialog.accept());
    const settle = () => page.waitForTimeout(260);
    const navigate = async (tab) => {
      await page.locator(`[data-nav="${tab}"]`).click();
      await settle();
    };
    const back = async () => {
      await page.locator("#back-button").click();
      await settle();
    };
    const settings = async () => {
      await navigate("settings");
      while (await page.locator("#back-button").isVisible()) await back();
    };
    const reset = async () => {
      await page.evaluate(
        (storageKey) => localStorage.removeItem(storageKey),
        key,
      );
      await page.reload();
      await page.locator("#tray-trigger").click();
      await settle();
    };
    const snapshot = async (label) => {
      if (!evidence) return;
      fs.mkdirSync(evidence, { recursive: true });
      await page.screenshot({
        path: path.join(evidence, `vnext-full-${name}-${label}.png`),
      });
    };
    const state = () =>
      page.evaluate(
        (storageKey) => JSON.parse(localStorage.getItem(storageKey)),
        key,
      );

    await page.goto(base);
    assert.equal(await page.locator("#sniper-popover").isVisible(), false);
    await page.locator("#tray-trigger").click();
    await settle();
    assert.equal(await page.locator("#human-list .review-open").count(), 3);
    assert.equal(await page.locator("#agent-list .review-open").count(), 18);
    assert.equal(await page.locator(".bottom-nav button:disabled").count(), 0);
    await snapshot("queue");
    await page.locator("#agent-tab").click();
    await settle();
    await page.locator("#agent-panel").evaluate((element) => {
      element.scrollTop = 420;
    });
    await page.locator("#agent-list .review-open").nth(4).click();
    await settle();
    assert.equal(
      await page.locator('#view-stage [data-page="review"]').count(),
      1,
    );
    await back();
    assert.equal(
      await page.locator("#agent-tab").getAttribute("aria-selected"),
      "true",
    );
    assert(
      (await page
        .locator("#agent-panel")
        .evaluate((element) => element.scrollTop)) > 200,
    );
    await page.keyboard.press("Escape");
    await page.locator("#tray-trigger").click();
    assert.equal(
      await page.locator("#agent-tab").getAttribute("aria-selected"),
      "true",
    );

    await navigate("running");
    assert.equal(await page.locator(".review-open").count(), 4);
    await snapshot("running");
    await page.locator(".review-open").first().click();
    await settle();
    await snapshot("detail");
    await page.locator('[data-action="advance"]').click();
    await page.locator('[data-form="complete"] select').selectOption("clear");
    await page.locator('[data-form="complete"] button[type="submit"]').click();
    assert(
      (await page.locator(".detail-hero").textContent()).includes(
        "Ready for you",
      ),
    );
    await back();
    assert.equal(await page.locator(".review-open").count(), 4);
    await page
      .locator(".review-open")
      .filter({ hasText: "Sentinel" })
      .first()
      .click();
    await settle();
    await page.locator('[data-form="complete"] select').selectOption("clear");
    await page.locator('[data-form="complete"] button[type="submit"]').click();
    assert(
      (await page.locator(".detail-hero").textContent()).includes(
        "Approved (demo)",
      ),
    );
    await navigate("reviewed");
    await page.locator('[data-filter="approved"]').click();
    assert.equal(await page.locator(".review-open").count(), 2);
    await snapshot("reviewed");

    await settings();
    await snapshot("settings");
    await page.locator('[data-page="capacity"]').click();
    await settle();
    await page.locator('input[name="limit"]').fill("2");
    await page.locator('[data-form="capacity"] button[type="submit"]').click();
    await settle();
    assert.equal((await state()).limit, 2);
    assert.equal(
      (await state()).reviews.filter((review) => review.state === "running")
        .length,
      2,
    );
    await page.reload();
    await page.locator("#tray-trigger").click();
    assert.equal(await page.locator("#running-nav-count").textContent(), "2");

    await settings();
    await page.locator('[data-page="agents"]').click();
    await settle();
    assert.equal(await page.locator(".configuration-card").count(), 6);
    await page.locator('[data-page="agent"]:not([data-id])').click();
    await settle();
    const agentName = "Observer <img src=x>";
    await page.locator('input[name="name"]').fill(agentName);
    await page.locator('select[name="accountId"]').selectOption("copilot-demo");
    await page.locator('select[name="model"]').selectOption("Reasoning (demo)");
    await page
      .locator('textarea[name="prompt"]')
      .fill("Look for boundary-condition defects using read-only evidence.");
    await page.locator('select[name="completion"]').selectOption("approve");
    await page
      .locator('input[name="doctrineIds"][value="correctness"]')
      .check();
    await snapshot("agent-editor");
    await page.keyboard.press("Escape");
    await page.locator("#tray-trigger").click();
    assert.equal(
      await page.locator('input[name="name"]').inputValue(),
      agentName,
    );
    await page.locator('[data-form="agent"] button[type="submit"]').click();
    await settle();
    assert.equal(await page.locator(".configuration-card").count(), 7);
    assert.equal(await page.locator(".configuration-card img").count(), 0);
    assert.equal((await state()).agents.at(-1).name, agentName);
    const newAgentId = (await state()).agents.at(-1).id;

    await settings();
    await page.locator('[data-page="repos"]').click();
    await settle();
    await page.locator('[data-page="repo"]:not([data-id])').click();
    await settle();
    await page.locator('input[name="name"]').fill("sample/new-project");
    await page
      .locator('select[name="accountId"]')
      .selectOption("github-personal");
    await page.locator(`input[name="agentIds"][value="${newAgentId}"]`).check();
    await page.locator('input[name="scope"]').check();
    await page.locator("details.check-group summary").click();
    await page.locator('input[name="watched"]').first().check();
    await page.locator('input[name="minutes"]').fill("");
    await page.locator('select[name="schedule"]').selectOption("cron");
    await page.locator('input[name="cron"]').fill("0 9 * * 1-5");
    await page.locator('input[name="zone"]').fill("Europe/London");
    await page.locator('[data-form="repo"] button[type="submit"]').click();
    await settle();
    assert.equal((await state()).repos.length, 4);
    const newRepoId = (await state()).repos.at(-1).id;
    let savedRepo = (await state()).repos.at(-1);
    assert.equal(savedRepo.schedule, "cron");
    assert.equal(savedRepo.minutes, 5);
    assert.equal(savedRepo.cron, "0 9 * * 1-5");
    assert.equal(savedRepo.zone, "Europe/London");
    const editRepo = async () => {
      await page.locator(`[data-page="repo"][data-id="${newRepoId}"]`).click();
      await settle();
    };
    const saveRepo = async () => {
      await page.locator('[data-form="repo"] button[type="submit"]').click();
      await settle();
    };
    const rejectActiveInput = async (field, value) => {
      const before = await state();
      const control = page.locator(`input[name="${field}"]`);
      await control.fill(value);
      await saveRepo();
      assert.deepEqual(await state(), before);
      assert.equal(await control.isVisible(), true);
      assert.equal(
        await control.evaluate(
          (element) =>
            element.willValidate &&
            !element.validity.valid &&
            !!element.validationMessage &&
            document.activeElement === element,
        ),
        true,
        `${name}: ${field}=${JSON.stringify(value)} must fail visibly`,
      );
      assert.equal(await control.inputValue(), value);
    };

    await editRepo();
    await page.locator('input[name="cron"]').fill("");
    await page.locator('input[name="zone"]').fill("");
    await page.locator('select[name="schedule"]').selectOption("interval");
    for (const minutes of ["", "0", "1.5"])
      await rejectActiveInput("minutes", minutes);
    await page.locator('input[name="minutes"]').fill("15");
    await saveRepo();
    assert.deepEqual((await state()).repos.at(-1), {
      ...savedRepo,
      schedule: "interval",
      minutes: 15,
    });
    savedRepo = (await state()).repos.at(-1);

    await editRepo();
    await page.locator('input[name="minutes"]').fill("");
    await page.locator('select[name="schedule"]').selectOption("cron");
    await rejectActiveInput("cron", "");
    await page.locator('input[name="cron"]').fill("0 9 * * 1-5");
    await rejectActiveInput("zone", "");
    await page.locator('input[name="zone"]').fill("Europe/London");
    for (const [field, value, error, valid] of [
      ["cron", "* *", /five-field cron/, "0 9 * * 1-5"],
      ["zone", "Invalid/Place", /IANA time zone/, "Europe/London"],
    ]) {
      const before = await state();
      await page.locator(`input[name="${field}"]`).fill(value);
      await saveRepo();
      assert.deepEqual(await state(), before);
      assert.equal(await page.locator("#app-notice").isVisible(), true);
      assert.match(await page.locator("#app-notice").textContent(), error);
      assert.equal(
        await page.locator(`input[name="${field}"]`).isVisible(),
        true,
      );
      assert.equal(
        await page.locator(`input[name="${field}"]`).inputValue(),
        value,
      );
      await page.locator(`input[name="${field}"]`).fill(valid);
    }
    await saveRepo();
    assert.deepEqual((await state()).repos.at(-1), {
      ...savedRepo,
      schedule: "cron",
    });
    await page.reload();
    await page.locator("#tray-trigger").click();
    assert.deepEqual((await state()).repos.at(-1), {
      ...savedRepo,
      schedule: "cron",
    });
    await settings();
    await page.locator('[data-page="doctrines"]').click();
    await settle();
    await page.locator('[data-page="doctrine"]:not([data-id])').click();
    await settle();
    await page.locator('input[name="name"]').fill("Evidence over guesses");
    await page
      .locator('textarea[name="body"]')
      .fill("Cite the actual failure path. Ask when intent is unclear.");
    await page.locator('[data-form="doctrine"] button[type="submit"]').click();
    await settle();
    assert.equal((await state()).doctrines.length, 4);
    const newDoctrineId = (await state()).doctrines.at(-1).id;
    for (const [collection, editor, id, expected] of [
      ["repos", "repo", newRepoId, 3],
      ["agents", "agent", newAgentId, 6],
      ["doctrines", "doctrine", newDoctrineId, 3],
    ]) {
      await settings();
      await page.locator(`[data-page="${collection}"]`).click();
      await settle();
      await page.locator(`[data-page="${editor}"][data-id="${id}"]`).click();
      await settle();
      await page.locator(`[data-action="delete-${editor}"]`).click();
      await settle();
      assert.equal((await state())[collection].length, expected);
    }

    await reset();
    await page.locator("#monitoring-toggle").click();
    assert.equal(await page.locator("#agent-count").textContent(), "22");
    assert.equal(await page.locator("#running-count").textContent(), "0");
    await page.reload();
    await page.locator("#tray-trigger").click();
    assert.equal(
      await page.locator("#monitoring-toggle").getAttribute("aria-checked"),
      "false",
    );
    await page.locator("#agent-tab").click();
    await snapshot("paused");
    await page.locator("#monitoring-toggle").click();
    assert.equal(await page.locator("#running-count").textContent(), "4");
    assert.equal(await page.locator("#agent-count").textContent(), "18");

    await settings();
    await page.locator('[data-page="accounts"]').click();
    await settle();
    await page
      .locator('[data-action="account"][data-id="copilot-demo"]')
      .click();
    assert.equal(await page.locator("#running-nav-count").textContent(), "0");
    await navigate("queue");
    await page.locator("#agent-tab").click();
    assert(
      (await page.locator("#agent-list").textContent()).includes(
        "AI account disconnected",
      ),
    );
    await settings();
    await page.locator('[data-page="accounts"]').click();
    await settle();
    await page
      .locator('[data-action="account"][data-id="copilot-demo"]')
      .click();
    assert.equal(await page.locator("#running-nav-count").textContent(), "4");

    for (const [width, height] of [
      [1024, 640],
      [390, 844],
      [320, 568],
    ]) {
      await page.setViewportSize({ width, height });
      for (const destination of ["queue", "running", "reviewed", "settings"]) {
        await navigate(destination);
        const bounds = await page.locator("#sniper-popover").boundingBox();
        assert(bounds.x >= 0 && bounds.x + bounds.width <= width + 1);
        assert(bounds.y >= 32 && bounds.y + bounds.height <= height + 1);
        const overflowing = await page
          .locator(".app-view:not([inert]) [data-scroll]")
          .evaluateAll((items) =>
            items.some((item) => item.scrollWidth > item.clientWidth + 1),
          );
        assert.equal(overflowing, false, `${destination} overflow at ${width}`);
      }
      await snapshot(`compact-${width}`);
    }
    await page.emulateMedia({ reducedMotion: "reduce" });
    await navigate("queue");
    assert.equal(
      await page
        .locator(".lane-track")
        .evaluate((element) => getComputedStyle(element).transitionDuration),
      "0s",
    );
    await page.locator("#monitoring-toggle").focus();
    await page.keyboard.press("Shift+Tab");
    assert.equal(
      await page
        .locator('[data-nav="settings"]')
        .evaluate((element) => element === document.activeElement),
      true,
    );
    await page.keyboard.press("Tab");
    assert.equal(
      await page
        .locator("#monitoring-toggle")
        .evaluate((element) => element === document.activeElement),
      true,
    );

    await reset();
    await page.setViewportSize({ width: 1440, height: 960 });
    await settings();
    await page.locator('[data-page="accounts"]').click();
    await settle();
    const startConnection = async (kind, handle) => {
      await page.locator('[data-page="add-account"]').click();
      await settle();
      await page.locator(`[data-provider="${kind}"]`).click();
      await settle();
      await page.locator('input[name="accountName"]').fill(handle);
      await page
        .locator('[data-form="account-signin"] button[type="submit"]')
        .click();
      await settle();
    };
    await page.locator('[data-page="add-account"]').click();
    await settle();
    assert.equal(
      await page.locator(".provider-option:not(:disabled)").count(),
      2,
    );
    for (const provider of ["azure-devops", "claude", "codex", "grok"]) {
      assert.equal(
        await page.locator(`[data-provider="${provider}"]`).isDisabled(),
        true,
      );
      assert(
        (
          await page.locator(`[data-provider="${provider}"]`).textContent()
        ).includes("Coming soon"),
      );
    }
    await snapshot("account-providers");
    await page.setViewportSize({ width: 320, height: 568 });
    assert.equal(
      await page
        .locator('[data-scroll="add-account"]')
        .evaluate((element) => element.scrollWidth > element.clientWidth),
      false,
    );
    await snapshot("account-providers-compact");
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.locator('[data-provider="github"]').click();
    await settle();
    await page.locator('input[name="accountName"]').fill("Designer-DEMO");
    await page.locator(".scenario-details summary").click();
    await page.locator('[data-action="account-signin-failed"]').click();
    assert(
      (await page.locator("#app-notice").textContent()).includes(
        "No identity was saved",
      ),
    );
    assert.equal(await state(), null);
    await page
      .locator('[data-form="account-signin"] button[type="submit"]')
      .click();
    await settle();
    assert.equal(await state(), null);
    assert.equal(
      await page
        .locator('[data-form="account-confirm"] button[type="submit"]')
        .isDisabled(),
      true,
    );
    await snapshot("account-confirm");
    await page.keyboard.press("Escape");
    await page.locator("#tray-trigger").click();
    assert.equal(await state(), null);
    await page.locator('[data-action="cancel-account"]').click();
    await settle();
    assert.equal(await page.locator(".account-row").count(), 3);
    await startConnection("github", "Designer-DEMO");
    await page.locator('input[name="confirmIdentity"]').check();
    await page
      .locator('[data-form="account-confirm"] button[type="submit"]')
      .click();
    await settle();
    assert.equal((await state()).accounts.length, 4);
    assert.equal((await state()).accounts.at(-1).name, "designer-demo");
    const githubAccountId = (await state()).accounts.at(-1).id;
    assert.deepEqual(
      (await state()).reviews,
      await page.evaluate(() => SniperDemo.seed().reviews),
    );
    assert.deepEqual(
      (await state()).repos,
      await page.evaluate(() => SniperDemo.seed().repos),
    );
    assert.deepEqual(
      (await state()).agents,
      await page.evaluate(() => SniperDemo.seed().agents),
    );

    await startConnection("github", "DESIGNER-DEMO");
    await page.locator('input[name="confirmIdentity"]').check();
    await page
      .locator('[data-form="account-confirm"] button[type="submit"]')
      .click();
    assert(
      (await page.locator("#app-notice").textContent()).includes(
        "already listed",
      ),
    );
    assert.equal((await state()).accounts.length, 4);
    await page.locator('[data-action="cancel-account"]').click();
    await settle();
    await startConnection("copilot", "designer-demo");
    await page.locator('input[name="confirmIdentity"]').check();
    await page
      .locator('[data-form="account-confirm"] button[type="submit"]')
      .click();
    await settle();
    assert.equal((await state()).accounts.length, 5);
    const copilotAccountId = (await state()).accounts.at(-1).id;
    await snapshot("accounts-connected");

    await settings();
    await page.locator('[data-page="repos"]').click();
    await settle();
    await page.locator('[data-page="repo"]:not([data-id])').click();
    await settle();
    assert.equal(
      await page.locator('select[name="accountId"]').inputValue(),
      "",
    );
    assert.equal(
      await page
        .locator(`select[name="accountId"] option[value="${githubAccountId}"]`)
        .count(),
      1,
    );
    assert.equal(
      await page
        .locator(`select[name="accountId"] option[value="${copilotAccountId}"]`)
        .count(),
      0,
    );
    await settings();
    await page.locator('[data-page="agents"]').click();
    await settle();
    await page.locator('[data-page="agent"]:not([data-id])').click();
    await settle();
    assert.equal(
      await page.locator('select[name="accountId"]').inputValue(),
      "",
    );
    assert.equal(
      await page
        .locator(`select[name="accountId"] option[value="${copilotAccountId}"]`)
        .count(),
      1,
    );
    assert.equal(
      await page
        .locator(`select[name="accountId"] option[value="${githubAccountId}"]`)
        .count(),
      0,
    );
    await settings();
    await page.locator('[data-page="accounts"]').click();
    await settle();
    await startConnection("github", "never-confirmed");
    await page.reload();
    await page.locator("#tray-trigger").click();
    assert.equal((await state()).accounts.length, 5);
    assert.equal(context.pages().length, 1);

    await settings();
    await page.locator('[data-page="advanced"]').click();
    await settle();
    await page.locator('[data-action="reset"]').click();
    assert.equal(await page.locator("#human-count").textContent(), "3");
    assert.equal((await state()).agents.length, 6);
    await page.goto(pathToFileURL(path.join(__dirname, "index.html")).href);
    await page.locator("#tray-trigger").click();
    assert.equal(await page.locator("#human-list .review-open").count(), 3);
    assert.equal(
      await page
        .locator(".sniper-brand-art")
        .evaluate((image) => image.complete && image.naturalWidth > 0),
      true,
    );
    assert.deepEqual(external, []);
    assert.deepEqual(errors, []);
    await context.close();

    const broken = await browser.newContext();
    await broken.addInitScript((storageKey) => {
      if (localStorage.getItem(storageKey) === null)
        localStorage.setItem(storageKey, "{invalid");
    }, key);
    const corrupt = await broken.newPage();
    await corrupt.goto(base);
    await corrupt.locator("#tray-trigger").click();
    assert.equal(await corrupt.locator("#app-notice").isVisible(), true);
    await corrupt.locator("#monitoring-toggle").click();
    assert.equal(
      await corrupt.locator("#monitoring-toggle").getAttribute("aria-checked"),
      "true",
    );
    assert.equal(
      await corrupt.evaluate(
        (storageKey) => localStorage.getItem(storageKey),
        key,
      ),
      "{invalid",
    );
    await broken.close();

    const denied = await browser.newContext();
    await denied.addInitScript(() => {
      Storage.prototype.setItem = () => {
        throw new DOMException("Unavailable", "QuotaExceededError");
      };
    });
    const unsaved = await denied.newPage();
    await unsaved.goto(base);
    await unsaved.locator("#tray-trigger").click();
    await unsaved.locator("#monitoring-toggle").click();
    assert.equal(
      await unsaved.locator("#monitoring-toggle").getAttribute("aria-checked"),
      "true",
    );
    assert(
      (await unsaved.locator("#app-notice").textContent()).includes(
        "Could not save",
      ),
    );
    await unsaved.locator('[data-nav="settings"]').click();
    await unsaved.waitForTimeout(260);
    await unsaved.locator('[data-page="accounts"]').click();
    await unsaved.waitForTimeout(260);
    await unsaved.locator('[data-page="add-account"]').click();
    await unsaved.waitForTimeout(260);
    await unsaved.locator('[data-provider="github"]').click();
    await unsaved.waitForTimeout(260);
    await unsaved.locator('input[name="accountName"]').fill("unsaved-demo");
    await unsaved
      .locator('[data-form="account-signin"] button[type="submit"]')
      .click();
    await unsaved.waitForTimeout(260);
    await unsaved.locator('input[name="confirmIdentity"]').check();
    await unsaved
      .locator('[data-form="account-confirm"] button[type="submit"]')
      .click();
    assert(
      (await unsaved.locator("#app-notice").textContent()).includes(
        "Could not save",
      ),
    );
    assert.equal(
      await unsaved.locator('[data-form="account-confirm"]').count(),
      1,
    );
    assert.equal(
      await unsaved.evaluate(
        (storageKey) => localStorage.getItem(storageKey),
        key,
      ),
      null,
    );
    await denied.close();
    const shared = await browser.newContext();
    const firstTab = await shared.newPage();
    const secondTab = await shared.newPage();
    await firstTab.goto(base);
    await secondTab.goto(base);
    await firstTab.locator("#tray-trigger").click();
    await secondTab.locator("#tray-trigger").click();
    await secondTab.locator("#monitoring-toggle").click();
    await firstTab.waitForFunction(
      () => !document.querySelector("#app-notice").hidden,
    );
    await firstTab.locator("#monitoring-toggle").click();
    assert(
      (await firstTab.locator("#app-notice").textContent()).includes(
        "Another tab",
      ),
    );
    assert.equal(
      await firstTab.locator("#monitoring-toggle").getAttribute("aria-checked"),
      "true",
    );
    assert.equal(
      await firstTab.evaluate(
        (storageKey) => JSON.parse(localStorage.getItem(storageKey)).monitoring,
        key,
      ),
      false,
    );
    await shared.close();
    console.log(
      `${name}: complete navigation, outcomes, settings, local persistence, pause/requeue, reconnect, compact layouts, focus, file://, corruption, failed saves, and zero external requests passed.`,
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
