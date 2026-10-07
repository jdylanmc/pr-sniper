import { test, expect } from "./fixtures.mjs";
import {
  actor,
  providerFixture,
  reviewer,
} from "./repository-provider-fixture.mjs";
import { section } from "./navigation.mjs";

const response = (body, status = 200) => ({
  status,
  headers: { "x-oauth-scopes": "repo" },
  body,
});
function responses() {
  const result = {
    22: {
      "/user": response({ id: 22, login: actor.login }),
      "/repos/orbit/one": response({
        id: 100,
        full_name: "orbit/one",
        private: true,
        archived: false,
        disabled: false,
        permissions: { pull: true },
      }),
      "/repos/orbit/one/pulls?state=open&per_page=1": response([]),
      "/repos/orbit/one/pulls/302": response({
        id: 900,
        number: 302,
        title: "Explicit unwatched PR",
        state: "open",
        draft: false,
        user: { id: 99, login: "unwatched" },
        requested_reviewers: [],
        requested_teams: [],
        head: { sha: "a".repeat(40), repo: { id: 100 } },
        base: { sha: "b".repeat(40), repo: { id: 100 } },
        updated_at: "2026-10-07T00:00:00Z",
      }),
    },
  };
  result[22]["/repos/orbit/one/pulls/303"] = response({
    ...result[22]["/repos/orbit/one/pulls/302"].body,
    id: 901,
    number: 303,
  });
  return result;
}

async function setup(page, store, genie = false) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  settings.defaults.watched_authors = [{ id: "11", login: "watched" }];
  settings.defaults.reviewer_assignment = false;
  await store("seed_settings", settings);
  await store("set_automation_paused", { paused: true });
  const state = { responses: responses() };
  const fixture = await providerFixture(page, store, (command, args) => {
    if (command === "resolve_provider_repository")
      return store("fixture_repository_browser", {
        ...args,
        operation: "resolve",
        responses: state.responses,
      });
    if (command === "admit_explicit_pull_request") {
      if (state.admit) return state.admit(args);
      return store("fixture_explicit_pr_intake", {
        ...args,
        responses: state.responses,
      });
    }
  });
  fixture.accounts = [{ ...actor, connection_generation: 0 }];
  if (genie) await store("fixture_show_panel");
  await page.goto(genie ? "/" : "/?view=settings");
  if (genie) {
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    await page
      .locator(".settings-window")
      .getByRole("button", { name: "Set up with Genie", exact: true })
      .click();
    await page.locator('[data-genie-edit="repositories"]').click();
  } else await section(page, "Repositories");
  return { fixture, state };
}

async function enter(page, url = "https://github.com/orbit/one/pull/302") {
  await page
    .getByRole("button", { name: "Add repository by URL", exact: true })
    .click();
  const intake = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await intake.getByLabel("Repository URL").fill(url);
  await intake
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  return page.getByRole("dialog", {
    name: "Settings for orbit/one",
    exact: true,
  });
}

async function assign(page, editor) {
  await editor
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assignment = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await assignment
    .getByLabel("Agent", { exact: true })
    .selectOption(reviewer.id);
  await assignment
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
}

async function replaceUrl(page, editor, number) {
  await editor
    .locator("summary")
    .filter({ hasText: "Repository and connection" })
    .click();
  await editor
    .getByRole("button", { name: "Edit repository", exact: true })
    .click();
  const intake = page.getByRole("dialog", {
    name: "Edit repository",
    exact: true,
  });
  await intake
    .getByLabel("Repository URL")
    .fill(`https://github.com/orbit/one/pull/${number}`);
  await intake
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  const replacement = page.getByRole("dialog", {
    name: "Settings for orbit/one",
    exact: true,
  });
  await expect(replacement.locator("[data-explicit-pr-status]")).toContainText(
    `PR #${number}`,
  );
  return replacement;
}

function holdOldAdmission(state, store) {
  const arrived = Promise.withResolvers();
  const gate = Promise.withResolvers();
  state.admit = async (args) => {
    let result;
    try {
      result = {
        value: await store("fixture_explicit_pr_intake", {
          ...args,
          responses: state.responses,
        }),
      };
    } catch (error) {
      result = { error };
    }
    if (args.number === 302) {
      arrived.resolve();
      await gate.promise;
    }
    if ("error" in result) throw result.error;
    return result.value;
  };
  return { arrived: arrived.promise, release: () => gate.resolve() };
}

