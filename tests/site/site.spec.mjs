import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";

const { version } = JSON.parse(
  readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
);
const sections = ["what", "why", "how", "changelog", "demo"];
// Landing page order with thematic off.
const pageOrder = [
  ".hero",
  "#panel-what",
  "#panel-why",
  "#panel-how",
  "#install",
  "#panel-changelog",
];
const NORD_BG = "rgb(46, 52, 64)";
const P1_BG = "rgb(2, 26, 10)";
const HOME = "https://jdylanmc.github.io/";
const FULL_CHANGELOG =
  "https://github.com/jdylanmc/pr-sniper/blob/main/CHANGELOG.md";

const thematicOn = (page) =>
  page.addInitScript(() => localStorage.setItem("termlink-thematic", "on"));

async function open(page, { thematic = false } = {}) {
  await page.addInitScript(() =>
    sessionStorage.setItem("termlink-booted", "1"),
  );
  if (thematic) await thematicOn(page);
  await page.goto("/");
  if (thematic) {
    await expect(page.locator("#tab-what")).toHaveAttribute(
      "aria-selected",
      "true",
    );
  } else {
    await expect(page.locator(".hero")).toBeVisible();
  }
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

// Thematic on: the DEMO tab. Off: the demo page.
async function openDemo(page, thematic) {
  if (thematic) {
    await open(page, { thematic });
    await page.locator("#tab-demo").click();
  } else {
    await page.goto("/demo.html");
  }
  await expect(page.locator("#demo-viewport")).toBeVisible();
}

const step = (page) =>
  page.locator("#mini").evaluate((node) => node.dataset.step);

async function expectLanding(page) {
  await expect(page.locator("html")).toHaveAttribute("data-thematic", "off");
  await expect(page.locator(".hero h1")).toHaveText("pr sniper");
  await expect(page.locator('[role="tablist"]')).toBeHidden();
  await expect(page.locator('[role="tab"]:visible')).toHaveCount(0);
  await expect(page.locator('[role="tabpanel"]')).toHaveCount(0);
  await expect(page.locator("#scroller section[hidden]")).toHaveCount(0);
  // The desktop demo lives on demo.html.
  await expect(page.locator("#panel-demo")).toBeHidden();
  await expect(page.locator("#demo-frame")).not.toHaveAttribute("src", /.+/);
  let previous = -Infinity;
  for (const selector of pageOrder) {
    const node = page.locator(selector);
    await expect(node).toBeVisible();
    const top = await node.evaluate(
      (el) => el.getBoundingClientRect().top + window.scrollY,
    );
    expect(top, selector).toBeGreaterThan(previous);
    previous = top;
  }
  // DOM order matches the visual order.
  const domOrder = await page.evaluate((selectors) => {
    const nodes = selectors.map((s) => document.querySelector(s));
    return nodes.every(
      (node, i) =>
        i === 0 ||
        nodes[i - 1].compareDocumentPosition(node) &
          Node.DOCUMENT_POSITION_FOLLOWING,
    );
  }, pageOrder);
  expect(domOrder).toBe(true);
}

const blur = (page) => page.evaluate(() => document.activeElement?.blur());
const background = (locator) =>
  locator.evaluate((node) => getComputedStyle(node).backgroundColor);
// The label shown in the current look; the other stays in the DOM.
const label = (button, thematic) =>
  button.locator(thematic ? ".t-on" : ".t-off");

test("thematic is off by default: the Nord landing page", async ({ page }) => {
  await page.goto("/");
  await expectLanding(page);
  expect(await background(page.locator("body"))).toBe(NORD_BG);
  await expect(page.locator("#boot")).toBeHidden();
  await expect(page.locator("#theme-btn")).toBeHidden();
  await expect(page.locator("#sound-btn")).toBeHidden();
  await expect(page.locator(".statusline")).toBeHidden();
  await expect(page.locator(".head-clock")).toBeHidden();
  await expect(page.locator(".info")).toBeHidden();
  await expect(page.locator("#panel-what h2 .prompt")).toBeHidden();
  await expect(page.locator(".hero-facts")).toContainText(
    `v${version} · macOS 13.5+ · Apple Silicon · MIT`,
  );
  await expect(page.locator(".hero .btn").first()).toHaveAttribute(
    "href",
    "#install",
  );
  await expect(page.locator(".hero-actions .btn--ghost")).toHaveAttribute(
    "href",
    "https://github.com/jdylanmc/pr-sniper",
  );
  await expect(page.locator("#panel-what .terms > div")).toHaveCount(3);
  await expect(page.locator(".hero figure.mini")).toBeVisible();
  const button = page.locator(".head #thematic-btn");
  await expect(button).toBeVisible();
  await expect(button).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator(".foot #thematic-btn")).toHaveCount(0);
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
    "content",
    "#2e3440",
  );
  expect(
    await page.evaluate(() => getComputedStyle(document.body).fontFamily),
  ).toMatch(/^"?JetBrains Mono/);
  await page.reload();
  await expect(page.locator("#boot")).toBeHidden();
});

