const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium, webkit } = require("playwright");
const base = "http://127.0.0.1:4178/";
const key = "pr-sniper.vnext.v2";

async function check(engine, name) {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({
      viewport: { width: 1440, height: 960 },
    });
    const errors = [];
    const external = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (!request.url().startsWith(base)) external.push(request.url());
    });
    const click = async (selector) => {
      await page.locator(selector).click();
      await page.waitForTimeout(250);
    };
    const state = () =>
      page.evaluate((key) => JSON.parse(localStorage.getItem(key)), key);
    const capture = async (label) => {
      if (!process.env.PROTOTYPE_SCREENSHOTS) return;
      fs.mkdirSync(process.env.PROTOTYPE_SCREENSHOTS, { recursive: true });
      await page.screenshot({
        path: path.join(
          process.env.PROTOTYPE_SCREENSHOTS,
          `${name}-repositories-${label}.png`,
        ),
      });
    };
    await page.goto(base);
    await click('[data-scenario="configured"]');
    await click('.bottom-nav [data-nav="settings"]');
    await click('[data-page="repos"]');
    assert.equal(
      await page.locator(".repository-items .settings-row").count(),
      3,
    );
    assert.equal(await page.locator(".repository-browse").count(), 2);
    assert.equal(
      await page
        .locator('[data-scroll="repos"] input[type="checkbox"]')
        .count(),
      0,
    );
    assert(
      !/Choose folder|Scan chosen folder/.test(
        await page.locator('[data-scroll="repos"]').innerText(),
      ),
    );
    await capture("list");
    // No headings or hints: the grouped card overlaps the navy band by ~23px.
    const overlap = await page
      .locator(".repository-items")
      .evaluate(
        (el) =>
          document.querySelector(".app-header").getBoundingClientRect().bottom -
          el.getBoundingClientRect().top,
      );
    assert(overlap >= 20 && overlap <= 26, `band overlap ${overlap}`);
    assert.equal(await page.locator('[data-scroll="repos"] h2').count(), 0);
    assert.equal(await page.locator(".mini-switch").count(), 3);
    assert.deepEqual(
      await page
        .locator(".mini-switch")
        .first()
        .boundingBox()
        .then((box) => [Math.round(box.width), Math.round(box.height)]),
      [40, 24],
    );
    assert.equal(await page.locator(".floating-pill").innerText(), "URL");
    await click('[data-page="repo-browser"][data-id="github-personal"]');
    assert.equal(
      await page.locator("#repository-owner").inputValue(),
      "alex-demo",
    );
    assert.equal(
      await page.locator("#repository-results .settings-row").count(),
      4,
    );
    await capture("personal");
    await page.locator("#repository-owner").selectOption("orbit");
    assert.equal(
      await page.locator("#repository-results .settings-row").count(),
      5,
    );
    assert.equal(
      await page.locator('[data-name="orbit/console"] .row-value').innerText(),
      "Added",
    );
    await capture("organization");
    await page.locator("#repository-filter").fill("no-such-repository");
    assert.equal(
      await page.locator("#repository-results .empty-state").count(),
      1,
    );
    await page.locator("#repository-filter").fill("docs");
    await click('[data-name="orbit/docs"]');
    assert.equal(await page.locator('[data-form="repo"]').count(), 1);
    let repo = (await state()).repos.at(-1);
    assert.equal(repo.name, "orbit/docs");
    assert.equal(repo.enabled, false);
    assert.equal(repo.scope, false);
    assert.equal(
      await page.locator('select[name="accountId"]').inputValue(),
      "github-personal",
    );
    await click("#back-button");
    assert.equal(
      await page.locator(".repository-items .settings-row").count(),
      4,
    );
    assert.equal(
      await page
        .locator(`.mini-switch[data-id="${repo.id}"]`)
        .getAttribute("aria-checked"),
      "false",
    );
    await click('[data-page="repo-browser"][data-id="github-personal"]');
    await page.locator("#repository-owner").selectOption("orbit");
    await click('[data-name="orbit/docs"]');
    assert.equal((await state()).repos.length, 4);
    await click("#back-button");
    await click('[data-page="repo-url"]');
    await capture("url");
    await page
      .locator('[name="repository"]')
      .fill("https://example.com/owner/repo");
    await page.locator('[name="accountId"]').selectOption("github-team");
    await click('[data-form="repo-url"] button[type="submit"]');
    assert.equal((await state()).repos.length, 4);
    assert.match(
      await page.locator("#app-notice").innerText(),
      /GitHub repository URL/,
    );
    await page
      .locator('[name="repository"]')
      .fill("https://github.com/sample/new-project.git");
    await click('[data-form="repo-url"] button[type="submit"]');
    repo = (await state()).repos.at(-1);
    assert.equal(repo.accountId, "github-team");
    assert.equal(repo.name, "sample/new-project");
    assert.equal(repo.enabled, false);
    await click("#back-button");
    await capture("added");
    await page.reload();
    await click("#tray-trigger");
    await click('.bottom-nav [data-nav="settings"]');
    await click('[data-page="repos"]');
    assert.equal(
      await page.locator(".repository-items .settings-row").count(),
      5,
    );
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(
      await page
        .locator('[data-scroll="repos"]')
        .evaluate((el) => el.scrollWidth > el.clientWidth),
      false,
    );
    await capture("compact");
    await page.locator('[data-page="repo-browser"]').first().focus();
    await page.keyboard.press("Enter");
    await page.waitForTimeout(250);
    await click("#back-button");
    assert.equal(
      await page.evaluate(() => document.activeElement.dataset.focusKey),
      "browse-github-personal",
    );
    // Azure DevOps: connected account browses projects; teammates autocomplete.
    await click("#back-button");
    await click('[data-page="accounts"]');
    await click('[data-page="azure-devops"]');
    await click(".scenario-details summary");
    await click('[data-action="azure-scenario"][data-id="connected"]');
    await click("#back-button");
    await click("#back-button");
    await click('[data-page="repos"]');
    await click('[data-page="ado-browser"]');
    assert.equal(await page.locator("#ado-project").inputValue(), "Web");
    await page.locator("#ado-project").selectOption("Api");
    await page.waitForTimeout(250);
    await click('[data-name="contoso/Api/gateway"]');
    repo = (await state()).repos.at(-1);
    assert.equal(repo.provider, "azure_devops");
    assert.equal(repo.name, "contoso/Api/gateway");
    assert.equal(repo.enabled, false);
    await capture("azure-devops-added");
    await page.locator('input[name="admissionAll"]').uncheck();
    await click('[data-action="open-filters"]');
    await click('[data-action="open-rule"][data-kind="include_users"]');
    await page.locator("[data-person-search]").fill("bl");
    await page.waitForTimeout(400);
    assert.equal(await page.locator('[data-action="pick-person"]').count(), 2);
    await capture("azure-devops-people");
    await click('[data-action="pick-person"][data-name="Blake Example"]');
    assert.equal(
      await page.locator(".admission-values .person-handle").innerText(),
      "Blake Example",
    );
    await click("#back-button");
    await click("#back-button");
    await click('[data-form="repo"] button[type="submit"]');
    repo = (await state()).repos.find((item) => item.id === repo.id);
    assert.equal(repo.provider, "azure_devops");
    assert.deepEqual(
      repo.admission.rules.find((rule) => rule.kind === "include_users").users,
      ["Blake Example"],
    );
    await click('[data-page="repo-url"]');
    await page
      .locator('[name="repository"]')
      .fill("https://dev.azure.com/contoso/Web/_git/app");
    await click('[data-form="repo-url"] button[type="submit"]');
    assert.equal((await state()).repos.at(-1).name, "contoso/Web/app");
    await click("#back-button");
    // Saved data from before admission filters migrates on load.
    await page.evaluate((key) => {
      const saved = JSON.parse(localStorage.getItem(key));
      saved.repos = saved.repos.map(({ admission, ...repo }) => ({
        ...repo,
        watched: [],
        reviewerRequests: "off",
      }));
      localStorage.setItem(key, JSON.stringify(saved));
    }, key);
    await page.reload();
    await click("#tray-trigger");
    await click('.bottom-nav [data-nav="settings"]');
    await click('[data-page="repos"]');
    await click('[data-page="repo"][data-id="repo-0"]');
    assert.equal(
      await page.locator('input[name="admissionAll"]').isChecked(),
      true,
    );
    assert.equal(
      await page.locator('[data-action="open-filters"]').isDisabled(),
      true,
    );
    await page.goto(base);
    await click('[data-scenario="fresh"]');
    await click('.bottom-nav [data-nav="settings"]');
    await click('[data-page="repos"]');
    assert.equal(await page.locator(".repository-browse").count(), 0);
    await click('[data-page="repo-url"]');
    assert.match(
      await page.locator('[data-scroll="repo-url"]').innerText(),
      /Connect GitHub first/,
    );
    assert.deepEqual(errors, []);
    assert.deepEqual(external, []);
    console.log(
      `${name}: repository browse, owner filter, URL, duplicate, persistence, keyboard, compact, Azure DevOps and empty-state checks passed`,
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