test("held explicit PR Save preserves assignment capabilities across replacement intent", async ({
  page,
  store,
}) => {
  const { state } = await setup(page, store);
  let editor = await enter(page);
  await assign(page, editor);
  await editor
    .locator(".assignment-row")
    .getByRole("button", { name: "Edit", exact: true })
    .click();
  const assignment = page.getByRole("dialog", {
    name: "Edit assignment",
    exact: true,
  });
  await assignment.getByRole("checkbox", { name: /^Publish Comment/ }).check();
  await assignment.getByRole("checkbox", { name: /^Reply Comment/ }).check();
  await assignment.getByRole("radio", { name: "Approve", exact: true }).check();
  await assignment
    .getByRole("button", { name: "Save assignment", exact: true })
    .click();
  const configured = (await store("snapshot")).settings.repositories[0];
  expect(configured.assignments[0]).toMatchObject({
    comment: true,
    actions: { reply: true, approve: true, merge: false },
  });
  const held = holdOldAdmission(state, store);
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await held.arrived;
  try {
    editor = await replaceUrl(page, editor, 303);
    held.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "PR #303",
    );
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "PR #303 is admitted",
    );
    const saved = await store("saved_resources");
    expect(saved.settings.repositories[0].assignments).toEqual(
      configured.assignments,
    );
    expect(saved.settings.repositories[0].primary_assignment_id).toBe(
      configured.primary_assignment_id,
    );
    expect(saved.readiness.repositories[0].assignments[0][1]).toEqual({
      primary: true,
      comment: true,
      reply: true,
      approve: true,
      merge: false,
    });
    expect(
      (await store("monitoring_snapshot")).jobs.map((job) => job.number),
    ).toEqual([302, 303]);
    expect((await store("automation_snapshot")).paused).toBe(true);
  } finally {
    held.release();
  }
});