test("with thematic off the page keeps normal keyboard behaviour", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  await blur(page);
  for (const key of ["t", "m", "2", "5", "ArrowRight"]) {
    await page.keyboard.press(key);
  }
  await expect(page.locator("body")).toHaveAttribute("data-theme", "p1");
  await expect(page.locator("#sound-btn")).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(
    await page.evaluate(() => localStorage.getItem("termlink-theme")),
  ).toBeNull();
  expect(new URL(page.url()).hash).toBe("");
  await expectLanding(page);
  // The document scrolls; there is no inner scroller.
  expect(
    await page
      .locator("#scroller")
      .evaluate((node) => getComputedStyle(node).overflowY),
  ).toBe("visible");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollHeight > window.innerHeight * 2,
    ),
  ).toBe(true);
  await page.mouse.move(700, 450);
  await page.mouse.wheel(0, 600);
  await expect
    .poll(() => page.evaluate(() => window.scrollY))
    .toBeGreaterThan(0);
  const foot = await page
    .locator(".foot")
    .evaluate((node) => node.getBoundingClientRect().top + window.scrollY);
  const changelog = await page
    .locator("#panel-changelog")
    .evaluate((node) => node.getBoundingClientRect().bottom + window.scrollY);
  expect(foot).toBeGreaterThanOrEqual(changelog);
});

test("header anchors scroll to their sections", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  const nav = page.locator('.head nav[aria-label="Page"] a');
  await expect(nav).toHaveText(["what", "why", "how", "demo", "changelog"]);
  await expect(page.locator(".head nav").getByText("demo")).toHaveAttribute(
    "href",
    "demo.html",
  );
  for (const name of ["changelog", "why", "how", "what"]) {
    await page.locator(`.head nav a[href="#${name}"]`).click();
    await expect.poll(() => new URL(page.url()).hash).toBe(`#${name}`);
    const target = page.locator(`#${name}`);
    await expect(target).toBeInViewport();
    // A short final section can reach the document end before top alignment.
    await expect
      .poll(() =>
        target.evaluate((node) => {
          const top = node.getBoundingClientRect().top + window.scrollY;
          const margin = Number.parseFloat(
            getComputedStyle(node).scrollMarginTop,
          );
          const maxScroll =
            document.documentElement.scrollHeight - window.innerHeight;
          const expected = Math.min(Math.max(0, top - margin), maxScroll);
          return Math.abs(window.scrollY - expected);
        }),
      )
      .toBeLessThanOrEqual(1);
    expect((await target.boundingBox()).y).toBeGreaterThanOrEqual(0);
  }
  await page.locator('.hero a[href="#install"]').click();
  await expect(page.locator("#install h2")).toBeInViewport();
});

test("the hero copies the install command", async ({
  page,
  context,
  browserName,
}) => {
  if (browserName === "chromium") {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  }
  await open(page);
  const button = page.locator(".hero .copy");
  await expect(button).toHaveAccessibleName("COPY");
  await button.click();
  await expect(label(button, false)).toHaveText("Copied");
  await expect(page.locator("#announcer")).toHaveText("Copied");
  if (browserName === "chromium") {
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
      "brew install --cask jdylanmc/pr-sniper/pr-sniper",
    );
  }
});

