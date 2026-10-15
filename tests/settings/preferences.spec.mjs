import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "./fixtures.mjs";
import {
  newDoctrine,
  section,
  seedAgent,
  chooseTimeZone,
} from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { clipboardWrites, recordClipboard } from "./diagnostics-copy.mjs";
import { target } from "./paths.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const preferences = (page) => page.locator(".preferences-view");
// The panel header Monitoring switch is the only pause/resume control.
const headerAutomation = (page) => page.locator("#automation-controls");
const save = (page) => page.locator("#save-settings");
const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

async function ready(page, store, panel = false) {
  // Initialize the real Store before the panel's independent readers start.
  await store("snapshot");
  await store("panel_snapshot");
  await page.goto(panel ? "/" : "/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  if (panel) {
    await tab(page, "Settings").click();
    await page.evaluate(() => window.__settingsIdle());
  }
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await section(page, "Preferences");
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#notification-enabled")).toBeEnabled();
  await expect(
    preferences(page).locator("[data-toggle-automation]"),
  ).toHaveCount(0);
  if (panel)
    await expect(
      headerAutomation(page).locator("[data-toggle-automation]"),
    ).toBeEnabled();
}

async function savePreferences(page) {
  await save(page).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
}

async function syntheticNative(page, options = {}) {
  await page.addInitScript((options) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__nativePreferences = {
      registration: "absent",
      isolated: false,
      startupError: null,
      startupRead: "available",
      notificationReadError: false,
      denied: false,
      ...options,
    };
    window.__preferenceCommands = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const native = window.__nativePreferences;
      window.__preferenceCommands.push(command);
      if (command === "save_login") {
        if (native.startupError) throw native.startupError;
        const { settings } = await original("snapshot");
        settings.launch_at_login = args.enabled;
        // Only fixture-owned storage changes; no OS registration is invoked.
        await original("seed_settings", settings);
        native.registration = args.enabled ? "registered" : "absent";
        return;
      }
      if (command === "notification_snapshot" && native.notificationReadError)
        throw "Synthetic native notification read failed.";
      if (command === "set_notifications_enabled" && native.denied)
        throw "Synthetic OS permission denied; notification opt-in remains off.";
      const value = await original(command, args);
      if (command === "snapshot") {
        if (native.startupRead === "rejected")
          throw "Synthetic native registration read failed.";
        if (native.startupRead === "missing") {
          value.settings = null;
          value.error = "Synthetic settings read unavailable; bytes retained.";
        }
        value.isolated = native.isolated;
        value.login_registration = native.registration;
      }
      if (command === "notification_snapshot") {
        value.permission = native.denied
          ? {
              authorization: "denied",
              alerts_enabled: false,
              center_enabled: false,
            }
          : {
              authorization: "authorized",
              alerts_enabled: null,
              center_enabled: null,
            };
      }
      return value;
    };
  }, options);
}

const automationReadError = "Synthetic automation snapshot unavailable.";
const automationActionError = "Synthetic native pause was rejected.";

async function automationFailures(page) {
  await page.addInitScript(
    ({ readError, actionError }) => {
      const original = window.__TAURI_INTERNALS__.invoke;
      const control = (window.__automationFailures = {
        failReads: 0,
        readUnavailable: false,
        rejectPause: false,
        holdPause: false,
        pauseRequests: [],
        // Fails only reads that start after a pause request, so reads left
        // over from an earlier tick cannot disable the switch before the click.
        failReadsAfterPause: false,
      });
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        if (command === "automation_snapshot") {
          const fail =
            control.readUnavailable ||
            control.failReads > 0 ||
            (control.failReadsAfterPause && control.pauseRequests.length > 0);
          if (control.failReads > 0) control.failReads--;
          const value = await original(command, args);
          if (fail) throw readError;
          return value;
        }
        if (command === "set_automation_paused") {
          control.pauseRequests.push(args.paused);
          if (control.holdPause)
            return new Promise((_resolve, reject) => {
              control.reject = () => reject(actionError);
            });
          if (control.rejectPause) throw actionError;
        }
        return original(command, args);
      };
    },
    { readError: automationReadError, actionError: automationActionError },
  );
}

async function capture(page, browserName, name) {
  const directory = join(target, "visual-84-screenshots", browserName);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `${name}.png`) });
}

