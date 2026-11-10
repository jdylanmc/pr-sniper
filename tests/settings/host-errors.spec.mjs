import { expect, test } from "./fixtures.mjs";
import { repositorySettings } from "./navigation.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const message = "Repository checks could not start.";
const checks = { kind: "checks" };
const raise = (store) =>
  store("fixture_host_error", { source: checks, message });
const recover = (store) => store("fixture_host_error", { source: checks });
const name = "fixture/repo-0";

async function seedRepository(store) {
  await store("save_repository", { repository: name });
  const { settings } = await store("snapshot");
  Object.assign(
    settings.repositories.find((r) => r.name === name),
    {
      provider_account_id: "22",
      provider_repository_id: "200",
      enabled: false,
      cron: "*/30 * * * *",
    },
  );
  await store("seed_settings", settings);
}

async function openSettings(page) {
  await page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await page.evaluate(() => window.__settingsIdle());
}

async function saveSchedule(page, every) {
  const editor = await repositorySettings(page, name);
  await expect(editor.locator("[data-schedule]")).not.toHaveAttribute(
    "aria-busy",
    "true",
  );
  await editor.locator("#schedule-every").selectOption(every);
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
}

async function gone(page) {
  await page.evaluate(() => window.__settingsIdle());
  await expect(page.locator("#error")).toBeHidden();
  await expect(
    page.locator("[role=alert]:visible", { hasText: message }),
  ).toHaveCount(0);
}

test("a dismissed background error stays gone across saves and reload until it occurs again", async ({
  page,
  store,
}) => {
  await seedRepository(store);
  await raise(store);
  const banner = page.locator("#error");

  await page.goto("/");
  await openSettings(page);
  await expect(banner).toBeVisible();
  await expect(banner).toHaveText(message);
  await banner.click();
  await gone(page);
  expect((await store("snapshot")).error).toBeNull();

  for (const every of ["15", "60"]) {
    await saveSchedule(page, every);
    await gone(page);
  }
  expect(
    (await store("snapshot")).settings.repositories.find((r) => r.name === name)
      .cron,
  ).not.toBe("*/30 * * * *");

  // The same source failing again before it recovers is the same occurrence.
  await raise(store);
  await page.reload();
  await openSettings(page);
  await gone(page);

  // Recovery ends the occurrence; the next failure is new and shows again.
  await recover(store);
  await raise(store);
  await page.reload();
  await openSettings(page);
  await expect(banner).toBeVisible();
  await expect(banner).toHaveText(message);
});

test("a shown background error is not shown again by a save or refresh", async ({
  page,
  store,
}) => {
  await seedRepository(store);
  await raise(store);
  await page.goto("/");
  await openSettings(page);
  const banner = page.locator("#error");
  await expect(banner).toHaveText(message);
  // The banner's own timeout hides it without dismissing the occurrence.
  await banner.evaluate((element) => (element.hidden = true));
  await saveSchedule(page, "15");
  await gone(page);
  // Refocus re-reads Settings, as every panel open does.
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await gone(page);
  expect((await store("snapshot")).error).toBe(message);
});

test("an older background error already shown stays hidden when a newer one ends", async ({
  page,
  store,
}) => {
  const journal = { kind: "journal" };
  const newer = "Diagnostic journal could not be written.";
  await raise(store);
  await page.goto("/");
  await openSettings(page);
  const banner = page.locator("#error");
  await expect(banner).toHaveText(message);
  const autoHide = () => banner.evaluate((element) => (element.hidden = true));
  const refocus = () =>
    page.evaluate(async () => {
      window.dispatchEvent(new Event("focus"));
      await window.__settingsIdle();
    });
  await autoHide();
  await store("fixture_host_error", { source: journal, message: newer });
  await refocus();
  await expect(banner).toHaveText(newer);
  await autoHide();
  await store("fixture_host_error", { source: journal });
  await refocus();
  await gone(page);
  expect((await store("snapshot")).error).toBe(message);
});
