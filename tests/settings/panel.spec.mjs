import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test.use({ viewport: { width: 400, height: 680 } });
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const heading = (page) => page.locator("[data-panel-heading]");
const section = async (page, name) => {
  const select = page.getByLabel("Settings section", { exact: true });
  await expect(select).toBeVisible();
  await select.selectOption({ label: name });
};
const invoke = (page, command, args) =>
  page.evaluate(
    ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args),
    { command, args },
  );
const queueRow = (page, number) =>
  page
    .locator("#handoff-queue")
    .getByRole("article", { name: `example/repo #${number}`, exact: true });

test("legacy viewport and dialog fallback preserves compact editor keyboard access to global navigation", async ({
  page,
}) => {
  await page.setViewportSize({ width: 400, height: 360 });
  await page.addInitScript(() => {
    delete HTMLElement.prototype.inert;
    Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
      value: undefined,
    });
    Object.defineProperty(HTMLDialogElement.prototype, "close", {
      value: undefined,
    });
  });
  let unsupported = 0;
  await page.route("**/assets/*.css", async (route) => {
    const response = await route.fetch();
    const body = (await response.text()).replace(/(\d+)dvh\b/g, (_, value) => {
      unsupported++;
      return `${value}unsupportedviewportunit`;
    });
    await route.fulfill({ response, body });
  });
  await page.goto("/");
  await tab(page, "Settings").click();
  await section(page, "Doctrines");
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New doctrine" });
  await modal
    .getByLabel("Title", { exact: true })
    .fill("Legacy retained draft");
  expect(unsupported).toBe(8);
  for (let i = 0; i < 18; i++) {
    await page.keyboard.press("Tab");
    expect(
      await page.evaluate(
        () =>
          !!document.activeElement?.closest(
            '[role="dialog"],.panel-header,.panel-tabs,.panel-footer',
          ),
      ),
    ).toBe(true);
  }
  await modal
    .getByRole("button", { name: "Save doctrine", exact: true })
    .scrollIntoViewIfNeeded();
  await expect(
    modal.getByRole("button", { name: "Save doctrine", exact: true }),
  ).toBeInViewport();
  await tab(page, "Running").click();
  await expect(heading(page)).toHaveText("Work queue");
  await tab(page, "Settings").click();
  await expect(modal.getByLabel("Title", { exact: true })).toHaveValue(
    "Legacy retained draft",
  );
  await page.keyboard.press("Escape");
  await expect(page.locator(".panel-shell")).toHaveAttribute(
    "data-native-visible",
    "false",
  );
});

test("one retained panel shares four destinations, editor drafts and hide-only dismissal", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const before = (await store("snapshot")).settings;
  await page.goto("/");
  await expect(tab(page, "Queue")).toHaveAttribute("aria-current", "page");
  await tab(page, "Settings").click();
  await section(page, "Doctrines");
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New doctrine" });
  await modal.getByLabel("Title", { exact: true }).fill("Retained draft");
  const body = modal.getByRole("textbox", { name: "Principles", exact: true });
  await body.fill("Keep unsaved text when navigating or hiding.");
  await body.evaluate((element) => {
    window.__draftElement = element;
  });
  for (const name of ["Queue", "Running", "Reviewed"]) {
    await tab(page, name).click();
    await expect(heading(page)).toHaveText(
      { Queue: "Your queue", Running: "Work queue", Reviewed: "Reviewed" }[
        name
      ],
    );
    await expect(modal).toBeHidden();
  }
  await expect(
    page.locator('[data-panel-view="reviewed"] article'),
  ).toHaveCount(4);
  await tab(page, "Settings").click();
  await expect(body).toHaveValue(
    "Keep unsaved text when navigating or hiding.",
  );
  await expect(body).toBeFocused();
  expect(
    await body.evaluate((element) => element === window.__draftElement),
  ).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.locator(".panel-shell")).toHaveAttribute(
    "data-native-visible",
    "false",
  );
  await invoke(page, "fixture_show_panel");
  await expect(body).toBeFocused();
  await expect(body).toHaveValue(
    "Keep unsaved text when navigating or hiding.",
  );
  await page.getByRole("button", { name: "Hide PR Sniper panel" }).click();
  await expect(page.locator(".panel-shell")).toHaveAttribute(
    "data-native-visible",
    "false",
  );
  await invoke(page, "fixture_show_panel");
  await expect(body).toBeFocused();
  expect((await store("snapshot")).settings).toEqual(before);
  await expect(page.locator(".settings-window")).toHaveCount(1);
  expect(new URL(page.url()).search).toBe("");
});

