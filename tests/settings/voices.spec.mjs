import { test, expect } from "./fixtures.mjs";
import { fixtureAgent, section } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const editor = (page, name) => page.getByRole("dialog", { name, exact: true });
const card = (page, kind, name) =>
  page.locator(`.${kind}-card`).filter({
    has: page.getByRole("heading", { name, exact: true }),
  });
const guardrails = "Fixed guardrails: tone only; critique code, never people";
const seeds = ["Caveman", "Gilfoyle", "Merlin"];

async function accountFixture(page) {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "copilot_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "copilot",
              account_id: "101",
              login: "fixture-ai",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        });
      if (command === "list_copilot_models")
        return Promise.resolve([
          { id: "fixture-model", name: "Fixture model" },
        ]);
      return original(command, args);
    };
  });
}

async function open(page, store, agent = {}) {
  const settings = (await store("snapshot")).settings;
  // Fixtures name references by title: Doctrine titles migrate on save; the
  // Voice title is looked up here.
  const { voice, ...rest } = agent;
  settings.agents = [
    {
      ...fixtureAgent,
      doctrines: [],
      ...rest,
      ...(voice
        ? { voice_id: settings.voices.find((v) => v.title === voice).id }
        : {}),
    },
  ];
  await store("seed_settings", settings);
  await accountFixture(page);
  await page.goto("/");
  await tab(page, "Settings").click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  return settings;
}

const openVoice = async (page, title) => {
  await card(page, "voice", title)
    .getByRole("heading", { name: title, exact: true })
    .click({ force: true });
  return editor(page, `Editing ${title}`);
};
const openAgent = async (page, name = fixtureAgent.name) => {
  await section(page, "Agents");
  await card(page, "agent", name)
    .getByRole("heading", { name, exact: true })
    .click({ force: true });
  return editor(page, `Editing ${name}`);
};
test.use({ viewport: { width: 408, height: 744 } });

const voices = async (store) => (await store("snapshot")).settings.voices;
/** The first Agent's references by id, resolved to library titles. */
const refs = async (store) => {
  const settings = (await store("snapshot")).settings;
  const agent = settings.agents[0];
  return {
    agent,
    voice: settings.voices.find((v) => v.id === agent.voice_id)?.title,
    doctrines: (agent.doctrine_ids ?? []).map(
      (id) => settings.doctrines.find((d) => d.id === id)?.title ?? id,
    ),
  };
};
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

