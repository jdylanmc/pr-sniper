import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });
const reference = new URL(
  "../../prototypes/v2/evidence/configured-queue.png",
  import.meta.url,
);
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

async function scenario(store, name) {
  const fixture = await queueFixture(store);
  if (name === "empty") {
    fixture.state = {
      jobs: [],
      reviews: [],
      publications: [],
      follow_ups: [],
    };
  } else if (name === "attention") {
    for (const key of ["jobs", "reviews", "publications"])
      fixture.state[key] = fixture.state[key].filter(
        (entry) => (entry.job ?? entry.review?.job ?? entry).number === 3,
      );
  }
  await store("seed_queue_state", fixture.state);
  if (name === "empty")
    await store("panel_navigate", { route: { tab: "running" } });
  return fixture;
}

async function capture(page, testInfo, name) {
  await page.evaluate(() => window.__settingsIdle());
  await page.locator(".panel-header strong").click();
  const bytes = await page.screenshot({
    path: testInfo.outputPath(`${name}.png`),
  });
  await testInfo.attach(name, { body: bytes, contentType: "image/png" });
  if (/^queue-(empty|populated|attention)$/.test(name)) {
    const approved = await readFile(reference);
    const comparison = await page.evaluate(
      async ({ approved, actual }) => {
        async function load(data) {
          const image = new Image();
          image.src = `data:image/png;base64,${data}`;
          await image.decode();
          return image;
        }
        const canvas = document.createElement("canvas");
        canvas.width = 816;
        canvas.height = 744;
        const context = canvas.getContext("2d");
        context.drawImage(
          await load(approved),
          813,
          42,
          408,
          744,
          0,
          0,
          408,
          744,
        );
        context.drawImage(await load(actual), 408, 0);
        return canvas.toDataURL("image/png").split(",")[1];
      },
      {
        approved: approved.toString("base64"),
        actual: bytes.toString("base64"),
      },
    );
    await writeFile(
      testInfo.outputPath(`${name}-reference-left-actual-right.png`),
      Buffer.from(comparison, "base64"),
    );
  }
  return bytes;
}

async function visualMeasurements(page, image) {
  const approved = await readFile(reference);
  return page.evaluate(
    async ({ approved, actual }) => {
      async function pixels(data) {
        const image = new Image();
        image.src = `data:image/png;base64,${data}`;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = image.width;
        canvas.height = image.height;
        const context = canvas.getContext("2d");
        context.drawImage(image, 0, 0);
        return (x, y) => [...context.getImageData(x, y, 1, 1).data].slice(0, 3);
      }
      const ref = await pixels(approved);
      const current = await pixels(actual);
      const box = (selector) => {
        const bounds = document.querySelector(selector).getBoundingClientRect();
        return {
          x: bounds.x,
          y: bounds.y,
          width: bounds.width,
          height: bounds.height,
        };
      };
      const ready = box("[data-queue-ready-card]");
      const attention = box("[data-queue-attention-card]");
      const list = box("#handoff-queue");
      const card = document.querySelector(".queue-item");
      const readyBadge = document.querySelector(
        '.queue-item[data-state="machine_signed_off"] .queue-state',
      );
      // The approved 1440x960 desktop capture contains a 408x744 panel
      // at (813,42). Compare its actual image colors and compact landmarks,
      // not the obsolete sample counts, Agent lane or desktop silhouette.
      return {
        ready,
        attention,
        list,
        header: box(".panel-context"),
        firstCard: document.querySelector(".queue-item")
          ? box(".queue-item")
          : box(".queue-empty"),
        cardStyle: card
          ? {
              radius: getComputedStyle(card).borderRadius,
              titleSize: getComputedStyle(card.querySelector("h3")).fontSize,
              referenceSize: getComputedStyle(
                card.querySelector(".queue-reference"),
              ).fontSize,
              badgeSize: getComputedStyle(card.querySelector(".queue-state"))
                .fontSize,
            }
          : null,
        colors: [
          { expected: ref(820, 120), actual: current(7, 78) },
          {
            expected: ref(850, 230),
            actual: current(ready.x + 8, ready.y + ready.height - 12),
          },
          {
            expected: ref(1040, 230),
            actual: current(
              attention.x + 8,
              attention.y + attention.height - 12,
            ),
          },
          { expected: ref(820, 330), actual: current(7, list.y + 35) },
          ...(readyBadge
            ? [
                {
                  expected: ref(850, 365),
                  actual: current(
                    readyBadge.getBoundingClientRect().x + 5,
                    readyBadge.getBoundingClientRect().y + 5,
                  ),
                },
              ]
            : []),
        ],
      };
    },
    {
      approved: approved.toString("base64"),
      actual: image.toString("base64"),
    },
  );
}