test("Reviewed shows a loading state without a premature empty-state message", async ({
  page,
  store,
  ipc,
}) => {
  await queueFixture(store);
  const held = ipc.holdNext("result_page");
  await page.goto("/");
  await tab(page, "Reviewed").click();
  await held.arrived;
  const reviewed = page.locator('[data-panel-view="reviewed"]');
  await expect(reviewed).toContainText("Loading Reviewed results...");
  await expect(reviewed).not.toContainText(
    "Reviewed results have not been loaded.",
  );

  held.release();
  await expect(reviewed.locator("article")).toHaveCount(4);
});

test("Back restores exact row, list scroll and focus and switching detail replaces one layer", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  await page.goto("/");
  const button = queueRow(page, 3).getByRole("button", {
    name: "Evidence and actions",
  });
  await button.focus();
  await button.scrollIntoViewIfNeeded();
  const scroll = await page
    .locator(".panel-content")
    .evaluate((element) => element.scrollTop);
  expect(scroll).toBeGreaterThan(0);
  await button.click();
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #3");
  const other = (await store("monitoring_snapshot")).items.find(
    (i) => i.job.number === 9,
  );
  await invoke(page, "open_queue_item", { itemId: other.id });
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #9");
  await expect(page.locator("[data-monitor-detail]")).toHaveCount(1);
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(button).toBeFocused();
  expect(
    await page
      .locator(".panel-content")
      .evaluate((element) => element.scrollTop),
  ).toBe(scroll);
  await expect(
    page.getByRole("button", { name: "Back", exact: true }),
  ).toBeHidden();
});

test("Back reacquires the exact accessible row action group after redraw with same-title neighbors", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const jobs = [1, 2].map((number) => {
    const job = fixture.review(number).job;
    return {
      ...job,
      title: "Same title",
      work: {
        id: `native-work-${number}`,
        item_id: `native-item-${number}`,
        iteration_id: `native-iteration-${number}`,
        iteration: 1,
        agent_id: fixture.settings.agents[0].id,
        enqueue_order: number,
        pass_ordinal: 1,
        trigger: "admission",
        admission: {
          watched_author: true,
          all_authors: false,
          requested_reviewer: false,
        },
      },
    };
  });
  await store("seed_queue_state", {
    jobs,
    reviews: [],
    publications: [],
    follow_ups: [],
  });
  const before = await store("monitoring_snapshot");
  await page.goto("/");
  const actions = (number) =>
    page.locator("#handoff-queue").getByRole("group", {
      name: `Actions for example/repo #${number}; account 22; item native-item-${number}`,
      exact: true,
    });
  await expect(actions(1)).toHaveCount(1);
  await expect(actions(2)).toHaveCount(1);
  const button = actions(2).getByRole("button", {
    name: "Evidence and actions",
    exact: true,
  });
  await button.evaluate((element) => {
    window.__originalRowButton = element;
  });
  await button.click();
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "example/repo #2: Same title",
  );
  expect(
    await page.evaluate(() => window.__originalRowButton.isConnected),
  ).toBe(false);
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(button).toBeFocused();
  await expect(
    actions(1).getByRole("button", {
      name: "Evidence and actions",
      exact: true,
    }),
  ).not.toBeFocused();
  expect(
    await button.evaluate((element) => element === window.__originalRowButton),
  ).toBe(false);
  expect(await store("monitoring_snapshot")).toEqual(before);
});

