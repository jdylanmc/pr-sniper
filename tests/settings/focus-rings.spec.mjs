import { test, expect } from "./fixtures.mjs";
import {
  closeDialog,
  fixtureAgent,
  repositorySettings,
  section,
  seedBoundRepositories,
} from "./navigation.mjs";

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const editor = (page, name) => page.getByRole("dialog", { name, exact: true });
const openCard = async (page, kind, title) => {
  await page
    .locator(`.${kind}-card`)
    .filter({ has: page.getByRole("heading", { name: title, exact: true }) })
    .getByRole("heading", { name: title, exact: true })
    .click({ force: true });
  return editor(page, `Editing ${title}`);
};

/** The focused control's painted ring and every clipping ancestor's visible box. */
async function ring(locator) {
  await locator.scrollIntoViewIfNeeded();
  await locator.evaluate((element) =>
    element.scrollIntoView({ block: "center", inline: "nearest" }),
  );
  // Keyboard modality, then focus: the ring a keyboard user sees.
  const page = locator.page();
  await page.keyboard.press("Shift");
  await locator.focus();
  return locator.evaluate((element) => {
    const style = getComputedStyle(element);
    const rect = element.getBoundingClientRect();
    // Playwright's WebKit does not report :focus-visible for checkboxes,
    // buttons and time fields (focus sits in an inner segment); measure the
    // painted ring, else the shared editor ring (3px + 2px offset).
    const assumed = !element.matches(":focus-visible");
    let spread = assumed ? 5 : 0;
    let width = assumed ? 3 : 0;
    if (style.outlineStyle !== "none") {
      width = parseFloat(style.outlineWidth);
      // An inset ring (negative offset) paints inside the element.
      spread = Math.max(spread, width + parseFloat(style.outlineOffset));
    }
    for (const shadow of style.boxShadow.split(/,(?![^(]*\))/)) {
      if (shadow === "none" || /inset/.test(shadow)) continue;
      const lengths = shadow.match(/-?[\d.]+px/g) ?? [];
      // offset-x offset-y blur spread
      const [x = 0, y = 0, , s = 0] = lengths.map(parseFloat);
      width = Math.max(width, s);
      spread = Math.max(spread, s + Math.max(Math.abs(x), Math.abs(y)));
    }
    const painted = {
      left: rect.left - spread,
      right: rect.right + spread,
      top: rect.top - spread,
      bottom: rect.bottom + spread,
    };
    const clips = [];
    for (let node = element.parentElement; node; node = node.parentElement) {
      const s = getComputedStyle(node);
      if (
        s.overflowX === "visible" &&
        s.overflowY === "visible" &&
        s.clipPath === "none" &&
        s.contain.indexOf("paint") < 0
      )
        continue;
      const box = node.getBoundingClientRect();
      // Content is clipped at the padding box.
      const left = box.left + node.clientLeft;
      const top = box.top + node.clientTop;
      clips.push({
        name: `${node.tagName.toLowerCase()}.${[...node.classList].join(".")}`,
        left,
        top,
        right: left + node.clientWidth,
        bottom: top + node.clientHeight,
      });
    }
    return {
      focusVisible:
        element.matches(":focus-visible") ||
        (assumed && /^(checkbox|submit|button)$/.test(element.type)) ||
        (element.type === "time" && element.matches(":focus-within")),
      width,
      painted,
      clips,
    };
  });
}

function expectUnclipped(measure, label) {
  expect(measure.focusVisible, `${label} shows a keyboard ring`).toBe(true);
  expect(measure.width, `${label} ring width`).toBeGreaterThanOrEqual(2);
  for (const clip of measure.clips)
    for (const [edge, inside] of [
      ["left", measure.painted.left >= clip.left - 0.5],
      ["right", measure.painted.right <= clip.right + 0.5],
      ["top", measure.painted.top >= clip.top - 0.5],
      ["bottom", measure.painted.bottom <= clip.bottom + 0.5],
    ])
      expect(inside, `${label} ring ${edge} clipped by ${clip.name}`).toBe(
        true,
      );
}

for (const width of [408, 320]) {
  test(`focus rings of Settings editor and Preferences fields are never clipped at ${width}px`, async ({
    page,
    store,
  }) => {
    const settings = (await store("snapshot")).settings;
    settings.agents = [{ ...fixtureAgent }];
    await store("seed_settings", settings);
    await seedBoundRepositories(store, ["fixture/rings"]);
    await page.setViewportSize({ width, height: 744 });
    await page.goto("/");
    await tab(page, "Settings").click();
    await expect(page.locator("#save-status")).toHaveText("All changes saved");

    await section(page, "Voices");
    let modal = await openCard(page, "voice", "Caveman");
    expectUnclipped(
      await ring(modal.getByLabel("Title", { exact: true })),
      "voice title",
    );
    await page.screenshot({
      path: test.info().outputPath(`voice-title-focus-${width}.png`),
    });
    expectUnclipped(
      await ring(modal.getByRole("textbox", { name: "Style", exact: true })),
      "voice style",
    );
    expectUnclipped(
      await ring(modal.getByRole("checkbox", { name: /Fixed guardrails/ })),
      "voice guardrails",
    );
    expectUnclipped(
      await ring(modal.getByRole("button", { name: "Save voice" })),
      "voice save",
    );
    await modal.getByRole("button", { name: "Cancel", exact: true }).click();

    await section(page, "Doctrines");
    modal = await openCard(page, "doctrine", "code");
    expectUnclipped(
      await ring(modal.getByLabel("Title", { exact: true })),
      "doctrine title",
    );
    await modal.getByRole("button", { name: "Cancel", exact: true }).click();

    await section(page, "Agents");
    modal = await openCard(page, "agent", fixtureAgent.name);
    expectUnclipped(
      await ring(modal.getByLabel("Name", { exact: true })),
      "agent name",
    );
    expectUnclipped(
      await ring(modal.locator("textarea[name=prompt]")),
      "agent prompt",
    );
    expectUnclipped(
      await ring(modal.locator("input[name=signature]")),
      "agent signature",
    );
    await modal.locator("[data-open-doctrines]").click();
    expectUnclipped(
      await ring(modal.getByLabel("Filter doctrines", { exact: true })),
      "doctrine filter",
    );
    await page.locator("[data-panel-back]").click();
    await modal.getByRole("button", { name: "Cancel", exact: true }).click();

    const repository = await repositorySettings(page, "fixture/rings");
    expectUnclipped(
      await ring(repository.getByRole("switch", { name: "All pull requests" })),
      "repository switch",
    );
    await expect(repository.locator("[data-schedule]")).not.toHaveAttribute(
      "aria-busy",
      "true",
    );
    expectUnclipped(
      await ring(repository.locator("#schedule-every")),
      "schedule interval",
    );
    expectUnclipped(
      await ring(repository.locator("#schedule-day-1")),
      "schedule day",
    );
    await repository.locator("#schedule-mode-daily").check();
    expectUnclipped(
      await ring(repository.locator("#schedule-time-0")),
      "schedule time",
    );
    expectUnclipped(
      await ring(repository.locator("#schedule-add-time")),
      "schedule add time",
    );
    await repository.locator("#schedule-mode-advanced").check();
    expectUnclipped(
      await ring(repository.locator("#schedule-cron")),
      "schedule cron",
    );
    await closeDialog(page);

    await section(page, "Preferences");
    for (const [selector, label] of [
      ["#global-timezone", "time zone"],
      ["#global-capacity", "AI capacity"],
    ])
      expectUnclipped(await ring(page.locator(selector)), label);
  });
}