test("compact saved controls persist exact timezone and full u32 capacity without cron or global action grants", async ({
  page,
  store,
  browserName,
}) => {
  const original = await seedAgent(store);
  await ready(page, store, true);
  await expect(
    preferences(page).locator("#global-cron, #cron-helper"),
  ).toHaveCount(0);
  await expect(preferences(page).getByLabel("Cron expression")).toHaveCount(0);
  await expect(preferences(page).getByLabel("Schedule helper")).toHaveCount(0);
  await expect(page.locator("#global-capacity")).toHaveValue("4");
  await expect(
    preferences(page).getByRole("switch", { name: /^Approve/ }),
  ).toHaveCount(0);
  await expect(
    preferences(page).getByRole("switch", { name: /^Merge/ }),
  ).toHaveCount(0);
  await capture(page, browserName, "saved-preferences");
  await chooseTimeZone(page, "America/New_York");
  await expect(page.locator("#automatic-review-start")).toHaveCount(0);
  for (const capacity of [1, 20, 4294967295]) {
    await page.locator("#global-capacity").fill(String(capacity));
    await savePreferences(page);
    const expected = structuredClone(original);
    expected.capacity = capacity;
    expected.time_zone = "America/New_York";
    expect((await store("snapshot")).settings).toEqual(expected);
    await expect(page.locator("#automatic-publication")).toHaveCount(0);
  }
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#global-capacity")).toHaveValue("4294967295");
  await expect(page.locator("#global-timezone")).toHaveValue(
    "America/New_York",
  );
  await capture(page, browserName, "maximum-capacity");
  await page.locator(".preferences-schedule").scrollIntoViewIfNeeded();
  await capture(page, browserName, "global-schedule");
});

for (const width of [408, 320]) {
  test(`Preferences top card overlaps the band with no occluded heading and uniform fields at ${width}px`, async ({
    page,
    store,
  }) => {
    await page.setViewportSize({ width, height: 744 });
    await ready(page, store, true);
    const layout = await page.evaluate(() => {
      const box = (element) => element.getBoundingClientRect();
      const band = box(document.querySelector(".panel-context"));
      const view = document.querySelector(".preferences-view");
      const card = view.firstElementChild;
      const headings = [...view.querySelectorAll("h2")]
        .map(box)
        .filter((rect) => rect.top < band.bottom && rect.bottom > band.top);
      const labels = [...view.querySelectorAll(".preferences-saved label")].map(
        (label) => ({
          text: label.firstChild.textContent.trim(),
          font: getComputedStyle(label).fontSize,
          weight: getComputedStyle(label).fontWeight,
          field: box(label.querySelector("input, select")).height,
        }),
      );
      return {
        overlap: band.bottom - box(card).top,
        cardIsGroup: card.classList.contains("settings-group"),
        occluded: headings.length,
        labels,
      };
    });
    expect(layout.cardIsGroup).toBe(true);
    expect(layout.overlap).toBeCloseTo(23, 0);
    expect(layout.occluded).toBe(0);
    expect(layout.labels.map((label) => label.text)).toEqual([
      "AI capacity",
      "Time zone",
    ]);
    for (const key of ["font", "weight", "field"])
      expect(new Set(layout.labels.map((label) => label[key])).size).toBe(1);
  });
}

for (const width of [408, 320]) {
  test(`Time zone is a dedicated full-width dropdown of host zones at ${width}px`, async ({
    page,
    store,
  }) => {
    await page.setViewportSize({ width, height: 744 });
    await ready(page, store, true);
    const select = page.locator("#global-timezone");
    await expect(select).toHaveJSProperty("tagName", "SELECT");
    await expect(select).toHaveValue("UTC");
    await expect(select.locator("option")).not.toHaveCount(1);
    const zones = await store("time_zones");
    const options = await select
      .locator("option")
      .evaluateAll((all) => all.map((option) => option.value));
    const system = await page.evaluate(
      () => Intl.DateTimeFormat().resolvedOptions().timeZone,
    );
    expect([...options].sort()).toEqual([...zones].sort());
    if (zones.includes(system)) expect(options[0]).toBe(system);
    expect(options.slice(zones.includes(system) ? 1 : 0)).toEqual(
      zones.filter((zone) => zone !== system),
    );
    await expect(page.locator("#save-status")).toHaveText("All changes saved");
    const rows = await page.evaluate(() =>
      ["#global-capacity", "#global-timezone"].map((id) => {
        const label = document.querySelector(id).closest("label");
        const rect = label.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, width: rect.width };
      }),
    );
    const [capacity, zone] = rows;
    expect(zone.top).toBeGreaterThanOrEqual(capacity.bottom - 0.5);
    expect(zone.width).toBeCloseTo(capacity.width, 0);
  });
}

