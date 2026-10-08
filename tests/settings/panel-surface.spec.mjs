import { expect, test } from "./fixtures.mjs";
import { section } from "./navigation.mjs";

for (const scale of [1, 2]) {
  test.describe(`document silhouette at ${scale}x (not native proof)`, () => {
    test.use({
      viewport: { width: 408, height: 744 },
      deviceScaleFactor: scale,
    });

    test("transparent roots and rounded clipping contain editors, focus and fixed spill", async ({
      page,
    }, testInfo) => {
      await page.goto("/");
      const shell = page.locator(".panel-shell");
      await expect(shell).toBeVisible();
      const assertContour = async () => {
        expect(
          await page.evaluate(() => {
            const shell = document.querySelector(".panel-shell");
            const style = getComputedStyle(shell);
            const { width, height } = shell.getBoundingClientRect();
            const corners = [
              [1, 1],
              [width - 2, 1],
              [1, height - 2],
              [width - 2, height - 2],
            ];
            return {
              root: getComputedStyle(document.documentElement).backgroundColor,
              body: getComputedStyle(document.body).backgroundColor,
              radius: style.borderRadius,
              clip: style.clipPath,
              outside: corners.every(
                ([x, y]) =>
                  !document.elementFromPoint(x, y)?.closest(".panel-shell"),
              ),
              inside: !!document
                .elementFromPoint(width / 2, 2)
                ?.closest(".panel-shell"),
              overflow:
                document.documentElement.scrollHeight > height ||
                document.documentElement.scrollWidth > width,
            };
          }),
        ).toEqual({
          root: "rgba(0, 0, 0, 0)",
          body: "rgba(0, 0, 0, 0)",
          radius: "19px",
          clip: "inset(0px round 19px)",
          outside: true,
          inside: true,
          overflow: false,
        });
      };

      await assertContour();
      await page
        .getByRole("navigation", { name: "Application destinations" })
        .getByRole("button", { name: "Settings", exact: true })
        .click();
      await section(page, "Doctrines");
      await page
        .getByRole("button", { name: "New doctrine", exact: true })
        .click();
      const dialog = page.getByRole("dialog", { name: "New doctrine" });
      await dialog.getByLabel("Title", { exact: true }).fill("Contained draft");
      await dialog
        .getByRole("textbox", { name: "Principles", exact: true })
        .fill(
          "Retain keyboard access and the draft while the surface is clipped.",
        );
      await dialog
        .getByRole("button", { name: "Save doctrine", exact: true })
        .hover();
      await dialog.getByLabel("Title", { exact: true }).focus();
      await page.keyboard.press("Tab");
      await expect(
        dialog.getByRole("textbox", { name: "Principles", exact: true }),
      ).toBeFocused();
      for (const media of [
        { colorScheme: "dark" },
        { forcedColors: "active", reducedMotion: "reduce" },
        { forcedColors: "none", reducedMotion: "reduce" },
      ]) {
        await page.emulateMedia(media);
        await assertContour();
      }
      await page.setViewportSize({ width: 320, height: 360 });
      await dialog
        .getByRole("button", { name: "Save doctrine", exact: true })
        .scrollIntoViewIfNeeded();
      await expect(
        dialog.getByRole("button", { name: "Save doctrine", exact: true }),
      ).toBeInViewport();
      await assertContour();
      await shell.screenshot({
        path: testInfo.outputPath("synthetic-document-editor.png"),
        omitBackground: true,
      });

      // A fixed child is deliberately larger than the viewport. Hit testing
      // must still exclude all four corners; this is DOM, not AppKit evidence.
      await shell.evaluate((element) => {
        const spill = document.createElement("div");
        spill.dataset.surfaceSpill = "true";
        spill.style.cssText =
          "position:fixed;inset:-50px;z-index:9999;background:red";
        element.append(spill);
      });
      await assertContour();
      await page
        .locator("[data-surface-spill]")
        .evaluate((element) => element.remove());
      await expect(dialog.getByLabel("Title", { exact: true })).toHaveValue(
        "Contained draft",
      );
    });
  });
}
