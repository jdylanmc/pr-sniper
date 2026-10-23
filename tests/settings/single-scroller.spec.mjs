import { test, expect } from "./fixtures.mjs";
import {
  fixtureAgent,
  repositorySettings,
  section,
  seedBoundRepositories,
} from "./navigation.mjs";

// A long library stays laid out (visibility: hidden) under an open editor's
// transparent host. It must never leave a second scrollbar beside the editor's.
const repositories = [
  "fixture/detail",
  ...Array.from({ length: 14 }, (_, i) => `fixture/library-${i}`),
];

async function seed(page, store) {
  const settings = await seedBoundRepositories(store, repositories);
  settings.agents = [fixtureAgent];
  settings.repositories.forEach((repository) => {
    repository.enabled = false;
  });
  settings.repositories[0].assignments = Array.from({ length: 4 }, (_, i) => ({
    id: `dddddddd-dddd-4ddd-8ddd-${String(i).padStart(12, "0")}`,
    agent_id: fixtureAgent.id,
    schedule: settings.defaults.schedule,
    comment: false,
    approve: false,
    actions: { reply: false, approve: false, merge: false },
  }));
  await store("seed_settings", settings);
  await page.goto("/");
}

// Visible scroll containers inside the panel that actually overflow.
const scrollers = (page) =>
  page.evaluate(() => {
    const shell = document.querySelector(".panel-shell");
    const header = shell.querySelector(".panel-header").getBoundingClientRect();
    const nav = shell.querySelector(".panel-tabs").getBoundingClientRect();
    return [shell, ...shell.querySelectorAll("*")]
      .filter((element) => {
        const style = getComputedStyle(element);
        return (
          /^(auto|scroll)$/.test(style.overflowY) &&
          style.visibility === "visible" &&
          element.getClientRects().length > 0 &&
          element.scrollHeight > element.clientHeight
        );
      })
      .map((element) => {
        const box = element.getBoundingClientRect();
        return {
          name: `${element.tagName.toLowerCase()}${element.id ? `#${element.id}` : ""}.${[...element.classList].join(".")}`,
          top: box.top,
          bottom: box.bottom,
          headerBottom: header.bottom,
          navTop: nav.top,
        };
      });
  });

const destination = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true })
    .click();

for (const viewport of [
  { width: 408, height: 439 },
  { width: 408, height: 744 },
]) {
  test(`repository detail has one scroll container clear of the header and navigation at ${viewport.width}x${viewport.height}`, async ({
    page,
    store,
  }) => {
    await page.setViewportSize(viewport);
    await seed(page, store);
    for (const tab of ["Queue", "Running", "Reviewed", "Settings"]) {
      await destination(page, tab);
      expect((await scrollers(page)).length, tab).toBeLessThanOrEqual(1);
    }
    for (const name of ["Preferences", "Accounts", "Agents", "Repositories"]) {
      await section(page, name);
      expect((await scrollers(page)).length, name).toBeLessThanOrEqual(1);
    }
    const editor = await repositorySettings(page, "fixture/detail");
    await expect(
      editor.getByRole("switch", { name: "Review drafts", exact: true }),
    ).toBeVisible();
    await expect
      .poll(async () => (await scrollers(page)).map(({ name }) => name))
      .toEqual(["div.dialog-body"]);
    const [body] = await scrollers(page);
    expect(body.top).toBeGreaterThanOrEqual(body.headerBottom);
    expect(body.bottom).toBeLessThanOrEqual(body.navTop);

    // A nested editor keeps the same single scroller.
    await editor
      .locator(".assignment-row")
      .first()
      .getByRole("button", { name: "Edit assignment", exact: true })
      .click();
    await expect(
      page.getByRole("dialog", { name: "Edit assignment", exact: true }),
    ).toBeVisible();
    const nested = await scrollers(page);
    expect(nested.length).toBeLessThanOrEqual(1);
    for (const scroller of nested) {
      expect(scroller.top).toBeGreaterThanOrEqual(scroller.headerBottom);
      expect(scroller.bottom).toBeLessThanOrEqual(scroller.navTop);
    }
  });
}
