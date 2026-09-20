import { expect, test } from "./fixtures.mjs";

const overrideLabels = [
  "Override schedule",
  "Override watched authors",
  "Override reviewer assignment",
  "Override adapter",
  "Override selector",
  "Override prompt",
  "Override automatic agent start",
  "Override automatic comment publication",
];

async function fillPolicy(form, policy) {
  await form
    .getByLabel("Schedule type", { exact: true })
    .selectOption(policy.schedule.kind);
  if (policy.schedule.kind === "interval") {
    await form
      .getByLabel("Interval minutes", { exact: true })
      .fill(String(policy.schedule.minutes));
  } else {
    await form
      .getByLabel("Cron expression", { exact: true })
      .fill(policy.schedule.expression);
  }
  await form
    .getByLabel("Time zone", { exact: true })
    .fill(policy.schedule.timezone);
  await form
    .getByLabel("Watched GitHub identities", { exact: true })
    .fill(
      policy.watched_authors
        .map(({ id, login }) => `${id}:${login}`)
        .join("\n"),
    );
  await form
    .getByLabel("Reviewer assignment", { exact: true })
    .setChecked(policy.reviewer_assignment);
  await form
    .getByLabel("Agent adapter", { exact: true })
    .selectOption(policy.adapter);
  await form
    .getByLabel("Selector type", { exact: true })
    .selectOption(policy.selector.kind);
  if (policy.selector.kind !== "default") {
    await form
      .getByLabel("Model or named agent", { exact: true })
      .fill(policy.selector.value);
  }
  await form.getByLabel("Review prompt", { exact: true }).fill(policy.prompt);
  await form
    .getByLabel("Automatic agent start", { exact: true })
    .setChecked(policy.automatic_agent_start);
  await form
    .getByLabel("Automatic comment publication", { exact: true })
    .setChecked(policy.automatic_comment_publication);
}

async function expectPolicy(form, policy) {
  await expect(form.getByLabel("Schedule type", { exact: true })).toHaveValue(
    policy.schedule.kind,
  );
  if (policy.schedule.kind === "interval") {
    await expect(
      form.getByLabel("Interval minutes", { exact: true }),
    ).toHaveValue(String(policy.schedule.minutes));
  } else {
    await expect(
      form.getByLabel("Cron expression", { exact: true }),
    ).toHaveValue(policy.schedule.expression);
  }
  await expect(form.getByLabel("Time zone", { exact: true })).toHaveValue(
    policy.schedule.timezone,
  );
  await expect(
    form.getByLabel("Watched GitHub identities", { exact: true }),
  ).toHaveValue(
    policy.watched_authors.map(({ id, login }) => `${id}:${login}`).join("\n"),
  );
  await expect(
    form.getByLabel("Reviewer assignment", { exact: true }),
  ).toBeChecked({ checked: policy.reviewer_assignment });
  await expect(form.getByLabel("Agent adapter", { exact: true })).toHaveValue(
    policy.adapter,
  );
  await expect(form.getByLabel("Selector type", { exact: true })).toHaveValue(
    policy.selector.kind,
  );
  if (policy.selector.kind !== "default") {
    await expect(
      form.getByLabel("Model or named agent", { exact: true }),
    ).toHaveValue(policy.selector.value);
  }
  await expect(form.getByLabel("Review prompt", { exact: true })).toHaveValue(
    policy.prompt,
  );
  await expect(
    form.getByLabel("Automatic agent start", { exact: true }),
  ).toBeChecked({ checked: policy.automatic_agent_start });
  await expect(
    form.getByLabel("Automatic comment publication", { exact: true }),
  ).toBeChecked({ checked: policy.automatic_comment_publication });
}

