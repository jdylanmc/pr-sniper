import { test, expect } from "./fixtures.mjs";
import {
  closeDialog,
  editAgent,
  fixtureAgent,
  section,
  seedAgent,
} from "./navigation.mjs";

test.use({ viewport: { width: 408, height: 744 } });

// The native Store broadcasts this after every committed settings write,
// whichever origin (local control, another window) made it.
const committed = (page, revision) =>
  page.evaluate(async (revision) => {
    window.__appEvent("pr-sniper:settings-changed", { revision });
    // Bursts coalesce before the view re-reads the Store.
    await new Promise((resolve) => setTimeout(resolve, 100));
    await window.__settingsIdle();
  }, revision);

async function saveElsewhere(store, change) {
  const { settings } = await store("snapshot");
  const agent = settings.agents[0];
  await store("save_resource", {
    edit: {
      kind: "agent",
      id: agent.id,
      expected: agent,
      value: { ...agent, ...change },
    },
  });
}

async function ready(page, store, panel = false) {
  await seedAgent(store);
  await store("panel_snapshot");
  await page.goto(panel ? "/" : "/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  if (panel) {
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    await page.evaluate(() => window.__settingsIdle());
  }
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await section(page, "Agents");
  await page.evaluate(() => window.__settingsIdle());
}

const card = (page) =>
  page.locator(".agent-card").filter({
    has: page.getByRole("heading", { name: fixtureAgent.name, exact: true }),
  });

test("an out-of-band commit refreshes the visible Settings view", async ({
  page,
  store,
}) => {
  await ready(page, store);
  await expect(card(page).locator(".resource-meta")).toContainText(
    fixtureAgent.model,
  );
  await saveElsewhere(store, {
    model: "external-model",
    signature: "External signature",
  });
  // Nothing re-reads without the commit notification.
  await expect(card(page).locator(".resource-meta")).toContainText(
    fixtureAgent.model,
  );
  await committed(page, 2);
  await expect(card(page).locator(".resource-meta")).toContainText(
    "external-model",
  );
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await expect(page.locator("#reload-settings")).toBeHidden();
  await expect(page.locator("#error")).toBeHidden();
  const modal = await editAgent(page);
  await expect(modal.locator("[name=signature]")).toHaveValue(
    "External signature",
  );
});

test("an out-of-band commit keeps an open Agent draft and marks it stale", async ({
  page,
  store,
}) => {
  await ready(page, store);
  const modal = await editAgent(page);
  await modal.locator("textarea[name=prompt]").fill("Local unfinished prompt");
  await saveElsewhere(store, {
    model: "external-model",
    signature: "External signature",
  });
  await committed(page, 2);
  await expect(page.locator("#error")).toHaveText("Changed elsewhere");
  await expect(modal).toBeVisible();
  await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
    "Local unfinished prompt",
  );
  await modal.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(modal.getByRole("alert")).toContainText("Resource changed");
  await expect(modal.locator("textarea[name=prompt]")).toHaveValue(
    "Local unfinished prompt",
  );
  const { settings } = await store("snapshot");
  expect(settings.agents[0].model).toBe("external-model");
  expect(settings.agents[0].prompt).toBe(fixtureAgent.prompt);
});

test("an out-of-band commit keeps a dirty Preferences draft until reload", async ({
  page,
  store,
}) => {
  await ready(page, store);
  await section(page, "Preferences");
  const minutes = page.getByLabel("Review inactivity (min)", { exact: true });
  await minutes.fill("9");
  const { settings } = await store("snapshot");
  settings.review_inactivity_minutes = 11;
  await store("seed_settings", settings);
  await committed(page, 2);
  await expect(page.locator("#error")).toHaveText("Changed elsewhere");
  await expect(minutes).toHaveValue("9");
  await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
  await page.locator("#reload-settings").click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(minutes).toHaveValue("11");
  await expect(page.locator("#reload-settings")).toBeHidden();
});

test("a commit landing while Settings opens is not missed", async ({
  page,
  store,
  ipc,
}) => {
  await seedAgent(store);
  // Hold the first read's reply; the commit lands after that read.
  const initial = ipc.holdNext("snapshot");
  await page.goto("/?view=settings");
  await initial.arrived;
  await saveElsewhere(store, { model: "external-model" });
  await page.evaluate(() =>
    window.__appEvent("pr-sniper:settings-changed", { revision: 2 }),
  );
  initial.release();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Agents");
  await expect(card(page).locator(".resource-meta")).toContainText(
    "external-model",
  );
});

test("a commit is applied from the read that observed it", async ({
  page,
  store,
}) => {
  // Types into Preferences while any further snapshot read is outstanding,
  // so a second read after the commit cannot silently drop the result.
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__reads = undefined;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "snapshot" && window.__reads !== undefined) {
        if (++window.__reads === 2) {
          const input = document.querySelector("#review-inactivity");
          input.value = "9";
          input.dispatchEvent(new Event("input", { bubbles: true }));
          input.dispatchEvent(new Event("change", { bubbles: true }));
        }
      }
      return original(command, args);
    };
  });
  await ready(page, store);
  await section(page, "Preferences");
  const minutes = page.getByLabel("Review inactivity (min)", { exact: true });
  const { settings } = await store("snapshot");
  settings.review_inactivity_minutes = 11;
  await store("seed_settings", settings);
  await page.evaluate(() => (window.__reads = 0));
  await committed(page, 2);
  // The read that observed the commit is applied; a second read, during
  // which the user starts a draft, would drop it without a stale marker.
  await expect(minutes).toHaveValue("11");
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  expect(await page.evaluate(() => window.__reads)).toBe(1);
});

test("an embedded Agent editor closed on a stale list shows the commit", async ({
  page,
  store,
}) => {
  await ready(page, store, true);
  await section(page, "Agents");
  await card(page)
    .locator("[data-edit]")
    .evaluate((button) => button.click());
  const modal = page.getByRole("dialog", {
    name: `Editing ${fixtureAgent.name}`,
    exact: true,
  });
  await saveElsewhere(store, { model: "external-model" });
  await committed(page, 2);
  await expect(modal).toBeVisible();
  await closeDialog(page);
  await page.evaluate(() => window.__settingsIdle());
  await expect(modal).toHaveCount(0);
  await expect(card(page).locator(".resource-meta")).toContainText(
    "external-model",
  );
});

test("a commit keeps an unsaved Azure DevOps Organization draft", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    const view = {
      cli: { state: "ready", version: "2.90.0" },
      minimum: "2.54.0",
      installer: "homebrew",
      organization: { state: "unset" },
      account: { state: "signed_out" },
      checking: false,
    };
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command.startsWith("azure_devops_")
        ? window.__settingsTrack(Promise.resolve(structuredClone(view)))
        : original(command, args);
  });
  await ready(page, store, true);
  await section(page, "Git Repository");
  await page
    .locator(".settings-overview")
    .getByRole("button", { name: "Azure DevOps", exact: true })
    .click();
  const organization = page.getByLabel("Organization", { exact: true });
  await organization.fill("typed-organization");
  await saveElsewhere(store, { model: "external-model" });
  await committed(page, 2);
  await expect(organization).toHaveValue("typed-organization");
  await expect(organization).toBeFocused();
  await section(page, "Agents");
  await expect(card(page).locator(".resource-meta")).toContainText(
    "external-model",
  );
});
