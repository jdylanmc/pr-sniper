import { test, expect } from "./fixtures.mjs";
import {
  fixtureAgent,
  section,
  chooseRepositoryAccount,
  setAllPullRequests,
  openFilters,
  openRule,
  backOne,
  pickAgent,
  withPermissions,
} from "./navigation.mjs";
import { holdFocusFrames } from "./focus-frames.mjs";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

const targetAgentId = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const assignmentIds = [
  "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
  "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
];
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
// The panel Back button sits outside the modal, so it keeps focus and the
// dialog does not restore it; close through the modal's own hidden control.
const closeEditor = (page) =>
  page
    .getByRole("dialog")
    .last()
    .locator(".dialog-head button")
    .first()
    .evaluate((button) => button.click());
const dialog = (page, name) => page.getByRole("dialog", { name, exact: true });
const repoOpener = (page, name = "fixture/target") =>
  page.getByRole("button", { name, exact: true });
const agentCard = (page, id = targetAgentId) =>
  page.locator(`[data-agent-id="${id}"]`);
const doctrineCard = (page, title) =>
  page.locator(".doctrine-card").filter({
    has: page.getByRole("heading", { name: title, exact: true }),
  });

async function seed(page, store, assignments = false) {
  await store("save_repository", { repository: "fixture/neighbor" });
  await store("save_repository", { repository: "fixture/target" });
  const settings = (await store("snapshot")).settings;
  settings.agents = [
    { ...fixtureAgent, ai_account: { provider: "copilot", account_id: "101" } },
    {
      ...fixtureAgent,
      id: targetAgentId,
      name: "Target reviewer",
      ai_account: { provider: "copilot", account_id: "101" },
      model: "fixture-model",
      doctrines: ["Target doctrine"],
    },
  ];
  settings.doctrines = [
    { title: "Neighbor doctrine", body: "Keep this neighbor." },
    { title: "Target doctrine", body: "Keep the target principles." },
  ];
  settings.repositories.forEach((repository, index) =>
    Object.assign(repository, {
      provider_account_id: "22",
      provider_repository_id: String(99 + index),
    }),
  );
  settings.repositories[1].enabled = assignments;
  if (assignments) {
    settings.repositories[1].assignments = settings.agents.map(
      (agent, index) => ({
        id: assignmentIds[index],
        agent_id: agent.id,
        schedule: settings.defaults.schedule,
        comment: true,
        approve: false,
        actions: { reply: false, approve: false, merge: false },
      }),
    );
    const rules = settings.repositories[1].admission.rules;
    settings.repositories[1].admission.mode = "filtered";
    Object.assign(
      rules.find((rule) => rule.kind === "include_users"),
      {
        enabled: true,
        users: [
          { id: "42", login: "neighbor-person" },
          { id: "43", login: "target-person" },
        ],
      },
    );
  }
  await store("seed_settings", settings);
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
      if (command === "github_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "github",
              account_id: "22",
              login: "fixture",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        });
      if (command === "resolve_provider_repository")
        return Promise.resolve({
          identity: { id: args.accountId },
          repository: {
            id: args.repository === "fixture/added" ? "200" : "100",
            name: args.repository,
          },
        });
      if (command === "list_copilot_models")
        return Promise.resolve([
          { id: "fixture-model", name: "Fixture model" },
        ]);
      return original(command, args);
    };
  });
  // Stored shape: library ids assigned, title references migrated to ids.
  return (await store("snapshot")).settings;
}

async function openSettings(page, mode) {
  if (mode === "legacy fallback")
    await page.addInitScript(() => {
      delete HTMLElement.prototype.inert;
      Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
        value: undefined,
      });
      Object.defineProperty(HTMLDialogElement.prototype, "close", {
        value: undefined,
      });
    });
  await page.goto(mode === "panel" ? "/" : "/?view=settings");
  if (mode === "panel") {
    await tab(page, "Settings").click();
  }
  await section(page, "Repositories");
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
}