for (const genie of [false, true]) {
  for (const existing of [false, true]) {
    test(`Cancel discards explicit PR intent before later ordinary Save (${genie ? "Genie" : "Settings"}; ${existing ? "existing" : "new"} repository)`, async ({
      page,
      store,
    }) => {
      const { fixture } = await setup(page, store, genie);
      if (existing) {
        const configured = await enter(page, "https://github.com/orbit/one");
        await assign(page, configured);
        await configured
          .getByRole("button", { name: "Save repository", exact: true })
          .click();
        await expect(configured).toBeHidden();
      }
      let editor = await enter(page);
      if (!existing) await assign(page, editor);
      await editor
        .getByRole("button", { name: "Cancel repository changes", exact: true })
        .click();
      expect((await store("monitoring_snapshot")).jobs).toEqual([]);
      await page.locator("[data-repository]").first().click();
      editor = page.getByRole("dialog", {
        name: "Settings for orbit/one",
        exact: true,
      });
      await editor.locator("[data-repository-enabled]").check();
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await page.evaluate(() => window.__settingsIdle());
      expect(
        fixture.calls.filter(
          (call) => call.command === "admit_explicit_pull_request",
        ),
      ).toEqual([]);
      expect((await store("monitoring_snapshot")).jobs).toEqual([]);
      await expect(editor).toBeHidden();
    });
  }

  test(`Back retains explicit PR intent for later Save (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture } = await setup(page, store, genie);
    let editor = await enter(page);
    await assign(page, editor);
    await editor
      .getByRole("button", { name: "Close dialog", exact: true })
      .click();
    await page.locator("[data-repository]").first().click();
    editor = page.getByRole("dialog", {
      name: "Settings for orbit/one",
      exact: true,
    });
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "PR #302",
    );
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "admitted",
    );
    expect(
      fixture.calls
        .filter((call) => call.command === "admit_explicit_pull_request")
        .map((call) => call.args.number),
    ).toEqual([302]);
    expect(
      (await store("monitoring_snapshot")).jobs.map((job) => job.number),
    ).toEqual([302]);
  });

  test(`held old PR completion cannot discard replacement URL intent (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture, state } = await setup(page, store, genie);
    let editor = await enter(page);
    await assign(page, editor);
    const held = holdOldAdmission(state, store);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await held.arrived;
    try {
      expect(
        (await store("monitoring_snapshot")).jobs.map((job) => job.number),
      ).toEqual([302]);
      editor = await replaceUrl(page, editor, 303);
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #303",
      );
      held.release();
      await page.evaluate(() => window.__settingsIdle());
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await page.evaluate(() => window.__settingsIdle());
      expect(
        fixture.calls
          .filter((call) => call.command === "admit_explicit_pull_request")
          .map((call) => call.args.number),
      ).toEqual([302, 303]);
      expect(
        (await store("monitoring_snapshot")).jobs.map((job) => job.number),
      ).toEqual([302, 303]);
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #303",
      );
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "admitted",
      );
    } finally {
      held.release();
    }
  });

  test(`earlier intake failure stays visible without replacing current PR intent (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture, state } = await setup(page, store, genie);
    let editor = await enter(page);
    await assign(page, editor);
    state.responses[22]["/repos/orbit/one/pulls/302"] = { error: "network" };
    const held = holdOldAdmission(state, store);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await held.arrived;
    try {
      editor = await replaceUrl(page, editor, 303);
      held.release();
      await page.evaluate(() => window.__settingsIdle());
      await expect(editor.locator("[data-earlier-intake-error]")).toContainText(
        "PR #302",
      );
      await expect(editor.locator("[data-earlier-intake-error]")).toContainText(
        "current request is unchanged",
      );
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #303",
      );
      await expect(editor.locator("[data-resource-error]")).toBeHidden();
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #303 is admitted",
      );
      expect(
        fixture.calls
          .filter((call) => call.command === "admit_explicit_pull_request")
          .map((call) => call.args.number),
      ).toEqual([302, 303]);
      expect(
        (await store("monitoring_snapshot")).jobs.map((job) => job.number),
      ).toEqual([303]);
    } finally {
      held.release();
    }
  });

  test(`held intake outcome follows the retained intent after Back and reopen (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture, state } = await setup(page, store, genie);
    let editor = await enter(page);
    await assign(page, editor);
    const held = holdOldAdmission(state, store);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await held.arrived;
    try {
      await editor
        .getByRole("button", { name: "Close dialog", exact: true })
        .click();
      await page.locator("[data-repository]").first().click();
      editor = page.getByRole("dialog", {
        name: "Settings for orbit/one",
        exact: true,
      });
      held.release();
      await page.evaluate(() => window.__settingsIdle());
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #302 is admitted",
      );
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(editor).toBeHidden();
      expect(
        fixture.calls
          .filter((call) => call.command === "admit_explicit_pull_request")
          .map((call) => call.args.number),
      ).toEqual([302]);
      expect(
        (await store("monitoring_snapshot")).jobs.map((job) => job.number),
      ).toEqual([302]);
    } finally {
      held.release();
    }
  });

  test(`unavailable intake after Back stays visible and retries the retained actor (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture, state } = await setup(page, store, genie);
    let editor = await enter(page);
    await assign(page, editor);
    state.responses[22]["/user"] = response({ message: "Unauthorized" }, 401);
    const held = holdOldAdmission(state, store);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await held.arrived;
    try {
      await editor
        .getByRole("button", { name: "Close dialog", exact: true })
        .click();
      held.release();
      await expect(page.locator("#error")).toContainText(
        "PR #302 intake for orbit/one",
      );
      await expect(page.locator("#error")).toContainText(
        "GitHub is disconnected",
      );
      expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
        true,
      );
      expect((await store("monitoring_snapshot")).jobs).toEqual([]);
      await page.locator("[data-repository]").first().click();
      editor = page.getByRole("dialog", {
        name: "Settings for orbit/one",
        exact: true,
      });
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #302",
      );
      await expect(editor.locator("[data-resource-error]")).toContainText(
        "GitHub is disconnected",
      );
      state.responses = responses();
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "admitted",
      );
      await expect(editor.locator("[data-resource-error]")).toBeHidden();
      await expect(page.locator("#error")).toBeHidden();
      const calls = fixture.calls.filter(
        (call) => call.command === "admit_explicit_pull_request",
      );
      expect(calls.map((call) => call.args.number)).toEqual([302, 302]);
      expect(
        calls.map((call) => call.args.expected.provider_account_id),
      ).toEqual(["22", "22"]);
      expect(
        (await store("monitoring_snapshot")).jobs.map((job) => job.number),
      ).toEqual([302]);
    } finally {
      held.release();
    }
  });

  test(`Cancel during a held response preserves admitted work and later replacement intent (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture, state } = await setup(page, store, genie);
    let editor = await enter(page);
    await assign(page, editor);
    const held = holdOldAdmission(state, store);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await held.arrived;
    try {
      const original = (await store("monitoring_snapshot")).jobs[0];
      await editor
        .getByRole("button", { name: "Cancel repository changes", exact: true })
        .click();
      editor = await enter(page, "https://github.com/orbit/one/pull/303");
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #303",
      );
      held.release();
      await page.evaluate(() => window.__settingsIdle());
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
        "PR #303 is admitted",
      );
      const jobs = (await store("monitoring_snapshot")).jobs;
      expect(jobs.map((job) => job.number)).toEqual([302, 303]);
      expect(jobs[0].work.id).toBe(original.work.id);
      expect(
        fixture.calls
          .filter((call) => call.command === "admit_explicit_pull_request")
          .map((call) => call.args.number),
      ).toEqual([302, 303]);
    } finally {
      held.release();
    }
  });

  test(`exact PR queues through native Store after configuration Save (${genie ? "Genie" : "Settings"})`, async ({
    page,
    store,
  }) => {
    const { fixture } = await setup(page, store, genie);
    let editor = await enter(page);
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "PR #302",
    );
    expect(
      fixture.calls.filter((c) => c.command === "admit_explicit_pull_request"),
    ).toHaveLength(0);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.getByRole("alert")).toContainText(
      "assign at least one",
    );
    expect(
      fixture.calls.filter((c) => c.command === "admit_explicit_pull_request"),
    ).toHaveLength(0);
    await editor
      .getByRole("button", { name: "Assign agent", exact: true })
      .click();
    const assignment = page.getByRole("dialog", {
      name: "Assign agent",
      exact: true,
    });
    await assignment
      .getByLabel("Agent", { exact: true })
      .selectOption(reviewer.id);
    await assignment
      .getByRole("button", { name: "Assign agent", exact: true })
      .click();
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "admitted",
    );
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "paused",
    );
    await expect(
      editor.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Enabled");
    await expect(
      editor.locator("[data-repository-monitoring-detail]"),
    ).toContainText("Global Monitoring paused");
    const work = await store("monitoring_snapshot");
    expect(work.jobs).toHaveLength(1);
    expect(work.jobs[0].number).toBe(302);
    expect(work.jobs[0].watched_author).toBe(false);
    const id = work.jobs[0].work.id;
    await editor
      .getByRole("button", { name: "Cancel repository changes", exact: true })
      .click();
    editor = await enter(page);
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
      "Existing work",
    );
    const repeated = await store("monitoring_snapshot");
    expect(repeated.jobs).toHaveLength(1);
    expect(repeated.jobs[0].work.id).toEqual(id);
    expect((await store("snapshot")).settings.repositories).toHaveLength(1);
    await page.reload();
    expect((await store("monitoring_snapshot")).jobs).toHaveLength(1);
  });
}

