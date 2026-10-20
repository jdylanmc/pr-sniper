import { test, expect } from "./fixtures.mjs";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import {
  closeDialog,
  repositorySettings,
  section,
  seedBoundRepositories,
} from "./navigation.mjs";

// The Store bridge previews from its fixture clock: Fri 2027-01-15 08:00 UTC.
const NAME = "fixture/schedule-0";

async function seed(store, crons, timeZone = "UTC") {
  const names = crons.map((_, index) => `fixture/schedule-${index}`);
  const settings = await seedBoundRepositories(store, names);
  settings.time_zone = timeZone;
  settings.repositories.forEach((repository, index) => {
    repository.enabled = false;
    repository.cron = crons[index];
  });
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

async function open(page, name = NAME) {
  const editor = await repositorySettings(page, name);
  await expect(editor.locator("[data-schedule]")).not.toHaveAttribute(
    "aria-busy",
    "true",
  );
  return editor;
}

const runs = (editor) => editor.locator("[data-schedule-runs] li");

async function settle(editor) {
  await editor.page().evaluate(() => window.__settingsIdle());
  await expect(editor.locator("[data-schedule]")).not.toHaveAttribute(
    "aria-busy",
    "true",
  );
}

async function saveRepository(editor) {
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
}

const savedCron = async (store, index = 0) =>
  (await store("snapshot")).settings.repositories[index].cron;

test("Preferences keeps capacity, review inactivity and time zone without repository schedules", async ({
  page,
  store,
}) => {
  await seed(store, ["*/30 * * * *"]);
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  const card = page.getByRole("group", { name: "Capacity and time zone" });
  await expect(card.getByLabel("AI capacity", { exact: true })).toBeVisible();
  const inactivity = card.getByLabel("Review inactivity (min)", {
    exact: true,
  });
  await expect(inactivity).toBeVisible();
  await expect(inactivity).toHaveValue("15");
  await expect(card.locator("#global-timezone")).toBeVisible();
  await expect(card.locator("label")).toHaveCount(3);
  await expect(page.getByLabel("Cron expression")).toHaveCount(0);
  await expect(page.getByLabel("Schedule helper")).toHaveCount(0);
});

test("each builder mode saves its cron through the real Store and reopens in the same mode", async ({
  page,
  store,
}) => {
  await seed(store, ["*/30 * * * *"]);
  await page.goto("/?view=settings");
  const cases = [
    {
      cron: "*/15 * * * 1-5",
      build: async (editor) => {
        await editor.locator("#schedule-every").selectOption("15");
        await editor.locator("#schedule-day-0").uncheck();
        await editor.locator("#schedule-day-6").uncheck();
      },
      check: async (editor) => {
        await expect(editor.locator("#schedule-mode-every")).toBeChecked();
        await expect(editor.locator("#schedule-every")).toHaveValue("15");
        await expect(editor.locator("#schedule-hours")).toHaveCount(0);
        await expect(editor.locator("#schedule-day-6")).not.toBeChecked();
        await expect(editor.locator("#schedule-day-5")).toBeChecked();
      },
    },
    {
      cron: "0 */2 * * *",
      build: async (editor) => {
        await editor.locator("#schedule-mode-every").check();
        await editor.locator("#schedule-every").selectOption("120");
        for (const day of [0, 6])
          await editor.locator(`#schedule-day-${day}`).check();
      },
      check: async (editor) => {
        await expect(editor.locator("#schedule-every")).toHaveValue("120");
        await expect(editor.locator("#schedule-day-0")).toBeChecked();
      },
    },
    {
      cron: "45 * * * *",
      build: async (editor) => {
        await editor.locator("#schedule-mode-hourly").check();
        await editor.locator("#schedule-minute").fill("45");
      },
      check: async (editor) => {
        await expect(editor.locator("#schedule-mode-hourly")).toBeChecked();
        await expect(editor.locator("#schedule-minute")).toHaveValue("45");
      },
    },
    {
      cron: "30 9,17 * * *",
      build: async (editor) => {
        await editor.locator("#schedule-mode-daily").check();
        await editor.locator("#schedule-time-0").fill("09:30");
        await editor.locator("#schedule-add-time").click();
        await expect(editor.locator("#schedule-time-1")).toBeFocused();
        await editor.locator("#schedule-time-1").fill("17:30");
      },
      check: async (editor) => {
        await expect(editor.locator("#schedule-mode-daily")).toBeChecked();
        await expect(editor.locator("#schedule-time-0")).toHaveValue("09:30");
        await expect(editor.locator("#schedule-time-1")).toHaveValue("17:30");
        await expect(editor.locator("#schedule-hours")).toHaveCount(0);
      },
    },
    {
      cron: "5 4 * * 0",
      build: async (editor) => {
        await editor.locator("#schedule-mode-advanced").check();
        await editor.locator("#schedule-cron").fill("5 4 * * 0");
      },
      check: async (editor) => {
        // Daily 04:05 on Sunday is representable; it reopens in Daily.
        await expect(editor.locator("#schedule-mode-daily")).toBeChecked();
        await expect(editor.locator("#schedule-time-0")).toHaveValue("04:05");
        await expect(editor.locator("#schedule-day-0")).toBeChecked();
        await expect(editor.locator("#schedule-day-1")).not.toBeChecked();
      },
    },
  ];
  for (const { cron, build, check } of cases) {
    const editor = await open(page);
    await build(editor);
    await settle(editor);
    await expect(runs(editor)).toHaveCount(3);
    await saveRepository(editor);
    expect(await savedCron(store)).toBe(cron);
    await page.reload();
    await check(await open(page));
    await closeDialog(page);
  }
});

test("saved crons decode into the matching mode, else Advanced with the raw cron", async ({
  page,
  store,
}) => {
  await seed(store, [
    "*/15 * * * 1-5",
    "0 9 * * MON-FRI",
    "7 */3 * * *",
    "*/15 9-17 * * 1-5",
  ]);
  await page.goto("/?view=settings");
  let editor = await open(page);
  await expect(editor.locator("#schedule-mode-every")).toBeChecked();
  await expect(editor.locator("#schedule-every")).toHaveValue("15");
  await expect(editor.locator("#schedule-day-6")).not.toBeChecked();
  await closeDialog(page);
  editor = await open(page, "fixture/schedule-1");
  await expect(editor.locator("#schedule-mode-daily")).toBeChecked();
  await expect(editor.locator("#schedule-time-0")).toHaveValue("09:00");
  await closeDialog(page);
  editor = await open(page, "fixture/schedule-2");
  await expect(editor.locator("#schedule-mode-advanced")).toBeChecked();
  await expect(editor.locator("#schedule-cron")).toHaveValue("7 */3 * * *");
  await closeDialog(page);
  // Hour ranges are not a builder concept: Advanced keeps them exactly.
  editor = await open(page, "fixture/schedule-3");
  await expect(editor.locator("#schedule-mode-advanced")).toBeChecked();
  await expect(editor.locator("#schedule-cron")).toHaveValue(
    "*/15 9-17 * * 1-5",
  );
  // Opening never rewrites an equivalent saved cron.
  expect((await store("snapshot")).settings.repositories[1].cron).toBe(
    "0 9 * * MON-FRI",
  );
});

test("invalid Advanced and Daily input shows the backend error and blocks Save", async ({
  page,
  store,
  dataRoot,
}) => {
  await seed(store, ["*/30 * * * *"]);
  const path = join(dataRoot, "config/settings.json");
  const before = await readFile(path);
  await page.goto("/?view=settings");
  const editor = await open(page);
  const alert = editor.locator("[data-schedule-error]");
  await editor.locator("#schedule-mode-advanced").check();
  await expect(editor.locator("#schedule-cron")).toHaveValue("*/30 * * * *");
  for (const cron of ["61 * * * *", "* * * *", "0 0 30 2 *"]) {
    await editor.locator("#schedule-cron").fill(cron);
    await settle(editor);
    await expect(alert).toBeVisible();
    await expect(alert).not.toBeEmpty();
    await expect(editor.locator("#schedule-cron")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    await expect(runs(editor)).toHaveCount(0);
  }
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor.locator("[data-resource-error]")).toBeVisible();
  expect(await readFile(path)).toEqual(before);

  await editor.locator("#schedule-cron").fill("0 6 * * *");
  await settle(editor);
  await expect(alert).toBeHidden();
  await expect(editor.locator("#schedule-cron")).not.toHaveAttribute(
    "aria-invalid",
  );
  await editor.locator("#schedule-mode-daily").check();
  await editor.locator("#schedule-time-0").fill("09:00");
  await editor.locator("#schedule-add-time").click();
  await editor.locator("#schedule-time-1").fill("13:30");
  await settle(editor);
  await expect(alert).toContainText("same minutes");
  await editor.locator("#schedule-mode-every").check();
  for (const day of [0, 1, 2, 3, 4, 5, 6])
    await editor.locator(`#schedule-day-${day}`).uncheck();
  await settle(editor);
  await expect(alert).toContainText("day");
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor.locator("[data-resource-error]")).toContainText("day");
  expect(await readFile(path)).toEqual(before);
});

