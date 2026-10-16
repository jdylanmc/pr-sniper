import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";

const { version } = JSON.parse(
  readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
);
const sections = ["what", "why", "how", "changelog", "demo"];
const NORD_BG = "rgb(46, 52, 64)";
const P1_BG = "rgb(2, 26, 10)";
const HOME = "https://jdylanmc.github.io/";

const thematicOn = (page) =>
  page.addInitScript(() => localStorage.setItem("termlink-thematic", "on"));

async function open(page, { thematic = false } = {}) {
  await page.addInitScript(() =>
    sessionStorage.setItem("termlink-booted", "1"),
  );
  if (thematic) await thematicOn(page);
  await page.goto("/");
  await expect(page.locator("#tab-what")).toHaveAttribute(
    "aria-selected",
    "true",
  );
}

async function expectSection(page, name) {
  for (const other of sections) {
    const selected = other === name;
    await expect(page.locator(`#tab-${other}`)).toHaveAttribute(
      "aria-selected",
      String(selected),
    );
    await expect(page.locator(`#tab-${other}`)).toHaveAttribute(
      "tabindex",
      selected ? "0" : "-1",
    );
    await expect(page.locator(`#panel-${other}`)).toBeVisible({
      visible: selected,
    });
  }
}

const blur = (page) => page.evaluate(() => document.activeElement?.blur());
const background = (locator) =>
  locator.evaluate((node) => getComputedStyle(node).backgroundColor);
// The label shown in the current look; the other stays in the DOM.
const label = (button, thematic) =>
  button.locator(thematic ? ".t-on" : ".t-off");

test("thematic is off by default: Nord, no boot, no theme or sound", async ({
  page,
}) => {
  await page.goto("/");
  const html = page.locator("html");
  await expect(html).toHaveAttribute("data-thematic", "off");
  await expect(page.locator("#tab-what")).toHaveAttribute(
    "aria-selected",
    "true",
  );
  expect(await background(page.locator("body"))).toBe(NORD_BG);
  expect(await background(page.locator(".screen"))).toBe(NORD_BG);
  await expect(page.locator("#boot")).toBeHidden();
  await expect(page.locator("#theme-btn")).toBeHidden();
  await expect(page.locator("#sound-btn")).toBeHidden();
  await expect(page.locator(".statusline")).toBeHidden();
  await expect(page.locator(".head-clock")).toBeHidden();
  await expect(page.locator(".panel h2 .prompt").first()).toBeHidden();
  await expect(page.locator("#thematic-btn")).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  await expect(label(page.locator("#thematic-btn"), true)).toHaveText(
    "[THEMATIC OFF]",
  );
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
    "content",
    "#2e3440",
  );
  expect(
    await page.evaluate(() => getComputedStyle(document.body).fontFamily),
  ).toMatch(/^"?JetBrains Mono/);
  await blur(page);
  await page.keyboard.press("t");
  await page.keyboard.press("m");
  await expect(page.locator("body")).toHaveAttribute("data-theme", "p1");
  await expect(page.locator("#sound-btn")).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(
    await page.evaluate(() => localStorage.getItem("termlink-theme")),
  ).toBeNull();
  await page.reload();
  await expect(page.locator("#boot")).toBeHidden();
});

test("the thematic toggle turns the terminal on, persists and turns off", async ({
  page,
}) => {
  await page.goto("/");
  const html = page.locator("html");
  const button = page.locator("#thematic-btn");
  await button.click();
  await expect(html).toHaveAttribute("data-thematic", "on");
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await expect(label(button, true)).toHaveText("[THEMATIC ON]");
  await expect(page.locator("#announcer")).toHaveText("Thematic on");
  expect(await background(page.locator(".screen"))).toBe(P1_BG);
  await expect(page.locator("#theme-btn")).toBeVisible();
  await expect(page.locator("#sound-btn")).toBeVisible();
  await expect(page.locator(".panel h2 .prompt").first()).toBeVisible();
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
    "content",
    "#021a0a",
  );
  // Turning it on never forces a boot, now or on reload this session.
  await expect(page.locator("#boot")).toBeHidden();
  await page.reload();
  await expect(html).toHaveAttribute("data-thematic", "on");
  await expect(page.locator("#boot")).toBeHidden();
  expect(await background(page.locator(".screen"))).toBe(P1_BG);
  await page.locator("#sound-btn").click();
  await expect(page.locator("#sound-btn")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await button.click();
  await expect(html).toHaveAttribute("data-thematic", "off");
  await expect(button).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#announcer")).toHaveText("Thematic off");
  await expect(page.locator("#sound-btn")).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(await background(page.locator(".screen"))).toBe(NORD_BG);
  await page.reload();
  await expect(html).toHaveAttribute("data-thematic", "off");
});