for (const width of [408, 320]) {
  test(`Voices library drills in, adds with the floating pill and persists the guard at ${width}px`, async ({
    page,
    store,
  }, testInfo) => {
    await page.setViewportSize({ width, height: 744 });
    await open(page, store);
    await expect(page.locator('[data-settings-count="voices"]')).toHaveText(
      "3",
    );
    await page
      .locator(".settings-overview")
      .getByRole("button", { name: "Voices", exact: true })
      .click();
    await expect(page.locator(".voice-card h3")).toHaveText(seeds);
    // One grouped list: contiguous rows inside a single shared card.
    const grouped = await page
      .locator(".voice-list.resource-library")
      .evaluateAll((lists) =>
        lists.map((list) => {
          const card = list.getBoundingClientRect();
          const rows = [...list.children].map((row) =>
            row.getBoundingClientRect(),
          );
          return {
            rows: rows.length,
            inside:
              rows[0].top >= card.top && rows.at(-1).bottom <= card.bottom,
            gaps: rows.slice(1).map((row, i) => row.top - rows[i].bottom),
          };
        }),
      );
    expect(grouped).toHaveLength(1);
    expect(grouped[0].rows).toBe(3);
    expect(grouped[0].inside).toBe(true);
    for (const gap of grouped[0].gaps) expect(Math.abs(gap)).toBeLessThan(1);
    await expect(page.locator(".voice-card .word-count").first()).toHaveText(
      /^\d+ words \/ 0 Agents$/,
    );
    const add = page.getByRole("button", { name: "New voice", exact: true });
    await expect(add).toBeVisible();
    const pill = await add.evaluate((element) => {
      const style = getComputedStyle(element);
      const box = element.getBoundingClientRect();
      return {
        position: style.position,
        background: style.backgroundColor,
        right: innerWidth - box.right,
        inside: box.left >= 0 && box.right <= innerWidth,
      };
    });
    expect(pill).toEqual({
      position: "fixed",
      background: "rgb(246, 135, 67)",
      right: 18,
      inside: true,
    });
    const overflow = await page
      .locator(".voice-list")
      .evaluate((list) => list.scrollWidth - list.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
    await page.screenshot({ path: testInfo.outputPath(`voices-${width}.png`) });

    await add.click();
    let modal = editor(page, "New voice");
    const guard = modal.getByRole("checkbox", {
      name: guardrails,
      exact: true,
    });
    await expect(guard).toBeChecked();
    await modal.getByLabel("Title", { exact: true }).fill("Pirate");
    await modal
      .getByRole("textbox", { name: "Style", exact: true })
      .fill("Talk like a pirate.");
    await expect(modal.locator("[data-count]")).toHaveText("4 words");
    await modal
      .getByRole("textbox", { name: "Style", exact: true })
      .fill("Arr.");
    await expect(modal.locator("[data-count]")).toHaveText("1 word");
    await guard.uncheck();
    await page.screenshot({
      path: testInfo.outputPath(`voice-editor-${width}.png`),
    });
    await modal
      .getByRole("button", { name: "Save voice", exact: true })
      .click();
    await expect(modal).toHaveCount(0);
    const created = (await voices(store)).at(-1);
    expect(created.id).toMatch(uuid);
    expect(created).toEqual({
      id: created.id,
      title: "Pirate",
      body: "Arr.",
      guardrails: false,
    });
    await expect(page.locator(".voice-card h3")).toHaveText([
      ...seeds,
      "Pirate",
    ]);

    await page.reload();
    await tab(page, "Settings").click();
    await section(page, "Voices");
    modal = await openVoice(page, "Pirate");
    await expect(
      modal.getByRole("checkbox", { name: guardrails, exact: true }),
    ).not.toBeChecked();
    await modal
      .getByRole("checkbox", { name: guardrails, exact: true })
      .check();
    await modal
      .getByRole("button", { name: "Save voice", exact: true })
      .click();
    await expect(modal).toHaveCount(0);
    expect((await voices(store)).at(-1).guardrails).toBe(true);
  });
}

test("edit keeps focus on the renamed row and seeds stay deleted after restart", async ({
  page,
  store,
}) => {
  await open(page, store);
  await section(page, "Voices");
  const merlin = (await voices(store)).find((v) => v.title === "Merlin");
  const modal = await openVoice(page, "Merlin");
  await modal.getByLabel("Title", { exact: true }).fill("Wizard");
  await modal.getByRole("button", { name: "Save voice", exact: true }).click();
  await expect(modal).toHaveCount(0);
  await expect(
    page.locator(`[data-focus-key="voice:${merlin.id}:edit"]`),
  ).toBeFocused();

  await card(page, "voice", "Gilfoyle").locator("[data-remove]").click();
  const confirm = editor(page, "Delete this voice?");
  await confirm.getByRole("button", { name: "Delete voice" }).click();
  await expect(card(page, "voice", "Gilfoyle")).toHaveCount(0);
  for (const title of ["Caveman", "Wizard"]) {
    await card(page, "voice", title).locator("[data-remove]").click();
    await editor(page, "Delete this voice?")
      .getByRole("button", { name: "Delete voice" })
      .click();
  }
  await expect(page.locator(".voice-card")).toHaveCount(0);
  await page.reload();
  expect(await voices(store)).toEqual([]);
  await tab(page, "Settings").click();
  await expect(page.locator('[data-settings-count="voices"]')).toHaveText("0");
});

test("Agent Voice drills in with None by default, renames follow and in-use delete is blocked", async ({
  page,
  store,
}, testInfo) => {
  await open(page, store);
  let modal = await openAgent(page);
  await expect(modal.locator("[data-voice-summary]")).toHaveText("None");
  await modal.locator("[data-open-voices]").scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("agent-voice-row.png") });
  await modal
    .getByRole("button", { name: "Choose voice", exact: true })
    .click();
  await expect(modal.locator('form[data-view="voices"]')).toBeVisible();
  const none = modal.getByRole("radio", { name: "None", exact: true });
  await expect(none).toBeChecked();
  await expect(none).toBeFocused();
  await expect(modal.locator("[data-voice-choice]")).toHaveText([
    "None",
    ...seeds,
  ]);
  await page.screenshot({ path: testInfo.outputPath("voice-picker.png") });
  await modal.getByRole("radio", { name: "Gilfoyle", exact: true }).check();
  await expect(modal.locator("[data-voice-summary]")).toHaveText("Gilfoyle");
  await page.locator("[data-panel-back]").click();
  await expect(modal.locator('form[data-view="voices"]')).toHaveCount(0);
  await expect(
    modal.getByRole("button", { name: "Choose voice", exact: true }),
  ).toBeFocused();
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  const chosen = await refs(store);
  expect(chosen.voice).toBe("Gilfoyle");

  await section(page, "Voices");
  await expect(
    card(page, "voice", "Gilfoyle").locator(".word-count"),
  ).toHaveText(/ \/ 1 Agent$/);
  await card(page, "voice", "Gilfoyle").locator("[data-remove]").click();
  await expect(page.locator("#error")).toHaveText("In use by an Agent");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  let voice = await openVoice(page, "Gilfoyle");
  await expect(voice.locator(".resource-impact")).toContainText(
    `Shared by 1 Agent: ${fixtureAgent.name}`,
  );
  await voice
    .getByRole("button", { name: "Delete voice", exact: true })
    .click();
  await expect(voice.getByRole("alert")).toContainText("In use by an Agent");
  await voice.getByLabel("Title", { exact: true }).fill("Sysadmin");
  await voice.getByRole("button", { name: "Save voice", exact: true }).click();
  await expect(voice).toHaveCount(0);
  const saved = (await store("snapshot")).settings;
  // Rename changes the title only; the Agent keeps the same id.
  expect(saved.agents[0]).toEqual(chosen.agent);
  expect((await refs(store)).voice).toBe("Sysadmin");
  expect(saved.voices.map((v) => v.title)).toEqual([
    "Caveman",
    "Sysadmin",
    "Merlin",
  ]);

  modal = await openAgent(page);
  await expect(modal.locator("[data-voice-summary]")).toHaveText("Sysadmin");
  await modal
    .getByRole("button", { name: "Choose voice", exact: true })
    .click();
  await expect(
    modal.getByRole("radio", { name: "Sysadmin", exact: true }),
  ).toBeChecked();
  await modal.getByRole("radio", { name: "None", exact: true }).check();
  await page.locator("[data-panel-back]").click();
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.agents[0].voice_id).toBeUndefined();
  await section(page, "Voices");
  voice = await openVoice(page, "Sysadmin");
  await voice
    .getByRole("button", { name: "Delete voice", exact: true })
    .click();
  await voice.getByRole("button", { name: "Confirm deletion" }).click();
  await expect(voice).toHaveCount(0);
  await expect(card(page, "voice", "Sysadmin")).toHaveCount(0);
});