test("the next three runs come from the backend in the saved time zone and follow every change", async ({
  page,
  store,
}) => {
  await seed(store, ["*/30 * * * *"]);
  await page.goto("/?view=settings");
  let editor = await open(page);
  await expect(editor.locator("[data-schedule-zone]")).toHaveText("UTC");
  await expect(runs(editor)).toHaveText([
    "Fri, Jan 15, 08:30",
    "Fri, Jan 15, 09:00",
    "Fri, Jan 15, 09:30",
  ]);
  await editor.locator("#schedule-every").selectOption("5");
  await expect(runs(editor)).toHaveText([
    "Fri, Jan 15, 08:05",
    "Fri, Jan 15, 08:10",
    "Fri, Jan 15, 08:15",
  ]);
  await editor.locator("#schedule-day-5").uncheck();
  await expect(runs(editor)).toHaveText([
    "Sat, Jan 16, 00:00",
    "Sat, Jan 16, 00:05",
    "Sat, Jan 16, 00:10",
  ]);
  await editor.locator("#schedule-mode-advanced").check();
  await editor.locator("#schedule-cron").fill("0 12 1 * *");
  await expect(runs(editor)).toHaveText([
    "Mon, Feb 1, 12:00",
    "Mon, Mar 1, 12:00",
    "Thu, Apr 1, 12:00",
  ]);
  await closeDialog(page);

  const settings = (await store("snapshot")).settings;
  settings.time_zone = "Asia/Tokyo";
  await store("seed_settings", settings);
  await page.reload();
  editor = await open(page);
  await expect(editor.locator("[data-schedule-zone]")).toHaveText("Asia/Tokyo");
  await expect(runs(editor)).toHaveText([
    "Fri, Jan 15, 17:30",
    "Fri, Jan 15, 18:00",
    "Fri, Jan 15, 18:30",
  ]);
});