test("a saved time zone the host does not list stays selected and visibly unavailable", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const value = await original(command, args);
      return command === "time_zones"
        ? value.filter((zone) => zone !== "UTC")
        : value;
    };
  });
  await ready(page, store, true);
  const select = page.locator("#global-timezone");
  await expect(select.locator("option")).not.toHaveCount(1);
  await expect(select).toHaveValue("UTC");
  await expect(select).toHaveAttribute("aria-invalid", "true");
  await expect(select.locator("option").first()).toHaveText(
    "UTC (unavailable)",
  );
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await chooseTimeZone(page, "Asia/Tokyo");
  await expect(select).not.toHaveAttribute("aria-invalid");
  await savePreferences(page);
  expect((await store("snapshot")).settings.time_zone).toBe("Asia/Tokyo");
});

test("invalid capacity keeps each rejected draft and the previous configuration bytes", async ({
  page,
  store,
  dataRoot,
}) => {
  await ready(page, store);
  const before = await readFile(join(dataRoot, "config/settings.json"));
  for (const value of ["0", "-1", "1.5", "4294967296"]) {
    await page.locator("#global-capacity").fill(value);
    await save(page).click();
    await page.evaluate(() => window.__settingsIdle());
    await expect(page.locator("#error")).toBeVisible();
    await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
    await expect(page.locator("#global-capacity")).toHaveValue(value);
    expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
      before,
    );
  }
});

test("a legacy global interval becomes the saved time zone and leaves no cron control", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.defaults.schedule = {
    kind: "interval",
    minutes: 7,
    timezone: "Europe/London",
  };
  await store("seed_settings", settings);
  const migrated = (await store("snapshot")).settings;
  expect(migrated.defaults.schedule).toBeUndefined();
  expect(migrated.time_zone).toBe("Europe/London");
  await ready(page, store);
  await expect(page.locator("#global-timezone")).toHaveValue("Europe/London");
  await expect(page.locator("#global-cron, #cron-helper")).toHaveCount(0);
  await page.locator("#global-capacity").fill("8");
  await savePreferences(page);
  expect((await store("snapshot")).settings).toEqual({
    ...migrated,
    capacity: 8,
  });
});

test("failed preference and pause writes keep committed state and exact drafts", async ({
  page,
  store,
  dataRoot,
  browserName,
}) => {
  await ready(page, store, true);
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await chooseTimeZone(page, "Pacific/Auckland");
  await page.locator("#global-capacity").fill("33");
  await expect(page.locator("#automatic-publication")).toHaveCount(0);
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await save(page).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#error")).toBeVisible();
  await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
  await expect(page.locator("#global-timezone")).toHaveValue(
    "Pacific/Auckland",
  );
  await expect(page.locator("#global-capacity")).toHaveValue("33");
  await expect(page.locator("#automatic-publication")).toHaveCount(0);
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
  await mkdir(join(dataRoot, "state/automation.json.tmp"), { recursive: true });
  await headerAutomation(page)
    .getByRole("button", { name: "Pause automation" })
    .click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(
    headerAutomation(page).locator("[data-automation-error]"),
  ).toHaveText(/.+/);
  await expect(
    headerAutomation(page).locator("[data-automation-status]"),
  ).toContainText("Running; 0 occupied / 4");
  expect((await store("automation_snapshot")).paused).toBe(false);
  await capture(page, browserName, "rejected-writes");
});

