import { expect, test } from "./fixtures.mjs";
import {
  advancedSchedule,
  closeDialog,
  repositorySettings,
  saveChanges,
  section,
  startupPreference,
} from "./navigation.mjs";

const overrideFields = [
  ["schedule", "Override schedule"],
  ["watched_authors", "Override people"],
  ["selector", "Override model"],
  ["prompt", "Override review instructions"],
  ["automatic_agent_start", "Override run reviews automatically"],
  [
    "automatic_comment_publication",
    "Override post review comments automatically",
  ],
];

async function schedule(root, value) {
  await root
    .getByLabel("Check frequency", { exact: true })
    .selectOption(value.kind === "cron" ? "cron" : "15");
  await advancedSchedule(root);
  await root
    .getByLabel(
      value.kind === "cron" ? "Cron expression" : "Interval minutes",
      { exact: true },
    )
    .fill(value.kind === "cron" ? value.expression : String(value.minutes));
  await root
    .getByLabel("Time zone", { exact: true })
    .selectOption(value.timezone);
}

async function people(page, root, values) {
  const removals = root.getByRole("button", { name: /^Remove / });
  while (await removals.count()) await removals.first().click();
  for (const { login } of values) {
    await root.getByRole("button", { name: "Add people", exact: true }).click();
    const picker = page.getByRole("dialog", {
      name: "Add people",
      exact: true,
    });
    await picker.getByLabel("GitHub login", { exact: true }).fill(login);
    await picker
      .getByRole("button", { name: "Add person", exact: true })
      .click();
    await expect(picker).toHaveCount(0);
    await expect(root.getByText(`@${login}`, { exact: true })).toBeVisible();
  }
}

async function automation(root, policy) {
  await root
    .getByRole("switch", { name: "Run reviews automatically", exact: true })
    .setChecked(policy.automatic_agent_start);
  await root
    .getByRole("switch", {
      name: "Post review comments automatically",
      exact: true,
    })
    .setChecked(policy.automatic_comment_publication);
  await schedule(root, policy.schedule);
}

async function expectPolicy(root, policy) {
  await advancedSchedule(root);
  await expect(root.getByLabel("Check frequency", { exact: true })).toHaveValue(
    policy.schedule.kind === "cron" ? "cron" : String(policy.schedule.minutes),
  );
  await expect(
    root.getByLabel(
      policy.schedule.kind === "cron" ? "Cron expression" : "Interval minutes",
      { exact: true },
    ),
  ).toHaveValue(
    policy.schedule.kind === "cron"
      ? policy.schedule.expression
      : String(policy.schedule.minutes),
  );
  await expect(root.getByLabel("Time zone", { exact: true })).toHaveValue(
    policy.schedule.timezone,
  );
  for (const { login } of policy.watched_authors)
    await expect(root.getByText(`@${login}`, { exact: true })).toBeVisible();
  await expect(root.locator(".person-row")).toHaveCount(
    policy.watched_authors.length,
  );
  await expect(
    root.getByLabel("Model", { exact: true }).locator("option:checked"),
  ).toHaveText(
    policy.selector.kind === "default"
      ? "Default"
      : `${policy.selector.value} (saved ${policy.selector.kind}, unverified)`,
  );
  await expect(root.getByLabel("Review prompt", { exact: true })).toHaveValue(
    policy.prompt,
  );
  await expect(
    root.getByRole("switch", {
      name: "Run reviews automatically",
      exact: true,
    }),
  ).toBeChecked({ checked: policy.automatic_agent_start });
  await expect(
    root.getByRole("switch", {
      name: "Post review comments automatically",
      exact: true,
    }),
  ).toBeChecked({ checked: policy.automatic_comment_publication });
  await expect(
    root.getByLabel("Reviewer assignment", { exact: true }),
  ).toHaveCount(0);
}