test("keyboard focus is visible with thematic off", async ({ page }) => {
  await open(page);
  await page.locator("#tab-what").focus();
  await page.keyboard.press("ArrowRight");
  const tab = page.locator("#tab-why");
  await expect(tab).toBeFocused();
  expect(
    await tab.evaluate((node) => {
      const style = getComputedStyle(node);
      return [style.outlineStyle, style.outlineColor];
    }),
  ).toEqual(["solid", "rgb(136, 192, 208)"]);
  await page.keyboard.press("Tab");
  const focused = page.locator(":focus");
  expect(
    await focused.evaluate((node) => getComputedStyle(node).outlineStyle),
  ).toBe("solid");
});

test("with thematic off the about sidebar starts collapsed and toggles", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  const about = page.locator("#about");
  const button = page.locator("#about-btn");
  await expect(button).toBeVisible();
  await expect(button).toHaveAccessibleName("about");
  await expect(button).toHaveAttribute("aria-controls", "about");
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(about).toBeHidden();
  const collapsed = await page.locator("#sections").boundingBox();
  await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(about).toBeVisible();
  expect(await about.evaluate((node) => getComputedStyle(node).position)).toBe(
    "sticky",
  );
  const aboutBox = await about.boundingBox();
  const expanded = await page.locator("#sections").boundingBox();
  // A column beside the content, not an overlay.
  expect(aboutBox.x + aboutBox.width).toBeLessThanOrEqual(expanded.x);
  expect(expanded.width).toBeLessThan(collapsed.width - 200);
  await expect(about.getByRole("link", { name: "Releases" })).toBeVisible();
  await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(about).toBeHidden();
  await button.click();
  await page.reload();
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(about).toBeHidden();
});

test("with thematic on the sidebar always shows and the toggle hides", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page, { thematic: true });
  await expect(page.locator("#about")).toBeVisible();
  await expect(page.locator("#about-btn")).toBeHidden();
});

test("with thematic off the document scrolls, not an inner box", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  expect(
    await page
      .locator("#scroller")
      .evaluate((node) => getComputedStyle(node).overflowY),
  ).toBe("visible");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollHeight > window.innerHeight,
    ),
  ).toBe(true);
  // The footer follows the content.
  const foot = await page.locator(".foot").boundingBox();
  expect(foot.y).toBeGreaterThan(900);
  await blur(page);
  await page.keyboard.press("ArrowDown");
  await expect
    .poll(() => page.evaluate(() => window.scrollY))
    .toBeGreaterThan(0);
  expect(
    await page.locator("#scroller").evaluate((node) => node.scrollTop),
  ).toBe(0);
});

test("H toggles thematic", async ({ page }) => {
  await open(page);
  await blur(page);
  await page.keyboard.press("h");
  await expect(page.locator("html")).toHaveAttribute("data-thematic", "on");
  await page.keyboard.press("H");
  await expect(page.locator("html")).toHaveAttribute("data-thematic", "off");
});

for (const saved of ["on", "off"]) {
  test(`saved thematic ${saved} applies before first paint`, async ({
    page,
  }) => {
    await page.addInitScript((value) => {
      localStorage.setItem("termlink-thematic", value);
      sessionStorage.setItem("termlink-booted", "1");
      // Seen when <body> is parsed, before app.js runs.
      new MutationObserver((_, observer) => {
        if (!document.body) return;
        window.atBody = document.documentElement.dataset.thematic;
        observer.disconnect();
      }).observe(document, { childList: true, subtree: true });
      const firstFrame = () => {
        const screen = document.querySelector(".screen");
        if (!screen) return requestAnimationFrame(firstFrame);
        window.firstBg = getComputedStyle(screen).backgroundColor;
      };
      requestAnimationFrame(firstFrame);
    }, saved);
    await page.goto("/");
    await page.waitForFunction(() => window.firstBg);
    expect(await page.evaluate(() => window.atBody)).toBe(saved);
    expect(await page.evaluate(() => window.firstBg)).toBe(
      saved === "on" ? P1_BG : NORD_BG,
    );
  });
}