async function changedPixels(page, first, second) {
  return page.evaluate(
    async ({ first, second }) => {
      async function pixels(data) {
        const image = new Image();
        image.src = `data:image/png;base64,${data}`;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = image.width;
        canvas.height = image.height;
        const context = canvas.getContext("2d");
        context.drawImage(image, 0, 0);
        return context.getImageData(0, 0, canvas.width, canvas.height).data;
      }
      const a = await pixels(first);
      const b = await pixels(second);
      if (a.length !== b.length) throw new Error("Capture dimensions changed.");
      let changed = 0;
      for (let index = 0; index < a.length; index += 4)
        if (
          [0, 1, 2].some(
            (channel) => Math.abs(a[index + channel] - b[index + channel]) > 16,
          )
        )
          changed++;
      return changed / (a.length / 4);
    },
    { first: first.toString("base64"), second: second.toString("base64") },
  );
}

for (const name of ["empty", "populated", "attention"]) {
  test(`Queue ${name} preserves approved image hierarchy with actual human-only state`, async ({
    page,
    store,
  }, testInfo) => {
    await scenario(store, name);
    await page.goto("/");
    if (name === "empty") await tab(page, "Queue").click();
    await expect(page.locator("[data-panel-heading]")).toHaveText("Your queue");
    const image = await capture(page, testInfo, `queue-${name}`);
    const ready = name === "populated" ? 1 : 0;
    const attention = name === "populated" ? 2 : name === "attention" ? 1 : 0;
    await expect(page.locator("[data-queue-ready]")).toHaveText(String(ready));
    await expect(page.locator("[data-queue-attention]")).toHaveText(
      String(attention),
    );
    await expect(page.locator("#handoff-queue article")).toHaveCount(
      ready + attention,
    );
    await expect(page.getByRole("term")).toHaveText([
      "Ready for you",
      "Needs attention",
    ]);
    await expect(page.getByRole("definition")).toHaveText([
      `${ready}need your eyes`,
      `${attention}need your input`,
    ]);
    await expect(
      page.getByRole("heading", { name: "Over to you", level: 2 }),
    ).toBeVisible();
    await expect(page.locator("#handoff-queue")).not.toContainText(
      "For agents",
    );
    await expect(
      page.getByRole("button", { name: /Start review|Simulate/ }),
    ).toHaveCount(0);
    if (name === "empty") {
      await expect(
        page.getByRole("heading", { name: "You're all caught up" }),
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "View Agent work in Running" }),
      ).toBeVisible();
    }
    const measurements = await visualMeasurements(page, image);
    // Approved: summary y=116, height=91, x=18/210, width=181;
    // first card y=301, width=372. Leave explicit room for truthful
    // production captions and platform font rasterization, not a new layout.
    expect(Math.abs(measurements.ready.y - 116)).toBeLessThanOrEqual(8);
    expect(Math.abs(measurements.ready.height - 91)).toBeLessThanOrEqual(8);
    expect(Math.abs(measurements.ready.width - 181)).toBeLessThanOrEqual(3);
    expect(Math.abs(measurements.attention.x - 210)).toBeLessThanOrEqual(3);
    expect(Math.abs(measurements.firstCard.y - 301)).toBeLessThanOrEqual(18);
    expect(Math.abs(measurements.firstCard.width - 372)).toBeLessThanOrEqual(3);
    expect(measurements.firstCard.height).toBeLessThanOrEqual(190);
    if (measurements.cardStyle)
      expect(measurements.cardStyle).toEqual({
        radius: "13px",
        titleSize: "15px",
        referenceSize: "10px",
        badgeSize: "9px",
      });
    for (const color of measurements.colors)
      for (let channel = 0; channel < 3; channel++)
        expect(
          Math.abs(color.actual[channel] - color.expected[channel]),
        ).toBeLessThanOrEqual(12);
    await writeFile(
      testInfo.outputPath(`queue-${name}-measurements.json`),
      JSON.stringify(
        {
          reference: reference.pathname,
          referenceSha256: createHash("sha256")
            .update(await readFile(reference))
            .digest("hex"),
          imageSha256: createHash("sha256").update(image).digest("hex"),
          measurements,
        },
        null,
        2,
      ),
    );
    await page.reload();
    await page.evaluate(() => window.__settingsIdle());
    if (name === "empty") await tab(page, "Queue").click();
    const reloaded = await capture(page, testInfo, `queue-${name}-reloaded`);
    // Same Store and renderer must retain the image, allowing only isolated
    // rasterization differences (0.1% of pixels, 16 levels per channel).
    expect(await changedPixels(page, image, reloaded)).toBeLessThanOrEqual(
      0.001,
    );
  });
}