async function activate(page, control, activation) {
  if (activation === "pointer") {
    // Panel cards are the pointer target; their Edit button is visually hidden.
    if (!(await control.isVisible()) && (await control.count())) {
      const card = control.locator(
        "xpath=ancestor::*[@data-agent-id or contains(@class,'doctrine-card')][1]",
      );
      if (await card.count()) return card.click({ position: { x: 20, y: 10 } });
    }
    // Standalone Settings has no icon sizing for trash buttons (zero-size).
    if (!(await control.isVisible()) && (await control.count()))
      return control.dispatchEvent("click");
    await control.click();
  } else if (activation === "keyboard") {
    await control.focus();
    await page.keyboard.press("Enter");
  } else {
    await page.locator(".settings-heading h1").focus();
    await control.evaluate((element) => element.click());
  }
}

async function settled(page) {
  await page.evaluate(async () => {
    await window.__settingsIdle();
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  });
}

async function afterRedraw(page, original, replacement) {
  await settled(page);
  expect(await original.evaluate((element) => element.isConnected)).toBe(false);
  // Panel sub-sections hide the Settings heading, which is the fallback focus
  // target after a removal; focus can then only rest on the document body.
  if (
    (await replacement.count()) &&
    !(await replacement.isVisible()) &&
    (await replacement.evaluate((element) =>
      element.matches(".settings-heading h1"),
    ))
  ) {
    await expect(page.locator("body")).toBeFocused();
    return;
  }
  if (await replacement.evaluate((element) => element.matches("[data-edit]")))
    await expect(replacement).toBeAttached();
  else await expect(replacement).toBeVisible();
  await expect(replacement).toBeFocused();
}

async function removeWatched(page, parent, login, activation, remaining = 1) {
  await openFilters(page, parent);
  await openRule(page, "include_users");
  const rows = page.locator(".watchlist-row");
  const remove = login
    ? page.getByRole("button", { name: `Remove @${login}`, exact: true })
    : rows.last().getByRole("button");
  const handle = await remove.elementHandle();
  await activate(page, remove, activation);
  await afterRedraw(
    page,
    handle,
    page.getByRole("button", { name: "Add people", exact: true }),
  );
  await expect(rows).toHaveCount(remaining);
  await backOne(page);
  await backOne(page);
}

async function draftPreferences(page) {
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("9");
}