for (const thematic of [false, true]) {
  test(`header links home with thematic ${thematic ? "on" : "off"}`, async ({
    page,
  }) => {
    await open(page, { thematic });
    const home = page.getByRole("link", { name: "dylan mccurry", exact: true });
    await expect(home).toBeVisible();
    await expect(home).toHaveAttribute("href", HOME);
    await expect(home).not.toHaveAttribute("target", /.+/);
    await expect(page.locator(".head .head-link")).toHaveAttribute(
      "href",
      "https://github.com/jdylanmc/pr-sniper",
    );
  });
}

test("tabs switch by click and keep a roving tabindex", async ({ page }) => {
  await open(page);
  await expect(
    page.locator('[role="tablist"][aria-label="Sections"]'),
  ).toBeVisible();
  await expectSection(page, "what");
  await page.locator("#tab-how").click();
  await expectSection(page, "how");
  await page.locator("#tab-demo").click();
  await expectSection(page, "demo");
  await page.locator("#tab-what").click();
  await expectSection(page, "what");
});

test("number keys and arrows switch tabs", async ({ page }) => {
  await open(page);
  await blur(page);
  await page.keyboard.press("2");
  await expectSection(page, "why");
  await page.keyboard.press("ArrowRight");
  await expectSection(page, "how");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expectSection(page, "what");
  await page.keyboard.press("4");
  await expectSection(page, "changelog");
  await page.keyboard.press("ArrowRight");
  await expectSection(page, "demo");
  await page.keyboard.press("ArrowLeft");
  await expectSection(page, "changelog");
  await page.keyboard.press("5");
  await expectSection(page, "demo");
  await page.locator("#tab-demo").focus();
  await page.keyboard.press("ArrowRight");
  await expectSection(page, "what");
  await expect(page.locator("#tab-what")).toBeFocused();
});

test("T cycles the theme and the choice persists", async ({ page }) => {
  await open(page, { thematic: true });
  const body = page.locator("body");
  await expect(body).toHaveAttribute("data-theme", "p1");
  await blur(page);
  await page.keyboard.press("t");
  await expect(body).toHaveAttribute("data-theme", "p3");
  await expect(page.locator("#theme-btn")).toHaveText("[THEME P3 AMBER]");
  await page.keyboard.press("T");
  await page.keyboard.press("t");
  await expect(body).toHaveAttribute("data-theme", "p7");
  await page.reload();
  await expect(body).toHaveAttribute("data-theme", "p7");
  await page.locator("#theme-btn").click();
  await expect(body).toHaveAttribute("data-theme", "p1");
});

test("boot runs once per session and any key skips it", async ({ page }) => {
  await thematicOn(page);
  await page.goto("/");
  await expect(page.locator("#boot")).toBeVisible();
  await page.keyboard.press("x");
  await expect(page.locator("#boot")).toBeHidden();
  await page.reload();
  await expect(page.locator("#boot")).toBeHidden();
});

test.describe("reduced motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("skips the boot", async ({ page }) => {
    await thematicOn(page);
    await page.goto("/");
    await expect(page.locator("#boot")).toBeHidden();
    await expect(page.locator("#tab-what")).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});

test("changelog renders versions from CHANGELOG.md", async ({ page }) => {
  await open(page);
  await page.locator("#tab-changelog").click();
  const headings = page.locator("#changelog h3");
  await expect(headings.filter({ hasText: version }).first()).toBeVisible();
  const links = await page
    .locator("#changelog a")
    .evaluateAll((nodes) => nodes.map((node) => node.href));
  expect(links.length).toBeGreaterThan(0);
  for (const href of links) expect(href).toMatch(/^https:\/\//);
  expect(await page.locator("#changelog script, #changelog img").count()).toBe(
    0,
  );
});

for (const thematic of [false, true]) {
  test(`What tab install steps each copy their command, thematic ${thematic ? "on" : "off"}`, async ({
    page,
    context,
    browserName,
  }) => {
    if (browserName === "chromium") {
      await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    }
    await open(page, { thematic });
    await expect(page.locator("#panel-what")).toBeVisible();
    const steps = page.locator("#panel-what .install > li");
    await expect(steps).toHaveCount(6);
    const buttons = page.locator("#panel-what .copy");
    await expect(buttons).toHaveCount(4);
    const expected = [
      "brew install --cask jdylanmc/pr-sniper/pr-sniper",
      'open -a "PR Sniper"',
      "brew upgrade --cask pr-sniper",
      "brew uninstall --cask pr-sniper",
    ];
    await expect(buttons.first()).toHaveAccessibleName(
      thematic ? "[COPY]" : "COPY",
    );
    for (const [index, command] of expected.entries()) {
      const button = buttons.nth(index);
      await expect(button.locator("xpath=..").locator("code")).toHaveText(
        command,
      );
      await button.click();
      await expect(label(button, thematic)).toHaveText(
        thematic ? "[Copied]" : "Copied",
      );
      if (browserName === "chromium") {
        expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
          command,
        );
      }
    }
    await expect(label(buttons.first(), thematic)).toHaveText(
      thematic ? "[COPY]" : "COPY",
      { timeout: 4000 },
    );
  });
}

