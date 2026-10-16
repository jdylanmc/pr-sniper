import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";

const { version } = JSON.parse(
  readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
);
const sections = ["demo", "how", "safety", "install", "changelog"];

async function open(page) {
  await page.addInitScript(() =>
    sessionStorage.setItem("termlink-booted", "1"),
  );
  await page.goto("/");
  await expect(page.locator("#tab-demo")).toHaveAttribute(
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

async function pauseAndRestart(page) {
  const play = page.locator("#demo-play");
  if ((await play.textContent()).includes("PAUSE")) await play.click();
  await expect(play).toContainText("PLAY");
  await page.locator("#demo-restart").click();
  await expect(page.locator("#demo-progress")).toHaveText("STEP 00/21");
}

async function step(page, count) {
  for (let i = 0; i < count; i += 1) await page.locator("#demo-step").click();
}

const blur = (page) => page.evaluate(() => document.activeElement?.blur());

test("tabs switch by click and keep a roving tabindex", async ({ page }) => {
  await open(page);
  await expect(
    page.locator('[role="tablist"][aria-label="Sections"]'),
  ).toBeVisible();
  await page.locator("#tab-how").click();
  await expectSection(page, "how");
  await page.locator("#tab-changelog").click();
  await expectSection(page, "changelog");
  await page.locator("#tab-demo").click();
  await expectSection(page, "demo");
});

test("number keys and arrows switch tabs", async ({ page }) => {
  await open(page);
  await blur(page);
  await page.keyboard.press("3");
  await expectSection(page, "safety");
  await page.keyboard.press("ArrowRight");
  await expectSection(page, "install");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expectSection(page, "how");
  await page.keyboard.press("5");
  await expectSection(page, "changelog");
  await page.keyboard.press("ArrowRight");
  await expectSection(page, "demo");
  await page.keyboard.press("ArrowLeft");
  await expectSection(page, "changelog");
  await page.keyboard.press("1");
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

  test("skips the boot and opens the demo paused mid-loop", async ({
    page,
  }) => {
    await page.goto("/");
    await expect(page.locator("#boot")).toBeHidden();
    await expect(page.locator("#demo-play")).toContainText("PLAY");
    await expect(page.locator("#demo-progress")).toHaveText("STEP 12/21");
    await page.waitForTimeout(2500);
    await expect(page.locator("#demo-progress")).toHaveText("STEP 12/21");
    await page.locator("#demo-step").click();
    await expect(page.locator("#demo-progress")).toHaveText("STEP 13/21");
  });

  test("shows banners without sliding", async ({ page }) => {
    await page.goto("/");
    await page.locator("#demo-restart").click();
    await step(page, 7);
    const banner = page.locator(".banner");
    await expect(banner).toHaveCount(1);
    expect(
      await banner.evaluate((node) => getComputedStyle(node).animationName),
    ).toBe("none");
  });
});

test("demo advances and space pauses it", async ({ page }) => {
  await open(page);
  const progress = page.locator("#demo-progress");
  await expect(page.locator("#demo-play")).toContainText("PAUSE");
  await expect(progress).not.toHaveText("STEP 00/21", { timeout: 5000 });
  await expect(page.locator("#log li").nth(1)).toContainText(
    "tick lumen/ledger",
  );
  await blur(page);
  await page.keyboard.press(" ");
  await expect(page.locator("#demo-play")).toContainText("PLAY");
  const paused = await progress.textContent();
  await page.waitForTimeout(2600);
  await expect(progress).toHaveText(paused);
  await page.keyboard.press(" ");
  await expect(page.locator("#demo-play")).toContainText("PAUSE");
});

test("panel sub-tabs switch by click and arrow keys", async ({ page }) => {
  await open(page);
  await pauseAndRestart(page);
  const tab = (name) => page.locator(`#tp-tab-${name}`);
  await tab("reviewed").click();
  await expect(tab("reviewed")).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".tp-title")).toHaveText("Reviewed");
  await expect(page.locator(".tp-list .row")).toHaveCount(2);
  await tab("reviewed").focus();
  await page.keyboard.press("ArrowLeft");
  await expect(tab("running")).toHaveAttribute("aria-selected", "true");
  await expect(tab("running")).toBeFocused();
  await expect(page.locator(".tp-title")).toHaveText("Work queue");
  await expectSection(page, "demo");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect(tab("queue")).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".tp-title")).toHaveText("Your queue");
});