test("native notification identity, legacy entry commands and missing routes never reload or substitute", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const item = (await store("monitoring_snapshot")).items.find(
    (i) => i.job.number === 1,
  );
  await page.goto("/");
  await page.evaluate(() => {
    window.__sameDocument = "retained";
  });
  await invoke(page, "open_settings");
  await expect(heading(page)).toHaveText("Settings");
  await invoke(page, "open_diagnostics");
  await expect(heading(page)).toHaveText("Diagnostics");
  await expect(page.locator('[data-panel-view="utility"]')).toContainText(
    "No host events recorded",
  );
  await store("set_notifications_enabled", { enabled: true });
  const id = await store("test_notification", { itemId: item.id });
  const destination = await store("notification_destination", { id });
  await invoke(page, "open_queue_item", { itemId: destination.item_id });
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #1");
  const missing = '<img src=x onerror="window.injected=true">deleted-iteration';
  await expect(
    invoke(page, "open_queue_item", { itemId: missing }),
  ).rejects.toThrow(/No other PR/);
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "No other PR, iteration or job was selected",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  await expect(page.locator("[data-item-evidence] img")).toHaveCount(0);
  expect(await store("queue_selection")).toBe(missing);
  expect(await page.evaluate(() => window.injected)).toBeUndefined();
  expect(await page.evaluate(() => window.__sameDocument)).toBe("retained");
  await invoke(page, "hide_panel");
  await invoke(page, "fixture_show_panel");
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "No other PR",
  );
});

test("cold profile selection and delayed native snapshots keep the newest exact destination", async ({
  page,
  store,
  ipc,
}) => {
  await queueFixture(store);
  const items = (await store("monitoring_snapshot")).items;
  const first = items.find((i) => i.job.number === 1);
  const next = items.find((i) => i.job.number === 9);
  await store("select_queue_item", { itemId: first.id });
  const held = ipc.holdNext("panel_snapshot");
  await page.goto("/");
  await held.arrived;
  await invoke(page, "open_queue_item", { itemId: next.id });
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #9");
  held.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #9");
  expect(await store("queue_selection")).toBe(next.id);
  await page.reload();
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #9");
});

test("Running and Reviewed use real native jobs and exact kind identities without starting work", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const waiting = fixture.state.reviews.find((r) => r.job.number === 3);
  waiting.operation.state = "queued";
  waiting.operation.initial_attempt_at = 0;
  waiting.operation.retry_deadline = 0;
  waiting.operation.attempt_count = 0;
  waiting.result = null;
  fixture.state.publications = fixture.state.publications.filter(
    (p) => p.review.job.number !== 3,
  );
  await store("seed_queue_state", fixture.state);
  const before = await store("monitoring_snapshot");
  await page.goto("/");
  await tab(page, "Running").click();
  const row = page
    .locator("[data-running-list] article")
    .filter({ hasText: "example/repo #3" });
  await expect(row.locator(".work-reference")).toContainText("Normal pass");
  await row.getByRole("button", { name: "Open job", exact: true }).click();
  await expect(page.locator("#agent-reviews article")).toHaveCount(1);
  await expect(page.locator("#agent-reviews")).toContainText("example/repo #3");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(
    row.getByRole("button", { name: "Open job", exact: true }),
  ).toBeFocused();
  await tab(page, "Reviewed").click();
  const completed = page
    .locator('[data-panel-view="reviewed"] article')
    .filter({ hasText: "example/repo #9" });
  await completed
    .getByRole("button", {
      name: "Open evidence for example/repo #9",
      exact: true,
    })
    .click();
  await expect(heading(page)).toHaveText("Saved evidence");
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "example/repo #9",
  );
  await invoke(page, "panel_navigate", {
    route: {
      tab: "reviewed",
      detail: { type: "job", kind: "mention", id: waiting.key },
    },
  });
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "No other PR",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
  expect((await store("monitoring_snapshot")).reviews).toEqual(before.reviews);
});