test("exact PR provider failure is visible and fresh retry resolves through saved actor", async ({
  page,
  store,
}) => {
  const { state } = await setup(page, store);
  state.responses[22]["/repos/orbit/one/pulls/302"] = response(
    { message: "Not Found" },
    404,
  );
  const editor = await enter(page);
  const intake = page.getByRole("dialog", {
    name: "Add repository by URL",
    exact: true,
  });
  await expect(intake.getByRole("alert")).toContainText("pull request");
  await expect(intake.getByRole("alert")).toContainText("read access");
  expect((await store("snapshot")).settings.repositories).toBeUndefined();
  state.responses = responses();
  await intake
    .getByRole("button", { name: "Add & configure", exact: true })
    .click();
  await expect(editor).toBeVisible();
});

test("ordinary repository URL never admits a PR and disabled explicit Save stays blocked", async ({
  page,
  store,
}) => {
  const { fixture } = await setup(page, store);
  let editor = await enter(page, "https://github.com/orbit/one");
  await expect(editor.locator("[data-explicit-pr-status]")).toBeHidden();
  await editor
    .getByRole("button", { name: "Cancel repository changes" })
    .click();
  editor = await enter(page);
  await editor.locator("[data-repository-enabled]").uncheck();
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
    "not queued",
  );
  await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
    "disabled",
  );
  expect((await store("monitoring_snapshot")).jobs).toEqual([]);
  expect(
    fixture.calls.filter((c) => c.command === "admit_explicit_pull_request"),
  ).toHaveLength(1);
});

test("failed post-Save PR read retains committed configuration and retry queues once", async ({
  page,
  store,
}) => {
  const { state } = await setup(page, store);
  const editor = await enter(page);
  await editor
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assignment = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await assignment
    .getByLabel("Agent", { exact: true })
    .selectOption(reviewer.id);
  await assignment
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  state.responses[22]["/repos/orbit/one/pulls/302"] = response(
    { message: "Not Found" },
    404,
  );
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor.getByRole("alert")).toContainText("Configuration saved");
  await expect(editor.getByRole("alert")).toContainText("read access");
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(true);
  expect((await store("monitoring_snapshot")).jobs).toEqual([]);
  state.responses = responses();
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor.locator("[data-explicit-pr-status]")).toContainText(
    "admitted",
  );
  await expect(editor.getByRole("alert")).toBeHidden();
  expect((await store("monitoring_snapshot")).jobs).toHaveLength(1);
});