test("the repository list shows each schedule as a compact value", async ({
  page,
  store,
}) => {
  await seed(store, [
    "*/30 * * * *",
    "*/15 * * * 1-5",
    "0 9,17 * * *",
    "7 */3 * * *",
    "*/15 9-17 * * 1-5",
  ]);
  await page.goto("/?view=settings");
  await section(page, "Integrations");
  const summary = (index) =>
    page.locator(`[data-repository-schedule]`).nth(index);
  await expect(summary(0)).toHaveText("Every 30 min");
  await expect(summary(1)).toHaveText("Mon–Fri · every 15 min");
  await expect(summary(2)).toHaveText("Daily 09:00, 17:00");
  await expect(summary(3)).toHaveText("7 */3 * * *");
  await expect(summary(4)).toHaveText("*/15 9-17 * * 1-5");
});

test("a preview landing during a Monitor switch save keeps the latest schedule edit", async ({
  page,
  store,
  ipc,
}) => {
  const settings = await seed(store, ["*/30 * * * *"]);
  settings.repositories[0].enabled = true;
  await store("seed_settings", settings);
  await page.goto("/?view=settings");
  const editor = await open(page);
  const preview = ipc.holdNext("preview_schedule");
  await editor.locator("#schedule-every").selectOption("5");
  await preview.arrived;
  // The switch save copies the draft before the held preview lands.
  const save = ipc.holdNext("save_resource");
  await editor.getByRole("switch", { name: `Monitor ${NAME}` }).click();
  await save.arrived;
  preview.release();
  await page.waitForFunction(() =>
    [...document.querySelectorAll("[data-schedule-runs] li")].some((li) =>
      li.textContent.endsWith(":05"),
    ),
  );
  save.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(
    editor.getByRole("switch", { name: `Monitor ${NAME}` }),
  ).not.toBeChecked();
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await expect(editor.locator("#schedule-every")).toHaveValue("5");
  await saveRepository(editor);
  const saved = (await store("snapshot")).settings.repositories[0];
  expect(saved.cron).toBe("*/5 * * * *");
  expect(saved.enabled).toBe(false);
});