test("shown version matches package.json", async ({ page }) => {
  await open(page);
  await expect(page.locator("#version")).toHaveText(`v${version}`);
});

for (const thematic of [false, true]) {
  test(`stays usable at 375px, thematic ${thematic ? "on" : "off"}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await open(page, { thematic });
    await expect(page.locator("#thematic-btn")).toBeVisible();
    await expect(
      page.getByRole("link", { name: "dylan mccurry" }),
    ).toBeVisible();
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(375);
    await page.locator("#tab-demo").click();
    await expect(page.locator("#demo-frame")).toBeVisible();
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(375);
  });
}

test("demo is the last tab and loads the v2 prototype only when opened", async ({
  page,
}) => {
  await open(page);
  const frame = page.locator("#demo-frame");
  await expect(frame).not.toHaveAttribute("src", /.+/);
  const tabs = page.locator(
    '[role="tablist"][aria-label="Sections"] [role="tab"]',
  );
  await expect(tabs.last()).toHaveAttribute("id", "tab-demo");
  await expect(tabs.last()).toContainText("5 DEMO");
  await page.locator("#tab-demo").click();
  await expect(frame).toHaveAttribute("src", "demo/index.html");
  await expect(frame).toHaveAttribute("title", /design prototype/);
  const box = await frame.boundingBox();
  expect(box.width).toBeGreaterThan(600);
  const demo = page.frameLocator("#demo-frame");
  await expect(demo.locator("body")).not.toBeEmpty();
  await expect(demo.locator("img[src*='sniper-mark']").first()).toBeAttached();
});

test("the prototype's own assets load", async ({ request }) => {
  for (const path of [
    "demo/index.html",
    "demo/style.css",
    "demo/app.js",
    "demo/model.js",
    "demo/sniper-mark.png",
    "demo/wallpaper.svg",
  ]) {
    const response = await request.get(path);
    expect(response.status(), path).toBe(200);
  }
  expect((await request.get("demo/README.md")).ok()).toBe(false);
});

for (const [width, height] of [
  [1920, 1080],
  [1440, 900],
  [1024, 768],
  [1280, 1024],
  [375, 812],
]) {
  for (const thematic of [false, true]) {
    test(`demo screen keeps 16:9 at ${width}x${height}, thematic ${thematic ? "on" : "off"}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      await open(page, { thematic });
      await page.locator("#tab-demo").click();
      const viewport = page.locator("#demo-viewport");
      await expect(viewport).toBeVisible();
      await expect
        .poll(async () => {
          const box = await viewport.boundingBox();
          return Math.abs(box.width / box.height - 16 / 9);
        })
        .toBeLessThan(0.02);
      const box = await viewport.boundingBox();
      const frame = await page.locator("#demo-frame").boundingBox();
      // The scaled desktop fills the viewport exactly.
      expect(Math.abs(frame.width - box.width)).toBeLessThan(2);
      expect(Math.abs(frame.height - box.height)).toBeLessThan(2);
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth),
      ).toBeLessThanOrEqual(width);
      // With thematic off the page scrolls; the desktop still fits the window.
      if (!thematic) expect(box.y + box.height).toBeLessThanOrEqual(height);
    });
  }
}

