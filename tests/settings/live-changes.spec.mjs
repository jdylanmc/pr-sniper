import { test, expect } from "./fixtures.mjs";
import { editAgent, fixtureAgent, section, seedAgent } from "./navigation.mjs";

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

async function ready(page, store) {
  await seedAgent(store);
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
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