test("startup, notification and pause commit independently while newer keyboard focus and drafts survive", async ({
  page,
  store,
  ipc,
  browserName,
}) => {
  await syntheticNative(page);
  await ready(page, store, true);
  const original = (await store("snapshot")).settings;
  await page.locator("#global-capacity").fill("17");
  await expect(page.locator("#automatic-publication")).toHaveCount(0);
  const held = ipc.holdNext("seed_settings");
  await page.locator("#login").click();
  await held.arrived;
  await expect(page.locator("#login")).toBeDisabled();
  await expect(page.locator("#login-status")).toHaveText("Updating");
  await page.locator("#global-timezone").focus();
  await chooseTimeZone(page, "Europe/Paris");
  held.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#login-status")).toContainText(
    "On; registration registered",
  );
  await expect(page.locator("#global-timezone")).toBeFocused();
  expect((await store("snapshot")).settings).toEqual({
    ...original,
    launch_at_login: true,
  });
  await page.locator("#notification-enabled").check();
  await page.evaluate(() => window.__settingsIdle());
  // Healthy: the switch alone; no status line.
  await expect(page.locator("#notification-enabled")).toBeChecked();
  await expect(page.locator("#notification-permission")).toBeHidden();
  expect((await store("notification_snapshot")).enabled).toBe(true);
  await headerAutomation(page)
    .getByRole("button", { name: "Pause automation" })
    .click();
  await page.evaluate(() => window.__settingsIdle());
  expect((await store("automation_snapshot")).paused).toBe(true);
  expect((await store("snapshot")).settings).toEqual({
    ...original,
    launch_at_login: true,
  });
  await expect(page.locator("#global-capacity")).toHaveValue("17");
  await expect(page.locator("#global-timezone")).toHaveValue("Europe/Paris");
  await expect(page.locator("#automatic-publication")).toHaveCount(0);
  await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
  await expect(page.locator("#automation-settings")).toHaveCount(0);
  await capture(page, browserName, "immediate-controls");
  await page.locator("#notification-settings").scrollIntoViewIfNeeded();
  await capture(page, browserName, "native-status");
  await page.locator("#login-status").scrollIntoViewIfNeeded();
  await capture(page, browserName, "startup-registration");
  await page.locator("#reset-settings").click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#global-capacity")).toHaveValue("4");
  await expect(page.locator("#login")).toBeChecked();
  await expect(page.locator("#notification-enabled")).toBeChecked();
  await expect(
    headerAutomation(page).locator("[data-automation-status]"),
  ).toContainText("Paused;");
  expect(await page.evaluate(() => window.__preferenceCommands)).not.toContain(
    "save_resource",
  );
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#login")).toBeChecked();
  await expect(page.locator("#notification-enabled")).toBeChecked();
  await expect(
    headerAutomation(page).locator("[data-automation-status]"),
  ).toContainText("Paused;");
});

// Controlled user agents prove browser presentation, not native registration.
for (const { os, userAgent, startupSettings } of [
  {
    os: "macOS",
    userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
    startupSettings: "macOS Login Items",
  },
  {
    os: "Windows",
    userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    startupSettings: "Windows Startup Apps",
  },
]) {
  test.describe(`${os} browser startup presentation`, () => {
    test.use({ userAgent });

    for (const registration of ["absent", "registered", "invalid", null]) {
      test(`startup reports actual ${registration ?? "unavailable"} registration separately from saved request`, async ({
        page,
        store,
      }) => {
        const settings = (await store("snapshot")).settings;
        settings.launch_at_login = true;
        await store("seed_settings", settings);
        await syntheticNative(page, { registration });
        await ready(page, store);
        await expect(page.locator("#login")).toBeChecked();
        await expect(page.locator("#login-status")).toHaveText(
          `On; registration ${registration ?? "unavailable"}`,
        );
        if (registration === null)
          await expect(page.locator("#login")).toBeDisabled();
        else await expect(page.locator("#login")).toBeEnabled();
      });
    }
  });
}

for (const startupRead of ["available", "missing", "rejected"]) {
  test(`rejected startup with ${startupRead} reload never infers successful registration`, async ({
    page,
    store,
  }) => {
    await syntheticNative(page);
    await ready(page, store);
    const before = (await store("snapshot")).settings;
    await page.locator("#global-capacity").fill("23");
    await page.evaluate((startupRead) => {
      window.__nativePreferences.startupError =
        "Synthetic native registration rejected the startup request.";
      window.__nativePreferences.startupRead = startupRead;
    }, startupRead);
    await page.locator("#login").click();
    await page.evaluate(() => window.__settingsIdle());
    await expect(page.locator("#error")).toBeVisible();
    await expect(page.locator("#login")).not.toBeChecked();
    await expect(page.locator("#global-capacity")).toHaveValue("23");
    await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
    await expect(page.locator("#login-status")).toContainText(
      startupRead === "available"
        ? "Off; registration absent"
        : "Off; registration unavailable",
    );
    if (startupRead !== "available")
      await expect(page.locator("#login")).toBeDisabled();
    else
      await expect(page.locator("#error")).toContainText(
        "Synthetic native registration rejected",
      );
    expect((await store("snapshot")).settings).toEqual(before);
    if (startupRead === "available") {
      await savePreferences(page);
      await expect(page.locator("#login-error")).toContainText(
        "Synthetic native registration rejected the startup request.",
      );
      expect((await store("snapshot")).settings.launch_at_login).toBe(false);
    }
  });
}