test("the hero mini panel sits beside the text, or under it on phones", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  const mini = page.locator(".hero figure.mini");
  await expect(mini).toBeVisible();
  await expect(mini.locator("figcaption")).toHaveClass(/sr-only/);
  await expect(mini.locator(".mini-scale")).toHaveAttribute(
    "aria-hidden",
    "true",
  );
  expect(
    await mini.evaluate(
      (node) => node.querySelectorAll("a, button, input, [tabindex]").length,
    ),
  ).toBe(0);
  const text = await page.locator(".hero-text").boundingBox();
  let box = await mini.boundingBox();
  expect(box.x).toBeGreaterThan(text.x + text.width);
  expect(box.width).toBeGreaterThanOrEqual(340);
  expect(box.width).toBeLessThanOrEqual(380);
  // Vertically centred against the text.
  expect(
    Math.abs(box.y + box.height / 2 - (text.y + text.height / 2)),
  ).toBeLessThan(80);
  const link = page.getByRole("link", { name: "Try the full demo ↗" });
  await expect(link).toHaveAttribute("href", "demo.html");

  await page.setViewportSize({ width: 375, height: 812 });
  const actions = await page.locator(".hero-actions").boundingBox();
  box = await mini.boundingBox();
  expect(box.y).toBeGreaterThan(actions.y + actions.height);
  expect(box.width).toBeLessThanOrEqual(420);
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(375);
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(375);
});

test("the mini panel acts out a review and hands it off", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  const mini = page.locator("#mini");
  const count = mini.locator(".mini-count");
  const badge = mini.locator(".mini-badge");
  await expect(count).toHaveText("2");
  await expect(mini).toHaveAttribute("data-step", "enter", { timeout: 4000 });
  await expect(mini.locator(".mini-row").first()).toHaveClass(/is-reviewing/);
  await expect(mini.locator(".mini-row").first()).toContainText(
    "Fix token refresh race",
  );
  await expect(badge).toHaveText("1");
  await expect(mini).toHaveAttribute("data-step", "lock", { timeout: 6000 });
  await expect(mini.locator(".mini-reticle")).toHaveClass(/is-locked/);
  await expect(mini).toHaveAttribute("data-step", "handoff", {
    timeout: 4000,
  });
  await expect(mini.locator(".mini-row").first()).not.toHaveClass(
    /is-reviewing/,
  );
  await expect(mini.locator(".mini-row").first()).toContainText(
    "Ready for you",
  );
  await expect(count).toHaveText("3");
  await expect(badge).toHaveText("0");
  await expect(mini.locator(".mini-note")).toHaveClass(/is-shown/, {
    timeout: 4000,
  });
  // The loop resets and the next pull request comes in.
  await expect(mini).toHaveAttribute("data-cycle", "1", { timeout: 8000 });
  await expect(count).toHaveText("2");
  await expect(mini.locator(".mini-row")).toHaveCount(2);
  await expect(mini).toHaveAttribute("data-step", "enter", { timeout: 4000 });
  await expect(mini.locator(".mini-row").first()).toContainText(
    "Escape markdown in titles",
  );
});

test("the mini panel pauses off-screen", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  const mini = page.locator("#mini");
  await expect(mini).toHaveClass(/is-running/);
  await page.locator("#panel-changelog").scrollIntoViewIfNeeded();
  await expect(mini).not.toHaveClass(/is-running/);
  const before = await step(page);
  const cycle = await mini.getAttribute("data-cycle");
  await page.waitForTimeout(4500);
  expect(await step(page)).toBe(before);
  expect(await mini.getAttribute("data-cycle")).toBe(cycle);
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(mini).toHaveClass(/is-running/);
  await expect.poll(() => step(page), { timeout: 8000 }).not.toBe(before);
});

test.describe("reduced motion mini panel", () => {
  test.use({ reducedMotion: "reduce" });

  test("shows a still of the lock-on", async ({ page }) => {
    await open(page);
    const mini = page.locator("#mini");
    await expect(mini).toHaveAttribute("data-step", "static");
    await expect(mini.locator(".mini-reticle")).toHaveClass(/is-locked/);
    await expect(mini.locator(".mini-row")).toHaveCount(3);
    await page.waitForTimeout(2000);
    await expect(mini).toHaveAttribute("data-step", "static");
    expect(
      await mini
        .locator(".mini-spin")
        .evaluate((node) => getComputedStyle(node).animationName),
    ).toBe("none");
  });
});

