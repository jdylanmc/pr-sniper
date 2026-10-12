import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";

for (const platform of ["Windows", "Macintosh"]) {
  test.describe(platform, () => {
    test.use({ userAgent: `Mozilla/5.0 (${platform})` });

    test("existing surfaces describe this host, not another platform", async ({
      page,
    }) => {
      await page.goto("/?view=status");
      await expect(page.locator("body")).not.toContainText(
        /menu bar|menu-bar|system tray|system-tray/i,
      );
      await page.goto("/?view=settings");
      await section(page, "Integrations");
      await expect(page.locator(".copilot-auth-card")).not.toContainText(
        /Keychain|Credential Manager/,
      );
      await section(page, "Preferences");
      await expect(
        page.getByRole("switch", { name: /Open PR Sniper at login/ }),
      ).toBeDisabled();
      await expect(page.locator("#login-status")).toHaveText(/isolated/);
      await expect(page.locator("#login-status")).not.toContainText(
        /Windows|macOS|login items|startup apps/i,
      );
    });
  });
}