async function expectSources(card, source, policy) {
  await expect(card.getByText(new RegExp(`\\(${source}\\)$`))).toHaveCount(8);
  await expect(
    card.getByText(
      `Automatic agent start: ${policy.automatic_agent_start ? "on" : "off"} (${source})`,
      { exact: true },
    ),
  ).toBeVisible();
  await expect(
    card.getByText(
      `Automatic comment publication: ${policy.automatic_comment_publication ? "on" : "off"} (${source})`,
      { exact: true },
    ),
  ).toBeVisible();
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
    schedule: { kind: "interval", minutes: 45, timezone: "Europe/London" },
    watched_authors: [{ id: "99999", login: "monalisa" }],
    reviewer_assignment: true,
    adapter: "copilot",
    selector: { kind: "default" },
    prompt: "Report only reproducible defects.",
    automatic_agent_start: true,
    automatic_comment_publication: true,
  };
  await store("seed_settings", { launch_at_login: true });
  await page.goto("/?view=settings");
  await expect(page.getByLabel("Request launch at login")).toBeChecked();
  for (const name of ["octo/hello-world", "neighbor/keep-me"]) {
    await page.getByLabel("GitHub repository", { exact: true }).fill(name);
    await page
      .getByRole("button", { name: "Add repository", exact: true })
      .click();
    await expect(
      page.getByRole("article", { name, exact: true }),
    ).toBeVisible();
  }
  const initial = (await store("snapshot")).settings;
  const primary = initial.repositories.find(
    ({ name }) => name === "octo/hello-world",
  );
  const neighbor = initial.repositories.find(
    ({ name }) => name === "neighbor/keep-me",
  );
  const primaryCard = page.getByRole("article", {
    name: "octo/hello-world",
    exact: true,
  });
  const neighborCard = page.getByRole("article", {
    name: "neighbor/keep-me",
    exact: true,
  });
  const globalForm = page.getByRole("form", {
    name: "Global defaults",
    exact: true,
  });
  const policyForm = primaryCard.getByRole("form", {
    name: "Repository policy",
    exact: true,
  });

  await test.step("save complete nondefault global policy through Settings", async () => {
    await expect(globalForm).toBeVisible();
    await expect(
      globalForm.getByLabel("Automatic agent start", { exact: true }),
    ).not.toBeChecked();
    await expect(
      globalForm.getByLabel("Automatic comment publication", { exact: true }),
    ).not.toBeChecked();
    await fillPolicy(globalForm, defaults);
    await globalForm
      .getByRole("button", { name: "Save defaults", exact: true })
      .click();
    await expectSources(primaryCard, "Global default", defaults);
    await expectSources(neighborCard, "Global default", defaults);
    expect((await store("snapshot")).settings.defaults).toEqual(defaults);
    await page.reload();
    await expectPolicy(globalForm, defaults);
  });

  await test.step("inherit all fields before explicitly overriding one repository", async () => {
    await primaryCard
      .getByRole("button", { name: "Policy", exact: true })
      .click();
    await expectPolicy(policyForm, defaults);
    for (const label of overrideLabels) {
      await expect(
        policyForm.getByLabel(label, { exact: true }),
      ).not.toBeChecked();
      await policyForm.getByLabel(label, { exact: true }).check();
    }
    await fillPolicy(policyForm, overrides);
    await policyForm
      .getByRole("button", { name: "Save policy", exact: true })
      .click();
    await expectSources(primaryCard, "Repository override", overrides);
    await expectSources(neighborCard, "Global default", defaults);
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

  await test.step("changed defaults propagate only to the inheriting neighbor after restart", async () => {
    await fillPolicy(globalForm, changedDefaults);
    await globalForm
      .getByRole("button", { name: "Save defaults", exact: true })
      .click();
    await expectSources(neighborCard, "Global default", changedDefaults);
    await page.reload();
    await expectPolicy(globalForm, changedDefaults);
    await expectSources(primaryCard, "Repository override", overrides);
    await expectSources(neighborCard, "Global default", changedDefaults);
    await primaryCard
      .getByRole("button", { name: "Policy", exact: true })
      .click();
    await expectPolicy(policyForm, overrides);
    await neighborCard
      .getByRole("button", { name: "Policy", exact: true })
      .click();
    await expectPolicy(
      neighborCard.getByRole("form", {
        name: "Repository policy",
        exact: true,
      }),
      changedDefaults,
    );
    const saved = (await store("snapshot")).settings;
    expect(saved.defaults).toEqual(changedDefaults);
    expect(saved.repositories.find(({ id }) => id === primary.id)).toEqual({
      ...primary,
      overrides,
    });
    expect(saved.repositories.find(({ id }) => id === neighbor.id)).toEqual(
      neighbor,
    );
  });

  await test.step("reset every override to current defaults and retain it after restart", async () => {
    for (const label of overrideLabels) {
      await policyForm.getByLabel(label, { exact: true }).uncheck();
    }
    await policyForm
      .getByRole("button", { name: "Save policy", exact: true })
      .click();
    await expectSources(primaryCard, "Global default", changedDefaults);
    const saved = (await store("snapshot")).settings;
    const reset = saved.repositories.find(({ id }) => id === primary.id);
    expect(reset).toMatchObject({
      id: primary.id,
      provider: "github",
      name: "octo/hello-world",
      enabled: true,
    });
    for (const field of Object.keys(overrides)) {
      expect(reset.overrides?.[field] ?? null).toBeNull();
    }
    expect(saved.defaults).toEqual(changedDefaults);
    expect(saved.repositories.find(({ id }) => id === neighbor.id)).toEqual(
      neighbor,
    );
    expect(saved.launch_at_login).toBe(true);
    await page.reload();
    await expectSources(primaryCard, "Global default", changedDefaults);
    await expectSources(neighborCard, "Global default", changedDefaults);
    await primaryCard
      .getByRole("button", { name: "Policy", exact: true })
      .click();
    await expectPolicy(policyForm, changedDefaults);
    for (const label of overrideLabels) {
      await expect(
        policyForm.getByLabel(label, { exact: true }),
      ).not.toBeChecked();
    }
    await expect(page.getByRole("alert")).toBeHidden();
  });
});