test("Queue keeps exact handoff, stale destination, keyboard and scroll focus across refresh and Back", async ({
  page,
  store,
}) => {
  const fixture = await scenario(store, "populated");
  await page.clock.install();
  await page.goto("/");
  const button = page
    .getByRole("article", { name: "example/repo #3", exact: true })
    .getByRole("button", { name: "Evidence and actions" });
  await button.focus();
  const scroll = await page
    .locator(".panel-content")
    .evaluate((element) => element.scrollTop);
  await page.clock.runFor(5100);
  await expect(button).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("[data-panel-heading]")).toHaveText(
    "Saved evidence",
  );
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "example/repo",
  );
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "Personal review, comments and approval happen on GitHub",
  );
  await expect(page.locator("[data-item-evidence] textarea")).toHaveCount(0);
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(button).toBeFocused();
  expect(
    await page
      .locator(".panel-content")
      .evaluate((element) => element.scrollTop),
  ).toBe(scroll);
  await button.click();
  fixture.state.jobs = [];
  fixture.state.reviews = [];
  fixture.state.publications = [];
  await store("seed_queue_state", fixture.state);
  await page.clock.runFor(5100);
  await expect(page.locator("[data-item-evidence]")).toContainText(
    "unavailable",
  );
  await expect(page.locator("#agent-reviews article")).toHaveCount(0);
});

test("Queue scales, supports forced colors and reduced motion, and leaves Agent execution in Running", async ({
  page,
  store,
}, testInfo) => {
  await scenario(store, "populated");
  await page.goto("/");
  for (const colorScheme of ["light", "dark"]) {
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await capture(page, testInfo, `queue-${colorScheme}`);
    await expect(page.locator(".queue-item")).toHaveCount(3);
  }
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });
  for (const size of [
    { width: 320, height: 300 },
    { width: 280, height: 440 },
    { width: 408, height: 744 },
  ]) {
    await page.setViewportSize(size);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await expect(tab(page, "Queue")).toBeInViewport();
    expect(
      await page
        .locator(".queue-capacity")
        .evaluate((element) => getComputedStyle(element).borderTopWidth),
    ).toBe("1px");
    const button = page.locator(".queue-card-open").last();
    await button.focus();
    await expect(button).toBeInViewport();
    expect(
      await button.evaluate(
        (element) => getComputedStyle(element).outlineStyle,
      ),
    ).not.toBe("none");
    expect(
      await button.evaluate(
        (element) => getComputedStyle(element).animationName,
      ),
    ).toBe("none");
    await capture(page, testInfo, `queue-forced-${size.width}x${size.height}`);
  }
  await page
    .getByRole("button", { name: "View Agent work in Running" })
    .click();
  await expect(page.locator("[data-panel-heading]")).toHaveText("Work queue");
});