test("repository policy overrides stay isolated until reset to current global defaults", async ({
  page,
  store,
}) => {
  const defaults = {
    schedule: {
      kind: "cron",
      expression: "0 9 * * MON-FRI",
      timezone: "America/New_York",
    },
    watched_authors: [{ id: "12345", login: "octo" }],
    reviewer_assignment: false,
    adapter: "copilot",
    selector: { kind: "model", value: "gpt-6-astra" },
    prompt: "Review changed code for correctness, not formatting.",
    automatic_agent_start: true,
    automatic_comment_publication: false,
  };
  const overrides = {
    schedule: { kind: "interval", minutes: 7, timezone: "UTC" },
    watched_authors: [{ id: "67890", login: "hubot" }],
    reviewer_assignment: true,
    adapter: "copilot",
    selector: { kind: "agent", value: "careful-reviewer" },
    prompt: "Focus on concurrency defects.",
    automatic_agent_start: false,
    automatic_comment_publication: true,
  };
  const changedDefaults = {
    ...defaults,
    schedule: { kind: "interval", minutes: 45, timezone: "Europe/London" },
    watched_authors: [{ id: "99999", login: "monalisa" }],
    selector: { kind: "default" },
    prompt: "Report only reproducible defects.",
    automatic_comment_publication: true,
  };
  const identities = [
    ...defaults.watched_authors,
    ...overrides.watched_authors,
    ...changedDefaults.watched_authors,
  ];
  const lookups = [];
  await page.exposeFunction("__personLookup", (args) => {
    lookups.push(args);
    const identity = identities.find(({ login }) => login === args.login);
    if (!identity) throw new Error(`Unexpected identity lookup: ${args.login}`);
    return identity;
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "resolve_github_person"
        ? window.__personLookup(args)
        : original(command, args);
  });
  await store("seed_settings", { launch_at_login: true });
  for (const repository of ["octo/hello-world", "neighbor/keep-me"])
    await store("save_repository", { repository });
  const initial = (await store("snapshot")).settings;
  // Legacy selectors and hidden policy values must survive; the UI must not invent model inventory.
  initial.defaults = {
    ...initial.defaults,
    reviewer_assignment: false,
    selector: defaults.selector,
  };
  const primary = initial.repositories.find(
    ({ name }) => name === "octo/hello-world",
  );
  const neighbor = initial.repositories.find(
    ({ name }) => name === "neighbor/keep-me",
  );
  const legacy = {
    reviewer_assignment: true,
    adapter: "copilot",
    selector: overrides.selector,
  };
  primary.overrides = { ...legacy };
  await store("seed_settings", initial);
  await page.goto("/?view=settings");
  await expect(await startupPreference(page)).toBeChecked();

  await test.step("save exposed global policy while preserving hidden values and saved model", async () => {
    await section(page, "Automation");
    await expect(
      page.getByRole("switch", {
        name: "Run reviews automatically",
        exact: true,
      }),
    ).not.toBeChecked();
    await expect(
      page.getByRole("switch", {
        name: "Post review comments automatically",
        exact: true,
      }),
    ).not.toBeChecked();
    await automation(page, defaults);
    await section(page, "People");
    await people(page, page, defaults.watched_authors);
    await section(page, "Review defaults");
    await page
      .getByLabel("Review prompt", { exact: true })
      .fill(defaults.prompt);
    expect((await store("snapshot")).settings).toEqual(initial);
    await saveChanges(page);
    expect((await store("snapshot")).settings.defaults).toEqual(defaults);
    await page.reload();
    const modal = await repositorySettings(page, neighbor.name);
    await expectPolicy(modal, defaults);
    for (const [, label] of overrideFields)
      await expect(modal.getByLabel(label, { exact: true })).not.toBeChecked();
    await closeDialog(page);
  });

  await test.step("explicit overrides affect one repository and keep automation choices independent", async () => {
    const modal = await repositorySettings(page, primary.name);
    await expectPolicy(modal, { ...defaults, ...legacy });
    for (const [field, label] of overrideFields) {
      await expect(modal.getByLabel(label, { exact: true })).toBeChecked({
        checked: field === "selector",
      });
      await modal.getByLabel(label, { exact: true }).check();
    }
    await modal
      .getByLabel("Review prompt", { exact: true })
      .fill(overrides.prompt);
    await people(page, modal, overrides.watched_authors);
    await automation(modal, overrides);
    await closeDialog(page);
    await saveChanges(page);
    const saved = (await store("snapshot")).settings;
    expect(saved.defaults).toEqual(defaults);
    expect(saved.repositories.find(({ id }) => id === primary.id)).toEqual({
      ...primary,
      overrides,
    });
    expect(saved.repositories.find(({ id }) => id === neighbor.id)).toEqual(
      neighbor,
    );
    expect(saved.launch_at_login).toBe(true);
  });

  await test.step("changed defaults propagate only to inheriting fields after reload", async () => {
    await section(page, "People");
    await people(page, page, changedDefaults.watched_authors);
    await section(page, "Review defaults");
    await page
      .getByLabel("Model", { exact: true })
      .selectOption({ label: "Default" });
    await page
      .getByLabel("Review prompt", { exact: true })
      .fill(changedDefaults.prompt);
    await section(page, "Automation");
    await automation(page, changedDefaults);
    await saveChanges(page);
    await page.reload();
    const saved = (await store("snapshot")).settings;
    expect(saved.defaults).toEqual(changedDefaults);
    expect(saved.repositories.find(({ id }) => id === primary.id)).toEqual({
      ...primary,
      overrides,
    });
    expect(saved.repositories.find(({ id }) => id === neighbor.id)).toEqual(
      neighbor,
    );
    const modal = await repositorySettings(page, primary.name);
    await expectPolicy(modal, overrides);
    await closeDialog(page);
    const inherited = await repositorySettings(page, neighbor.name);
    await expectPolicy(inherited, changedDefaults);
    await closeDialog(page);
  });

  await test.step("reset each exposed override independently without erasing hidden legacy policy", async () => {
    const remaining = { ...overrides };
    for (const [field, label] of overrideFields) {
      const modal = await repositorySettings(page, primary.name);
      await modal.getByLabel(label, { exact: true }).uncheck();
      delete remaining[field];
      await expectPolicy(modal, { ...changedDefaults, ...remaining });
      for (const [key, otherLabel] of overrideFields) {
        await expect(modal.getByLabel(otherLabel, { exact: true })).toBeChecked(
          { checked: key in remaining },
        );
      }
      await closeDialog(page);
      await saveChanges(page);
      await page.reload();
      const saved = (await store("snapshot")).settings;
      expect(saved.repositories.find(({ id }) => id === primary.id)).toEqual({
        ...primary,
        overrides: remaining,
      });
      expect(saved.repositories.find(({ id }) => id === neighbor.id)).toEqual(
        neighbor,
      );
      expect(saved.defaults).toEqual(changedDefaults);
      expect(saved.launch_at_login).toBe(true);
    }
    expect(remaining).toEqual({
      reviewer_assignment: true,
      adapter: "copilot",
    });
    const modal = await repositorySettings(page, primary.name);
    await expectPolicy(modal, changedDefaults);
    await closeDialog(page);
    expect(lookups).toEqual([
      { login: "octo" },
      { login: "hubot" },
      { login: "monalisa" },
    ]);
    await expect(page.getByRole("alert")).toBeHidden();
  });
});
