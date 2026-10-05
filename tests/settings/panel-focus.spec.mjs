import { test, expect } from "./fixtures.mjs";
import { holdFocusFrames } from "./focus-frames.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { section } from "./navigation.mjs";

test.use({ viewport: { width: 400, height: 680 } });
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });
const heading = (page) => page.locator("[data-panel-heading]");
const invoke = (page, command) =>
  page.evaluate(
    (command) => window.__TAURI_INTERNALS__.invoke(command),
    command,
  );
const settled = (page) =>
  page.evaluate(async () => {
    await window.__settingsIdle();
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  });

for (const newerFocus of [false, true]) {
  test(`current route frame preserves ${newerFocus ? "newer explicit focus" : "initial navigation focus"}`, async ({
    page,
  }, testInfo) => {
    await page.goto("/");
    await settled(page);
    const frames = await holdFocusFrames(page);
    await tab(page, "Running").click();
    await frames.waitForCount(1);
    await expect(heading(page)).toHaveText("Work queue");
    await expect(heading(page)).toBeFocused();
    if (newerFocus) await tab(page, "Running").focus();
    await frames.release();
    await expect(
      newerFocus ? tab(page, "Running") : heading(page),
    ).toBeFocused();
    await frames.attach(testInfo);
  });

  test(`delayed Settings mount preserves ${newerFocus ? "newer explicit focus" : "initial navigation focus"}`, async ({
    page,
    ipc,
  }) => {
    await page.goto("/");
    await settled(page);
    const hold = ipc.holdNext("snapshot");
    await tab(page, "Settings").click();
    await hold.arrived;
    await expect(heading(page)).toHaveText("Settings");
    if (newerFocus) await tab(page, "Running").focus();
    hold.release();
    await settled(page);
    await expect(page.locator("#save-status")).toHaveText("All changes saved");
    await expect(
      newerFocus ? tab(page, "Running") : heading(page),
    ).toBeFocused();
    await expect(tab(page, "Settings")).toHaveAttribute("aria-current", "page");
  });

  test(`deferred Back retains exact row and scroll with ${newerFocus ? "newer navigation focus" : "unchanged focus"}`, async ({
    page,
    store,
  }, testInfo) => {
    await queueFixture(store);
    await page.goto("/");
    await settled(page);
    const button = page
      .locator("#handoff-queue")
      .getByRole("article", { name: "example/repo #3", exact: true })
      .getByRole("button", { name: "Evidence and actions", exact: true });
    await button.scrollIntoViewIfNeeded();
    const scroller = page.locator(".panel-content");
    const scroll = await scroller.evaluate((element) => element.scrollTop);
    expect(scroll).toBeGreaterThan(0);
    await button.click();
    await expect(heading(page)).toHaveText("Saved evidence");
    await settled(page);
    const frames = await holdFocusFrames(page);
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await frames.waitForCount(1);
    await expect(button).toBeFocused();
    if (newerFocus) await tab(page, "Queue").focus();
    await frames.release();
    await expect(newerFocus ? tab(page, "Queue") : button).toBeFocused();
    expect(await scroller.evaluate((element) => element.scrollTop)).toBe(
      scroll,
    );
    await frames.attach(testInfo);
  });
}

test("rapid route changes reject an old frame even when focus stays on the shared heading", async ({
  page,
  store,
}, testInfo) => {
  await queueFixture(store);
  await page.goto("/");
  await settled(page);
  const frames = await holdFocusFrames(page);
  await tab(page, "Running").click();
  await frames.waitForCount(1);
  await expect(heading(page)).toBeFocused();
  // Native destinations can replace a route without a pointer focus change.
  await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("panel_navigate", {
      route: { tab: "queue" },
    }),
  );
  await frames.waitForCount(2);
  await expect(heading(page)).toHaveText("Your queue");
  await expect(heading(page)).toBeFocused();
  const scroller = page.locator(".panel-content");
  await scroller.evaluate((element) => (element.scrollTop = 100));
  const scroll = await scroller.evaluate((element) => element.scrollTop);
  expect(scroll).toBeGreaterThan(0);
  await frames.release(0);
  expect(await scroller.evaluate((element) => element.scrollTop)).toBe(scroll);
  await frames.release(1);
  await expect(heading(page)).toBeFocused();
  await frames.attach(testInfo);
});

test("hide invalidates pending positioning and reopen restores the retained editor draft", async ({
  page,
}, testInfo) => {
  await page.goto("/");
  await tab(page, "Settings").click();
  await section(page, "Doctrines");
  await page.getByRole("button", { name: "New doctrine", exact: true }).click();
  const title = page
    .getByRole("dialog", { name: "New doctrine" })
    .getByLabel("Title", { exact: true });
  await title.fill("Retained across deferred frames");
  await settled(page);
  await tab(page, "Running").click();
  await settled(page);
  const frames = await holdFocusFrames(page);
  await tab(page, "Settings").click();
  await frames.waitForCount(1);
  await expect(title).toBeFocused();
  await invoke(page, "hide_panel");
  await tab(page, "Running").focus();
  await frames.release();
  await expect(tab(page, "Running")).toBeFocused();
  await expect(page.locator(".panel-shell")).toHaveAttribute(
    "data-native-visible",
    "false",
  );
  await invoke(page, "fixture_show_panel");
  await frames.waitForCount(2);
  await expect(title).toBeFocused();
  await frames.release(1);
  await expect(title).toBeFocused();
  await expect(title).toHaveValue("Retained across deferred frames");
  await frames.attach(testInfo);
});

test("deferred positioning yields after focus moves away and back to the same heading", async ({
  page,
  store,
}, testInfo) => {
  await queueFixture(store);
  await page.goto("/");
  await tab(page, "Running").click();
  await settled(page);
  const frames = await holdFocusFrames(page);
  await tab(page, "Queue").click();
  await frames.waitForCount(1);
  await expect(heading(page)).toBeFocused();
  await tab(page, "Queue").focus();
  await heading(page).focus();
  const scroller = page.locator(".panel-content");
  await scroller.evaluate((element) => (element.scrollTop = 100));
  const scroll = await scroller.evaluate((element) => element.scrollTop);
  expect(scroll).toBeGreaterThan(0);
  await frames.release();
  await expect(heading(page)).toBeFocused();
  expect(await scroller.evaluate((element) => element.scrollTop)).toBe(scroll);
  await frames.attach(testInfo);
});

test("late initial Settings loading cannot restore a superseded route", async ({
  page,
  ipc,
}) => {
  await page.goto("/");
  await settled(page);
  const hold = ipc.holdNext("snapshot");
  await tab(page, "Settings").click();
  await hold.arrived;
  await tab(page, "Running").click();
  await expect(heading(page)).toHaveText("Work queue");
  await expect(heading(page)).toBeFocused();
  await tab(page, "Running").focus();
  hold.release();
  await settled(page);
  await expect(tab(page, "Running")).toBeFocused();
  await expect(heading(page)).toHaveText("Work queue");
  await expect(page.locator('[data-panel-view="settings"]')).toBeHidden();
  await tab(page, "Settings").click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await expect(heading(page)).toBeFocused();
});