test("denied and unreadable native notifications stay explicit without saving preferences", async ({
  page,
  store,
  browserName,
}) => {
  await syntheticNative(page, { denied: true });
  await page.clock.install();
  await ready(page, store, true);
  const before = (await store("snapshot")).settings;
  await page.locator("#global-capacity").fill("21");
  await page.locator("#notification-enabled").click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#notification-enabled")).not.toBeChecked();
  await expect(page.locator("#notification-error")).toContainText(
    "Synthetic OS permission denied",
  );
  await expect(
    page.locator("#notification-permission [data-notification-alert]"),
  ).toHaveText(/^Blocked by (macOS|Windows)$/);
  await page.locator("#notification-settings").scrollIntoViewIfNeeded();
  await capture(page, browserName, "permission-denied");
  await page.evaluate(() => {
    window.__nativePreferences.notificationReadError = true;
  });
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#notification-error")).toContainText(
    "Unavailable",
  );
  await expect(page.locator("#notification-enabled")).toBeDisabled();
  await expect(page.locator("#notification-types")).toBeHidden();
  await expect(page.locator("#notification-permission")).toBeHidden();
  await expect(page.locator("#global-capacity")).toHaveValue("21");
  expect((await store("notification_snapshot")).enabled).toBe(false);
  expect((await store("snapshot")).settings).toEqual(before);
});

test("an immediate notification operation and its rejection survive an unrelated preference save", async ({
  page,
  store,
}) => {
  await syntheticNative(page);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "set_notifications_enabled")
        return new Promise((_resolve, reject) => {
          window.__rejectNotification = () =>
            reject("Synthetic pending native opt-in was denied.");
        });
      return original(command, args);
    };
  });
  await ready(page, store);
  await page.locator("#global-capacity").fill("12");
  await page.locator("#notification-enabled").click();
  await expect(page.locator("#notification-enabled")).toBeDisabled();
  await savePreferences(page);
  await expect(page.locator("#notification-enabled")).toBeDisabled();
  await expect(page.locator("#notification-permission")).toBeHidden();
  await page.evaluate(() => window.__rejectNotification());
  await expect(page.locator("#notification-error")).toContainText(
    "Synthetic pending native opt-in was denied.",
  );
  await expect(page.locator("#notification-enabled")).not.toBeChecked();
  await page.locator("#global-capacity").fill("13");
  await savePreferences(page);
  await expect(page.locator("#notification-error")).toContainText(
    "Synthetic pending native opt-in was denied.",
  );
  expect((await store("notification_snapshot")).enabled).toBe(false);
  expect((await store("snapshot")).settings.capacity).toBe(13);
});

test("a pending pause and its rejection survive an unrelated preference save without showing paused state", async ({
  page,
  store,
}) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "set_automation_paused")
        return new Promise((_resolve, reject) => {
          window.__rejectPause = () =>
            reject("Synthetic native pause was rejected.");
        });
      return original(command, args);
    };
  });
  await ready(page, store, true);
  await page.locator("#global-capacity").fill("12");
  await headerAutomation(page)
    .getByRole("button", { name: "Pause automation" })
    .click();
  await savePreferences(page);
  await expect(
    headerAutomation(page).locator("[data-toggle-automation]"),
  ).toBeDisabled();
  await expect(
    headerAutomation(page).locator("[data-automation-status]"),
  ).toContainText("Pending");
  await page.evaluate(() => window.__rejectPause());
  await expect(
    headerAutomation(page).locator("[data-automation-error]"),
  ).toContainText("Synthetic native pause was rejected.");
  await expect(
    headerAutomation(page).locator("[data-automation-status]"),
  ).toContainText("Running;");
  // The panel toast mirrors the rejection; dismiss it to reach Save.
  await page.locator("[data-panel-error]").click();
  await page.locator("#global-capacity").fill("13");
  await savePreferences(page);
  await expect(
    headerAutomation(page).locator("[data-automation-error]"),
  ).toContainText("Synthetic native pause was rejected.");
  expect((await store("automation_snapshot")).paused).toBe(false);
});