test("Reviewed pages preserve native activity order and load older results without duplicates", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const reviews = Array.from({ length: 25 }, (_, index) =>
    fixture.review(index + 1),
  );
  const state = {
    jobs: reviews.map((review) => review.job),
    reviews,
    publications: [],
    follow_ups: [],
  };
  await store("seed_queue_state", state);
  reviews[7].result.output.synopsis = "Most recently changed result.";
  await store("seed_queue_state", state);

  const expected = [];
  let cursor = null;
  do {
    const page = await store("result_page", {
      request: { limit: 12, cursor, unavailable_cursor: null },
    });
    expected.push(...page.results);
    cursor = page.next_cursor;
  } while (cursor);
  const expectedIds = expected.map((row) => row.item_id);
  expect(expected).toHaveLength(25);
  expect(new Set(expectedIds).size).toBe(25);
  expect(expected[0].job.number).toBe(8);

  await page.goto("/");
  await tab(page, "Reviewed").click();
  const list = page.locator(".reviewed-list");
  const articles = list.locator("article");
  await expect(articles).toHaveCount(12);
  await expect(articles.first().locator(".reviewed-meta")).toHaveText(
    "1 completed pass, 1 review attempt",
  );
  expect(
    await articles.evaluateAll((entries) =>
      entries.map((entry) => entry.dataset.reviewedItem),
    ),
  ).toEqual(expectedIds.slice(0, 12));

  await page
    .getByRole("button", { name: "Load older results", exact: true })
    .click();
  await expect(articles).toHaveCount(24);
  await page
    .getByRole("button", { name: "Load older results", exact: true })
    .click();
  await expect(articles).toHaveCount(25);
  expect(
    await articles.evaluateAll((entries) =>
      entries.map((entry) => entry.dataset.reviewedItem),
    ),
  ).toEqual(expectedIds);
  await expect(
    page.getByRole("button", { name: "Refresh results", exact: true }),
  ).toBeFocused();
});

test("Reviewed filters require an approval receipt and distinguish follow-up from readiness", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const otherReady = fixture.review(4);
  fixture.state.jobs.push(otherReady.job);
  fixture.state.reviews.push(otherReady);
  fixture.state.publications.push(fixture.published(otherReady));
  const approved = fixture.state.reviews.find(
    (review) => review.job.number === 9,
  );
  const itemId = (
    await store("result_page", {
      request: { limit: 12, cursor: null, unavailable_cursor: null },
    })
  ).results.find((row) => row.job.number === 9).item_id;
  const effect = {
    cancelled: false,
    reconcile_attempts: 0,
    reconcile_requested: false,
    id: "reviewed-approval",
    final_id: "reviewed-final",
    item_id: itemId,
    action: "approve",
    operation: {
      ...approved.operation,
      id: "reviewed-approval-operation",
      operation_type: "github_approve",
    },
    observation: {
      write_capability: true,
      node_id: "PR_node",
      repository_id: approved.job.repository_id,
      pull_request_id: approved.job.pull_request_id,
      account_id: approved.job.account_id,
      author_id: approved.job.author_id,
      head_repository_id: approved.job.repository_id,
      head: approved.job.head_sha,
      base: approved.result.reviewed_base_sha,
      base_name: "main",
      merge_rules: [],
      merge_rules_error: null,
      state: "OPEN",
      draft: false,
      permission: "WRITE",
      mergeable: "MERGEABLE",
      merge_state: "CLEAN",
      review_decision: "APPROVED",
      checks: "SUCCESS",
      check_contexts: [],
      in_merge_queue: false,
      method: "SQUASH",
      protection: null,
      threads: [],
      reviews: [],
      comments: [],
      merged_by: null,
      merged_at: null,
      merge_commit: null,
    },
    body: "Recorded approval fixture.",
    state: "confirmed",
    error: null,
    receipt: {
      id: "reviewed-receipt",
      actor_id: approved.job.account_id,
      head: approved.job.head_sha,
      action: "approve",
      merge_commit: null,
    },
  };
  fixture.state.actions = { finals: [], effects: [effect], observations: [] };
  await store("seed_queue_state", fixture.state);

  const snapshot = await store("monitoring_snapshot");
  expect(
    snapshot.items.find((item) => item.job.number === 9).action_status.effects,
  ).toEqual([effect]);
  expect(
    snapshot.items.find((item) => item.job.number === 4).action_status.effects,
  ).toEqual([]);

  await page.goto("/");
  await tab(page, "Reviewed").click();
  const reviewed = page.locator('[data-panel-view="reviewed"]');
  const list = reviewed.locator(".reviewed-list");

  const approvedFilter = reviewed.getByRole("button", {
    name: "Approved",
    exact: true,
  });
  await approvedFilter.click();
  await expect(approvedFilter).toHaveAttribute("aria-pressed", "true");
  await expect(approvedFilter).toBeFocused();
  await expect(list.locator("article")).toHaveCount(1);
  await expect(list).toContainText("example/repo #9");
  await expect(list).not.toContainText("example/repo #4");
  await expect(list).toContainText("This is not personal review.");

  await reviewed.getByRole("button", { name: "Ready", exact: true }).click();
  await expect(list.locator("article")).toHaveCount(2);
  await expect(list).toContainText("example/repo #9");
  await expect(list).toContainText("example/repo #4");

  await reviewed
    .getByRole("button", { name: "Follow-up", exact: true })
    .click();
  await expect(list.locator("article")).toHaveCount(2);
  await expect(list).toContainText("example/repo #1");
  await expect(list).toContainText("example/repo #2");
});

