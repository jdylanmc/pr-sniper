import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";

for (const platform of ["Windows", "Macintosh"]) {
  test.describe(platform, () => {
    test.use({ userAgent: `Mozilla/5.0 (${platform})` });

    test("existing surfaces describe this host, not another platform", async ({
      page,
    }) => {
      const windows = platform === "Windows";
      await page.goto("/?view=status");
      await expect(page.locator("footer")).toContainText(
        windows ? "system tray" : "menu bar",
      );
      await expect(
        page.getByRole("heading", {
          name: windows
            ? "System-tray host is running"
            : "Menu-bar host is running",
        }),
      ).toBeVisible();
      await page.goto("/?view=settings");
      await section(page, "Integrations");
      await expect(page.locator(".copilot-auth-card")).toContainText(
        windows ? "Windows Credential Manager" : "macOS Keychain",
      );
      if (windows)
        await expect(page.locator(".copilot-auth-card")).not.toContainText(
          "macOS",
        );
      await section(page, "Preferences");
      await expect(page.getByText(/Settings and logs live/)).toContainText(
        windows ? "Windows local application-data" : "macOS app-support",
      );
      await expect(
        page.getByRole("switch", { name: /Open PR Sniper at login/ }),
      ).toBeDisabled();
      await expect(page.getByText(/Isolated development run:/)).toContainText(
        windows ? "Windows startup apps" : "macOS login items",
      );
    });
  });
}