for (const mode of ["panel", "legacy", "legacy fallback"]) {
  for (const activation of ["pointer", "keyboard", "nonfocusing"]) {
    test(`${mode} ${activation} repository completion focuses exact replacements after Cancel, Save, add and removal`, async ({
      page,
      store,
    }) => {
      const initial = await seed(page, store);
      const targetId = initial.repositories[1].id;
      await openSettings(page, mode);
      await draftPreferences(page);
      await section(page, "Integrations");
      const neighbor = repoOpener(page, "fixture/neighbor");
      await activate(page, neighbor, activation);
      const neighborEditor = dialog(page, "fixture/neighbor");
      await setAllPullRequests(neighborEditor, false);
      await closeEditor(page);
      await expect(neighbor).toBeFocused();

      for (const action of ["Cancel repository changes", "Save repository"]) {
        const opener = repoOpener(page);
        const original = await opener.elementHandle();
        await activate(page, opener, activation);
        const modal = dialog(page, "fixture/target");
        await setAllPullRequests(modal, false);
        await modal.getByRole("button", { name: action, exact: true }).click();
        await expect(modal).toHaveCount(0);
        await afterRedraw(page, original, opener);
        const saved = (await store("snapshot")).settings;
        expect(saved.repositories[1].id).toBe(targetId);
        expect(saved.repositories[1].admission).toEqual(
          action === "Save repository"
            ? { ...initial.repositories[1].admission, mode: "filtered" }
            : initial.repositories[1].admission,
        );
        expect(saved.repositories[0]).toEqual(initial.repositories[0]);
        expect(saved.capacity).toBe(initial.capacity);
        await expect(neighbor).toContainText("Draft");
      }

      let modal;

      const add = page.getByRole("button", {
        name: "Add repository by URL",
        exact: true,
      });
      const originalAdd = await add.elementHandle();
      await activate(page, add, activation);
      modal = dialog(page, "Add repository by URL");
      await modal
        .getByLabel("Repository URL", { exact: true })
        .fill("fixture/added");
      await chooseRepositoryAccount(modal, "22");
      await modal
        .getByRole("button", { name: "Add & configure", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      await closeEditor(page);
      await afterRedraw(page, originalAdd, repoOpener(page, "fixture/added"));
      expect((await store("snapshot")).settings.repositories[2]).toMatchObject({
        name: "fixture/added",
        enabled: false,
      });

      const removed = await repoOpener(page, "fixture/target").elementHandle();
      await activate(page, repoOpener(page, "fixture/target"), activation);
      modal = dialog(page, "fixture/target");
      await modal.locator("details.repository-group > summary").click();
      await modal
        .getByLabel("Type repository name to confirm")
        .fill("fixture/target");
      await modal
        .getByRole("button", { name: "Remove repository", exact: true })
        .click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await afterRedraw(page, removed, page.locator(".settings-heading h1"));
      const saved = (await store("snapshot")).settings;
      expect(
        saved.repositories.some((repository) => repository.id === targetId),
      ).toBe(false);
      expect(saved.repositories[0]).toEqual(initial.repositories[0]);
      expect(saved.capacity).toBe(initial.capacity);
      await expect(neighbor).toContainText("Draft");
      await section(page, "Preferences");
      await expect(page.locator("#global-capacity")).toHaveValue("9");
    });

    test(`${mode} ${activation} Agent and doctrine saves, renames and deletes retain focus after the final library redraw`, async ({
      page,
      store,
    }) => {
      const initial = await seed(page, store);
      await openSettings(page, mode);
      await draftPreferences(page);
      await section(page, "Integrations");
      await repoOpener(page).click();
      await setAllPullRequests(dialog(page, "fixture/target"), false);
      await closeEditor(page);
      await section(page, "Agents");

      const edit = agentCard(page).getByRole("button", {
        name: "Edit",
        exact: true,
      });
      const originalEdit = await edit.elementHandle();
      await activate(page, edit, activation);
      let modal = dialog(page, "Editing Target reviewer");
      await modal.getByLabel("Name", { exact: true }).fill("Renamed reviewer");
      await modal
        .getByRole("button", { name: "Save agent", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      await afterRedraw(page, originalEdit, edit);
      expect((await store("snapshot")).settings.agents[1]).toEqual({
        ...initial.agents[1],
        name: "Renamed reviewer",
      });

      const addAgent = page.getByRole("button", {
        name: "New agent",
        exact: true,
      });
      const originalAdd = await addAgent.elementHandle();
      await activate(page, addAgent, activation);
      modal = dialog(page, "New agent");
      await modal.getByLabel("Name", { exact: true }).fill("Added reviewer");
      await modal.getByLabel("AI account", { exact: true }).selectOption("101");
      await modal
        .getByLabel("Model", { exact: true })
        .selectOption("fixture-model");
      await modal
        .getByRole("button", { name: "Save agent", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      await afterRedraw(page, originalAdd, addAgent);

      await section(page, "Doctrines");
      const editDoctrine = doctrineCard(page, "Target doctrine").getByRole(
        "button",
        { name: "Edit", exact: true },
      );
      const originalDoctrine = await editDoctrine.elementHandle();
      await activate(page, editDoctrine, activation);
      modal = dialog(page, "Editing Target doctrine");
      await modal.getByLabel("Title", { exact: true }).fill("Renamed doctrine");
      await modal
        .getByRole("button", { name: "Save doctrine", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      await afterRedraw(
        page,
        originalDoctrine,
        doctrineCard(page, "Renamed doctrine").getByRole("button", {
          name: "Edit",
          exact: true,
        }),
      );
      // Rename changes the title only; the Agent keeps the same id reference.
      const renamed = (await store("snapshot")).settings;
      expect(renamed.agents[1].doctrine_ids).toEqual(
        initial.agents[1].doctrine_ids,
      );
      expect(
        renamed.doctrines.find(
          (d) => d.id === renamed.agents[1].doctrine_ids[0],
        ).title,
      ).toBe("Renamed doctrine");

      const addDoctrine = page.getByRole("button", {
        name: "New doctrine",
        exact: true,
      });
      const originalNewDoctrine = await addDoctrine.elementHandle();
      await activate(page, addDoctrine, activation);
      modal = dialog(page, "New doctrine");
      await modal.getByLabel("Title", { exact: true }).fill("Added doctrine");
      await modal
        .getByLabel("Principles", { exact: true })
        .fill("Added principles.");
      await modal
        .getByRole("button", { name: "Save doctrine", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      await afterRedraw(page, originalNewDoctrine, addDoctrine);

      await section(page, "Agents");
      const removeAgent = agentCard(page).getByRole("button", {
        name: "Delete",
        exact: true,
      });
      const originalRemoveAgent = await removeAgent.elementHandle();
      await activate(page, removeAgent, activation);
      await dialog(page, "Delete this agent?")
        .getByRole("button", { name: "Delete agent", exact: true })
        .click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await afterRedraw(
        page,
        originalRemoveAgent,
        page.locator(".settings-heading h1"),
      );
      await section(page, "Doctrines");
      const removeDoctrine = doctrineCard(page, "Renamed doctrine").getByRole(
        "button",
        { name: "Delete", exact: true },
      );
      const originalRemoveDoctrine = await removeDoctrine.elementHandle();
      await activate(page, removeDoctrine, activation);
      await dialog(page, "Delete this doctrine?")
        .getByRole("button", { name: "Delete doctrine", exact: true })
        .click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await afterRedraw(
        page,
        originalRemoveDoctrine,
        page.locator(".settings-heading h1"),
      );
      const saved = (await store("snapshot")).settings;
      expect(saved.agents.map((agent) => agent.name)).toEqual([
        fixtureAgent.name,
        "Added reviewer",
      ]);
      expect(saved.agents[0]).toEqual(initial.agents[0]);
      expect(saved.doctrines.map((doctrine) => doctrine.title)).toEqual([
        "Neighbor doctrine",
        "Added doctrine",
      ]);
      expect(saved.doctrines[0]).toEqual(initial.doctrines[0]);
      expect(saved.repositories).toEqual(initial.repositories);
      expect(saved.capacity).toBe(initial.capacity);
      await section(page, "Integrations");
      await repoOpener(page).click();
      await expect(
        dialog(page, "fixture/target").getByRole("switch", {
          name: "All pull requests",
        }),
      ).not.toBeChecked();
      await closeEditor(page);
      await section(page, "Preferences");
      await expect(page.locator("#global-capacity")).toHaveValue("9");
    });

    test(`${mode} ${activation} nested assignment completion restores its exact row and removals use explicit Add fallbacks`, async ({
      page,
      store,
    }) => {
      const initial = await seed(page, store, true);
      await openSettings(page, mode);
      await draftPreferences(page);
      await section(page, "Integrations");
      const outerOpener = repoOpener(page);
      const originalOuter = await outerOpener.elementHandle();
      await activate(page, outerOpener, activation);
      const parent = dialog(page, "fixture/target");
      const row = parent
        .locator(".assignment-row")
        .filter({ hasText: "Target reviewer" });
      const edit = row.getByRole("button", {
        name: "Edit assignment",
        exact: true,
      });
      const original = await edit.elementHandle();
      await activate(page, edit, activation);
      const modal = dialog(page, "Edit assignment");
      await withPermissions(page, modal, () =>
        modal.getByRole("switch", { name: "Publish Comment" }).uncheck(),
      );
      await modal
        .getByRole("button", { name: "Save assignment", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      await afterRedraw(page, original, edit);
      const savedAssignment = (await store("snapshot")).settings.repositories[1]
        .assignments;
      expect(savedAssignment).toEqual([
        initial.repositories[1].assignments[0],
        { ...initial.repositories[1].assignments[1], comment: false },
      ]);

      const remove = row.getByRole("button", {
        name: "Remove assignment",
        exact: true,
      });
      const removed = await remove.elementHandle();
      await activate(page, remove, activation);
      await afterRedraw(
        page,
        removed,
        parent.getByRole("button", { name: "Assign agent", exact: true }),
      );
      await removeWatched(page, parent, "target-person", activation);
      expect(
        (await store("snapshot")).settings.repositories[1].assignments,
      ).toEqual(savedAssignment);
      expect(
        (await store("snapshot")).settings.repositories[1].admission,
      ).toEqual(initial.repositories[1].admission);
      await parent
        .getByRole("button", { name: "Cancel repository changes", exact: true })
        .click();
      await expect(parent).toHaveCount(0);
      await afterRedraw(page, originalOuter, outerOpener);

      const nextOuter = await outerOpener.elementHandle();
      await activate(page, outerOpener, activation);
      await expect(row).toHaveCount(1);
      await openFilters(page, parent);
      await openRule(page, "include_users");
      await expect(
        page.getByRole("button", {
          name: "Remove @target-person",
          exact: true,
        }),
      ).toHaveCount(1);
      await backOne(page);
      await backOne(page);
      await activate(
        page,
        row.getByRole("button", { name: "Remove assignment", exact: true }),
        activation,
      );
      const assign = parent.getByRole("button", {
        name: "Assign agent",
        exact: true,
      });
      await activate(page, assign, activation);
      const create = dialog(page, "Assign agent");
      await pickAgent(create, targetAgentId);
      await withPermissions(page, create, async () => {
        await expect(
          create.getByRole("switch", { name: "Publish Comment" }),
        ).not.toBeChecked();
        await create.getByRole("switch", { name: "Publish Comment" }).check();
      });
      await create
        .locator("form")
        .getByRole("button", { name: "Assign agent", exact: true })
        .click();
      await expect(create).toHaveCount(0);
      await settled(page);
      await expect(assign).toBeFocused();
      const saved = (await store("snapshot")).settings;
      expect(saved.repositories[1].assignments).toHaveLength(2);
      expect(saved.repositories[1].assignments[1].id).not.toBe(
        assignmentIds[1],
      );
      expect(saved.repositories[1].assignments[1]).toMatchObject({
        agent_id: targetAgentId,
        comment: true,
        actions: { reply: false, approve: false, merge: false },
      });
      expect(saved.repositories[0]).toEqual(initial.repositories[0]);
      expect(saved.capacity).toBe(initial.capacity);
      for (let remaining = 2; remaining > 0; remaining--) {
        await activate(
          page,
          parent
            .locator(".assignment-row")
            .last()
            .getByRole("button", { name: "Remove assignment", exact: true }),
          activation,
        );
        await settled(page);
        await expect(assign).toBeFocused();
        await expect(parent.locator(".assignment-row")).toHaveCount(
          remaining - 1,
        );
        await removeWatched(page, parent, undefined, activation, remaining - 1);
      }
      await parent
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(parent.locator("[data-resource-error]")).toContainText(
        "assign at least one saved Agent",
      );
      expect(
        (await store("snapshot")).settings.repositories[1].assignments,
      ).toEqual(saved.repositories[1].assignments);
      await parent.getByRole("switch", { name: /^Monitor / }).click();
      await expect(
        parent.locator("[data-repository-monitoring-state]"),
      ).toHaveText("Disabled");
      await parent
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(parent).toHaveCount(0);
      await afterRedraw(page, nextOuter, outerOpener);
      const cleared = (await store("snapshot")).settings.repositories[1];
      expect(cleared.assignments ?? []).toEqual([]);
      expect(
        cleared.admission.rules.find((rule) => rule.kind === "include_users")
          .users ?? [],
      ).toEqual([]);
      await section(page, "Preferences");
      await expect(page.locator("#global-capacity")).toHaveValue("9");
    });
  }
}

for (const focusTarget of ["Running", "Settings"]) {
  for (const resource of ["repository", "agent", "doctrine", "assignment"]) {
    for (const timing of focusTarget === "Running"
      ? ["normal", "frame before save", "frame after save"]
      : ["normal"]) {
      test(`late ${resource} save completes without stealing focus from the ${focusTarget} navigation control${timing === "normal" ? "" : ` (${timing})`}`, async ({
        page,
        store,
        ipc,
      }, testInfo) => {
        await seed(page, store, true);
        await openSettings(page, "panel");
        let modal;
        if (resource === "agent") {
          await section(page, "Agents");
          await activate(
            page,
            agentCard(page).getByRole("button", { name: "Edit", exact: true }),
            "pointer",
          );
          modal = dialog(page, "Editing Target reviewer");
          await modal
            .getByLabel("Name", { exact: true })
            .fill("Background reviewer");
        } else if (resource === "doctrine") {
          await section(page, "Doctrines");
          await activate(
            page,
            doctrineCard(page, "Target doctrine").getByRole("button", {
              name: "Edit",
              exact: true,
            }),
            "pointer",
          );
          modal = dialog(page, "Editing Target doctrine");
          await modal
            .getByLabel("Title", { exact: true })
            .fill("Background doctrine");
        } else {
          await repoOpener(page).click();
          modal = dialog(page, "fixture/target");
          if (resource === "assignment") {
            await modal
              .locator(".assignment-row")
              .nth(1)
              .getByRole("button", { name: "Edit assignment", exact: true })
              .click();
            modal = dialog(page, "Edit assignment");
            await withPermissions(page, modal, () =>
              modal.getByRole("switch", { name: "Publish Comment" }).uncheck(),
            );
          } else {
            await setAllPullRequests(modal, true);
          }
        }
        const hold = ipc.holdNext("save_resource");
        await modal
          .getByRole("button", { name: `Save ${resource}`, exact: true })
          .click();
        await hold.arrived;
        const frames = timing === "normal" ? null : await holdFocusFrames(page);
        if (focusTarget === "Running") await tab(page, "Running").click();
        const editorHeading = {
          repository: "fixture/target",
          agent: "Editing Target reviewer",
          doctrine: "Editing Target doctrine",
          assignment: "Edit assignment",
        }[resource];
        const completedEditor = page.getByRole("dialog", {
          name: editorHeading,
          exact: true,
          includeHidden: true,
        });
        const heading =
          focusTarget === "Running" ? "Work queue" : editorHeading;
        await expect(page.locator("[data-panel-heading]")).toHaveText(heading);
        if (frames) await frames.waitForCount(1);
        await tab(page, focusTarget).focus();
        await frames?.mark("newer-navigation-focus");
        if (timing === "frame before save") await frames.release();
        hold.release();
        if (frames) {
          await page.evaluate(() => window.__settingsIdle());
          await expect(modal).toHaveCount(0);
          await frames.mark("save-completed");
          if (timing === "frame after save") await frames.release();
          await frames.resume();
          await frames.attach(testInfo);
        }
        await settled(page);
        await expect(completedEditor).toHaveCount(0);
        await expect(tab(page, focusTarget)).toBeFocused();
        await expect(page.locator("[data-panel-heading]")).toHaveText(
          focusTarget === "Running"
            ? heading
            : {
                repository: "Repositories",
                agent: "Agents",
                doctrine: "Doctrines",
                assignment: "fixture/target",
              }[resource],
        );
        const saved = (await store("snapshot")).settings;
        if (resource === "agent")
          expect(saved.agents[1].name).toBe("Background reviewer");
        else if (resource === "doctrine")
          expect(saved.doctrines[1].title).toBe("Background doctrine");
        else if (resource === "assignment")
          expect(saved.repositories[1].assignments[1].comment).toBe(false);
        else expect(saved.repositories[1].admission.mode).toBe("all");
        await tab(page, "Settings").click();
        await expect(modal).toHaveCount(0);
        if (resource === "assignment")
          await expect(dialog(page, "fixture/target")).toBeVisible();
      });
    }
  }
}

for (const mode of ["panel", "legacy"]) {
  for (const outcome of ["verified", "reconnect", "focus moved"]) {
    test(`${mode} new Agent completion waits for current account verification with ${outcome} and never steals later focus`, async ({
      page,
      store,
    }) => {
      await seed(page, store);
      await page.addInitScript(() => {
        const original = window.__TAURI_INTERNALS__.invoke;
        window.__TAURI_INTERNALS__.invoke = (command, args) => {
          if (command === "copilot_auth_state" && window.__holdCopilotRefresh)
            return new Promise((resolve) => {
              window.__releaseCopilotRefresh = (connected) =>
                resolve({
                  accounts: [
                    {
                      provider: "copilot",
                      account_id: "101",
                      login: "fixture-ai",
                      state: connected ? "connected" : "reconnect_required",
                    },
                  ],
                  flow: { state: "idle" },
                });
            });
          return original(command, args);
        };
      });
      await openSettings(page, mode);
      await section(page, "Agents");
      const opener = page.getByRole("button", {
        name: "New agent",
        exact: true,
      });
      const original = await opener.elementHandle();
      await activate(page, opener, "nonfocusing");
      const modal = dialog(page, "New agent");
      await modal
        .getByLabel("Name", { exact: true })
        .fill("Verified resource save");
      await modal.getByLabel("AI account", { exact: true }).selectOption("101");
      await modal
        .getByLabel("Model", { exact: true })
        .selectOption("fixture-model");
      await page.evaluate(() => {
        window.__holdCopilotRefresh = true;
      });
      await modal
        .getByRole("button", { name: "Save agent", exact: true })
        .click();
      await expect(modal).toHaveCount(0);
      await afterRedraw(page, original, page.locator(".settings-heading h1"));
      await expect(opener).toBeDisabled();
      const elsewhere =
        mode === "panel"
          ? tab(page, "Settings")
          : page
              .getByRole("navigation", { name: "Settings sections" })
              .getByRole("button", { name: "Agents", exact: true });
      if (outcome === "focus moved") await elsewhere.focus();
      await page.evaluate(
        (connected) => window.__releaseCopilotRefresh(connected),
        outcome !== "reconnect",
      );
      await settled(page);
      const expected =
        outcome === "verified"
          ? opener
          : outcome === "focus moved"
            ? elsewhere
            : // The panel hides the Settings heading inside sub-sections.
              page.locator(mode === "panel" ? "body" : ".settings-heading h1");
      await expect(expected).toBeFocused();
      if (outcome === "reconnect") await expect(opener).toBeDisabled();
      else await expect(opener).toBeEnabled();
      expect((await store("snapshot")).settings.agents[2].name).toBe(
        "Verified resource save",
      );
    });
  }

  test(`${mode} delayed account redraw restores the exact repository binding, not a same-name neighbor`, async ({
    page,
    store,
  }) => {
    const settings = await seed(page, store);
    settings.repositories[0].name = "fixture/target";
    settings.repositories.forEach((repository, index) => {
      repository.provider_account_id = String(index + 101);
      repository.provider_repository_id = "900";
    });
    await store("seed_settings", settings);
    await page.addInitScript(() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command === "github_auth_state" && window.__holdAccountRefresh)
          return new Promise((resolve) => {
            window.__releaseAccountRefresh = () =>
              resolve({ accounts: [], flow: { state: "idle" } });
          });
        return original(command, args);
      };
    });
    await openSettings(page, mode);
    const opener = page.locator(
      `[data-repository="${settings.repositories[1].id}"]`,
    );
    await opener.click();
    await page.evaluate(() => {
      window.__holdAccountRefresh = true;
    });
    await dialog(page, "fixture/target")
      .getByRole("button", { name: "Cancel repository changes", exact: true })
      .click();
    await settled(page);
    await expect(opener).toBeFocused();
    const original = await opener.elementHandle();
    // Resource redraws retain the account widgets; exercise their real refresh.
    await page.evaluate(() =>
      window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts")),
    );
    await expect
      .poll(() => page.evaluate(() => typeof window.__releaseAccountRefresh))
      .toBe("function");
    await page.evaluate(() => window.__releaseAccountRefresh());
    await afterRedraw(page, original, opener);
    expect((await store("snapshot")).settings).toEqual(settings);
  });
}