test("unavailable Queue and capacity reads never claim zero handoffs or zero Agent work", async ({
  page,
  store,
}, testInfo) => {
  await scenario(store, "populated");
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      ["monitoring_snapshot", "automation_snapshot"].includes(command)
        ? Promise.reject("Synthetic state read unavailable")
        : original(command, args);
  });
  await page.goto("/");
  await expect(page.locator("[data-queue-ready]")).toHaveText("?");
  await expect(page.locator("[data-queue-attention]")).toHaveText("?");
  await expect(page.locator("[data-summary-detail]")).toHaveText(
    "Handoffs unavailable",
  );
  await expect(page.locator("[data-queue-work-status]")).toHaveText(
    "Work state unavailable",
  );
  await expect(page.locator("[data-panel-error]")).toBeVisible();
  await expect(page.locator(".queue-empty")).toHaveCount(0);
  await capture(page, testInfo, "queue-unavailable");
});

for (const deviceScaleFactor of [1, 2]) {
  test.describe(`Queue at ${deviceScaleFactor}x device scale`, () => {
    test.use({ deviceScaleFactor });
    test("retains readable handoff hierarchy with enlarged text", async ({
      page,
      store,
    }, testInfo) => {
      await scenario(store, "populated");
      await page.goto("/");
      await page.evaluate(() => window.__settingsIdle());
      // Enlarge the actual computed text, including explicit compact px sizes.
      await page.locator(".panel-shell").evaluate((root) => {
        const elements = [root, ...root.querySelectorAll("*")];
        const sizes = elements.map(
          (element) => parseFloat(getComputedStyle(element).fontSize) * 2,
        );
        elements.forEach((element, index) => {
          element.style.fontSize = `${sizes[index]}px`;
        });
      });
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      const button = page.locator(".queue-card-open").last();
      await button.focus();
      await expect(button).toBeInViewport();
      await expect(tab(page, "Queue")).toBeInViewport();
      await capture(page, testInfo, `queue-${deviceScaleFactor}x-text-200`);
      await button.focus();
      await page.keyboard.press("Enter");
      await expect(page.locator("[data-panel-heading]")).toHaveText(
        "Saved evidence",
      );
    });
  });
}

