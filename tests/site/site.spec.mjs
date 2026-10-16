import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";

const { version } = JSON.parse(
  readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
);
const sections = ["how", "safety", "install", "changelog", "demo"];

async function open(page) {
  await page.addInitScript(() =>
    sessionStorage.setItem("termlink-booted", "1"),
  );
  await page.goto("/");
  await expect(page.locator("#tab-how")).toHaveAttribute(
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

test("tabs switch by click and keep a roving tabindex", async ({ page }) => {
  await open(page);
  await expect(
    page.locator('[role="tablist"][aria-label="Sections"]'),
  ).toBeVisible();
  await expectSection(page, "how");
  await page.locator("#tab-changelog").click();
  await expectSection(page, "changelog");
  await page.locator("#tab-demo").click();
  await expectSection(page, "demo");
  await page.locator("#tab-how").click();
  await expectSection(page, "how");
});

test("number keys and arrows switch tabs", async ({ page }) => {
  await open(page);
  await blur(page);
  await page.keyboard.press("2");
  await expectSection(page, "safety");
  await page.keyboard.press("ArrowRight");
  await expectSection(page, "install");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expectSection(page, "how");
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
  await expectSection(page, "how");
  await expect(page.locator("#tab-how")).toBeFocused();
});

test("T cycles the theme and the choice persists", async ({ page }) => {
  await open(page);
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
    await page.goto("/");
    await expect(page.locator("#boot")).toBeHidden();
    await expect(page.locator("#tab-how")).toHaveAttribute(
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

test("install steps each copy their command", async ({
  page,
  context,
  browserName,
}) => {
  if (browserName === "chromium") {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  }
  await open(page);
  await page.locator("#tab-install").click();
  const steps = page.locator("#panel-install .steps > li");
  await expect(steps).toHaveCount(6);
  const buttons = page.locator("#panel-install .copy");
  await expect(buttons).toHaveCount(4);
  const expected = [
    "brew install --cask jdylanmc/pr-sniper/pr-sniper",
    'open -a "PR Sniper"',
    "brew upgrade --cask pr-sniper",
    "brew uninstall --cask pr-sniper",
  ];
  for (const [index, command] of expected.entries()) {
    const button = buttons.nth(index);
    await expect(button.locator("xpath=..").locator("code")).toHaveText(
      command,
    );
    await button.click();
    await expect(button).toHaveText("[Copied]");
    if (browserName === "chromium") {
      expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
        command,
      );
    }
  }
  await expect(buttons.first()).toHaveText("[COPY]", { timeout: 4000 });
});

test("shown version matches package.json", async ({ page }) => {
  await open(page);
  await expect(page.locator("#version")).toHaveText(`v${version}`);
});

test("stays usable at 375px", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await open(page);
  await page.locator("#tab-demo").click();
  await expect(page.locator("#demo-frame")).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(375);
});

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
  expect(box.height).toBeGreaterThan(400);
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
