import { readFile } from "node:fs/promises";
import { test, expect } from "./fixtures.mjs";
import { saveChanges } from "./navigation.mjs";

test("FR1 normal production CSS retains all five legacy viewport fallbacks", async () => {
  const html = await readFile(
    new URL("../../dist/index.html", import.meta.url),
    "utf8",
  );
  const href = html.match(/<link[^>]+href="([^"]+\.css)"/)?.[1];
  expect(
    href,
    "the normal production entry must load its emitted stylesheet",
  ).toBeTruthy();
  const css = await readFile(
    new URL(`../../dist${href}`, import.meta.url),
    "utf8",
  );
  const pairs = [
    [".settings-window", "height", "calc(100vh - 60px)"],
    [".settings-dialog", "max-height", "90vh"],
    [".dialog-body", "max-height", "70vh"],
    [".settings-window", "height", "calc(100vh - 32px)"],
    [".settings-window", "height", "calc(100vh - 20px)"],
  ];
  const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)];
  const retainedPairs = pairs.map(([selector, property, value]) =>
    rules.some(([, selectors, block]) => {
      if (!selectors.split(",").includes(selector)) return false;
      const declarations = block.replace(/\s/g, "").split(";");
      const legacy = declarations.indexOf(
        `${property}:${value.replace(/\s/g, "")}`,
      );
      const dynamic = declarations.indexOf(
        `${property}:${value.replace(/\s/g, "").replace("vh", "dvh")}`,
      );
      return legacy >= 0 && dynamic > legacy;
    }),
  );
  console.log(
    JSON.stringify({
      stylesheet: href,
      legacyUnits: (css.match(/\d+vh\b/g) ?? []).length,
      dynamicUnits: (css.match(/\d+dvh\b/g) ?? []).length,
      retainedPairs,
    }),
  );
  for (const [index, pair] of pairs.entries())
    expect.soft(retainedPairs[index], pair.join(" ")).toBe(true);
});

