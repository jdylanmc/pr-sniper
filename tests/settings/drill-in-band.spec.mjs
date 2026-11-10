import { test, expect } from "./fixtures.mjs";
import { fixtureAgent, section, seedBoundRepositories } from "./navigation.mjs";

// Drill-ins share the reference screens' band geometry: cards overlap the navy
// band at rest and scrolled content slides under it. A list kept scrolled under
// an open drill-in must not lend its band clip to the drill-in.
const maintainer = {
  ...fixtureAgent,
  id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
  name: "The Maintainer",
};
const repositories = [
  ...Array.from({ length: 14 }, (_, i) => `fixture/library-${i}`),
  "fixture/detail",
];

async function seed(page, store) {
  const settings = await seedBoundRepositories(store, repositories);
  settings.agents = [maintainer, fixtureAgent];
  const repository = settings.repositories.find(
    (r) => r.name === "fixture/detail",
  );
  repository.paths = Array.from({ length: 16 }, (_, i) => `src/module-${i}/**`);
  repository.assignments = [maintainer, fixtureAgent, fixtureAgent].map(
    (agent, i) => ({
      id: `dddddddd-dddd-4ddd-8ddd-00000000000${i}`,
      agent_id: agent.id,
      schedule: settings.defaults.schedule,
      comment: true,
      approve: !i,
      actions: { reply: !i, approve: !i, merge: !i },
    }),
  );
  repository.primary_assignment_id = repository.assignments[0].id;
  await store("seed_settings", settings);
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await expect(page.locator(".settings-overview")).toBeVisible();
  const home = await geometry(page.locator(".settings-window #content"));
  await section(page, "Repositories");
  await page.evaluate(() => window.__settingsIdle());
  return home.band - home.cardTop;
}