for (const recovery of ["refresh", "remount"]) {
  test(`automation read recovery after ${recovery === "remount" ? "Settings navigation" : "refresh"} clears only the transient alert without a pause action`, async ({
    page,
    store,
  }) => {
    await automationFailures(page);
    await page.clock.install();
    await ready(page, store, true);
    const before = (await store("snapshot")).settings;
    const root = headerAutomation(page);
    const button = root.locator("[data-toggle-automation]");
    const error = root.locator("[data-automation-error]");
    await page.evaluate(() => {
      window.__automationFailures.readUnavailable = true;
    });
    await page.clock.fastForward(5000);
    await page.evaluate(() => window.__settingsIdle());
    await expect(root.locator("[data-automation-status]")).toHaveText(
      "Unavailable",
    );
    await expect(button).toBeDisabled();
    await expect(error).toHaveText(automationReadError);
    if (recovery === "remount") {
      // Settings navigation never remounts the header switch or fakes recovery.
      await section(page, "Doctrines");
      await section(page, "Preferences");
      await page.evaluate(() => window.__settingsIdle());
      await expect(root.locator("[data-automation-status]")).toHaveText(
        "Unavailable",
      );
    }
    await page.evaluate(() => {
      window.__automationFailures.readUnavailable = false;
    });
    await page.clock.fastForward(5000);
    await page.evaluate(() => window.__settingsIdle());
    await expect(root.locator("[data-automation-status]")).toHaveText(
      "Running; 0 occupied / 4 AI slots (0 stopping); 0 waiting; 0 blocked",
    );
    await expect(button).toBeEnabled();
    await expect(button).toHaveAccessibleName("Pause automation");
    await expect(error).toBeHidden();
    await expect(error).toHaveText("");
    await section(page, "Doctrines");
    await section(page, "Preferences");
    await page.evaluate(() => window.__settingsIdle());
    await expect(button).toBeEnabled();
    await expect(error).toBeHidden();
    await expect(error).toHaveText("");
    expect(
      await page.evaluate(() => window.__automationFailures.pauseRequests),
    ).toEqual([]);
    expect((await store("snapshot")).settings).toEqual(before);
    expect((await store("automation_snapshot")).paused).toBe(false);
  });
}

test("automation read recovery retains a rejected pause until another pause action", async ({
  page,
  store,
}) => {
  await automationFailures(page);
  await page.clock.install();
  await ready(page, store, true);
  const root = headerAutomation(page);
  const button = root.locator("[data-toggle-automation]");
  const error = root.locator("[data-automation-error]");
  await page.evaluate(() => {
    window.__automationFailures.rejectPause = true;
    window.__automationFailures.readUnavailable = true;
  });
  await button.click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(root.locator("[data-automation-status]")).toHaveText(
    "Unavailable",
  );
  await expect(button).toBeDisabled();
  await expect(error).toHaveText(
    `${automationActionError}\n${automationReadError}`,
  );
  await page.evaluate(() => {
    window.__automationFailures.readUnavailable = false;
  });
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await expect(root.locator("[data-automation-status]")).toContainText(
    "Running; 0 occupied / 4",
  );
  await expect(button).toBeEnabled();
  await expect(error).toHaveText(automationActionError);
  await section(page, "Doctrines");
  await section(page, "Preferences");
  await page.evaluate(() => window.__settingsIdle());
  await expect(error).toHaveText(automationActionError);
  expect((await store("automation_snapshot")).paused).toBe(false);
  await page.evaluate(() => {
    window.__automationFailures.rejectPause = false;
  });
  await button.click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(root.locator("[data-automation-status]")).toContainText(
    "Paused; 0 occupied / 4",
  );
  await expect(error).toBeHidden();
  await expect(error).toHaveText("");
  expect((await store("automation_snapshot")).paused).toBe(true);
  expect(
    await page.evaluate(() => window.__automationFailures.pauseRequests),
  ).toEqual([true, true]);
});

test("automation read recovery preserves pending pause and rejection across unrelated resource saves", async ({
  page,
  store,
}) => {
  await automationFailures(page);
  await ready(page, store, true);
  const root = headerAutomation(page);
  const button = root.locator("[data-toggle-automation]");
  const error = root.locator("[data-automation-error]");
  await page.locator("#global-capacity").fill("31");
  await page.evaluate(() => {
    window.__automationFailures.holdPause = true;
  });
  await button.click();
  await newDoctrine(page, "Pending pause", "A separate saved resource.");
  await section(page, "Preferences");
  await expect(button).toBeDisabled();
  await expect(root.locator("[data-automation-status]")).toContainText(
    "Pending",
  );
  await page.evaluate(() => window.__automationFailures.reject());
  await expect(error).toHaveText(automationActionError);
  await expect(button).toBeEnabled();
  await expect(root.locator("[data-automation-status]")).toContainText(
    "Running; 0 occupied / 4",
  );
  await newDoctrine(page, "Rejected pause", "Another separate resource.");
  await section(page, "Preferences");
  await page.evaluate(() => window.__settingsIdle());
  await expect(error).toHaveText(automationActionError);
  await expect(button).toBeEnabled();
  await expect(page.locator("#global-capacity")).toHaveValue("31");
  const settings = (await store("snapshot")).settings;
  expect(settings.capacity).toBe(4);
  const library = settings.doctrines.map(({ title, body }) => ({
    title,
    body,
  }));
  expect(library).toContainEqual({
    title: "Pending pause",
    body: "A separate saved resource.",
  });
  expect(library).toContainEqual({
    title: "Rejected pause",
    body: "Another separate resource.",
  });
  expect((await store("automation_snapshot")).paused).toBe(false);
  expect(
    await page.evaluate(() => window.__automationFailures.pauseRequests),
  ).toEqual([true]);
});