test("an unavailable Voice selection stays visible until explicitly replaced", async ({
  page,
  store,
}) => {
  // The Store never saves a missing reference; simulate a library that no
  // longer holds the Agent's Voice in what the UI received.
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const result = await original(command, args);
      if (command === "snapshot" && result?.settings)
        result.settings.voices = result.settings.voices.filter(
          (voice) => voice.title !== "Merlin",
        );
      return result;
    };
  });
  await open(page, store, { voice: "Merlin" });
  const gone = `${(await store("snapshot")).settings.agents[0].voice_id} (unavailable)`;
  const modal = await openAgent(page);
  await expect(modal.locator("[data-voice-summary]")).toHaveText(gone);
  await modal
    .getByRole("button", { name: "Choose voice", exact: true })
    .click();
  const missing = modal.getByRole("radio", { name: gone, exact: true });
  await expect(missing).toBeChecked();
  await expect(missing).toBeDisabled();
  // Focus lands on the first enabled radio, not the disabled selection.
  const none = modal.getByRole("radio", { name: "None", exact: true });
  await expect(none).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(
    modal.getByRole("radio", { name: "Caveman", exact: true }),
  ).toBeFocused();
  await expect(modal.locator('form[data-view="voices"]')).toBeVisible();
  await expect(modal.locator("[data-voice-choice]")).toHaveText([
    "None",
    "Caveman",
    "Gilfoyle",
    gone,
  ]);
  await modal.getByRole("radio", { name: "Caveman", exact: true }).check();
  await expect(modal.locator("[data-voice-summary]")).toHaveText("Caveman");
  await expect(missing).toBeVisible();
});

