import { expect, test } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";

const geometry = (page, selector) =>
  page.locator(selector).evaluateAll((lists) =>
    lists.map((list) => {
      const card = list.getBoundingClientRect();
      return {
        card: { top: card.top, bottom: card.bottom },
        rows: [...list.children].map((row) => {
          const rect = row.getBoundingClientRect();
          return { top: rect.top, bottom: rect.bottom };
        }),
      };
    }),
  );

function expectSingleContiguousList(lists, count) {
  expect(lists).toHaveLength(1);
  const [{ card, rows }] = lists;
  expect(rows).toHaveLength(count);
  expect(rows[0].top).toBeGreaterThanOrEqual(card.top);
  expect(rows.at(-1).bottom).toBeLessThanOrEqual(card.bottom);
  for (let index = 1; index < rows.length; index++)
    expect(Math.abs(rows[index].top - rows[index - 1].bottom)).toBeLessThan(1);
}

for (const width of [408, 320]) {
  test(`Agents and Doctrines render as one grouped list with singular counts at ${width}px`, async ({
    page,
    store,
  }) => {
    await queueFixture(store);
    const settings = (await store("snapshot")).settings;
    const [assigned] = settings.agents;
    settings.agents.push({
      ...structuredClone(assigned),
      id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      name: "Unassigned reviewer",
    });
    settings.doctrines = settings.doctrines.slice(0, 3);
    await store("seed_settings", settings);
    await page.setViewportSize({ width, height: 744 });
    await page.goto("/");
    await page
      .getByRole("navigation", { name: "Application destinations" })
      .getByRole("button", { name: "Settings", exact: true })
      .click();
    await section(page, "Agents");
    const agents = page.locator(".agent-card");
    await expect(agents).toHaveCount(2);
    expectSingleContiguousList(await geometry(page, ".resource-library"), 2);
    await expect(agents.first().locator(".resource-meta")).toHaveText(
      `${assigned.model} / 1 repository`,
    );
    await expect(agents.nth(1).locator(".resource-meta")).toHaveText(
      `${assigned.model} / 0 repositories`,
    );
    await section(page, "Doctrines");
    await expect(page.locator(".doctrine-card")).toHaveCount(3);
    expectSingleContiguousList(await geometry(page, ".resource-library"), 3);
    const counts = await page
      .locator(".doctrine-card .word-count")
      .allTextContents();
    for (const text of counts) {
      expect(text).toMatch(/words \/ \d+ Agents?$/);
      expect(text).not.toMatch(/ 1 Agents$/);
    }
  });
}
