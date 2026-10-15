import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

const theme = (page) => page.locator("#theme");
const root = (page) => page.locator("html");
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

async function openPanel(page, store) {
  await store("snapshot");
  await store("panel_snapshot");
  await page.goto("/");
  await page.evaluate(() => window.__settingsIdle());
}

async function openPreferences(page) {
  await tab(page, "Settings").click();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await page.evaluate(() => window.__settingsIdle());
  await expect(theme(page)).toBeEnabled();
}

async function choose(page, value) {
  await theme(page).selectOption(value);
  await page.evaluate(() => window.__settingsIdle());
  await expect(theme(page)).toBeEnabled();
}

const styles = (locator, properties) =>
  locator.evaluate(
    (element, properties) =>
      Object.fromEntries(
        properties.map((p) => [p, getComputedStyle(element)[p]]),
      ),
    properties,
  );

// Computed values captured from the pre-theme panel on main; Light must not move.
const lightReference = [
  [
    ".panel-shell",
    { color: "rgb(32, 43, 59)", backgroundColor: "rgb(239, 242, 245)" },
  ],
  [
    ".panel-header",
    { color: "rgb(255, 255, 255)", backgroundColor: "rgb(36, 50, 71)" },
  ],
  [
    ".panel-tabs",
    {
      backgroundColor: "rgb(255, 255, 255)",
      borderTopColor: "rgb(229, 233, 238)",
    },
  ],
  [
    '[data-panel-tab="queue"]',
    { color: "rgb(169, 74, 22)", backgroundColor: "rgb(255, 240, 228)" },
  ],
  ['[data-panel-tab="running"]', { color: "rgb(82, 100, 123)" }],
  [
    ".queue-metric[aria-pressed=true]",
    { color: "rgb(32, 43, 59)", backgroundColor: "rgb(246, 135, 67)" },
  ],
  [
    ".queue-metric[aria-pressed=false]",
    {
      backgroundColor: "rgb(255, 255, 255)",
      borderTopColor: "rgb(227, 231, 236)",
    },
  ],
  [
    ".queue-state",
    { color: "rgb(148, 68, 16)", backgroundColor: "rgb(255, 240, 224)" },
  ],
];

async function expectLightReference(page) {
  for (const [selector, expected] of lightReference) {
    const element = page.locator(selector).first();
    // Route and active-tab transitions settle before the colors are compared.
    await expect
      .poll(() => styles(element, Object.keys(expected)), { message: selector })
      .toEqual(expected);
  }
}

// WCAG 2.x contrast of every visible text run against its composited background.
function contrastFailures(page) {
  return page.evaluate(() => {
    const parse = (value) => {
      const m = value.match(/rgba?\(([^)]+)\)/);
      if (!m) return null;
      const [r, g, b, a = 1] = m[1]
        .split(/[ ,/]+/)
        .filter(Boolean)
        .map(Number);
      return { r, g, b, a };
    };
    const over = (top, under) => ({
      r: top.r * top.a + under.r * (1 - top.a),
      g: top.g * top.a + under.g * (1 - top.a),
      b: top.b * top.a + under.b * (1 - top.a),
      a: 1,
    });
    const background = (element) => {
      const layers = [];
      for (let e = element; e; e = e.parentElement) {
        const style = getComputedStyle(e);
        // Icon images (url) do not sit behind text; gradients do.
        if (style.backgroundImage.includes("gradient")) return null;
        const color = parse(style.backgroundColor);
        if (color && color.a > 0) {
          layers.push(color);
          if (color.a === 1) break;
        }
      }
      return layers.reduceRight((under, top) => over(top, under), {
        r: 255,
        g: 255,
        b: 255,
        a: 1,
      });
    };
    const luminance = ({ r, g, b }) => {
      const c = (x) => {
        x /= 255;
        return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b);
    };
    const ratio = (a, b) => {
      const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
      return (hi + 0.05) / (lo + 0.05);
    };
    const failures = [];
    const walker = document.createTreeWalker(
      document.body,
      NodeFilter.SHOW_TEXT,
    );
    const seen = new Set();
    const runs = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode())
      if (node.textContent.trim() && node.parentElement)
        runs.push([node.parentElement, node.textContent]);
    // Field values are not text nodes; check their rendered value too.
    for (const field of document.querySelectorAll(
      "input:not([type=checkbox]):not([type=radio]):not([type=hidden]), select, textarea",
    ))
      if (field.value) runs.push([field, field.value]);
    for (const [element, text] of runs) {
      if (seen.has(element)) continue;
      seen.add(element);
      if (element.closest("[hidden], [aria-hidden=true], option")) continue;
      // Disabled controls are exempt from WCAG 1.4.3.
      if (element.closest(":disabled, [aria-disabled=true]")) continue;
      const rect = element.getBoundingClientRect();
      if (!rect.width || !rect.height) continue;
      const style = getComputedStyle(element);
      if (style.visibility !== "visible" || Number(style.opacity) === 0)
        continue;
      let faded = false;
      for (let e = element; e; e = e.parentElement)
        if (Number(getComputedStyle(e).opacity) < 1) faded = true;
      if (faded) continue;
      const clip = element.closest(".visually-hidden, .sr-only");
      if (clip) continue;
      const back = background(element);
      const fore = parse(style.color);
      if (!back || !fore) continue;
      const size = parseFloat(style.fontSize);
      const large =
        size >= 24 || (size >= 18.66 && Number(style.fontWeight) >= 700);
      const value = ratio(over(fore, back), back);
      if (value < (large ? 3 : 4.5))
        failures.push(
          `${value.toFixed(2)} ${element.tagName.toLowerCase()}.${element.className} "${text.trim().slice(0, 30)}" ${style.color} on rgb(${back.r},${back.g},${back.b})`,
        );
    }
    return failures;
  });
}