test("the Voice picker pencil edits the shared Voice and keeps the drill-in", async ({
  page,
  store,
}) => {
  await open(page, store);
  const modal = await openAgent(page);
  await modal
    .getByRole("button", { name: "Choose voice", exact: true })
    .click();
  await modal.getByRole("radio", { name: "Merlin", exact: true }).check();
  await modal
    .getByRole("button", { name: "Edit voice Merlin", exact: true })
    .click();
  const voice = editor(page, "Editing Merlin");
  await voice.getByLabel("Title", { exact: true }).fill("Wizard");
  await voice.getByRole("button", { name: "Save voice", exact: true }).click();
  await expect(voice).toHaveCount(0);
  await expect(modal.locator('form[data-view="voices"]')).toBeVisible();
  await expect(
    modal.getByRole("radio", { name: "Wizard", exact: true }),
  ).toBeChecked();
  await expect(modal.locator("[data-voice-summary]")).toHaveText("Wizard");
  expect((await store("snapshot")).settings.voices[2].title).toBe("Wizard");
});

test("job details show the captured Voice; legacy jobs show it unavailable", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const review = fixture.review(9);
  const settings = (await store("snapshot")).settings;
  const merlin = "8f14e45f-ceea-4e67-a3f5-1d2c3b4a5f6e";
  review.selection.agent.voice_id = merlin;
  review.selection.configuration = {
    repository: settings.repositories.find(
      (r) => r.id === review.job.configuration_id,
    ),
    authority: {
      primary: true,
      comment: true,
      reply: false,
      approve: false,
      merge: false,
    },
    doctrines: [],
    voice: {
      id: merlin,
      title: "Merlin",
      body: "Captured wizard style.",
      guardrails: false,
    },
  };
  const show = async () => {
    await store("seed_queue_state", {
      jobs: [review.job],
      reviews: [review],
      publications: [],
      follow_ups: [],
    });
    await store("panel_navigate", {
      route: {
        tab: "running",
        detail: { type: "job", kind: "normal", id: review.key },
      },
    });
    await page.goto("/");
    const captured = page.locator(".work-configuration").first();
    await captured.locator("summary").first().click();
    return captured;
  };
  let captured = await show();
  await expect(captured).toContainText("Voice: Merlin");
  await expect(captured).toContainText("Captured wizard style.");
  await expect(captured).toContainText(/Fixed guardrails\s*No/);
  delete review.selection.configuration.voice;
  captured = await show();
  await expect(captured).toContainText(`${merlin} (unavailable)`);
  await expect(captured).not.toContainText("Captured wizard style.");
  // A teammate's packet never carried its Agent's Voice.
  review.selection.configuration.authority.primary = false;
  captured = await show();
  await expect(captured).toContainText(/Voice\s*None/);
  await expect(captured).not.toContainText(`${merlin} (unavailable)`);
  await expect(captured).not.toContainText("Fixed guardrails");
});

const renamedInPicker = [
  {
    kind: "voice",
    agent: { voice: "Merlin" },
    from: "Merlin",
    to: "Wizard",
    open: "Choose voice",
    stored: (r) => r.voice,
    save: "Save voice",
  },
  {
    kind: "doctrine",
    agent: { doctrines: ["code"] },
    from: "code",
    to: "code-renamed",
    open: "Choose doctrines",
    stored: (r) => r.doctrines,
    save: "Save doctrine",
  },
];
for (const c of renamedInPicker) {
  test(`renaming the selected ${c.kind} from the Agent picker keeps the Agent savable; real concurrent edits still conflict`, async ({
    page,
    store,
  }) => {
    await open(page, store, c.agent);
    let modal = await openAgent(page);
    await modal.getByRole("button", { name: c.open, exact: true }).click();
    await modal
      .getByRole("button", { name: `Edit ${c.kind} ${c.from}`, exact: true })
      .click();
    const library = editor(page, `Editing ${c.from}`);
    await library.getByLabel("Title", { exact: true }).fill(c.to);
    await library.getByRole("button", { name: c.save, exact: true }).click();
    await expect(library).toHaveCount(0);
    await page.locator("[data-panel-back]").click();
    await modal
      .getByRole("button", { name: "Save agent", exact: true })
      .click();
    await expect(modal).toHaveCount(0);
    const before = await refs(store);
    expect(c.stored(before)).toEqual(c.kind === "voice" ? c.to : [c.to]);

    // Rename back through the picker, then another window edits the Agent.
    modal = await openAgent(page);
    await modal.getByRole("button", { name: c.open, exact: true }).click();
    await modal
      .getByRole("button", { name: `Edit ${c.kind} ${c.to}`, exact: true })
      .click();
    const back = editor(page, `Editing ${c.to}`);
    await back.getByLabel("Title", { exact: true }).fill(c.from);
    await back.getByRole("button", { name: c.save, exact: true }).click();
    await expect(back).toHaveCount(0);
    await page.locator("[data-panel-back]").click();
    let agent = (await store("snapshot")).settings.agents[0];
    // The renames never rewrote the Agent.
    expect(agent).toEqual(before.agent);
    await store("save_resource", {
      edit: {
        kind: "agent",
        id: agent.id,
        expected: agent,
        value: { ...agent, prompt: "Changed in another window." },
      },
    });
    await modal
      .getByRole("button", { name: "Save agent", exact: true })
      .click();
    await expect(modal.getByRole("alert")).toContainText(
      "Resource changed in another window",
    );
    await expect(modal).toHaveCount(1);
    const after = await refs(store);
    expect(after.agent.prompt).toBe("Changed in another window.");
    expect(c.stored(after)).toEqual(c.kind === "voice" ? c.from : [c.from]);
  });
}