for (const scenario of [
  { name: "large-text-320x440", height: 440, textScale: 2, error: false },
  { name: "read-error-320x300", height: 300, textScale: 1, error: true },
  { name: "large-text-error-320x300", height: 300, textScale: 2, error: true },
]) {
  test(`constrained Queue keeps evidence and pinned recovery usable: ${scenario.name}`, async ({
    page,
    store,
  }, testInfo) => {
    await queueFixture(store);
    const nextKey =
      page.context().browser().browserType().name() === "webkit" &&
      process.platform === "darwin"
        ? "Alt+Tab"
        : "Tab";
    await page.setViewportSize({ width: 320, height: scenario.height });
    await page.clock.install();
    await page.addInitScript(() => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__queueReadFailure = false;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        window.__queueReadFailure &&
        ["monitoring_snapshot", "automation_snapshot"].includes(command)
          ? Promise.reject(
              "Could not read monitoring state. Check local storage and diagnostics; this is not an empty successful check.",
            )
          : original(command, args);
    });
    await page.goto("/");
    await page.evaluate(() => window.__settingsIdle());
    if (scenario.textScale === 2) {
      // Retain enlarged card text when real selection/refresh recreates rows.
      const rules = await page.evaluate(() =>
        [
          ".queue-item",
          ".queue-item h3",
          ".queue-state",
          ".queue-card-top",
          ".queue-reference",
          ".queue-summary-text",
          ".queue-card-footer",
          ".queue-avatar",
          ".queue-trigger",
        ]
          .map((selector) => {
            const element = document.querySelector(selector);
            return `.panel-shell ${selector} { font-size: ${parseFloat(getComputedStyle(element).fontSize) * 2}px; }`;
          })
          .join("\n"),
      );
      await page.locator(".panel-shell").evaluate((root) => {
        const elements = [root, ...root.querySelectorAll("*")];
        const sizes = elements.map(
          (element) => parseFloat(getComputedStyle(element).fontSize) * 2,
        );
        elements.forEach((element, index) => {
          element.style.fontSize = `${sizes[index]}px`;
        });
      });
      await page.addStyleTag({ content: rules });
    }
    if (scenario.error) {
      await page.evaluate(() => {
        window.__queueReadFailure = true;
      });
      await page.clock.runFor(5100);
      await page.evaluate(() => window.__settingsIdle());
      await expect(page.locator("[data-queue-ready]")).toHaveText("?");
      await expect(page.locator("[data-panel-error]")).toContainText(
        "Could not read monitoring state",
      );
    }
    const geometry = await page.evaluate(() => {
      const box = (selector) => {
        const element = document.querySelector(selector);
        const bounds = element.getBoundingClientRect();
        return {
          y: bounds.y,
          height: bounds.height,
          bottom: bounds.bottom,
          clientHeight: element.clientHeight,
          scrollHeight: element.scrollHeight,
        };
      };
      return {
        viewport: { width: innerWidth, height: innerHeight },
        summary: box(".panel-summary"),
        error: box("[data-panel-error]"),
        content: box(".panel-content"),
        navigation: box(".panel-tabs"),
        footer: box(".panel-footer"),
        controls: [
          ...document.querySelectorAll(
            ".panel-tabs button,.panel-footer button",
          ),
        ].map((element) => {
          const bounds = element.getBoundingClientRect();
          return {
            label: element.textContent,
            x: bounds.x,
            right: bounds.right,
            y: bounds.y,
            bottom: bounds.bottom,
            fontSize: getComputedStyle(element).fontSize,
          };
        }),
      };
    });
    await writeFile(
      testInfo.outputPath(`${scenario.name}-geometry.json`),
      JSON.stringify(
        {
          ...geometry,
          browser: {
            name: page.context().browser().browserType().name(),
            version: page.context().browser().version(),
          },
        },
        null,
        2,
      ),
    );
    await page.screenshot({
      path: testInfo.outputPath(`${scenario.name}.png`),
    });
    expect(geometry.content.clientHeight).toBeGreaterThanOrEqual(64);
    expect(geometry.content.scrollHeight).toBeGreaterThan(
      geometry.content.clientHeight,
    );
    for (const bounds of [geometry.navigation, geometry.footer]) {
      expect(bounds.y).toBeGreaterThanOrEqual(0);
      expect(bounds.bottom).toBeLessThanOrEqual(scenario.height);
    }
    for (const bounds of geometry.controls) {
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.right).toBeLessThanOrEqual(320);
      expect(bounds.y).toBeGreaterThanOrEqual(0);
      expect(bounds.bottom).toBeLessThanOrEqual(scenario.height);
      expect(bounds.fontSize).toBe(`${9 * scenario.textScale}px`);
    }
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    if (scenario.error) {
      await page.getByRole("button", { name: "Hide PR Sniper panel" }).focus();
      await page.keyboard.press(nextKey);
      await expect(
        page.getByRole("region", { name: "Queue content", exact: true }),
      ).toBeFocused();
      for (let step = 0; step < 12; step++) {
        const visible = await page
          .locator("[data-panel-error]")
          .evaluate((element) => {
            const bounds = element.getBoundingClientRect();
            const viewport = document
              .querySelector(".panel-content")
              .getBoundingClientRect();
            return bounds.bottom > viewport.top && bounds.top < viewport.bottom;
          });
        if (visible) break;
        const before = await page
          .locator(".panel-content")
          .evaluate((element) => element.scrollTop);
        await page.keyboard.press("PageDown");
        await page.clock.runFor(250);
        await expect
          .poll(() =>
            page
              .locator(".panel-content")
              .evaluate((element) => element.scrollTop),
          )
          .toBeGreaterThan(before + 20);
        let previous = -1;
        await expect
          .poll(async () => {
            const scroll = await page
              .locator(".panel-content")
              .evaluate((element) => element.scrollTop);
            const settled = scroll === previous;
            previous = scroll;
            return settled;
          })
          .toBe(true);
      }
      await writeFile(
        testInfo.outputPath(`${scenario.name}-reading-geometry.json`),
        JSON.stringify(
          await page.evaluate(() => {
            const region = document.querySelector(".panel-content");
            const error = document.querySelector("[data-panel-error]");
            return {
              scrollTop: region.scrollTop,
              region: region.getBoundingClientRect().toJSON(),
              error: error.getBoundingClientRect().toJSON(),
              active: document.activeElement.className,
              shellScroll: document.querySelector(".panel-shell").scrollTop,
            };
          }),
          null,
          2,
        ),
      );
      await page.screenshot({
        path: testInfo.outputPath(`${scenario.name}-reading-error.png`),
      });
      await expect(page.locator("[data-panel-error]")).toBeInViewport();
      // Recover through an ordinary pinned utility, not a forced interaction.
      await page.getByRole("button", { name: "Status", exact: true }).click();
      await expect(page.locator("[data-panel-heading]")).toHaveText("Status");
      await page.getByRole("button", { name: "Back", exact: true }).click();
      await page.evaluate(() => {
        window.__queueReadFailure = false;
      });
      await page.clock.runFor(5100);
      await page.evaluate(() => window.__settingsIdle());
    }
    const lastArticle = page.getByRole("article", {
      name: "example/repo #3",
      exact: true,
    });
    const last = lastArticle.getByRole("button", {
      name: "Evidence and actions",
      exact: true,
    });
    // Tab follows the real document order and the browser scrolls focused work.
    await page.locator("[data-panel-heading]").focus();
    for (let step = 0; step < 12; step++) {
      if (await last.evaluate((element) => element === document.activeElement))
        break;
      await page.keyboard.press(nextKey);
    }
    await expect(last).toBeFocused();
    const scroll = await page
      .locator(".panel-content")
      .evaluate((element) => element.scrollTop);
    expect(scroll).toBeGreaterThan(0);
    await page.screenshot({
      path: testInfo.outputPath(`${scenario.name}-focused-evidence.png`),
    });
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-item-evidence]")).toContainText(
      "example/repo #3",
    );
    expect(
      await page
        .locator(".panel-content")
        .evaluate((element) => element.clientHeight),
    ).toBeGreaterThanOrEqual(64);
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(last).toBeFocused();
    if (scenario.textScale === 2)
      await expect(lastArticle.locator("h3")).toHaveCSS("font-size", "30px");
    expect(
      await page
        .locator(".panel-content")
        .evaluate((element) => element.scrollTop),
    ).toBe(scroll);
    await page
      .getByRole("button", { name: "Diagnostics", exact: true })
      .click();
    await expect(page.locator("[data-panel-heading]")).toHaveText(
      "Diagnostics",
    );
    expect(
      await page
        .locator(".panel-content")
        .evaluate((element) => element.clientHeight),
    ).toBeGreaterThanOrEqual(64);
  });
}