test("keyboard moves through modes and times with focus kept by stable id", async ({
  page,
  store,
}) => {
  await seed(store, ["*/30 * * * *"]);
  await page.goto("/?view=settings");
  const editor = await open(page);
  await editor.locator("#schedule-mode-every").focus();
  await page.keyboard.press("ArrowRight");
  await settle(editor);
  await expect(editor.locator("#schedule-mode-hourly")).toBeFocused();
  await expect(editor.locator("#schedule-mode-hourly")).toBeChecked();
  await page.keyboard.press("ArrowRight");
  await settle(editor);
  await expect(editor.locator("#schedule-mode-daily")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(editor.locator("#schedule-time-0")).toBeFocused();
  await editor.locator("#schedule-add-time").focus();
  await page.keyboard.press("Enter");
  await expect(editor.locator("#schedule-time-1")).toBeFocused();
  await editor.locator("#schedule-remove-time-1").focus();
  await page.keyboard.press("Enter");
  await expect(editor.locator("#schedule-time-1")).toHaveCount(0);
  await expect(editor.locator("#schedule-time-0")).toBeFocused();
  await expect(editor.locator("#schedule-remove-time-0")).toBeDisabled();
  await settle(editor);
  expect(
    (await store("snapshot")).settings.repositories[0].cron,
    "unsaved draft",
  ).toBe("*/30 * * * *");
});

for (const width of [408, 320]) {
  test(`schedule builder fits ${width}px in every mode`, async ({
    page,
    store,
  }, testInfo) => {
    await page.setViewportSize({ width, height: 744 });
    await seed(store, ["*/15 * * * 1-5"]);
    await page.goto("/?view=settings");
    const editor = await open(page);
    for (const mode of ["every", "hourly", "daily", "advanced"]) {
      await editor.locator(`#schedule-mode-${mode}`).check();
      await settle(editor);
      const overflow = await editor
        .locator("[data-schedule]")
        .evaluate((element) => element.scrollWidth - element.clientWidth);
      expect(overflow, mode).toBeLessThanOrEqual(0);
      await editor.locator("[data-schedule]").scrollIntoViewIfNeeded();
      await page.screenshot({
        path: testInfo.outputPath(`schedule-${mode}-${width}.png`),
      });
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
    }
  });
}

test("Dark theme draws the builder from theme variables", async ({
  page,
  store,
}) => {
  await seed(store, ["*/15 * * * 1-5"]);
  await store("save_theme", { theme: "dark" });
  await page.goto("/?view=settings");
  const editor = await open(page);
  const colors = await editor.evaluate(() => {
    const root = getComputedStyle(document.documentElement);
    const probe = (value) => {
      const node = document.createElement("i");
      node.style.color = value;
      document.body.append(node);
      const color = getComputedStyle(node).color;
      node.remove();
      return color;
    };
    return {
      selected: getComputedStyle(
        document.querySelector("#schedule-mode-every").parentElement,
      ).backgroundColor,
      teal: probe(root.getPropertyValue("--bg-teal-327e84")),
      day: getComputedStyle(document.querySelector("#schedule-day-0 + span"))
        .color,
      muted: probe(root.getPropertyValue("--text-slate-526274")),
    };
  });
  expect(colors.selected).toBe(colors.teal);
  expect(colors.day).toBe(colors.muted);
  expect(colors.muted).toBe("rgb(170, 182, 195)");
});