// The topmost wide rounded, painted card in a scroller, plus the clip every ancestor
// (and the scroller itself) applies to it.
const geometry = (scroller) =>
  scroller.evaluate((element) => {
    const band = document
      .querySelector(".panel-shell .panel-context")
      .getBoundingClientRect().bottom;
    const nav = document
      .querySelector(".panel-shell .panel-tabs")
      .getBoundingClientRect().top;
    const width = element.getBoundingClientRect().width;
    const [card] = [...element.querySelectorAll("*")]
      .filter((node) => {
        const style = getComputedStyle(node);
        return (
          style.visibility === "visible" &&
          node.getClientRects().length > 0 &&
          node.getBoundingClientRect().width >= width * 0.4 &&
          style.backgroundColor !== "rgba(0, 0, 0, 0)" &&
          parseFloat(style.borderTopLeftRadius) > 0
        );
      })
      .sort(
        (a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top,
      );
    const clips = [];
    for (let node = element; node; node = node.parentElement) {
      const clip = getComputedStyle(node).clipPath;
      if (clip !== "none") clips.push(clip);
    }
    const box = element.getBoundingClientRect();
    return {
      band,
      nav,
      top: box.top,
      bottom: box.bottom,
      cardTop: card.getBoundingClientRect().top,
      clips,
      lastBottom: Math.max(
        ...[...element.children].map((c) => c.getBoundingClientRect().bottom),
      ),
    };
  });

// What paints at a point just inside the band overlap: clip-path also clips
// hit testing, so a sliced card yields the band instead of the card.
const paintsCard = (page, scroller, y) =>
  scroller.evaluate((element, y) => {
    const box = element.getBoundingClientRect();
    const hit = document.elementFromPoint(box.left + box.width / 2, y);
    return element.contains(hit);
  }, y);

for (const browserViewport of [
  { width: 408, height: 744 },
  { width: 408, height: 520 },
]) {
  test(`drill-ins keep the reference band overlap at ${browserViewport.width}x${browserViewport.height}`, async ({
    page,
    store,
  }) => {
    await page.setViewportSize(browserViewport);
    // Reference: Settings home's card overlap at rest, the Repositories list's
    // clip when scrolled.
    const overlap = await seed(page, store);
    expect(overlap).toBeGreaterThanOrEqual(20);
    const list = page.locator(".settings-window #content");
    const reference = await geometry(list);
    expect(reference.clips).toEqual([]);
    await list.evaluate((element) => (element.scrollTop = 60));
    await expect
      .poll(async () => (await geometry(list)).clips)
      .toEqual([`inset(${reference.band - reference.top}px 0px 0px)`]);
    const referenceClip = (await geometry(list)).clips;

    // Open a repository deep in the scrolled list.
    const row = page.locator("[data-repository]").last();
    await row.scrollIntoViewIfNeeded();
    expect(await list.evaluate((e) => e.scrollTop)).toBeGreaterThan(23);
    const name = await row.getAttribute("aria-label");
    await row.click();
    const editor = page.getByRole("dialog", { name, exact: true });
    const body = editor.locator(".dialog-body");
    await expect(
      editor.getByRole("switch", { name: "Review drafts", exact: true }),
    ).toBeVisible();

    // At rest: same overlap as the reference, nothing slices the first card.
    const rest = await geometry(body);
    expect(Math.abs(rest.band - rest.cardTop - overlap)).toBeLessThanOrEqual(1);
    expect(rest.top).toBeLessThan(rest.band);
    expect(rest.clips).toEqual([]);
    expect(await paintsCard(page, body, rest.cardTop + 4)).toBe(true);

    // Scrolled: the drill-in clips exactly like the reference list.
    await body.evaluate((element) => (element.scrollTop = 60));
    await expect
      .poll(async () => (await geometry(body)).clips)
      .toEqual(referenceClip);

    // Long assignment summaries wrap inside their row.
    const summary = editor
      .locator(".assignment-row")
      .filter({ hasText: "The Maintainer" })
      .locator("p")
      .first();
    await expect(summary).toContainText("Approve & Merge");
    const fit = await summary.evaluate((p) => {
      const row = p.closest(".assignment-row").getBoundingClientRect();
      const box = p.getBoundingClientRect();
      return {
        overflow: p.scrollWidth - p.clientWidth,
        right: row.right - box.right,
        bottom: row.bottom - box.bottom,
      };
    });
    expect(fit.overflow).toBeLessThanOrEqual(0);
    expect(fit.right).toBeGreaterThanOrEqual(0);
    expect(fit.bottom).toBeGreaterThanOrEqual(0);

    // Bottom: the last card scrolls fully clear of the tab bar.
    await body.evaluate((element) => (element.scrollTop = 1e6));
    const end = await geometry(body);
    expect(end.lastBottom).toBeLessThanOrEqual(end.bottom + 0.5);
    expect(end.bottom).toBeLessThanOrEqual(end.nav);

    // Paths drill-in: at rest and scrolled, same treatment.
    await body.evaluate((element) => (element.scrollTop = 0));
    await editor.getByRole("button", { name: "Paths", exact: true }).click();
    // Paths is a view inside the same editor dialog.
    await expect(editor.locator("[data-repository-paths]")).toBeVisible();
    await expect(editor.getByText("src/module-0/**")).toBeVisible();
    const pathsBody = body;
    const pathsRest = await geometry(pathsBody);
    expect(
      Math.abs(pathsRest.band - pathsRest.cardTop - overlap),
    ).toBeLessThanOrEqual(1);
    expect(pathsRest.clips).toEqual([]);
    expect(await paintsCard(page, pathsBody, pathsRest.cardTop + 4)).toBe(true);
    await pathsBody.evaluate((element) => (element.scrollTop = 60));
    await expect
      .poll(async () => (await geometry(pathsBody)).clips)
      .toEqual(referenceClip);
    await pathsBody.evaluate((element) => (element.scrollTop = 1e6));
    const pathsEnd = await geometry(pathsBody);
    expect(pathsEnd.lastBottom).toBeLessThanOrEqual(pathsEnd.bottom + 0.5);
    expect(pathsEnd.bottom).toBeLessThanOrEqual(pathsEnd.nav);

    // Back restores each level's own clip.
    const back = page.locator("[data-panel-back]");
    await back.click();
    await expect(editor.locator("[data-repository-paths]")).toHaveCount(0);
    await body.evaluate((element) => (element.scrollTop = 0));
    await expect.poll(async () => (await geometry(body)).clips).toEqual([]);

    // The assignment editor's cards overlap the band too.
    await editor
      .locator(".assignment-row")
      .first()
      .getByRole("button", { name: "Edit assignment", exact: true })
      .click();
    const assignment = page.getByRole("dialog", {
      name: "Edit assignment",
      exact: true,
    });
    await expect(assignment).toBeVisible();
    const assigned = await geometry(assignment.locator(".dialog-body"));
    expect(
      Math.abs(assigned.band - assigned.cardTop - overlap),
    ).toBeLessThanOrEqual(1);
    expect(assigned.clips).toEqual([]);
    await back.click();
    await expect(assignment).toHaveCount(0);
    await back.click();
    await expect(editor).toHaveCount(0);
    await expect
      .poll(async () => (await geometry(list)).clips)
      .toEqual(referenceClip);
  });
}

// A page banner shown over a drill-in renders whole: never under the drill-in,
// never sliced by its band clip, never over its first card.
for (const browserViewport of [
  { width: 408, height: 744 },
  { width: 408, height: 520 },
]) {
  test(`a drill-in banner renders whole above the first card at ${browserViewport.width}x${browserViewport.height}`, async ({
    page,
    store,
  }) => {
    await page.setViewportSize(browserViewport);
    await seed(page, store);
    const row = page.locator("[data-repository]").last();
    await row.scrollIntoViewIfNeeded();
    const name = await row.getAttribute("aria-label");
    await row.click();
    const editor = page.getByRole("dialog", { name, exact: true });
    const body = editor.locator(".dialog-body");
    const drafts = editor.getByRole("switch", { name: "Review drafts" });
    await drafts.click();
    const { settings } = await store("snapshot");
    settings.review_inactivity_minutes = 11;
    await store("seed_settings", settings);
    await page.evaluate(async () => {
      window.__appEvent("pr-sniper:settings-changed", { revision: 2 });
      await new Promise((resolve) => setTimeout(resolve, 100));
      await window.__settingsIdle();
    });
    const banner = page.locator("#error");
    await expect(banner).toHaveText("Changed elsewhere");

    const whole = async () => {
      const card = await geometry(body);
      return banner.evaluate((element, cardTop) => {
        const box = element.getBoundingClientRect();
        const x = box.left + box.width / 2;
        const painted = [box.top + 2, box.top + box.height / 2, box.bottom - 2]
          .map((y) => document.elementFromPoint(x, y))
          .every((hit) => element.contains(hit));
        return { painted, gap: cardTop - box.bottom, height: box.height };
      }, card.cardTop);
    };
    for (const scrollTop of [0, 60, 1e6]) {
      await body.evaluate(
        (element, top) => (element.scrollTop = top),
        scrollTop,
      );
      await expect.poll(whole).toMatchObject({ painted: true });
      const { gap, height } = await whole();
      expect(height).toBeGreaterThan(20);
      if (scrollTop === 0) expect(gap).toBeGreaterThanOrEqual(8);
    }
    await expect(banner).not.toHaveAttribute("aria-hidden", "true");
    // Dismissed, the drill-in's cards overlap the band again.
    await body.evaluate((element) => (element.scrollTop = 0));
    await banner.click();
    await expect(banner).toBeHidden();
    await expect
      .poll(async () => {
        const { band, cardTop } = await geometry(body);
        return cardTop < band;
      })
      .toBe(true);
  });
}