test("tray icon toggles the panel and badges queue items", async ({ page }) => {
  await open(page);
  await pauseAndRestart(page);
  const tray = page.locator("#tray");
  const badge = page.locator("#tray-badge");
  await expect(tray).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator("#tray-panel")).toBeVisible();
  await expect(badge).toHaveText("1");
  await tray.click();
  await expect(tray).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator("#tray-panel")).toBeHidden();
  await tray.focus();
  await page.keyboard.press("Enter");
  await expect(tray).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator("#tray-panel")).toBeVisible();
  await step(page, 7);
  await expect(badge).toHaveText("2");
  await expect(tray).toHaveAttribute("aria-label", "PR Sniper, 2 in queue");
  await step(page, 12);
  await expect(badge).toHaveText("4");
  await step(page, 1);
  await expect(badge).toHaveText("3");
});

test("a banner fires at the Ready step and opens its row", async ({ page }) => {
  await open(page);
  await pauseAndRestart(page);
  await step(page, 6);
  await expect(page.locator(".banner")).toHaveCount(0);
  await step(page, 1);
  const banner = page.locator(".banner");
  await expect(banner).toHaveCount(1);
  await expect(banner).toContainText("Ready for your final review");
  await expect(page.locator("#announcer")).toHaveText(
    /Ready for your final review/,
  );
  await page.locator("#tp-tab-reviewed").click();
  await page.locator("#tray").click();
  await expect(page.locator("#tray-panel")).toBeHidden();
  await banner.click();
  await expect(page.locator(".banner")).toHaveCount(0);
  await expect(page.locator("#tray-panel")).toBeVisible();
  await expect(page.locator("#tp-tab-queue")).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.locator('.tp-card[data-card="ready"]')).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  const row = page.locator('[data-key="ready:p142"]');
  await expect(row).toHaveAttribute("aria-current", "true");
  await expect(row).toBeFocused();
});

test("banners pause with the demo and auto-dismiss while playing", async ({
  page,
}) => {
  await open(page);
  await pauseAndRestart(page);
  await step(page, 7);
  await expect(page.locator(".banner")).toHaveCount(1);
  await page.waitForTimeout(5600);
  await expect(page.locator(".banner")).toHaveCount(1);
  await page.locator("#demo-play").click();
  await expect(page.locator(".banner")).toHaveCount(0, { timeout: 7000 });
});

test("failure and merge banners open Needs attention and Reviewed", async ({
  page,
}) => {
  await open(page);
  await pauseAndRestart(page);
  await step(page, 19);
  await expect(page.locator(".banner").first()).toContainText(
    "PR Sniper needs attention",
  );
  await page.locator(".banner").first().click();
  await expect(page.locator('.tp-card[data-card="attention"]')).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.locator('[data-key="attention:p61"]')).toHaveAttribute(
    "aria-current",
    "true",
  );
  await step(page, 1);
  await expect(page.locator(".banner").first()).toContainText(
    "PR merged on GitHub",
  );
  expect(await page.locator(".banner").count()).toBeLessThanOrEqual(3);
  await page.locator(".banner").first().click();
  await expect(page.locator("#tp-tab-reviewed")).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.locator('[data-key="reviewed:p58"]')).toHaveAttribute(
    "aria-current",
    "true",
  );
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
  await expect(page.locator(".tp-version")).toHaveText(`v${version}`);
});

test("stays usable at 375px", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await open(page);
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(375);
  await expect(page.locator("#tray")).toBeVisible();
  await expect(page.locator(".menubar-left")).toBeHidden();
  await expect(page.locator("#tray-panel")).toBeVisible();
  await page.locator("#tp-tab-running").click();
  await expect(page.locator(".tp-title")).toHaveText("Work queue");
});