for (const staleRead of ["success", "failure"]) {
  test(`automation read recovery ignores an older ${staleRead} after a rejected pause and current read failure`, async ({
    page,
    store,
    ipc,
  }) => {
    await automationFailures(page);
    await page.clock.install();
    await ready(page, store, true);
    const root = headerAutomation(page);
    const button = root.locator("[data-toggle-automation]");
    const error = root.locator("[data-automation-error]");
    await page.evaluate((staleRead) => {
      // The first read of the tick is both the failing one and the held one.
      window.__automationFailures.failReads = staleRead === "failure" ? 1 : 0;
    }, staleRead);
    const older = ipc.holdNext("automation_snapshot");
    // The tick's monitor read straggles; its completion refreshes the header.
    const monitor = ipc.holdNext("monitoring_snapshot");
    await page.clock.fastForward(5000);
    await older.arrived;
    await monitor.arrived;
    await expect(button).toBeEnabled();
    await page.evaluate(() => {
      window.__automationFailures.rejectPause = true;
      window.__automationFailures.failReadsAfterPause = true;
    });
    monitor.release();
    await button.click();
    await expect(error).toHaveText(
      `${automationActionError}\n${automationReadError}`,
    );
    older.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(root.locator("[data-automation-status]")).toHaveText(
      "Unavailable",
    );
    await expect(button).toBeDisabled();
    await expect(error).toHaveText(
      `${automationActionError}\n${automationReadError}`,
    );
    await page.evaluate(() => {
      window.__automationFailures.failReadsAfterPause = false;
    });
    await page.clock.fastForward(5000);
    await page.evaluate(() => window.__settingsIdle());
    await expect(root.locator("[data-automation-status]")).toContainText(
      "Running; 0 occupied / 4",
    );
    await expect(button).toBeEnabled();
    await expect(error).toHaveText(automationActionError);
    expect((await store("automation_snapshot")).paused).toBe(false);
    expect(
      await page.evaluate(() => window.__automationFailures.pauseRequests),
    ).toEqual([true]);
  });
}

test("a background refresh between press and release never drops the Monitoring switch click", async ({
  page,
  store,
}) => {
  await page.clock.install();
  await ready(page, store, true);
  const button = headerAutomation(page).locator("[data-toggle-automation]");
  const box = await button.boundingBox();
  // Press on the label text, let a scheduled refresh land, then release.
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await page.mouse.up();
  await page.evaluate(() => window.__settingsIdle());
  await expect
    .poll(async () => (await store("automation_snapshot")).paused)
    .toBe(true);
  await expect(button).toHaveAccessibleName("Resume automation");
});

test("automation read recovery restores compact header status without a pause action", async ({
  page,
  store,
}) => {
  await automationFailures(page);
  await page.clock.install();
  await store("snapshot");
  await store("panel_snapshot");
  await page.goto("/");
  await page.evaluate(() => window.__settingsIdle());
  // This exercises the monitoring header, not Welcome's setup-only header.
  await tab(page, "Queue").click();
  await page.evaluate(() => window.__settingsIdle());
  const root = page.locator("#automation-controls");
  const button = root.locator("[data-toggle-automation]");
  const error = root.locator("[data-automation-error]");
  await expect(button).toBeEnabled();
  await expect(button).toHaveAccessibleName("Pause automation");
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await page.evaluate(() => {
    window.__automationFailures.readUnavailable = true;
  });
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await expect(button).toBeDisabled();
  await expect(button).toHaveAccessibleName("Monitoring unavailable");
  await expect(button).not.toHaveAttribute("aria-pressed");
  await expect(root.locator("[data-monitoring-label]")).toHaveText(
    "Unavailable",
  );
  await expect(root.locator("[data-automation-status]")).toHaveText(
    "Unavailable",
  );
  await expect(error).toHaveText(automationReadError);
  await page.evaluate(() => {
    window.__automationFailures.readUnavailable = false;
  });
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await expect(button).toBeEnabled();
  await expect(button).toHaveAccessibleName("Pause automation");
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await expect(root.locator("[data-monitoring-label]")).toHaveText(
    "Monitoring",
  );
  await expect(root.locator("[data-automation-status]")).toContainText(
    "Running; 0 occupied / 4",
  );
  await expect(error).toBeHidden();
  await expect(error).toHaveText("");
  expect((await store("automation_snapshot")).paused).toBe(false);
  expect(
    await page.evaluate(() => window.__automationFailures.pauseRequests),
  ).toEqual([]);
});

