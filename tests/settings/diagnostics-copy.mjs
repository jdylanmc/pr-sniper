import { expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";

// Replaces navigator.clipboard with a recorder; no system clipboard is used.
export async function recordClipboard(page, { fail = false } = {}) {
  await page.addInitScript((fail) => {
    window.__clipboardWrites = [];
    const record = async (text) => {
      if (fail) throw new DOMException("Synthetic denial", "NotAllowedError");
      window.__clipboardWrites.push(text);
    };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (text) => record(text),
        write: async (items) => {
          const blob = await items[0].getType("text/plain");
          await record(await blob.text());
        },
      },
    });
  }, fail);
}

export const clipboardWrites = (page) =>
  page.evaluate(() => window.__clipboardWrites);

// Opens Settings > Preferences (panel or component harness) and copies.
export async function copyDiagnostics(page, { panel = false } = {}) {
  if (!(await page.locator("#copy-diagnostics").count())) {
    if (panel) {
      await page
        .getByRole("navigation", { name: "Application destinations" })
        .getByRole("button", { name: "Settings", exact: true })
        .click();
      await page.evaluate(() => window.__settingsIdle());
    }
    await section(page, "Preferences");
    await page.evaluate(() => window.__settingsIdle());
  }
  const button = page.getByRole("button", {
    name: "Copy diagnostics",
    exact: true,
  });
  const before = (await clipboardWrites(page)).length;
  await button.click();
  await expect(button).toHaveAttribute("data-state", /copied|failed/);
  const writes = await clipboardWrites(page);
  return writes.length > before ? writes.at(-1) : undefined;
}
