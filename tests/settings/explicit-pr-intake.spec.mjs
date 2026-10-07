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
  return {
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
    if (command === "admit_explicit_pull_request")
      return store("fixture_explicit_pr_intake", {
        ...args,
        responses: state.responses,
      });
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

for (const genie of [false, true]) {
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