for (const thematic of [false, true]) {
  test(`full screen fills the window at 16:9 and Esc closes it, thematic ${thematic ? "on" : "off"}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await open(page, { thematic });
    await page.locator("#tab-demo").click();
    const button = page.locator("#demo-full");
    const viewport = page.locator("#demo-viewport");
    const before = await viewport.boundingBox();
    // A small button on the desktop's frame, not a caption row.
    await expect(page.locator(".demo-screen > #demo-full")).toHaveCount(1);
    await expect(button).toBeVisible();
    await expect(button).toHaveAccessibleName("Full screen");
    if (thematic) await expect(label(button, true)).toHaveText("[FULL]");
    const frameBox = await page.locator(".demo-screen").boundingBox();
    const buttonBox = await button.boundingBox();
    expect(buttonBox.width).toBeLessThan(80);
    expect(buttonBox.x + buttonBox.width).toBeLessThanOrEqual(
      frameBox.x + frameBox.width,
    );
    // Centred on or below the frame edge, never over the desktop's middle.
    expect(buttonBox.y + buttonBox.height / 2).toBeGreaterThanOrEqual(
      before.y + before.height - 1,
    );
    await button.click();
    await expect(button).toHaveAttribute("aria-pressed", "true");
    await expect(button).toHaveAccessibleName("Exit full screen");
    await expect
      .poll(async () => (await viewport.boundingBox()).width)
      .toBeGreaterThan(before.width + 100);
    const full = await viewport.boundingBox();
    expect(Math.abs(full.width / full.height - 16 / 9)).toBeLessThan(0.02);
    // Tab keys are ignored while full screen.
    await page.keyboard.press("1");
    await expect(page.locator("#tab-demo")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await page.keyboard.press("Escape");
    await expect(button).toHaveAttribute("aria-pressed", "false");
    await expect(button).toHaveAccessibleName("Full screen");
    await expect
      .poll(async () => Math.round((await viewport.boundingBox()).width))
      .toBe(Math.round(before.width));
  });
}

test("the demo tab is only the desktop, with no caption row", async ({
  page,
}) => {
  for (const thematic of [false, true]) {
    await open(page, { thematic });
    await page.locator("#tab-demo").click();
    await expect(page.locator(".demo-caption")).toHaveCount(0);
    await expect(page.locator("#panel-demo")).not.toContainText("Fake data");
    await expect(page.locator("#panel-demo > *")).toHaveCount(1);
  }
});

test("Esc inside the demo closes the full screen overlay", async ({ page }) => {
  await page.addInitScript(() => {
    // Force the overlay fallback, as on browsers without the Fullscreen API.
    Element.prototype.requestFullscreen = undefined;
    Element.prototype.webkitRequestFullscreen = undefined;
  });
  await open(page);
  await page.locator("#tab-demo").click();
  const demo = page.frameLocator("#demo-frame");
  await expect(demo.locator("body")).not.toBeEmpty();
  await page.locator("#demo-full").click();
  await expect(page.locator("#demo-stage")).toHaveClass(/demo-overlay/);
  await demo.locator("body").click({ position: { x: 5, y: 400 } });
  await page.keyboard.press("Escape");
  await expect(page.locator("#demo-stage")).not.toHaveClass(/demo-overlay/);
  await expect(page.locator("#demo-full")).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});

test("switching tabs redraws the new section once", async ({ page }) => {
  await open(page, { thematic: true });
  await expect(page.locator("#panel-what")).not.toHaveClass(/redraw/);
  await page.locator("#tab-why").click();
  await expect(page.locator("#panel-why")).toHaveClass(/redraw/);
  await expect(page.locator("#panel-why")).not.toHaveClass(/redraw/, {
    timeout: 2000,
  });
});

test("with thematic off, switching tabs fades instead of redrawing", async ({
  page,
}) => {
  await open(page);
  await page.locator("#tab-why").click();
  await expect(page.locator("#panel-why")).toHaveClass(/fade/);
  await expect(page.locator("#panel-why")).not.toHaveClass(/redraw/);
  await expect(page.locator("#panel-why")).not.toHaveClass(/fade/, {
    timeout: 2000,
  });
});

test.describe("reduced motion tabs", () => {
  test.use({ reducedMotion: "reduce" });

  test("switch without the redraw", async ({ page }) => {
    for (const thematic of [false, true]) {
      await open(page, { thematic });
      await page.locator("#tab-how").click();
      await expect(page.locator("#panel-how")).toBeVisible();
      await expect(page.locator("#panel-how")).not.toHaveClass(/redraw|fade/);
    }
  });
});