for (const viewport of [
  { width: 408, height: 744 },
  { width: 320, height: 600 },
]) {
  test.describe(`${viewport.width}x${viewport.height}`, () => {
    test.use({ viewport });

    test("Preferences offers Light, Dark and OS themes and defaults to Light", async ({
      page,
      store,
    }) => {
      await queueFixture(store);
      await page.emulateMedia({ colorScheme: "dark" });
      await openPanel(page, store);

      await expect(root(page)).toHaveAttribute("data-theme", "light");
      await expectLightReference(page);
      await openPreferences(page);
      const row = page.getByRole("group", { name: "Appearance" });
      await expect(
        row.getByRole("combobox", { name: "Theme", exact: true }),
      ).toHaveValue("light");
      expect(
        await theme(page)
          .locator("option")
          .evaluateAll((options) =>
            options.map((option) => [option.value, option.textContent]),
          ),
      ).toEqual([
        ["light", "Light"],
        ["dark", "Dark"],
        ["os", "OS"],
      ]);
      const label = await row.locator(".setting-row > span").boundingBox();
      const select = await theme(page).boundingBox();
      expect(label.height).toBeLessThan(30);
      expect(select.x + select.width).toBeLessThanOrEqual(viewport.width);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
    });

    test("Dark applies immediately, persists through the Store and survives reload", async ({
      page,
      store,
    }) => {
      await queueFixture(store);
      await openPanel(page, store);
      await openPreferences(page);

      await choose(page, "dark");

      await expect(root(page)).toHaveAttribute("data-theme", "dark");
      await expect(page.locator("#save-settings")).toBeDisabled();
      expect((await store("snapshot")).settings.theme).toBe("dark");
      expect(
        await styles(page.locator(".panel-shell"), [
          "backgroundColor",
          "color",
          "colorScheme",
        ]),
      ).toEqual({
        backgroundColor: "rgb(17, 23, 31)",
        color: "rgb(227, 233, 241)",
        colorScheme: "dark",
      });
      expect(
        await styles(
          page.locator(".preferences-view .settings-group").first(),
          ["backgroundColor"],
        ),
      ).toEqual({ backgroundColor: "rgb(28, 37, 49)" });

      // The paint hint is cleared so only the native Store can restore Dark.
      await page.evaluate(() => localStorage.clear());
      await page.reload();
      await page.evaluate(() => window.__settingsIdle());
      await expect(root(page)).toHaveAttribute("data-theme", "dark");
      await openPreferences(page);
      await expect(theme(page)).toHaveValue("dark");

      await choose(page, "light");
      await expect(root(page)).toHaveAttribute("data-theme", "light");
      expect((await store("snapshot")).settings.theme).toBeUndefined();
      await tab(page, "Queue").click();
      await expectLightReference(page);
    });

    test("OS follows live appearance changes while Light and Dark ignore them", async ({
      page,
      store,
    }) => {
      await queueFixture(store);
      await page.emulateMedia({ colorScheme: "light" });
      await openPanel(page, store);
      await openPreferences(page);

      await choose(page, "os");
      await expect(root(page)).toHaveAttribute("data-theme", "light");
      await page.emulateMedia({ colorScheme: "dark" });
      await expect(root(page)).toHaveAttribute("data-theme", "dark");
      await page.emulateMedia({ colorScheme: "light" });
      await expect(root(page)).toHaveAttribute("data-theme", "light");
      expect((await store("snapshot")).settings.theme).toBe("os");

      await choose(page, "dark");
      await page.emulateMedia({ colorScheme: "light" });
      await expect(root(page)).toHaveAttribute("data-theme", "dark");
      await choose(page, "light");
      await page.emulateMedia({ colorScheme: "dark" });
      await expect(root(page)).toHaveAttribute("data-theme", "light");
    });
  });
}