const renameInPicker = async (page, modal, kind, from, to) => {
  await modal
    .getByRole("button", { name: `Edit ${kind} ${from}`, exact: true })
    .click();
  const library = editor(page, `Editing ${from}`);
  await library.getByLabel("Title", { exact: true }).fill(to);
  await library
    .getByRole("button", { name: `Save ${kind}`, exact: true })
    .click();
  await expect(library).toHaveCount(0);
  await page.locator("[data-panel-back]").click();
};
const saveAgent = async (store, modal) => {
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal).toHaveCount(0);
  return refs(store);
};

test("a pencil rename of a non-last doctrine keeps the Agent's doctrine order", async ({
  page,
  store,
}) => {
  await open(page, store, { doctrines: ["code", "data", "testing"] });
  const modal = await openAgent(page);
  await modal
    .getByRole("button", { name: "Choose doctrines", exact: true })
    .click();
  await renameInPicker(page, modal, "doctrine", "data", "zz-data");
  const saved = await saveAgent(store, modal);
  expect(saved.doctrines).toEqual(["code", "zz-data", "testing"]);
});

test("case-only pencil renames keep the new casing for legacy doctrine and Voice", async ({
  page,
  store,
}) => {
  const settings = await open(page, store, { voice: "Merlin" });
  // Legacy single-doctrine Agent: no `doctrines` list.
  settings.agents[0] = {
    ...settings.agents[0],
    doctrine: "code",
  };
  delete settings.agents[0].doctrines;
  await store("seed_settings", settings);
  await page.reload();
  await tab(page, "Settings").click();
  const modal = await openAgent(page);
  await modal
    .getByRole("button", { name: "Choose doctrines", exact: true })
    .click();
  await renameInPicker(page, modal, "doctrine", "code", "Code");
  await modal
    .getByRole("button", { name: "Choose voice", exact: true })
    .click();
  await renameInPicker(page, modal, "voice", "Merlin", "merlin");
  const saved = await saveAgent(store, modal);
  expect(saved.doctrines).toEqual(["Code"]);
  expect(saved.agent.doctrine).toBeUndefined();
  expect(saved.agent.doctrines).toBeUndefined();
  expect(saved.voice).toBe("merlin");
});

test("an unmatched legacy doctrine title stays as a visible unavailable reference until removed", async ({
  page,
  store,
}) => {
  // Seeding migrates titles to ids; "Gone" has no entry and is kept as is.
  await open(page, store, { doctrines: ["code", "Gone"] });
  expect((await refs(store)).doctrines).toEqual(["code", "Gone"]);
  const modal = await openAgent(page);
  await expect(modal.locator("[data-doctrine-summary]")).toHaveText(
    "2 selected",
  );
  await modal
    .getByRole("button", { name: "Choose doctrines", exact: true })
    .click();
  const gone = modal.getByRole("checkbox", {
    name: "Gone (unavailable)",
    exact: true,
  });
  await expect(gone).toBeChecked();
  await expect(
    modal.getByRole("button", { name: "Edit doctrine Gone (unavailable)" }),
  ).toHaveCount(0);
  await page.locator("[data-panel-back]").click();
  await modal.locator("textarea[name=prompt]").fill("Kept reference.");
  let saved = await saveAgent(store, modal);
  expect(saved.doctrines).toEqual(["code", "Gone"]);
  const again = await openAgent(page);
  await again
    .getByRole("button", { name: "Choose doctrines", exact: true })
    .click();
  await again
    .getByRole("checkbox", { name: "Gone (unavailable)", exact: true })
    .uncheck();
  await page.locator("[data-panel-back]").click();
  saved = await saveAgent(store, again);
  expect(saved.doctrines).toEqual(["code"]);
});