test("Reviewed opens the exact cleaned destination without substituting another PR", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const other = fixture.review(4);
  fixture.state.jobs.push(other.job);
  fixture.state.reviews.push(other);
  fixture.state.publications.push(fixture.published(other));
  await store("seed_queue_state", fixture.state);
  const target = (
    await store("result_page", {
      request: { limit: 12, cursor: null, unavailable_cursor: null },
    })
  ).results.find((row) => row.job.number === 9);
  await page.clock.install();
  await page.goto("/");
  await tab(page, "Reviewed").click();
  const entry = page
    .locator('[data-panel-view="reviewed"] article')
    .filter({ hasText: "example/repo #9" });
  await expect(entry).toHaveCount(1);

  const job = target.job;
  const tracked = {
    provider: job.provider,
    configuration_id: job.configuration_id,
    account_id: job.account_id,
    repository_id: job.repository_id,
    pull_request_id: job.pull_request_id,
    number: job.number,
    head_sha: job.head_sha,
    lifecycle: "closed",
    terminal_observed: true,
    iteration_id: "reviewed-cleaned-iteration",
    item_id: target.item_id,
    iteration: 1,
    admission: {
      watched_author: true,
      all_authors: false,
      requested_reviewer: false,
    },
    admitted_at: 100,
    observed_at: 101,
  };
  await store("seed_queue_state", { ...fixture.state, tracked: [tracked] });
  expect(await store("fixture_retention")).toEqual({ cleaned: 1 });

  await entry
    .getByRole("button", {
      name: "Open evidence for example/repo #9",
      exact: true,
    })
    .click();
  const evidence = page.locator("[data-item-evidence]");
  await expect(evidence).toContainText(
    "Detail for this exact destination was cleaned",
  );
  await expect(evidence).toContainText(
    "No other PR or iteration was selected.",
  );
  await expect(evidence).not.toContainText("example/repo #4");
});

test("external auth blur and native-picker return retain unsaved preferences and connecting identity", async ({
  page,
  store,
}) => {
  await queueFixture(store);
  const before = (await store("snapshot")).settings;
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    let started = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "start_github_browser_auth") started = true;
      if (
        command === "github_auth_state" ||
        command === "start_github_browser_auth"
      )
        return Promise.resolve({
          accounts: [],
          flow: started
            ? {
                state: "connecting",
                user_code: "FIXTURE-CODE",
                verification_uri: "https://github.com/login/device",
              }
            : { state: "idle" },
        });
      if (command === "choose_repository_folder")
        return new Promise((resolve) => {
          window.__finishPicker = () => resolve(null);
        });
      return original(command, args);
    };
  });
  await page.goto("/");
  await tab(page, "Settings").click();
  await section(page, "Preferences");
  const cron = page.getByLabel("Cron expression", { exact: true });
  await cron.fill("*/23 * * * *");
  await section(page, "Integrations");
  await page.getByRole("button", { name: "Add GitHub account" }).click();
  await expect(page.locator(".github-auth-card")).toContainText("FIXTURE-CODE");
  await page.evaluate(() => window.dispatchEvent(new Event("blur")));
  await invoke(page, "hide_panel");
  await invoke(page, "fixture_show_panel");
  await expect(page.locator(".github-auth-card")).toContainText("FIXTURE-CODE");
  await page
    .getByRole("button", { name: "Choose folder...", exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => typeof window.__finishPicker))
    .toBe("function");
  await page.evaluate(() => {
    window.dispatchEvent(new Event("blur"));
    window.__finishPicker();
    window.dispatchEvent(new Event("focus"));
  });
  await expect(
    page.getByRole("button", { name: "Choose folder...", exact: true }),
  ).toBeEnabled();
  await section(page, "Preferences");
  await expect(cron).toHaveValue("*/23 * * * *");
  expect((await store("snapshot")).settings).toEqual(before);
});

