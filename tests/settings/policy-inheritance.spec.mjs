import { expect, test } from "./fixtures.mjs";
import {
  fixtureAgent,
  assignment,
  saveAssignment,
  saveChanges,
  setAgentPrompt,
  repositorySettings,
  closeDialog,
  section,
} from "./navigation.mjs";

test("reusable Agents keep independent repository assignments and preserve legacy policy exactly", async ({
  page,
  store,
}) => {
  for (const repository of ["octo/hello-world", "neighbor/keep-me"])
    await store("save_repository", { repository });
  const initial = (await store("snapshot")).settings;
  initial.defaults = {
    ...initial.defaults,
    schedule: {
      kind: "cron",
      expression: "0 9 * * MON-FRI",
      timezone: "America/New_York",
    },
    watched_authors: [{ id: "12345", login: "octo" }],
    reviewer_assignment: false,
    selector: { kind: "model", value: "legacy-model" },
    prompt: "Keep the legacy global instructions.",
    automatic_agent_start: true,
  };
  initial.repositories[0].overrides = {
    schedule: { kind: "interval", minutes: 7, timezone: "UTC" },
    watched_authors: [{ id: "67890", login: "hubot" }],
    reviewer_assignment: true,
    selector: { kind: "agent", value: "legacy-reviewer" },
    prompt: "Keep the legacy repository instructions.",
    automatic_comment_publication: true,
  };
  initial.presets = [
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      name: "Legacy preset",
      body: "Legacy preset text.",
    },
  ];
  initial.agents = [
    { ...fixtureAgent, ai_account: { provider: "copilot", account_id: "33" } },
    {
      ...fixtureAgent,
      ai_account: { provider: "copilot", account_id: "33" },
      id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      name: "Second reviewer",
      prompt: "Second Agent instructions.",
    },
  ];
  await store("seed_settings", initial);
  await page.goto("/?view=settings");
  for (const [index, repository] of [
    "octo/hello-world",
    "octo/hello-world",
    "neighbor/keep-me",
  ].entries()) {
    const modal = await assignment(page, repository);
    await modal
      .getByRole("combobox", { name: "Agent", exact: true })
      .selectOption(initial.agents[index === 1 ? 1 : 0].id);
    await expect(
      modal.locator("[name=frequency],[name=cron],[name=timezone]"),
    ).toHaveCount(0);
    await modal
      .getByRole("checkbox", { name: /^Comment/ })
      .setChecked(index === 1);
    await expect(
      modal.getByRole("checkbox", { name: /^Approve/ }),
    ).toBeEnabled();
    await expect(
      modal.getByRole("checkbox", { name: /^Approve/ }),
    ).not.toBeChecked();
    await saveAssignment(page, modal);
  }
  expect(
    (await store("snapshot")).settings.repositories[0].assignments,
  ).toHaveLength(2);
  await saveChanges(page);
  let saved = (await store("snapshot")).settings;
  expect(
    saved.repositories[0].assignments.map(({ schedule }) => schedule),
  ).toEqual([initial.defaults.schedule, initial.defaults.schedule]);
  expect(saved.repositories[1].assignments[0].schedule).toEqual(
    initial.defaults.schedule,
  );
  const assignments = saved.repositories.map(({ assignments }) => assignments);
  await setAgentPrompt(page, "Updated shared Agent instructions.");
  await saveChanges(page);
  await page.reload();
  saved = (await store("snapshot")).settings;
  expect(saved.agents[0].prompt).toBe("Updated shared Agent instructions.");
  expect(saved.agents[1]).toEqual(initial.agents[1]);
  expect(saved.repositories.map(({ assignments }) => assignments)).toEqual(
    assignments,
  );
  expect(saved.defaults).toEqual(initial.defaults);
  expect(saved.presets).toEqual(initial.presets);
  expect(saved.repositories[0].overrides).toEqual(
    initial.repositories[0].overrides,
  );
  expect(saved.repositories[1].overrides).toEqual(
    initial.repositories[1].overrides,
  );
  for (const [index, repository] of [
    "octo/hello-world",
    "neighbor/keep-me",
  ].entries()) {
    const modal = await repositorySettings(page, repository);
    await expect(modal.locator(".assignment-row")).toHaveCount(
      index === 0 ? 2 : 1,
    );
    await modal
      .locator(".assignment-row")
      .first()
      .getByRole("button", { name: "Remove", exact: true })
      .click();
    await closeDialog(page);
    await section(page, "Preferences");
    await page.locator("#reset-settings").click();
    const restored = await repositorySettings(page, repository);
    await expect(restored.locator(".assignment-row")).toHaveCount(
      index === 0 ? 2 : 1,
    );
    await closeDialog(page);
    expect(
      (await store("snapshot")).settings.repositories.map(
        ({ assignments }) => assignments,
      ),
    ).toEqual(assignments);
  }
});