test("the demo page shows the full desktop", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  await page.getByRole("link", { name: "Try the full demo ↗" }).click();
  await expect(page).toHaveURL(/\/demo\.html$/);
  await expect(page.locator("html")).toHaveAttribute("data-thematic", "off");
  const home = page.getByRole("link", { name: "dylan mccurry", exact: true });
  await expect(home).toHaveAttribute("href", HOME);
  await expect(page.locator(".head nav a[aria-current=page]")).toHaveText(
    "demo",
  );
  await expect(
    page.locator('.head nav a[href="index.html#why"]'),
  ).toBeVisible();
  const frame = page.locator("#demo-frame");
  await expect(frame).toHaveAttribute("src", "demo/index.html");
  const demo = page.frameLocator("#demo-frame");
  await expect(demo.locator("body")).not.toBeEmpty();
  // Fills the window below the header.
  const header = await page.locator(".head").boundingBox();
  const box = await page.locator("#demo-viewport").boundingBox();
  expect(box.y).toBeGreaterThan(header.y + header.height);
  expect(box.y + box.height).toBeLessThanOrEqual(900);
  expect(box.width).toBeGreaterThan(1200);
});

test("the demo page's thematic switch opens the terminal DEMO tab", async ({
  page,
}) => {
  await page.goto("/demo.html");
  await page.locator("#thematic-btn").click();
  await expect(page).toHaveURL(/\/index\.html#demo$/);
  await expect(page.locator("html")).toHaveAttribute("data-thematic", "on");
  await expect(page.locator("#boot")).toBeHidden();
  await expectSection(page, "demo");
});

test("the changelog shows the latest release and links the full one", async ({
  page,
}) => {
  await open(page);
  const latest = page.locator("#changelog-latest");
  await expect(latest.locator("h3")).toHaveCount(1);
  await expect(latest.locator("h3")).toContainText(version);
  await expect(latest).not.toContainText("Unreleased");
  await expect(latest.locator("li").first()).toBeVisible();
  await expect(page.locator("#changelog-body")).toBeHidden();
  const more = page.getByRole("link", { name: "Full changelog ↗" });
  await expect(more).toBeVisible();
  await expect(more).toHaveAttribute("href", FULL_CHANGELOG);
});

test("toggling switches between the landing page and the terminal", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  const button = page.locator("#thematic-btn");
  await button.click();
  await expect(page.locator("html")).toHaveAttribute("data-thematic", "on");
  await expect(page.locator(".hero")).toBeHidden();
  await expect(page.locator('[role="tablist"]')).toBeVisible();
  await expect(page.locator('[role="tabpanel"]')).toHaveCount(5);
  await expect(page.locator("#panel-what #install")).toHaveCount(1);
  await expect(page.locator(".info")).toBeVisible();
  await expectSection(page, "what");
  await page.locator("#tab-why").click();
  await expectSection(page, "why");
  await blur(page);
  await page.keyboard.press("3");
  await expectSection(page, "how");
  await button.click();
  await expectLanding(page);
  await expect(page.locator("#panel-what #install")).toHaveCount(0);
  await button.click();
  await expectSection(page, "how");
  await page.locator("#tab-what").click();
  await expectSection(page, "what");
  await expect(page.locator("#panel-what #install .copy")).toHaveCount(4);
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
  await expect(page.locator(".head #thematic-btn")).toBeVisible();
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
  const link = page.locator('.head nav a[href="#what"]');
  // WebKit skips links on Tab by default, so focus after a key press.
  await page.keyboard.press("Shift");
  await link.focus();
  await expect(link).toBeFocused();
  expect(
    await link.evaluate((node) => {
      const style = getComputedStyle(node);
      return [style.outlineStyle, style.outlineColor];
    }),
  ).toEqual(["solid", "rgb(136, 192, 208)"]);
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
        const hero = document.querySelector(".hero");
        const tabs = document.querySelector(".tabs");
        const demo = document.getElementById("panel-demo");
        const what = document.getElementById("panel-what");
        if (!screen || !hero || !tabs || !demo || !what) {
          requestAnimationFrame(firstFrame);
          return;
        }
        window.first = {
          bg: getComputedStyle(screen).backgroundColor,
          hero: getComputedStyle(hero).display,
          tabs: getComputedStyle(tabs).display,
          demoShown: getComputedStyle(demo).display,
          whyShown: getComputedStyle(document.getElementById("panel-why"))
            .display,
        };
      };
      requestAnimationFrame(firstFrame);
    }, saved);
    await page.goto("/");
    await page.waitForFunction(() => window.first);
    expect(await page.evaluate(() => window.atBody)).toBe(saved);
    const first = await page.evaluate(() => window.first);
    if (saved === "on") {
      expect(first.bg).toBe(P1_BG);
      expect(first.hero).toBe("none");
      expect(first.tabs).toBe("flex");
      expect(first.whyShown).toBe("none");
    } else {
      expect(first.bg).toBe(NORD_BG);
      expect(first.hero).toBe("grid");
      expect(first.tabs).toBe("none");
      expect(first.demoShown).toBe("none");
      expect(first.whyShown).toBe("block");
    }
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
  await open(page, { thematic: true });
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
  await open(page, { thematic: true });
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

  test("anchors jump without smooth scrolling", async ({ page }) => {
    await open(page);
    expect(
      await page.evaluate(
        () => getComputedStyle(document.documentElement).scrollBehavior,
      ),
    ).toBe("auto");
  });
});

