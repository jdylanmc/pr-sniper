const assert = require("node:assert/strict");
const { pathToFileURL } = require("node:url");
const path = require("node:path");
const { chromium } = require("@playwright/test");

const key = "pr-sniper.poc.configuration-wizard.v1";
const url = pathToFileURL(path.join(__dirname, "index.html")).href;

(async () => {
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1200, height: 900 },
  });
  const page = await context.newPage();
  const errors = [];
  const externalRequests = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (/^https?:/.test(request.url())) externalRequests.push(request.url());
  });
  const saved = () =>
    page.evaluate(
      (storageKey) => JSON.parse(localStorage.getItem(storageKey)),
      key,
    );
  const next = () => page.locator("#next").click();
  const disabled = async () =>
    assert.equal(await page.locator("#next").isDisabled(), true);
  const openLibrary = () =>
    page.getByRole("button", { name: "Shared library", exact: true }).click();
  const scenario = async (value) => {
    if (!(await page.locator("#tools-panel").isVisible()))
      await page.getByText("Prototype controls", { exact: true }).click();
    await page.locator("#scenario").selectOption(value);
  };
  const reset = async () => {
    await page.goto(url);
    await page.evaluate(
      (storageKey) => localStorage.removeItem(storageKey),
      key,
    );
    await page.reload();
  };
  const toAgent = async () => {
    await page.locator('[name="ai"][value="demo-1001"]').check();
    await next();
    assert.equal(await page.locator("#github-account").inputValue(), "");
    await page.locator("#github-account").selectOption("demo-1002");
    await disabled();
    await page.locator('[name="repository"][value="demo-repo-1"]').check();
    await next();
  };
  try {
    await reset();
    assert.equal((await saved()).version, 3);
    assert.equal(
      await page
        .getByRole("button", { name: /Save for later|Resume|Discard/i })
        .count(),
      0,
    );
    const legacy = {
      version: 1,
      agents: (await saved()).agents.map(({ doctrines, ...a }) => ({
        ...a,
        doctrine: doctrines[0] ?? "",
      })),
      saved: false,
    };
    await page.evaluate(
      ({ storageKey, data }) =>
        localStorage.setItem(storageKey, JSON.stringify(data)),
      { storageKey: key, data: legacy },
    );
    await page.reload();
    assert.equal((await saved()).version, 3);
    assert.deepEqual((await saved()).agents[0].doctrines, []);
    assert.deepEqual((await saved()).agents[1].doctrines, ["Boundaries"]);
    await disabled();
    assert.equal(await page.locator('input[type="radio"]:checked').count(), 0);
    assert.equal("step" in (await saved()), false);
    assert.equal("mode" in (await saved()), false);
    for (const value of ["empty", "loading", "error"]) {
      await scenario(value);
      await disabled();
      await page.locator('[data-action="recover"]').click();
      assert.equal(await page.locator('[name="ai"]').count(), 2);
    }

    // Account confirmation is an immediate role-specific resource operation.
    await page
      .getByRole("button", { name: "Connect another mock AI account" })
      .click();
    await page.locator("#auth-identity").selectOption("demo-1003");
    await page.locator("#begin-auth").click();
    await page.locator("#auth-deny").click();
    assert.equal((await saved()).connections.ai.includes("demo-1003"), false);
    await page.locator("#auth-retry").click();
    await page.locator("#begin-auth").click();
    await page.locator("#auth-return").click();
    assert.equal((await saved()).connections.ai.includes("demo-1003"), false);
    await page.locator("#confirm-auth").click();
    assert.equal((await saved()).connections.ai.includes("demo-1003"), true);
    assert.equal(
      (await saved()).connections.github.includes("demo-1003"),
      false,
    );
    await page.locator("#close-wizard").click();
    assert.equal(await page.locator("#modal").isVisible(), false);
    await page.reload();
    assert.equal(await page.locator('input[type="radio"]:checked').count(), 0);
    assert.equal(
      await page.locator('[name="ai"][value="demo-1003"]').count(),
      1,
    );
    assert.equal((await saved()).connections.ai.includes("demo-1003"), true);
    await page.locator('[name="ai"][value="demo-1001"]').check();
    await scenario("reconnect");
    await page.locator('[data-action="recover"]').click();
    assert.equal(await page.locator("#auth-identity").isDisabled(), true);
    await page.locator("#begin-auth").click();
    await page.locator("#auth-return").click();
    await page.locator("#confirm-auth").click();
    await next();
    await page.locator("#github-account").selectOption("demo-1001");
    await page.locator('[name="repository"][value="demo-repo-1"]').check();
    await page.locator("#github-account").selectOption("demo-1002");
    assert.equal(await page.locator('[name="repository"]:checked').count(), 0);
    await disabled();
    await page
      .getByText("Use an owner/repository directly", { exact: true })
      .click();
    await page.locator("#manual-repo").fill("not-in-fixtures/unknown");
    await page.getByRole("button", { name: "Resolve mock repository" }).click();
    assert.equal(await page.locator("#repo-error").isVisible(), true);
    await page
      .locator("#manual-repo")
      .fill("https://github.com/northstar-demo/desktop");
    await page.getByRole("button", { name: "Resolve mock repository" }).click();
    await next();
    await disabled();
    assert.equal(
      await page.locator('[name="agent"][value="demo-agent-3"]').isDisabled(),
      true,
    );

    for (const response of ["empty", "error", "loading", "reconnect"]) {
      await scenario(response);
      await page
        .getByRole("button", { name: "Create Agent", exact: true })
        .click();
      assert.equal(
        await page.getByText("Mock model response", { exact: true }).count(),
        0,
      );
      await page.locator("#load-models").click();
      if (response === "loading") {
        await page.getByRole("button", { name: "Cancel mock loading" }).click();
      } else {
        await page.waitForFunction(
          () =>
            !document
              .querySelector("#model-status")
              .textContent.startsWith("Loading"),
        );
      }
      assert.equal(await page.locator('[name="model"]').isDisabled(), true);
      if (response !== "reconnect") {
        await page.locator("#load-models").click();
        await page.waitForFunction(
          () => !document.querySelector('[name="model"]').disabled,
        );
      }
      await page.locator("#cancel-agent").click();
    }
    await scenario("normal");

    // A doctrine created within an unfinished Agent form is already shared.
    await page
      .getByRole("button", { name: "Create Agent", exact: true })
      .click();
    await page.getByText("Review instructions", { exact: true }).click();
    await page.locator("#new-doctrine").click();
    await page.locator('[name="doctrine-title"]').fill("Immediate doctrine");
    await page
      .locator('[name="doctrine-body"]')
      .fill("Review error paths and preserve state.");
    await page
      .getByRole("button", { name: "Save doctrine", exact: true })
      .click();
    assert.equal(
      (await saved()).doctrines.some((d) => d.title === "Immediate doctrine"),
      true,
    );
    assert.equal(
      await page
        .locator('[name="doctrines"][value="Immediate doctrine"]')
        .isChecked(),
      false,
    );
    await page.locator("#cancel-agent").click();
    await page.locator("#close-wizard").click();
    await page.reload();
    assert.equal(
      (await saved()).doctrines.some((d) => d.title === "Immediate doctrine"),
      true,
    );
    await toAgent();

    await page
      .getByRole("button", { name: "Create Agent", exact: true })
      .click();
    await page.locator('[name="name"]').fill("My shared reviewer");
    await page.locator('#agent-form button[type="submit"]').click();
    assert.match(
      await page.locator("#agent-error").innerText(),
      /explicitly choose/,
    );
    await page.locator("#load-models").click();
    await page.waitForFunction(
      () => !document.querySelector('[name="model"]').disabled,
    );
    assert.equal(await page.locator('[name="model"]').inputValue(), "");
    assert.equal(
      await page
        .locator('[value="mock-restricted"]')
        .evaluate((option) => option.disabled),
      true,
    );
    await page.locator('[name="model"]').selectOption("mock-claude-sonnet");
    await page.locator('#agent-form [name="ai"]').selectOption("demo-1002");
    assert.equal(await page.locator('[name="model"]').inputValue(), "");
    await page.locator("#load-models").click();
    await page.waitForFunction(
      () => !document.querySelector('[name="model"]').disabled,
    );
    assert.equal(await page.locator('[value="mock-claude-sonnet"]').count(), 0);
    await page.locator('[name="model"]').selectOption("mock-gpt");
    await page.getByText("Review instructions", { exact: true }).click();
    const options = page.locator('[name="doctrines"]');
    const doctrineNames = await options.evaluateAll((items) =>
      items.map((item) => item.value),
    );
    const list = page.getByRole("region", {
      name: "Available doctrines",
      exact: true,
    });
    assert.equal(
      await list.evaluate(
        (element) => element.scrollHeight > element.clientHeight,
      ),
      true,
    );
    await list.hover();
    await page.mouse.wheel(0, 2500);
    await page.waitForFunction(
      () => document.querySelector(".doctrine-options").scrollTop > 0,
    );
    await options.last().check();
    await list.focus();
    const bottom = await list.evaluate((element) => element.scrollTop);
    await page.keyboard.press("PageUp");
    await page.waitForFunction(
      (value) => document.querySelector(".doctrine-options").scrollTop < value,
      bottom,
    );
    await page
      .getByRole("searchbox", { name: "Find doctrines" })
      .fill("Boundaries");
    assert.equal(
      await page.locator(".doctrine-options label:visible").count(),
      1,
    );
    assert.equal(await options.last().isChecked(), true);
    await page.getByRole("searchbox", { name: "Find doctrines" }).fill("");
    for (const option of await options.all()) await option.check();
    assert.equal(
      await page.locator("#doctrine-count").innerText(),
      `${doctrineNames.length} selected`,
    );
    await page
      .getByRole("button", { name: "Clear selections", exact: true })
      .click();
    assert.equal(await page.locator('[name="doctrines"]:checked').count(), 0);
    for (const option of await options.all()) await option.check();
    await page.locator('#agent-form button[type="submit"]').click();
    const createdId = (await saved()).agents.at(-1).id;
    assert.deepEqual((await saved()).agents.at(-1).doctrines, doctrineNames);
    assert.equal((await saved()).configurations.length, 0);
    await page.locator("#close-wizard").click();
    await page.reload();
    assert.equal(
      (await saved()).agents.some((a) => a.id === createdId),
      true,
    );
    assert.equal(await page.locator('input[type="radio"]:checked').count(), 0);

    // CRUD acts on the same resources outside and inside the wizard.
    await openLibrary();
    await page
      .getByRole("button", { name: "Edit My shared reviewer", exact: true })
      .click();
    await page.locator('[name="name"]').fill("Everywhere reviewer");
    await page.getByRole("button", { name: "Save Agent", exact: true }).click();
    assert.equal(
      (await saved()).agents.find((a) => a.id === createdId).name,
      "Everywhere reviewer",
    );
    await page.locator('[data-edit-doctrine="Immediate doctrine"]').click();
    await page.locator('[name="doctrine-title"]').fill("Renamed doctrine");
    await page
      .getByRole("button", { name: "Save doctrine", exact: true })
      .click();
    assert.equal(
      (await saved()).agents
        .find((a) => a.id === createdId)
        .doctrines.includes("Renamed doctrine"),
      true,
    );
    // Select the account explicitly when creating directly in the shared library.
    await page
      .getByRole("button", { name: "Create Agent", exact: true })
      .click();
    await page.locator('[name="name"]').fill("Second shared reviewer");
    await page.locator('#agent-form [name="ai"]').selectOption("demo-1002");
    await page.locator("#load-models").click();
    await page.waitForFunction(
      () => !document.querySelector('[name="model"]').disabled,
    );
    await page.locator('[name="model"]').selectOption("mock-gpt");
    await page.getByText("Review instructions", { exact: true }).click();
    await page.locator('[name="doctrines"][value="Renamed doctrine"]').check();
    await page.locator('#agent-form button[type="submit"]').click();
    await page.locator('[data-delete-doctrine="Renamed doctrine"]').click();
    assert.equal(
      (await saved()).agents.some((a) =>
        a.doctrines.includes("Renamed doctrine"),
      ),
      false,
    );
    await page
      .getByRole("button", {
        name: "Delete Second shared reviewer",
        exact: true,
      })
      .click();
    assert.equal(
      (await saved()).agents.some((a) => a.name === "Second shared reviewer"),
      false,
    );
    await page
      .getByRole("button", { name: "Back to wizard", exact: true })
      .click();
    await page.locator('[name="ai"][value="demo-1002"]').check();
    await next();
    await page.locator("#github-account").selectOption("demo-1001");
    await page.locator('[name="repository"][value="demo-repo-1"]').check();
    await next();
    await page.locator(`[name="agent"][value="${createdId}"]`).check();
    assert.ok(
      (await page.locator("#content").innerText()).includes(
        "Everywhere reviewer",
      ),
    );
    await next();
    await disabled();
    await page.getByText("Advanced scheduling", { exact: true }).click();
    await page.locator('[name="frequency"][value="cron"]').check();
    for (const expression of [
      "60 9 * * *",
      "* * * *",
      "0 24 * * *",
      "*/0 9 * * MON-FRI",
    ]) {
      await page.locator("#cron").fill(expression);
      await disabled();
    }
    await page.locator("#cron").fill("0 9 * * MON-FRI");
    await page.locator("#timezone").fill("Not/A_Zone");
    await disabled();
    await page.locator('[name="frequency"][value="15"]').check();
    assert.equal(await page.locator("#timezone").isVisible(), false);
    assert.equal(await page.locator("#next").isDisabled(), false);
    await page.locator('[name="frequency"][value="cron"]').check();
    await page.locator("#timezone").fill("America/New_York");
    await next();
    assert.equal(await page.locator("#auto-run").isChecked(), false);
    assert.equal(await page.locator("#auto-post").isChecked(), false);
    await page.locator("#auto-post").check();
    assert.equal(await page.locator("#auto-run").isChecked(), false);
    assert.match(
      await page.locator("#gate-explanation").innerText(),
      /manual start.*posted automatically/,
    );
    await page.locator("#auto-run").check();
    await page.locator("#auto-post").uncheck();
    assert.equal(await page.locator("#auto-run").isChecked(), true);
    await page.locator("#auto-run").uncheck();
    await next();
    const beforeSave = await saved();
    await scenario("error");
    await next();
    assert.deepEqual(await saved(), beforeSave);
    await scenario("normal");
    await next();
    assert.equal((await saved()).configurations.length, 1);
    assert.equal((await saved()).configurations[0].agent, createdId);
    assert.equal((await saved()).configurations[0].autoRun, false);
    await page.getByRole("button", { name: "Done", exact: true }).click();
    await page.reload();
    assert.equal((await saved()).configurations.length, 1);
    assert.equal(await page.locator('input[type="radio"]:checked').count(), 0);
    assert.equal(
      await page
        .getByRole("button", { name: /Save for later|Resume|Discard/i })
        .count(),
      0,
    );

    // Disconnect is role-local; dependent Agents and configurations are retained.
    await openLibrary();
    await page
      .locator('[data-account-role="ai"][data-account-id="demo-1002"]')
      .click();
    assert.equal((await saved()).connections.ai.includes("demo-1002"), false);
    assert.equal(
      (await saved()).connections.github.includes("demo-1002"),
      true,
    );
    assert.equal(
      (await saved()).agents.some((a) => a.id === createdId),
      true,
    );
    assert.equal((await saved()).configurations.length, 1);
    await page
      .locator('[data-account-role="ai"][data-account-id="demo-1002"]')
      .click();
    await page.locator("#begin-auth").click();
    await page.locator("#auth-return").click();
    await page.locator("#confirm-auth").click();
    await page.locator("[data-edit-configuration]").click();
    assert.equal(
      await page.locator('[name="ai"][value="demo-1002"]').isChecked(),
      true,
    );

    for (const viewport of [
      { width: 390, height: 844 },
      { width: 320, height: 568 },
    ]) {
      await page.setViewportSize(viewport);
      const layout = await page.evaluate(() => ({
        overflow: document.documentElement.scrollWidth > innerWidth,
        footerBottom: document.querySelector("#footer").getBoundingClientRect()
          .bottom,
        height: innerHeight,
      }));
      assert.equal(layout.overflow, false);
      assert.ok(layout.footerBottom <= layout.height);
    }
    await page.setViewportSize({ width: 1200, height: 900 });
    await reset();
    await toAgent();
    await page
      .getByRole("button", { name: "Create Agent", exact: true })
      .click();
    await page.getByText("Review instructions", { exact: true }).click();
    for (const viewport of [
      { width: 540, height: 700 },
      { width: 320, height: 568 },
    ]) {
      await page.setViewportSize(viewport);
      const region = page.getByRole("region", {
        name: "Available doctrines",
        exact: true,
      });
      await region.hover();
      await page.mouse.wheel(0, -3000);
      await page.waitForFunction(
        () => document.querySelector(".doctrine-options").scrollTop === 0,
      );
      await page.mouse.wheel(0, 3000);
      await page.waitForFunction(() => {
        const element = document.querySelector(".doctrine-options");
        return (
          element.scrollTop + element.clientHeight >= element.scrollHeight - 1
        );
      });
      const last = page.locator('[name="doctrines"]').last();
      await last.click();
      assert.equal(await last.isChecked(), true);
      await last.click();
      assert.equal(
        await page
          .locator("#modal")
          .evaluate((element) => element.scrollWidth > element.clientWidth),
        false,
      );
    }
    await page.setViewportSize({ width: 1200, height: 900 });
    await page.locator("#cancel-agent").focus();
    await page.keyboard.press("Tab");
    assert.equal(
      await page.evaluate(() =>
        document.querySelector("#modal").contains(document.activeElement),
      ),
      true,
    );
    await page.keyboard.press("Escape");
    assert.equal(await page.locator("#modal").isVisible(), false);

    // Failed writes never mutate the shared resource or report success.
    await openLibrary();
    await page
      .getByRole("button", { name: "New doctrine", exact: true })
      .click();
    await page.locator('[name="doctrine-title"]').fill("Must not save");
    await page
      .locator('[name="doctrine-body"]')
      .fill("Storage failure fixture.");
    const beforeFailure = await saved();
    await page.evaluate(() => {
      window.originalSetItem = Storage.prototype.setItem;
      Storage.prototype.setItem = () => {
        throw new Error("Mock quota failure");
      };
    });
    await page
      .getByRole("button", { name: "Save doctrine", exact: true })
      .click();
    assert.equal(
      await page.locator("#library-doctrine-editor [role=alert]").isVisible(),
      true,
    );
    assert.deepEqual(await saved(), beforeFailure);
    await page.evaluate(() => {
      Storage.prototype.setItem = window.originalSetItem;
    });
    await page
      .getByRole("button", { name: "Save doctrine", exact: true })
      .click();
    assert.equal(
      (await saved()).doctrines.some((d) => d.title === "Must not save"),
      true,
    );
    await page
      .getByRole("button", { name: "Back to wizard", exact: true })
      .click();
    await page.locator('[name="agent"][value="demo-agent-1"]').check();
    await next();
    await page.locator('[name="frequency"][value="15"]').check();
    await next();
    await next();
    const beforeConfigurationFailure = await saved();
    await page.evaluate(() => {
      Storage.prototype.setItem = () => {
        throw new Error("Mock quota failure");
      };
    });
    await next();
    assert.equal(await page.locator("#save-error").isVisible(), true);
    assert.deepEqual(await saved(), beforeConfigurationFailure);
    await page.evaluate(() => {
      Storage.prototype.setItem = window.originalSetItem;
    });
    await next();
    await openLibrary();
    await page
      .getByRole("button", { name: "Edit Code reviewer", exact: true })
      .click();
    await page.locator('#agent-form [name="ai"]').selectOption("demo-1002");
    await page.locator("#load-models").click();
    await page.waitForFunction(
      () => !document.querySelector('[name="model"]').disabled,
    );
    await page.locator('[name="model"]').selectOption("mock-gpt");
    await page.getByRole("button", { name: "Save Agent", exact: true }).click();
    assert.equal((await saved()).configurations[0].ai, "demo-1002");
    assert.equal((await saved()).configurations[0].github, "demo-1002");
    await page
      .getByRole("button", { name: "Edit Legacy reviewer", exact: true })
      .click();
    await page.locator("#load-models").click();
    await page.waitForFunction(
      () => !document.querySelector('[name="model"]').disabled,
    );
    assert.equal(
      await page.locator('[name="model"]').inputValue(),
      "mock-retired-model",
    );
    await page.locator('[name="name"]').fill("Retained legacy reviewer");
    await page.getByRole("button", { name: "Save Agent", exact: true }).click();
    assert.equal(
      (await saved()).agents.find((a) => a.id === "demo-agent-3").model,
      "mock-retired-model",
    );
    assert.deepEqual(errors, []);
    assert.deepEqual(externalRequests, []);
    console.log(
      "PASS: immediate account/Agent/doctrine CRUD, shared references, no draft/resume/rollback, model isolation, schedules, gates, wheel/keyboard scrolling, compact layouts, storage errors, zero network.",
    );
  } finally {
    await context.close();
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