test("compact and small-monitor panels keep navigation and editor controls reachable", async ({
  page,
}) => {
  await page.goto("/");
  await tab(page, "Settings").click();
  await section(page, "Doctrines");
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "New doctrine" });
  for (const viewport of [
    { width: 400, height: 680 },
    { width: 400, height: 360 },
    { width: 320, height: 300 },
  ]) {
    await page.setViewportSize(viewport);
    await expect(tab(page, "Queue")).toBeInViewport();
    await expect(
      page.getByRole("button", { name: "Hide PR Sniper panel" }),
    ).toBeInViewport();
    await modal
      .getByRole("textbox", { name: "Principles", exact: true })
      .fill("Small monitor draft");
    await modal
      .getByRole("button", { name: "Save doctrine", exact: true })
      .scrollIntoViewIfNeeded();
    await expect(
      modal.getByRole("button", { name: "Save doctrine", exact: true }),
    ).toBeInViewport();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await tab(page, "Queue").click();
    await expect(heading(page)).toHaveText("Your queue");
    await tab(page, "Settings").click();
    await expect(
      modal.getByRole("textbox", { name: "Principles", exact: true }),
    ).toHaveValue("Small monitor draft");
  }
});

test("approved shell keeps bottom destinations, authoritative pause and unavailable occupancy", async ({
  page,
  store,
}) => {
  await page.setViewportSize({ width: 408, height: 744 });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__failAutomation = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "automation_snapshot" && window.__failAutomation)
        return Promise.reject("Synthetic automation read failure");
      return original(command, args);
    };
  });

  await page.goto("/");
  await expect(page.locator("[data-setup-needed]")).toBeVisible();
  await tab(page, "Running").click();
  const toggle = page.locator(".panel-header [data-toggle-automation]");
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("[data-running-count]")).toHaveText("0");
  const navigation = page.getByRole("navigation", {
    name: "Application destinations",
  });
  await expect(navigation.getByRole("button")).toHaveCount(4);
  expect((await navigation.boundingBox()).y).toBeGreaterThan(600);
  await expect(page.locator(".panel-art")).toBeVisible();
  await expect(page.locator("[data-ai-work]")).toHaveCount(0);
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-label", "Resume automation");
  expect((await store("automation_snapshot")).paused).toBe(true);
  await page.getByRole("button", { name: "Hide PR Sniper panel" }).click();
  expect((await store("automation_snapshot")).paused).toBe(true);
  await invoke(page, "fixture_show_panel");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-label", "Pause automation");
  await page.evaluate(() => {
    window.__failAutomation = true;
  });
  await tab(page, "Running").click();
  await expect(toggle).toBeDisabled();
  await expect(toggle).toHaveAttribute("aria-label", "Monitoring unavailable");
  await expect(page.locator("[data-running-count]")).toHaveText("?");
  await expect(page.locator("[data-summary-main]")).toHaveText(
    "Work state unavailable",
  );
  await expect(page.locator("[data-panel-error]")).toContainText(
    "Synthetic automation read failure",
  );
});

