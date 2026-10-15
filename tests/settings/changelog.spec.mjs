import { readFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";
import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { section } from "./navigation.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

// Band, art and first-card overlap shared by every titled sub-page.
const chrome = (page) =>
  page.evaluate(() => {
    const context = document.querySelector(".panel-context");
    const art = context.querySelector(".panel-art");
    const content = document.querySelector(".panel-content");
    const card = [...content.children].find(
      (child) => !child.hidden && child.getBoundingClientRect().height > 0,
    );
    const band = context.getBoundingClientRect();
    const first =
      card.querySelector(".resource-library, .changelog") ??
      (card.matches(".changelog") ? card : card.firstElementChild);
    return {
      band: Math.round(band.height),
      art: getComputedStyle(art).display,
      summary: getComputedStyle(document.querySelector("[data-panel-summary]"))
        .display,
      overlap: Math.round(band.bottom - first.getBoundingClientRect().top),
    };
  });

for (const width of [408, 320]) {
  test(`Changelog uses the shared sub-page header and renders inline Markdown at ${width}px`, async ({
    page,
    store,
  }) => {
    await queueFixture(store);
    await page.setViewportSize({ width, height: 744 });
    await page.goto("/");
    await tab(page, "Settings").click();
    await section(page, "Agents");
    const agents = await chrome(page);
    for (const [origin, title] of [
      ["Queue", "Your queue"],
      ["Settings", "Agents"],
    ]) {
      await tab(page, origin).click();
      await expect(page.locator("[data-panel-heading]")).toHaveText(title);
      await page.evaluate(() => window.__settingsIdle());
      await page.locator("[data-panel-version]").click();
      await expect(page.locator("[data-panel-heading]")).toHaveText(
        "Changelog",
      );
      const changelog = await chrome(page);
      expect(changelog.band).toBe(agents.band);
      expect(changelog.art).toBe(agents.art);
      expect(changelog.summary).toBe("none");
      expect(changelog.overlap).toBe(23);
      await page.getByRole("button", { name: "Back", exact: true }).click();
    }
    await page.locator("[data-panel-version]").click();
    const view = page.locator('[data-panel-view="changelog"]');
    await expect(view.locator("strong").first()).toBeVisible();
    await expect(view.locator("code").first()).toBeVisible();
    await expect(view.locator("strong", { hasText: "Tools" })).toHaveCount(1);
    await expect(
      view.locator("code", { hasText: "read_check_log" }).first(),
    ).toBeVisible();
    const text = await view.textContent();
    expect(text).not.toContain("**");
    expect(text).not.toContain("`");
    const scroll = await page
      .locator(".panel-content")
      .evaluate((element) => [element.scrollHeight, element.clientHeight]);
    expect(scroll[0]).toBeGreaterThan(scroll[1]);
  });
}

test("inline Markdown never parses changelog text as HTML", async ({
  page,
}) => {
  const source = stripTypeScriptTypes(
    await readFile(new URL("../../src/markdown.ts", import.meta.url), "utf8"),
  ).replace("export function", "function");
  await page.goto("about:blank");
  const rendered = await page.evaluate((source) => {
    const inlineMarkdown = new Function(`${source}; return inlineMarkdown;`)();
    const root = document.createElement("li");
    inlineMarkdown(
      root,
      'Plain <img src=x onerror="window.pwned=1"> **<b>bold</b>** and `<script>window.pwned=2</script>` tail',
    );
    return {
      html: root.innerHTML,
      tags: [...root.querySelectorAll("*")].map((node) => node.tagName),
      pwned: window.pwned ?? null,
    };
  }, source);
  expect(rendered.tags).toEqual(["STRONG", "CODE"]);
  expect(rendered.pwned).toBeNull();
  expect(rendered.html).toBe(
    'Plain &lt;img src=x onerror="window.pwned=1"&gt; <strong>&lt;b&gt;bold&lt;/b&gt;</strong> and <code>&lt;script&gt;window.pwned=2&lt;/script&gt;</code> tail',
  );
});