test("unavailable stored preferences are not replaced and recovery stays reachable", async ({
  page,
  store,
}) => {
  await store("snapshot");
  await store("panel_snapshot");
  const before = (await store("snapshot")).settings;
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      const value = await original(command, args);
      if (command === "snapshot")
        return {
          ...value,
          settings: null,
          error: "Synthetic unreadable settings; original data retained.",
        };
      return value;
    };
  });
  await page.goto("/");
  await page.evaluate(() => window.__settingsIdle());
  await tab(page, "Settings").click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#error")).toContainText(
    "Synthetic unreadable settings",
  );
  await expect(page.locator("#save-settings")).toBeDisabled();
  await expect(page.locator("#global-cron")).toHaveCount(0);
  await expect(page.locator("[data-panel-status]")).toHaveCount(0);
  expect((await store("snapshot")).settings).toEqual(before);
});

test("Copy diagnostics exports content-free text in place without disturbing the draft or focus", async ({
  page,
  store,
  browserName,
}) => {
  await queueFixture(store);
  const state = (await store("snapshot")).settings;
  state.agents[0].prompt = "SYNTHETIC_PRIVATE_PROMPT_NOT_A_CREDENTIAL";
  await store("seed_settings", state);
  await recordClipboard(page);
  await ready(page, store, true);
  await page.locator("#global-capacity").fill("19");
  await expect(page.locator("#preferences-status")).toHaveCount(0);
  await expect(page.locator("#diagnostics")).toHaveCount(0);
  const copy = page.getByRole("button", {
    name: "Copy diagnostics",
    exact: true,
  });
  await copy.focus();
  await page.keyboard.press("Enter");
  await expect(copy).toHaveAttribute("data-state", "copied");
  const [text] = await clipboardWrites(page);
  expect(text).toMatch(/^PR Sniper \S+ \u00b7 Isolated\n\n/);
  expect(text).toContain("Recorded job Intelligence\n");
  expect(text).not.toContain("SYNTHETIC_PRIVATE_PROMPT_NOT_A_CREDENTIAL");
  expect(text).not.toMatch(/gh[opsu]_|github_pat_|Bearer |refresh_token/);
  await expect(page.locator("[data-panel-heading]")).toHaveText("Preferences");
  await expect(copy).toBeFocused();
  await expect(page.locator("#global-capacity")).toHaveValue("19");
  await capture(page, browserName, "copy-diagnostics");
});

test("compact keyboard navigation, reduced motion and hide/reopen retain the exact draft controls", async ({
  page,
  store,
  browserName,
}) => {
  await ready(page, store, true);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 320, height: 300 });
  const capacity = page.locator("#global-capacity");
  await capacity.fill("9");
  await capacity.focus();
  await page.keyboard.press("Tab");
  await expect(page.locator("#global-timezone")).toBeFocused();
  await expect(page.locator("#global-timezone")).toBeInViewport();
  const field = await page.locator("#global-timezone").boundingBox();
  const scroll = await page.locator("#content").boundingBox();
  expect(field.y).toBeGreaterThanOrEqual(scroll.y);
  expect(field.y + field.height).toBeLessThanOrEqual(scroll.y + scroll.height);
  await chooseTimeZone(page, "Asia/Tokyo");
  await page.keyboard.press("Escape");
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator(".panel-shell")).toHaveAttribute(
    "data-native-visible",
    "false",
  );
  await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("fixture_show_panel"),
  );
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#global-timezone")).toBeFocused();
  await expect(page.locator("#global-timezone")).toHaveValue("Asia/Tokyo");
  await expect(tab(page, "Settings")).toBeInViewport();
  await expect(save(page)).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  expect(
    await page
      .locator(".preferences-view")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
  await capture(page, browserName, "320x300-keyboard");
  await page.locator("#copy-diagnostics").focus();
  await expect(page.locator("#copy-diagnostics")).toBeInViewport();
  await expect(capacity).toHaveValue("9");
});