for (const viewport of [
  { width: 1180, height: 800 },
  { width: 390, height: 844 },
]) {
  test(`FR1 emitted CSS without dvh or dialog APIs bounds scrolling and focus at ${viewport.width}`, async ({
    page,
    store,
  }, testInfo) => {
    await page.setViewportSize(viewport);
    await page.addInitScript(() => {
      Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
        configurable: true,
        value: undefined,
      });
      Object.defineProperty(HTMLDialogElement.prototype, "close", {
        configurable: true,
        value: undefined,
      });
      delete HTMLElement.prototype.inert;
    });
    let unsupportedDeclarations = 0;
    await page.route("**/assets/*.css", async (route) => {
      const response = await route.fetch();
      const css = await response.text();
      // An unknown unit makes Chromium reject just the unsupported declaration,
      // preserving the production cascade without supplying any fallback itself.
      const body = css.replace(/(\d+)dvh\b/g, (_, value) => {
        unsupportedDeclarations++;
        return `${value}unsupportedviewportunit`;
      });
      await route.fulfill({ response, body });
    });
    await store("save_repository", { repository: "octo/legacy-viewport" });
    await page.goto("/?view=settings");
    const opener = page
      .getByRole("article", { name: "octo/legacy-viewport", exact: true })
      .getByRole("button", { name: "Settings", exact: true });
    await opener.click();
    expect(unsupportedDeclarations).toBe(5);
    const modal = page.getByRole("dialog", {
      name: "Settings for octo/legacy-viewport",
      exact: true,
    });
    const close = modal.getByRole("button", {
      name: "Close dialog",
      exact: true,
    });
    const body = modal.locator(".dialog-body");
    const geometry = await page.evaluate(() => {
      const rect = (selector) => {
        const element = document.querySelector(selector);
        const { x, y, width, height, bottom } = element.getBoundingClientRect();
        const {
          height: computedHeight,
          maxHeight,
          overflowY,
        } = getComputedStyle(element);
        return {
          x,
          y,
          width,
          height,
          bottom,
          computedHeight,
          maxHeight,
          overflowY,
          clientHeight: element.clientHeight,
          scrollHeight: element.scrollHeight,
        };
      };
      return {
        settings: rect(".settings-window"),
        modal: rect(".settings-dialog"),
        body: rect(".dialog-body"),
        header: rect(".dialog-head"),
      };
    });
    console.log(JSON.stringify({ viewport, geometry }));
    await testInfo.attach("computed-geometry", {
      body: JSON.stringify(geometry, null, 2),
      contentType: "application/json",
    });
    await page.screenshot({
      path: testInfo.outputPath(
        `legacy-viewport-${viewport.width}-initial.png`,
      ),
    });
    expect(geometry.settings.height).toBeCloseTo(
      viewport.height - (viewport.width < 600 ? 20 : 60),
      0,
    );
    expect(geometry.settings.y).toBeGreaterThanOrEqual(0);
    expect(geometry.settings.bottom).toBeLessThanOrEqual(viewport.height);
    expect(geometry.modal.height).toBeLessThanOrEqual(viewport.height * 0.9);
    expect(geometry.modal.y).toBeGreaterThanOrEqual(0);
    expect(geometry.modal.bottom).toBeLessThanOrEqual(viewport.height);
    expect(parseFloat(geometry.body.maxHeight)).toBeCloseTo(
      viewport.height * 0.7,
      0,
    );
    expect(geometry.body.overflowY).toBe("auto");
    expect(geometry.body.scrollHeight).toBeGreaterThan(
      geometry.body.clientHeight,
    );

    await modal
      .getByRole("checkbox", {
        name: "Override review instructions",
        exact: true,
      })
      .check();
    await modal
      .getByRole("textbox", { name: "Review prompt", exact: true })
      .fill("Legacy viewport review instructions.");
    await modal.getByText("Repository and connection", { exact: true }).click();
    const last = modal.getByRole("button", {
      name: "Verify GitHub connection",
      exact: true,
    });
    await last.scrollIntoViewIfNeeded();
    await last.focus();
    expect(await body.evaluate((element) => element.scrollTop)).toBeGreaterThan(
      0,
    );
    const headerAfterScroll = await modal.locator(".dialog-head").boundingBox();
    expect(headerAfterScroll.y).toBeCloseTo(geometry.header.y, 0);
    expect(headerAfterScroll.y + headerAfterScroll.height).toBeLessThanOrEqual(
      viewport.height,
    );
    const lastBounds = await last.boundingBox();
    expect(lastBounds.y).toBeGreaterThanOrEqual(
      headerAfterScroll.y + headerAfterScroll.height,
    );
    expect(lastBounds.y + lastBounds.height).toBeLessThanOrEqual(
      geometry.modal.bottom,
    );
    await page.keyboard.press("Tab");
    await expect(close).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(last).toBeFocused();
    await page
      .locator(
        viewport.width < 600
          ? '[aria-label="Settings section"]'
          : '[data-section="reviews"]',
      )
      .evaluate((element) => element.focus());
    await expect(close).toBeFocused();
    await page.screenshot({
      path: testInfo.outputPath(
        `legacy-viewport-${viewport.width}-scrolled.png`,
      ),
    });
    await close.click();
    await expect(opener).toBeFocused();

    const save = page.getByRole("button", {
      name: "Save changes",
      exact: true,
    });
    await save.focus();
    await expect(save).toBeFocused();
    const footer = await save.boundingBox();
    expect(footer.y).toBeGreaterThanOrEqual(0);
    expect(footer.y + footer.height).toBeLessThanOrEqual(viewport.height);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: testInfo.outputPath(`legacy-viewport-${viewport.width}-footer.png`),
    });
    await saveChanges(page);
    expect(
      (await store("snapshot")).settings.repositories[0].overrides.prompt,
    ).toBe("Legacy viewport review instructions.");
    await opener.click();
    await page.keyboard.press("Escape");
    await expect(modal).toHaveCount(0);
    await expect(opener).toBeFocused();
  });
}
