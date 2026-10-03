import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "./fixtures.mjs";
import { newDoctrine, section, seedAgent } from "./navigation.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import { target } from "./paths.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const preferences = (page) => page.locator(".preferences-view");
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
      });
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        if (command === "automation_snapshot") {
          const fail = control.readUnavailable || control.failReads > 0;
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

test("compact saved controls persist exact cron, timezone and full u32 capacity without global action grants", async ({
  page,
  store,
  browserName,
}) => {
  const original = await seedAgent(store);
  await ready(page, store, true);
  await expect(page.locator("#global-cron")).toHaveValue("*/15 * * * *");
  await expect(page.locator("#cron-helper")).toHaveValue("*/15 * * * *");
  await expect(page.locator("#global-capacity")).toHaveValue("4");
  await expect(
    preferences(page).getByRole("switch", { name: /^Approve/ }),
  ).toHaveCount(0);
  await expect(
    preferences(page).getByRole("switch", { name: /^Merge/ }),
  ).toHaveCount(0);
  await expect(preferences(page)).toContainText(
    "normal passes, final primary reviews, replies and mentions",
  );
  await expect(preferences(page)).toContainText("independently of polling");
  await capture(page, browserName, "saved-preferences");
  await page.locator("#cron-helper").selectOption("0 9 * * MON-FRI");
  await page.locator("#global-timezone").fill("America/New_York");
  await page.locator("#automatic-review-start").check();
  for (const capacity of [1, 20, 4294967295]) {
    await page.locator("#global-capacity").fill(String(capacity));
    await savePreferences(page);
    const expected = structuredClone(original);
    expected.capacity = capacity;
    expected.defaults.automatic_agent_start = true;
    expected.defaults.schedule = {
      kind: "cron",
      expression: "0 9 * * MON-FRI",
      timezone: "America/New_York",
    };
    expect((await store("snapshot")).settings).toEqual(expected);
    await expect(page.locator("#cron-helper")).toHaveValue("0 9 * * MON-FRI");
    await expect(page.locator("#automatic-publication")).not.toBeChecked();
  }
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#global-capacity")).toHaveValue("4294967295");
  await expect(page.locator("#global-timezone")).toHaveValue(
    "America/New_York",
  );
  await expect(page.locator("#cron-helper")).toHaveValue("0 9 * * MON-FRI");
  await capture(page, browserName, "maximum-capacity");
  await page.locator(".preferences-schedule").scrollIntoViewIfNeeded();
  await capture(page, browserName, "global-schedule");
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

test("legacy interval stays explicit and the helper follows exact custom draft expressions", async ({
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
  await ready(page, store);
  await expect(preferences(page)).toContainText(
    "Saved legacy interval: 7 minutes",
  );
  await expect(page.locator("#global-cron")).toHaveValue("");
  await expect(page.locator("#cron-helper")).toHaveValue("");
  await page.locator("#global-capacity").fill("8");
  await savePreferences(page);
  expect((await store("snapshot")).settings.defaults.schedule).toEqual(
    settings.defaults.schedule,
  );
  await page.locator("#cron-helper").selectOption("0 * * * *");
  await page.locator("#global-cron").fill("5 10 * * MON");
  await expect(page.locator("#cron-helper")).toHaveValue("");
  await savePreferences(page);
  expect((await store("snapshot")).settings.defaults.schedule).toEqual({
    kind: "cron",
    expression: "5 10 * * MON",
    timezone: "Europe/London",
  });
});

test("failed preference and pause writes keep committed state, exact drafts and open disclosures", async ({
  page,
  store,
  dataRoot,
  browserName,
}) => {
  await ready(page, store, true);
  const before = await readFile(join(dataRoot, "config/settings.json"));
  await page.locator("#global-cron").fill("5 * * * *");
  await page.locator("#global-timezone").fill("Pacific/Auckland");
  await page.locator("#global-capacity").fill("33");
  await page.locator("#automatic-publication").check();
  await preferences(page)
    .getByText("Capacity work details", { exact: true })
    .click();
  await mkdir(join(dataRoot, "config/settings.json.tmp"));
  await save(page).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#error")).toBeVisible();
  await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
  await expect(page.locator("#global-cron")).toHaveValue("5 * * * *");
  await expect(page.locator("#global-timezone")).toHaveValue(
    "Pacific/Auckland",
  );
  await expect(page.locator("#global-capacity")).toHaveValue("33");
  await expect(page.locator("#automatic-publication")).toBeChecked();
  await expect(page.locator("#preferences-capacity-work")).toHaveAttribute(
    "open",
    "",
  );
  expect(await readFile(join(dataRoot, "config/settings.json"))).toEqual(
    before,
  );
  await mkdir(join(dataRoot, "state/automation.json.tmp"), { recursive: true });
  await preferences(page)
    .getByRole("button", { name: "Pause automation" })
    .click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(
    preferences(page).locator("[data-automation-error]"),
  ).toBeVisible();
  await expect(
    preferences(page).locator("[data-automation-status]"),
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
  await page.locator("#global-cron").fill("7 * * * *");
  await page.locator("#automatic-publication").check();
  const held = ipc.holdNext("seed_settings");
  await page.locator("#login").click();
  await held.arrived;
  await expect(page.locator("#login")).toBeDisabled();
  await expect(page.locator("#login-status")).toContainText(
    "Updating startup request",
  );
  await page.locator("#global-timezone").focus();
  await page.locator("#global-timezone").fill("Europe/Paris");
  held.release();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#login-status")).toContainText(
    "Saved request: On. Registration: registered.",
  );
  await expect(page.locator("#global-timezone")).toBeFocused();
  expect((await store("snapshot")).settings).toEqual({
    ...original,
    launch_at_login: true,
  });
  await page.locator("#notification-enabled").check();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#notification-permission")).toContainText(
    "banners unknown (not exposed by the OS API)",
  );
  expect((await store("notification_snapshot")).enabled).toBe(true);
  await preferences(page)
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
  await expect(page.locator("#automatic-publication")).toBeChecked();
  await expect(page.locator("#save-status")).toHaveText("Unsaved changes");
  await page.locator("#automation-settings").scrollIntoViewIfNeeded();
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
    preferences(page).locator("[data-automation-status]"),
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
    preferences(page).locator("[data-automation-status]"),
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
          `Saved request: On. Registration: ${registration ?? "unavailable"}. Registration is not effective ${os} launch state; ${startupSettings} can disable a registered entry.`,
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
        ? "Registration: absent."
        : "Registration: unavailable.",
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
  await expect(page.locator("#notification-permission")).toContainText(
    "permission: denied; banners disabled; Notification Center disabled",
  );
  await page.locator("#notification-settings").scrollIntoViewIfNeeded();
  await capture(page, browserName, "permission-denied");
  await page.evaluate(() => {
    window.__nativePreferences.notificationReadError = true;
  });
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#notification-error")).toContainText(
    "Notification state could not be read. No permission or delivery is assumed.",
  );
  await expect(page.locator("#notification-enabled")).toBeDisabled();
  await expect(page.locator("#notification-test")).toBeDisabled();
  await expect(page.locator("#notification-permission")).toContainText(
    "saved opt-in and OS permission are unknown",
  );
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
  await expect(page.locator("#notification-permission")).toContainText(
    "Notification operation pending",
  );
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
  await ready(page, store);
  await page.locator("#global-capacity").fill("12");
  await preferences(page)
    .getByRole("button", { name: "Pause automation" })
    .click();
  await savePreferences(page);
  await expect(
    preferences(page).locator("[data-toggle-automation]"),
  ).toBeDisabled();
  await expect(
    preferences(page).locator("[data-automation-status]"),
  ).toContainText("Automation change pending");
  await page.evaluate(() => window.__rejectPause());
  await expect(
    preferences(page).locator("[data-automation-error]"),
  ).toContainText("Synthetic native pause was rejected.");
  await expect(
    preferences(page).locator("[data-automation-status]"),
  ).toContainText("Running;");
  await page.locator("#global-capacity").fill("13");
  await savePreferences(page);
  await expect(
    preferences(page).locator("[data-automation-error]"),
  ).toContainText("Synthetic native pause was rejected.");
  expect((await store("automation_snapshot")).paused).toBe(false);
});

for (const recovery of ["refresh", "remount"]) {
  test(`automation read recovery by ${recovery} clears only the transient alert without a pause action`, async ({
    page,
    store,
  }) => {
    await automationFailures(page);
    await page.clock.install();
    await ready(page, store);
    const before = (await store("snapshot")).settings;
    const root = page.locator("#automation-settings");
    const button = root.locator("[data-toggle-automation]");
    const error = root.locator("[data-automation-error]");
    await page.evaluate(() => {
      window.__automationFailures.failReads = 1;
    });
    await page.clock.fastForward(5000);
    await page.evaluate(() => window.__settingsIdle());
    await expect(root.locator("[data-automation-status]")).toHaveText(
      "Automation state unavailable; occupancy is unknown.",
    );
    await expect(button).toBeDisabled();
    await expect(error).toBeVisible();
    await expect(error).toHaveText(automationReadError);

    if (recovery === "remount") {
      await section(page, "Doctrines");
      await section(page, "Preferences");
    } else await page.clock.fastForward(5000);
    await page.evaluate(() => window.__settingsIdle());
    await expect(root.locator("[data-automation-status]")).toHaveText(
      "Running; 0 occupied / 4 AI slots (0 stopping); 0 waiting; 0 blocked.",
    );
    await expect(button).toBeEnabled();
    await expect(button).toHaveText("Pause automation");
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
  await ready(page, store);
  const root = page.locator("#automation-settings");
  const button = root.locator("[data-toggle-automation]");
  const error = root.locator("[data-automation-error]");
  await page.evaluate(() => {
    window.__automationFailures.rejectPause = true;
    window.__automationFailures.failReads = 1;
  });
  await button.click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(root.locator("[data-automation-status]")).toHaveText(
    "Automation state unavailable; occupancy is unknown.",
  );
  await expect(button).toBeDisabled();
  await expect(error).toBeVisible();
  await expect(error).toHaveText(
    `${automationActionError}\n${automationReadError}`,
  );
  await page.clock.fastForward(5000);
  await page.evaluate(() => window.__settingsIdle());
  await expect(root.locator("[data-automation-status]")).toContainText(
    "Running; 0 occupied / 4",
  );
  await expect(button).toBeEnabled();
  await expect(error).toBeVisible();
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
  await ready(page, store);
  const root = page.locator("#automation-settings");
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
    "Automation change pending",
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
  await expect(error).toBeVisible();
  await expect(error).toHaveText(automationActionError);
  await expect(button).toBeEnabled();
  await expect(page.locator("#global-capacity")).toHaveValue("31");
  const settings = (await store("snapshot")).settings;
  expect(settings.capacity).toBe(4);
  expect(settings.doctrines).toContainEqual({
    title: "Pending pause",
    body: "A separate saved resource.",
  });
  expect(settings.doctrines).toContainEqual({
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
    await ready(page, store);
    const root = page.locator("#automation-settings");
    const button = root.locator("[data-toggle-automation]");
    const error = root.locator("[data-automation-error]");
    await page.evaluate((staleRead) => {
      window.__automationFailures.failReads = staleRead === "failure" ? 1 : 0;
    }, staleRead);
    const older = ipc.holdNext("automation_snapshot");
    await page.clock.fastForward(5000);
    await older.arrived;
    await page.evaluate(() => {
      window.__automationFailures.rejectPause = true;
      window.__automationFailures.failReads = 1;
    });
    await button.click();
    await expect(error).toHaveText(
      `${automationActionError}\n${automationReadError}`,
    );
    older.release();
    await page.evaluate(() => window.__settingsIdle());
    await expect(root.locator("[data-automation-status]")).toHaveText(
      "Automation state unavailable; occupancy is unknown.",
    );
    await expect(button).toBeDisabled();
    await expect(error).toHaveText(
      `${automationActionError}\n${automationReadError}`,
    );
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
    "Automation state unavailable; occupancy is unknown.",
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
  await page.locator("[data-panel-status]").click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("[data-panel-heading]")).toHaveText("Status");
  await expect(page.locator("#notification-history")).toBeVisible();
  expect((await store("snapshot")).settings).toEqual(before);
});

test("Status retains notification destinations, pending recovery and exact Back focus without exposing settings in diagnostics", async ({
  page,
  store,
  browserName,
}) => {
  await queueFixture(store);
  await store("set_notifications_enabled", { enabled: true });
  const notices = await store("observe_notifications");
  const failure = notices.notices.find(
    (notice) => notice.event.category === "failure",
  );
  const destination = await store("notification_destination", {
    id: failure.id,
  });
  const history = await store("notification_snapshot");
  const noticeIndex = history.notices.findIndex(
    (notice) => notice.id === failure.id,
  );
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "open_notification") {
        const destination = await original("notification_destination", args);
        window.__openedPreferenceNotice = { id: args.id, destination };
        if (destination.kind !== "queue_item")
          throw "Unexpected fixture destination.";
        return original("open_queue_item", { itemId: destination.item_id });
      }
      return original(command, args);
    };
  });
  const state = (await store("snapshot")).settings;
  state.agents[0].prompt = "SYNTHETIC_PRIVATE_PROMPT_NOT_A_CREDENTIAL";
  await store("seed_settings", state);
  await ready(page, store, true);
  await page.locator("#global-capacity").fill("19");
  const opener = page.locator("#preferences-status");
  await opener.focus();
  await page.keyboard.press("Enter");
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("[data-panel-heading]")).toHaveText("Status");
  await expect(page.locator("#schedule-health")).toBeVisible();
  await page.locator("#notification-history summary").click();
  await expect(page.locator("#notification-history article")).toHaveCount(3);
  await capture(page, browserName, "status-recovery");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(opener).toBeFocused();
  await expect(page.locator("#global-capacity")).toHaveValue("19");
  await opener.click();
  await page.evaluate(() => window.__settingsIdle());
  const noticesRoot = page.locator("#notification-history");
  await expect(noticesRoot.locator("details")).toHaveAttribute("open", "");
  await noticesRoot
    .getByRole("button", { name: "Open saved destination" })
    .nth(noticeIndex)
    .click();
  await page.evaluate(() => window.__settingsIdle());
  expect(await page.evaluate(() => window.__openedPreferenceNotice)).toEqual({
    id: failure.id,
    destination,
  });
  expect((await store("panel_snapshot")).route.detail).toEqual({
    type: "item",
    item_id: destination.item_id,
  });
  await expect(
    page.getByRole("button", { name: "Reconcile / retry publication" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Reconcile / retry publication" }),
  ).toBeDisabled();
  await capture(page, browserName, "pending-publication-recovery");
  await tab(page, "Settings").click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#global-capacity")).toHaveValue("19");
  await page.locator("#diagnostics").click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("[data-panel-heading]")).toHaveText("Diagnostics");
  await expect(page.locator('[data-panel-view="utility"] pre')).toBeVisible();
  await expect(page.locator('[data-panel-view="utility"]')).not.toContainText(
    "SYNTHETIC_PRIVATE_PROMPT_NOT_A_CREDENTIAL",
  );
  await capture(page, browserName, "redacted-diagnostics");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#global-capacity")).toHaveValue("19");
  await expect(page.locator("#diagnostics")).toBeFocused();
  await expect(page.locator("#global-capacity")).toBeVisible();
});

test("compact keyboard navigation, reduced motion and hide/reopen retain the exact draft controls", async ({
  page,
  store,
  browserName,
}) => {
  await ready(page, store, true);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 320, height: 300 });
  const cron = page.locator("#global-cron");
  await cron.fill("9 11 * * MON-FRI");
  await cron.focus();
  await page.keyboard.press("Tab");
  await expect(page.locator("#cron-helper")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.locator("#global-timezone")).toBeFocused();
  await expect(page.locator("#global-timezone")).toBeInViewport();
  const field = await page.locator("#global-timezone").boundingBox();
  const scroll = await page.locator("#content").boundingBox();
  expect(field.y).toBeGreaterThanOrEqual(scroll.y);
  expect(field.y + field.height).toBeLessThanOrEqual(scroll.y + scroll.height);
  await page.locator("#global-timezone").fill("Asia/Tokyo");
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
  await page.locator("#preferences-status").focus();
  await expect(page.locator("#preferences-status")).toBeInViewport();
  await page.keyboard.press("Enter");
  await page.evaluate(() => window.__settingsIdle());
  await expect(
    page.getByRole("button", { name: "Back", exact: true }),
  ).toBeInViewport();
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(cron).toHaveValue("9 11 * * MON-FRI");
});