test("changelog renders versions from CHANGELOG.md", async ({ page }) => {
  await open(page, { thematic: true });
  await page.locator("#tab-changelog").click();
  const headings = page.locator("#changelog-body h3");
  await expect(headings.filter({ hasText: version }).first()).toBeVisible();
  const links = await page
    .locator("#changelog-body a")
    .evaluateAll((nodes) => nodes.map((node) => node.href));
  expect(links.length).toBeGreaterThan(0);
  for (const href of links) expect(href).toMatch(/^https:\/\//);
  expect(
    await page.locator("#panel-changelog script, #panel-changelog img").count(),
  ).toBe(0);
});

for (const thematic of [false, true]) {
  test(`install steps each copy their command, thematic ${thematic ? "on" : "off"}`, async ({
    page,
    context,
    browserName,
  }) => {
    if (browserName === "chromium") {
      await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    }
    await open(page, { thematic });
    const install = page.locator("#install");
    await expect(install).toBeVisible();
    const steps = install.locator(".install > li");
    await expect(steps).toHaveCount(6);
    const buttons = install.locator(".copy");
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

  test(`platform switcher toggles between macOS and Windows install commands, thematic ${thematic ? "on" : "off"}`, async ({
    page,
    context,
    browserName,
  }) => {
    if (browserName === "chromium") {
      await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await open(page, { thematic });
    const install = page.locator("#install");
    const winTab = thematic
      ? page.locator('.info .os-tab[data-os="windows"]')
      : page.locator('.hero .os-tab[data-os="windows"]');
    const macTab = install.locator('.os-tab[data-os="macos"]');
    await expect(winTab).toHaveAttribute("aria-pressed", "false");
    await expect(macTab).toHaveAttribute("aria-pressed", "true");

    await winTab.click();
    await expect(page.locator("html")).toHaveAttribute("data-os", "windows");
    for (const btn of await page.locator('.os-tab[data-os="windows"]').all()) {
      await expect(btn).toHaveAttribute("aria-pressed", "true");
    }
    for (const btn of await page.locator('.os-tab[data-os="macos"]').all()) {
      await expect(btn).toHaveAttribute("aria-pressed", "false");
    }
    await expect(page.locator(".hero-cmd code")).toHaveText(
      "choco install pr-sniper",
    );
    await expect(page.locator(".info .cmd code")).toHaveText(
      "choco install pr-sniper",
    );
    await expect(page.locator("#info-host")).toHaveText("Windows 10/11 · x64");
    await expect(install).toContainText("Windows 10 or 11 (x64)");
    await expect(install).toContainText("NO CHOCOLATEY");

    const buttons = install.locator(".copy");
    await expect(buttons).toHaveCount(4);
    const expectedWindows = [
      "choco install pr-sniper",
      'Start-Process "$env:LOCALAPPDATA\\PR Sniper\\pr-sniper.exe"',
      "choco upgrade pr-sniper",
      "choco uninstall pr-sniper",
    ];
    for (const [index, command] of expectedWindows.entries()) {
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

    await macTab.click();
    await expect(page.locator("html")).toHaveAttribute("data-os", "macos");
    await expect(page.locator(".hero-cmd code")).toHaveText(
      "brew install --cask jdylanmc/pr-sniper/pr-sniper",
    );
    await expect(page.locator(".info .cmd code")).toHaveText(
      "brew install --cask jdylanmc/pr-sniper/pr-sniper",
    );
    await expect(page.locator("#info-host")).toHaveText(
      "macOS 13.5+ · Apple Silicon",
    );
  });
}

test("shown version matches package.json", async ({ page }) => {
  await open(page);
  await expect(page.locator("#version")).toHaveText(`v${version}`);
  await expect(page.locator("#hero-version")).toHaveText(`v${version}`);
});

for (const thematic of [false, true]) {
  test(`stays usable at 375px, thematic ${thematic ? "on" : "off"}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await open(page, { thematic });
    const button = page.locator(".head #thematic-btn");
    await expect(button).toBeVisible();
    await expect(
      page.getByRole("link", { name: "dylan mccurry" }),
    ).toBeVisible();
    // The toggle shares the header's first row with the home link.
    const home = await page
      .getByRole("link", { name: "dylan mccurry" })
      .boundingBox();
    const toggle = await button.boundingBox();
    if (!thematic) expect(Math.abs(toggle.y - home.y)).toBeLessThan(16);
    expect(toggle.x + toggle.width).toBeLessThanOrEqual(375);
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(375);
    if (thematic) {
      await page.locator("#tab-demo").click();
      await expect(page.locator("#demo-frame")).toBeVisible();
    } else {
      await expect(page.locator(".hero figure.mini")).toBeVisible();
    }
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth),
    ).toBeLessThanOrEqual(375);
  });
}

test("demo is the last tab and loads the v2 prototype only when opened", async ({
  page,
}) => {
  await open(page, { thematic: true });
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
      await openDemo(page, thematic);
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
      // The demo page fits the desktop in the window.
      if (!thematic) expect(box.y + box.height).toBeLessThanOrEqual(height);
    });
  }
}

for (const thematic of [false, true]) {
  test(`full screen fills the window at 16:9 and Esc closes it, thematic ${thematic ? "on" : "off"}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openDemo(page, thematic);
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
    // The demo page already fills the window, so it grows less there.
    await expect
      .poll(async () => (await viewport.boundingBox()).width)
      .toBeGreaterThan(before.width + (thematic ? 100 : 20));
    const full = await viewport.boundingBox();
    expect(Math.abs(full.width / full.height - 16 / 9)).toBeLessThan(0.02);
    // Tab keys are ignored while full screen.
    await page.keyboard.press("1");
    if (thematic) {
      await expect(page.locator("#tab-demo")).toHaveAttribute(
        "aria-selected",
        "true",
      );
    }
    await page.keyboard.press("Escape");
    await expect(button).toHaveAttribute("aria-pressed", "false");
    await expect(button).toHaveAccessibleName("Full screen");
    await expect
      .poll(async () => Math.round((await viewport.boundingBox()).width))
      .toBe(Math.round(before.width));
  });
}

test("the demo is only the desktop, with no caption row", async ({ page }) => {
  for (const thematic of [false, true]) {
    await openDemo(page, thematic);
    await expect(page.locator(".demo-caption")).toHaveCount(0);
    await expect(page.locator("body")).not.toContainText("Fake data");
    const stage = page.locator(thematic ? "#panel-demo" : "main");
    await expect(stage.locator(":scope > .demo-stage")).toHaveCount(1);
    await expect(
      stage.locator(":scope > :not(.demo-stage):not(.sr-only)"),
    ).toHaveCount(0);
  }
});

for (const thematic of [false, true]) {
  test(`Esc inside the demo closes the full screen overlay, thematic ${thematic ? "on" : "off"}`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      // Force the overlay fallback, as on browsers without the Fullscreen API.
      Element.prototype.requestFullscreen = undefined;
      Element.prototype.webkitRequestFullscreen = undefined;
    });
    await openDemo(page, thematic);
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
}

test("switching tabs redraws the new section once", async ({ page }) => {
  await open(page, { thematic: true });
  await expect(page.locator("#panel-what")).not.toHaveClass(/redraw/);
  await page.locator("#tab-why").click();
  await expect(page.locator("#panel-why")).toHaveClass(/redraw/);
  await expect(page.locator("#panel-why")).not.toHaveClass(/redraw/, {
    timeout: 2000,
  });
});

test.describe("reduced motion tabs", () => {
  test.use({ reducedMotion: "reduce" });

  test("switch without the redraw", async ({ page }) => {
    await open(page, { thematic: true });
    await page.locator("#tab-how").click();
    await expect(page.locator("#panel-how")).toBeVisible();
    await expect(page.locator("#panel-how")).not.toHaveClass(/redraw/);
  });
});