for (const destination of ["Queue", "Running", "Reviewed"]) {
  test(`nonfocusing ${destination} activation retains the exact opener across a list redraw`, async ({
    page,
    store,
  }) => {
    const fixture = await queueFixture(store);
    const waiting = fixture.state.reviews.find((run) => run.job.number === 3);
    waiting.operation.state = "queued";
    waiting.operation.attempt_count = 0;
    waiting.result = null;
    fixture.state.publications = fixture.state.publications.filter(
      (publication) => publication.review.job.number !== 3,
    );
    await store("seed_queue_state", fixture.state);
    await page.clock.install();
    await page.goto("/");
    await tab(page, destination).click();
    const opener =
      destination === "Queue"
        ? queueRow(page, 9).getByRole("button", {
            name: "Evidence and actions",
          })
        : page
            .locator(
              destination === "Running"
                ? "[data-running-list] article"
                : '[data-panel-view="reviewed"] article',
            )
            .filter({
              hasText: `example/repo #${destination === "Running" ? 3 : 9}`,
            })
            .getByRole("button", {
              name:
                destination === "Running"
                  ? "Open job"
                  : "Open evidence for example/repo #9",
            });
    await tab(page, destination).focus();
    await opener.evaluate((element) => {
      window.__nonfocusingOpener = element;
      element.click();
    });
    await expect(heading(page)).toHaveText(
      destination === "Running" ? "Job details" : "Saved evidence",
    );
    // Redraw the hidden list without choosing a new destination or changing identity.
    for (const job of fixture.state.jobs) job.title += " updated";
    for (const run of fixture.state.reviews) run.job.title += " updated";
    if (destination === "Reviewed") {
      waiting.operation.state = "completed";
      waiting.result = fixture.review(3).result;
    }
    await store("seed_queue_state", fixture.state);
    await page.clock.runFor(5100);
    await page.evaluate(() => window.__settingsIdle());
    await expect
      .poll(() => page.evaluate(() => window.__nonfocusingOpener.isConnected))
      .toBe(false);
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(opener).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(heading(page)).toHaveText(
      destination === "Running" ? "Job details" : "Saved evidence",
    );
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(opener).toBeFocused();
  });
}

for (const embedded of [true, false]) {
  for (const activation of ["pointer", "nonfocusing", "keyboard"]) {
    test(`${embedded ? "panel" : "legacy fallback"} Settings restores explicit ${activation} openers through nested and replacement editors`, async ({
      page,
      store,
    }) => {
      await store("save_repository", { repository: "example/focus" });
      await page.addInitScript(() => {
        delete HTMLElement.prototype.inert;
        Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
          value: undefined,
        });
        Object.defineProperty(HTMLDialogElement.prototype, "close", {
          value: undefined,
        });
      });
      await page.goto(embedded ? "/" : "/?view=settings");
      if (embedded) await tab(page, "Settings").click();
      const activate = async (control) => {
        if (activation === "pointer") await control.click();
        else if (activation === "nonfocusing")
          await control.evaluate((element) => element.click());
        else {
          await control.focus();
          await page.keyboard.press("Enter");
        }
      };
      const opener = page
        .getByRole("article", { name: "example/focus", exact: true })
        .getByRole("button", { name: "Settings", exact: true });
      await page.getByLabel("Find a repository", { exact: true }).focus();
      await activate(opener);
      const repository = page.getByRole("dialog", {
        name: "Settings for example/focus",
        exact: true,
      });
      const people = repository.getByRole("button", {
        name: "Add people",
        exact: true,
      });
      await repository
        .getByLabel("Review start", { exact: true })
        .selectOption("manual");
      await activate(people);
      const picker = page.getByRole("dialog", {
        name: "Add people",
        exact: true,
      });
      await picker
        .getByLabel("GitHub login", { exact: true })
        .fill("retained-person");
      if (embedded) {
        await tab(page, "Running").click();
        await tab(page, "Settings").click();
        await expect(
          picker.getByLabel("GitHub login", { exact: true }),
        ).toHaveValue("retained-person");
      }
      await picker
        .getByRole("button", { name: "Close dialog", exact: true })
        .click();
      await expect(people).toBeFocused();
      await expect(
        repository.getByLabel("Review start", { exact: true }),
      ).toHaveValue("manual");
      await repository
        .getByText("Repository and connection", { exact: true })
        .click();
      await activate(
        repository.getByRole("button", {
          name: "Edit repository",
          exact: true,
        }),
      );
      const editor = page.getByRole("dialog", {
        name: "Edit repository",
        exact: true,
      });
      await editor
        .getByLabel("GitHub repository", { exact: true })
        .fill("example/unsaved");
      await editor
        .getByRole("button", { name: "Close dialog", exact: true })
        .click();
      await expect(opener).toBeFocused();
      expect((await store("snapshot")).settings.repositories[0].name).toBe(
        "example/focus",
      );
    });
  }
}