test.describe("contrast", () => {
  test.use({ viewport: { width: 408, height: 744 } });

  test("Dark keeps WCAG AA text contrast across the panel", async ({
    page,
    store,
  }) => {
    test.setTimeout(90_000);
    await queueFixture(store);
    const { settings } = await store("snapshot");
    await store("seed_settings", {
      ...settings,
      theme: "dark",
      voices: [{ title: "Plain", body: "Say it plainly." }],
    });
    await openPanel(page, store);
    await expect(root(page)).toHaveAttribute("data-theme", "dark");

    const failures = {};
    const check = async (name) => {
      await page.evaluate(() => window.__settingsIdle());
      const found = await contrastFailures(page);
      if (found.length) failures[name] = found;
    };
    await check("queue");
    await page.locator(".queue-card-open:visible").first().click();
    await check("queue evidence");
    await page.locator("[data-panel-back]").click();
    await page.locator('[data-queue-filter="attention"]').click();
    await check("queue attention");
    await tab(page, "Running").click();
    await check("running");
    await tab(page, "Reviewed").click();
    await check("reviewed");
    await page.locator("[data-panel-version]").click();
    await check("changelog");
    await page.locator("[data-panel-back]").click();
    await tab(page, "Settings").click();
    await check("settings");
    for (const name of [
      "Preferences",
      "Repositories",
      "Agents",
      "Doctrines",
      "Voices",
      "Accounts",
    ]) {
      await section(page, name);
      await check(name);
    }
    await section(page, "Preferences");
    // Native-gated rows: reveal the per-type switches and alert row to check their colors.
    await page.evaluate(() => {
      document.querySelector("#notification-types").hidden = false;
      const alert = document.querySelector("#notification-error");
      alert.textContent = "Synthetic notification alert";
      alert.hidden = false;
      const problem = document.querySelector("#notification-permission");
      problem.querySelector("[data-notification-alert]").textContent =
        "Blocked by macOS";
      problem.hidden = false;
    });
    await check("notification types and alert");
    await section(page, "Voices");
    await page.locator("#new-voice").click();
    await check("voice editor");
    await section(page, "Agents");
    await page
      .locator(".settings-window [data-edit]")
      .first()
      .evaluate((button) => button.click());
    await check("agent editor");
    await page.locator("[data-open-voices]").click();
    await check("agent voices");
    await page.locator("[data-panel-back]").click();
    await page.locator("[data-open-doctrines]").click();
    await check("agent doctrines");
    await section(page, "Repositories");
    await page.locator("[data-repository]").first().click();
    await check("repository dialog");
    await page
      .locator("[data-edit][aria-label='Edit assignment']")
      .first()
      .evaluate((button) => button.click());
    await check("assignment editor");
    await page.locator("[data-open-duck]").click();
    await check("rubber duck choices");
    const back = page.locator("[data-panel-back]");
    while (await back.isVisible()) await back.click();
    await page
      .locator("[data-context-genie]:visible, [data-home-genie]:visible")
      .first()
      .click();
    await expect(page.locator(".genie-card").first()).toBeVisible();
    await check("genie");
    expect(failures).toEqual({});
  });

  test("Dark keeps AA on a failed handoff with Fix and Retry", async ({
    page,
    store,
  }) => {
    const fixture = await queueFixture(store);
    fixture.state.publications.find(
      (entry) => entry.review.job.number === 3,
    ).operation.failure = "credentials";
    await store("seed_queue_state", fixture.state);
    const { settings } = await store("snapshot");
    await store("seed_settings", { ...settings, theme: "dark" });
    await openPanel(page, store);
    await showQueueGroup(page, "attention");
    const failed = page.getByRole("article", {
      name: "example/repo #3",
      exact: true,
    });
    await expect(failed.locator("[data-fix-account]")).toHaveText("Fix");
    expect(await contrastFailures(page)).toEqual([]);

    await failed.locator(".queue-card-open").click();
    const retry = page.locator(".queue-status-actions .queue-secondary");
    await expect(retry).toHaveText("Retry");
    await expect(page.locator('[data-queue-primary="fix"]')).toBeVisible();
    expect(await contrastFailures(page)).toEqual([]);

    // Light keeps the pre-theme literals for these controls.
    await page.evaluate(
      () => (document.documentElement.dataset.theme = "light"),
    );
    expect(await styles(retry, ["color", "backgroundColor"])).toEqual({
      color: "rgb(36, 50, 71)",
      backgroundColor: "rgb(255, 255, 255)",
    });
    expect(
      await styles(page.locator('[data-queue-primary="fix"]'), [
        "color",
        "backgroundColor",
      ]),
    ).toEqual({
      color: "rgb(255, 255, 255)",
      backgroundColor: "rgb(232, 118, 29)",
    });
  });

  test("Dark focus indicators keep 3:1 against their surface", async ({
    page,
    store,
  }) => {
    await queueFixture(store);
    const { settings } = await store("snapshot");
    await store("seed_settings", { ...settings, theme: "dark" });
    await openPanel(page, store);
    const ratio = (control) =>
      control.evaluate((element) => {
        const parse = (v) =>
          v
            .match(/\d+(\.\d+)?/g)
            .slice(0, 3)
            .map(Number);
        const lum = (rgb) =>
          rgb
            .map((x) => {
              x /= 255;
              return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
            })
            .reduce((sum, x, i) => sum + x * [0.2126, 0.7152, 0.0722][i], 0);
        const style = getComputedStyle(element);
        // An inset ring (negative offset) sits on the control's own surface.
        let e =
          parseFloat(style.outlineOffset) < 0 ? element : element.parentElement;
        while (
          e &&
          /rgba\(0, 0, 0, 0\)|transparent/.test(
            getComputedStyle(e).backgroundColor,
          )
        )
          e = e.parentElement;
        const [a, b] = [
          lum(parse(style.outlineColor)),
          lum(parse(getComputedStyle(e).backgroundColor)),
        ].sort((x, y) => y - x);
        return { style: style.outlineStyle, ratio: (a + 0.05) / (b + 0.05) };
      });
    const ratios = [];
    for (const selector of [
      ".queue-card-open",
      '[data-panel-tab="running"]',
      "[data-panel-hide]",
    ]) {
      const control = page.locator(`${selector}:visible`).first();
      await control.focus();
      ratios.push(await ratio(control));
    }
    await tab(page, "Settings").click();
    await section(page, "Voices");
    await page.locator("#new-voice").click();
    const focusRing = async (selector) => {
      const control = page.locator(selector).first();
      await control.focus();
      ratios.push(await ratio(control));
    };
    await focusRing(".shared-resource-editor input:visible");
    await focusRing(".shared-resource-editor textarea:visible");
    await section(page, "Agents");
    await page
      .locator(".settings-window [data-edit]")
      .first()
      .evaluate((button) => button.click());
    await focusRing(".shared-resource-editor .agent-section textarea:visible");
    for (const { style, ratio } of ratios) {
      expect(style, JSON.stringify(ratios)).not.toBe("none");
      expect(ratio).toBeGreaterThanOrEqual(3);
    }
  });
});

test("a rejected theme save restores the previous theme and reports it", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "save_theme"
        ? Promise.reject("Synthetic theme save failure.")
        : original(command, args);
  });
  await queueFixture(store);
  await openPanel(page, store);
  await openPreferences(page);

  await choose(page, "dark");

  await expect(root(page)).toHaveAttribute("data-theme", "light");
  await expect(theme(page)).toHaveValue("light");
  await expect(page.locator("#theme-error")).toHaveText(
    "Synthetic theme save failure.",
  );
  expect((await store("snapshot")).settings.theme).toBeUndefined();
});
