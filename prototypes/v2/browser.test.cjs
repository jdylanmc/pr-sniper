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
    assert.equal(await page.locator("#human-list .review-open").count(), 2);
    assert.equal(await page.locator("#ready-count").textContent(), "2");
    assert.equal(await page.locator("#attention-count").textContent(), "1");
    assert.equal(await page.locator(".bottom-nav button:disabled").count(), 0);
    // No prose: no work link, heading, or close-hint footer text.
    const queueText = await page.locator("#sniper-popover").innerText();
    for (const gone of [
      "Agent work in Running",
      "Over to you",
      "Close hides only",
    ])
      assert(!queueText.includes(gone), gone);
    assert.equal(
      await page
        .locator('[data-queue-filter="ready"]')
        .getAttribute("aria-pressed"),
      "true",
    );
    await snapshot("queue");
    await page.locator('[data-queue-filter="attention"]').click();
    await settle();
    assert.equal(await page.locator("#human-list .review-open").count(), 1);
    assert.equal(
      await page
        .locator('[data-queue-filter="attention"]')
        .getAttribute("aria-pressed"),
      "true",
    );
    await page.keyboard.press("Escape");
    await page.locator("#tray-trigger").click();
    assert.equal(
      await page
        .locator('[data-queue-filter="attention"]')
        .getAttribute("aria-pressed"),
      "true",
    );
    await page.locator("#version-link").click();
    await settle();
    assert.equal(await page.locator("#page-title").textContent(), "Changelog");
    assert(
      (await page.locator(".changelog h2").first().textContent()).includes(
        "Unreleased",
      ),
    );
    await back();
    await page.locator('[data-nav="running"]').click();
    await settle();
    assert.equal(await page.locator(".work-row").count(), 22);
    await page.locator('[data-scroll="running"]').evaluate((element) => {
      element.scrollTop = 420;
    });
    await page.locator(".work-row").nth(12).click();
    await settle();
    assert.equal(
      await page.locator('#view-stage [data-page="review"]').count(),
      1,
    );
    await back();
    assert(
      (await page
        .locator('[data-scroll="running"]')
        .evaluate((element) => element.scrollTop)) > 200,
    );
    await page.keyboard.press("Escape");
    await page.locator("#tray-trigger").click();
    assert(
      (await page
        .locator('[data-scroll="running"]')
        .evaluate((element) => element.scrollTop)) > 200,
    );

    await navigate("running");
    assert.equal(await page.locator('[data-display="running"]').count(), 4);
    assert.equal(await page.locator('[data-display="queued"]').count(), 18);
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
    assert.equal(await page.locator('[data-display="running"]').count(), 4);
    await page
      .locator('[data-display="running"]')
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
    assert.equal(
      await page.locator('input[name="signature"]').inputValue(),
      "\u{1F52B} pr-sniper(observer-img-src-x:reasoning-demo)",
    );
    await page.locator('input[name="signature"]').fill("custom signature");
    await page.locator('input[name="name"]').fill(`${agentName} two`);
    assert.equal(
      await page.locator('input[name="signature"]').inputValue(),
      "custom signature",
    );
    await page.locator('[data-action="reset-signature"]').click();
    assert.equal(
      await page.locator('input[name="signature"]').inputValue(),
      "\u{1F52B} pr-sniper(observer-img-src-x-two:reasoning-demo)",
    );
    await page.locator('input[name="name"]').fill(agentName);
    await page.locator('[data-action="reset-signature"]').click();
    assert.equal(
      await page.locator("#sniper-popover").getAttribute("data-editor"),
      "true",
    );
    assert.equal(await page.locator(".sniper-brand-art").isVisible(), false);
    await page.locator('[data-action="refresh-models"]').click();
    assert.equal(await page.locator(".icon-button.is-loading").count(), 1);
    await page.waitForTimeout(900);
    assert.equal(await page.locator(".icon-button.is-loading").count(), 0);
    await page.locator('[data-action="prompt-tip"]').click();
    assert.equal(await page.locator("#prompt-tips").isVisible(), true);
    assert.equal(await page.locator("#prompt-tips [data-token]").count(), 8);
    await page.locator('[data-token="pr_title"]').click();
    assert(
      (await page.locator('textarea[name="prompt"]').inputValue()).includes(
        "{{pr_title}}",
      ),
    );
    await page.keyboard.press("Escape");
    assert.equal(await page.locator("#prompt-tips").isVisible(), false);
    assert.equal(await page.locator("#sniper-popover").isVisible(), true);
    await page.locator('[data-action="open-doctrines"]').click();
    await settle();
    assert.equal(await page.locator("#page-title").textContent(), "Doctrines");
    await page.locator("#doctrine-filter").fill("correct");
    assert.equal(
      await page.locator("[data-doctrine-choice]:not([hidden])").count(),
      1,
    );
    await page.locator("#doctrine-filter").fill("");
    await page
      .locator('input[name="doctrineIds"][value="correctness"]')
      .check();
    assert.equal(
      await page.locator("[data-doctrine-summary]").textContent(),
      "1 selected",
    );
    await page
      .locator('[data-action="edit-doctrine"][data-id="correctness"]')
      .click();
    await settle();
    assert.equal(
      await page.locator("#page-title").textContent(),
      "Editing Correctness first",
    );
    await back();
    assert.equal(await page.locator("#page-title").textContent(), "Doctrines");
    assert.equal(
      await page
        .locator('input[name="doctrineIds"][value="correctness"]')
        .isChecked(),
      true,
    );
    await back();
    assert.equal(await page.locator("#page-title").textContent(), "New agent");
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
    await page.locator('[data-page="repo-url"]').click();
    await settle();
    await page
      .locator('input[name="repository"]')
      .fill("https://github.com/sample/new-project");
    await page
      .locator('select[name="accountId"]')
      .selectOption("github-personal");
    await page.locator('[data-form="repo-url"] button[type="submit"]').click();
    await settle();
    await page.locator('input[name="enabled"]').check();
    await page.locator('[data-action="assign-agent"]').click();
    await page.locator('[data-picker="agents"]').selectOption(newAgentId);
    assert.equal(
      await page
        .locator(`input[name="agentIds"][value="${newAgentId}"]`)
        .count(),
      1,
    );
    // The old watch/reviewer-request controls are gone from the editor.
    assert.equal(
      await page.locator('select[name="reviewerRequests"]').count(),
      0,
    );
    assert.equal(await page.locator('input[name="watched"]').count(), 0);
    assert.equal(await page.getByText("People you watch").count(), 0);
    const filtersRow = page.locator('[data-action="open-filters"]');
    const allSwitch = page.locator('input[name="admissionAll"]');
    assert.equal(await allSwitch.isChecked(), true);
    assert.equal(await filtersRow.isDisabled(), true);
    await allSwitch.uncheck();
    assert.equal(await filtersRow.isEnabled(), true);
    assert.match(await filtersRow.innerText(), /Filtered pull requests\s+2 on/);
    await allSwitch.check();
    assert.equal(await filtersRow.isDisabled(), true);
    await allSwitch.uncheck();
    await filtersRow.click();
    await settle();
    assert.equal(
      await page.locator("#page-title").innerText(),
      "Filtered pull requests",
    );
    assert.deepEqual(await page.locator(".admission-label").allInnerTexts(), [
      "Assigned to me for review",
      "Mentioning me in comments",
      "Include users",
      "Exclude users",
      "Include paths",
      "Exclude paths",
    ]);
    assert.equal(
      await page
        .locator('[data-rule="mentioned"] .admission-open')
        .isDisabled(),
      true,
    );
    assert.equal(
      await page.locator('[data-rule="assigned"] [data-dir="-1"]').isDisabled(),
      true,
    );
    assert.equal(
      await page
        .locator('[data-rule="exclude_paths"] [data-dir="1"]')
        .isDisabled(),
      true,
    );
    await page.locator('[data-rule="include_users"] [data-dir="-1"]').click();
    assert.deepEqual(
      await page
        .locator("[data-rule]")
        .evaluateAll((rows) => rows.map((row) => row.dataset.rule)),
      [
        "assigned",
        "include_users",
        "mentioned",
        "exclude_users",
        "include_paths",
        "exclude_paths",
      ],
    );
    await page.locator('[data-rule="include_users"] [data-dir="-1"]').click();
    assert.equal(
      await page.locator("[data-rule]").first().getAttribute("data-rule"),
      "include_users",
    );
    await page.locator('[data-rule="include_users"] .admission-open').click();
    await settle();
    assert.equal(
      await page.locator("#page-title").innerText(),
      "Include users",
    );
    await page.locator('[data-action="add-people"]').click();
    await page.locator('[data-picker="people"]').selectOption({ index: 1 });
    assert.equal(
      await page.locator(".admission-values .assign-row").count(),
      1,
    );
    assert.match(
      await page.locator(".person-handle").innerText(),
      /^@[a-z-]+$/,
    );
    await back();
    assert.equal(
      (
        await page.locator('[data-rule="include_users"] .kv-value').innerText()
      ).trim(),
      "1",
    );
    assert.equal(
      await page.locator('[data-rule="include_users"] input').isChecked(),
      true,
    );
    await page.locator('[data-rule="exclude_paths"] .admission-open').click();
    await settle();
    assert.equal(
      await page.locator("#page-title").innerText(),
      "Exclude paths",
    );
    const pathInput = page.getByLabel("Path pattern");
    assert.equal(
      await pathInput.getAttribute("placeholder"),
      "docs/ or src/**/*.ts",
    );
    for (const [value, message] of [
      ["", /Enter a path/],
      ["/abs", /without \.\./],
      ["a/../b", /without \.\./],
      ["x".repeat(257), /without \.\./],
    ]) {
      await pathInput.fill(value);
      await page.locator('[data-action="add-path"]').click();
      assert.match(
        await page.locator('.admission-path-form [role="alert"]').innerText(),
        message,
      );
    }
    await pathInput.fill("docs/");
    await page.locator('[data-action="add-path"]').click();
    assert.equal(
      await page.locator(".admission-values code").innerText(),
      "docs/",
    );
    await pathInput.fill("docs/");
    await page.locator('[data-action="add-path"]').click();
    assert.match(
      await page.locator('.admission-path-form [role="alert"]').innerText(),
      /Already added/,
    );
    await back();
    assert.equal(
      await page.locator('[data-rule="exclude_paths"] input').isChecked(),
      true,
    );
    await page.locator('[data-rule="exclude_paths"] input').uncheck();
    await page.locator('[data-rule="exclude_paths"] input').check();
    await back();
    assert.equal(
      await page.locator("#page-title").innerText(),
      "sample/new-project",
    );
    assert.match(await filtersRow.innerText(), /Filtered pull requests\s+4 on/);
    await page.locator(".schedule-details > summary").click();
    await page.locator('input[name="scope"]').check();
    await page.locator('select[name="schedule"]').selectOption("interval");
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
    assert.equal(savedRepo.minutes, 15);
    assert.equal(savedRepo.cron, "0 9 * * 1-5");
    assert.equal(savedRepo.zone, "Europe/London");
    assert.equal(savedRepo.admission.mode, "filtered");
    assert.deepEqual(
      savedRepo.admission.rules.map((rule) => rule.kind),
      [
        "include_users",
        "assigned",
        "mentioned",
        "exclude_users",
        "include_paths",
        "exclude_paths",
      ],
    );
    assert.equal(savedRepo.admission.rules[0].users.length, 1);
    assert.deepEqual(savedRepo.admission.rules.at(-1).paths, ["docs/"]);
    assert.equal(savedRepo.admission.rules.at(-1).enabled, true);
    assert.equal("watched" in savedRepo, false);
    const editRepo = async () => {
      await page.locator(`[data-page="repo"][data-id="${newRepoId}"]`).click();
      await settle();
      await page.locator(".schedule-details > summary").click();
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
      if (editor === "repo") {
        await page.locator(`[data-page="repo"][data-id="${id}"]`).click();
        await settle();
        await page
          .locator(".repo-disclosure:not(.schedule-details) > summary")
          .click();
        await page.locator('[data-action="delete-repo"]').click();
      } else {
        // The whole card opens the editor; the inline trash icon deletes.
        await page
          .locator(`[data-action="delete-${editor}"][data-id="${id}"]`)
          .click();
      }
      await settle();
      assert.equal((await state())[collection].length, expected);
    }
    // A freshly assigned agent is removed from its repositories before deletion.
    await settings();
    await page.locator('[data-page="agents"]').click();
    await settle();
    assert.equal(
      await page.locator(".configuration-card .row-trash").count(),
      6,
    );

    await reset();
    await page.locator("#monitoring-toggle").click();
    assert.equal(await page.locator("#running-nav-count").textContent(), "0");
    await navigate("running");
    assert.equal(await page.locator('[data-display="running"]').count(), 0);
    assert.equal(await page.locator(".work-row").count(), 22);
    await page.reload();
    await page.locator("#tray-trigger").click();
    assert.equal(
      await page.locator("#monitoring-toggle").getAttribute("aria-checked"),
      "false",
    );
    await navigate("running");
    await snapshot("paused");
    await page.locator("#monitoring-toggle").click();
    assert.equal(await page.locator("#running-nav-count").textContent(), "4");
    assert.equal(await page.locator('[data-display="queued"]').count(), 18);

    await settings();
    await page.locator('[data-page="accounts"]').click();
    await settle();
    await page.locator('[data-page="copilot"]').click();
    await settle();
    assert(
      (await page.locator(".account-row").first().textContent()).includes(
        "\u2713 Connected",
      ),
    );
    await page
      .locator('[data-action="account"][data-id="copilot-demo"]')
      .click();
    assert.equal(await page.locator("#running-nav-count").textContent(), "0");
    assert.equal(await page.locator(".account-row").count(), 0);
    assert.equal(await page.locator(".queue-empty").count(), 1);
    await navigate("running");
    assert.equal(await page.locator('[data-display="blocked"]').count(), 22);
    assert(
      (
        await page
          .locator('[data-display="blocked"]')
          .first()
          .getAttribute("title")
      ).includes("AI account disconnected"),
    );
    await settings();
    await page.locator('[data-page="agents"]').click();
    await settle();
    assert.equal(await page.locator(".row-warning").count(), 6);
    assert(
      (await page.locator(".row-warning").first().textContent()).includes(
        "Reconnect required",
      ),
    );
    await settings();
    await page.locator('[data-page="accounts"]').click();
    await settle();
    await page.locator('[data-page="copilot"]').click();
    await settle();
    await page
      .locator('[data-page="account-signin"][data-id="copilot"]')
      .click();
    await settle();
    await page.locator('input[name="accountName"]').fill("alex-ai-demo");
    await page
      .locator('[data-form="account-signin"] button[type="submit"]')
      .click();
    await settle();
    assert.equal(await page.locator(".account-row").count(), 1);
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
    await settings();
    await page.locator('[data-page="repos"]').click();
    await settle();
    assert.equal(
      await page
        .locator(".mini-switch")
        .first()
        .evaluate((element) => getComputedStyle(element).transitionDuration),
      "0s",
    );
    await settings();
    await navigate("queue");
    await page.locator("#monitoring-toggle").focus();
    await page.keyboard.press("Shift+Tab");
    assert.equal(
      await page
        .locator("#version-link")
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
    assert.equal(await page.locator(".settings-list .settings-row").count(), 2);
    await page.locator('[data-page="github"]').click();
    await settle();
    assert.equal(await page.locator(".account-row").count(), 2);
    assert.equal(await page.locator('[data-scroll="github"] h2').count(), 0);
    await snapshot("account-list");
    const startConnection = async (kind, handle) => {
      await page
        .locator(`[data-page="account-signin"][data-id="${kind}"]`)
        .click();
      await settle();
      await page.locator('input[name="accountName"]').fill(handle);
      await page
        .locator('[data-form="account-signin"] button[type="submit"]')
        .click();
      await settle();
    };
    await page.locator('[data-page="account-signin"]').click();
    await settle();
    await page.locator('input[name="accountName"]').fill("Designer-DEMO");
    await page.locator(".scenario-details summary").click();
    await page.locator('[data-action="account-signin-failed"]').click();
    assert(
      (await page.locator("#app-notice").textContent()).includes(
        "No identity was saved",
      ),
    );
    await page.locator("#notice-dismiss").click();
    assert.equal(await page.locator("#app-notice").isHidden(), true);
    assert.equal(await state(), null);
    await page.keyboard.press("Escape");
    await page.locator("#tray-trigger").click();
    assert.equal(await state(), null);
    await page.locator('[data-action="cancel-account"]').click();
    await settle();
    assert.equal(await page.locator(".account-row").count(), 2);
    await startConnection("github", "Designer-DEMO");
    assert.equal(
      await page.locator('[data-form="account-confirm"]').count(),
      0,
    );
    assert.equal((await state()).accounts.length, 4);
    assert.equal((await state()).accounts.at(-1).name, "designer-demo");
    assert.equal(await page.locator(".account-row").count(), 3);
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
    assert(
      (await page.locator("#app-notice").textContent()).includes(
        "already connected",
      ),
    );
    assert.equal((await state()).accounts.length, 4);
    await page.locator('[data-action="cancel-account"]').click();
    await settle();
    await back();
    await page.locator('[data-page="copilot"]').click();
    await settle();
    await startConnection("copilot", "designer-demo");
    assert.equal((await state()).accounts.length, 5);
    const copilotAccountId = (await state()).accounts.at(-1).id;
    await snapshot("accounts-connected");

    await settings();
    await page.locator('[data-page="repos"]').click();
    await settle();
    await page.locator('[data-page="repo-url"]').click();
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
    await page.locator('[data-page="github"]').click();
    await settle();
    await page
      .locator('[data-page="account-signin"][data-id="github"]')
      .click();
    await settle();
    await page.locator('input[name="accountName"]').fill("never-submitted");
    await page.reload();
    await page.locator("#tray-trigger").click();
    assert.equal((await state()).accounts.length, 5);
    assert.equal(context.pages().length, 1);

    await settings();
    await page.locator('[data-page="advanced"]').click();
    await settle();
    await page.locator('[data-action="reset"]').click();
    assert.equal(await page.locator("#ready-count").textContent(), "2");
    assert.equal((await state()).agents.length, 6);
    await page.goto(pathToFileURL(path.join(__dirname, "index.html")).href);
    await page.locator("#tray-trigger").click();
    assert.equal(await page.locator("#human-list .review-open").count(), 2);
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
    await unsaved.locator('[data-page="github"]').click();
    await unsaved.waitForTimeout(260);
    await unsaved.locator('[data-page="account-signin"]').click();
    await unsaved.waitForTimeout(260);
    await unsaved.locator('input[name="accountName"]').fill("unsaved-demo");
    await unsaved
      .locator('[data-form="account-signin"] button[type="submit"]')
      .click();
    assert(
      (await unsaved.locator("#app-notice").textContent()).includes(
        "Could not save",
      ),
    );
    assert.equal(
      await unsaved.locator('[data-form="account-signin"]').count(),
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
