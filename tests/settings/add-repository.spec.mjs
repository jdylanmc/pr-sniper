import { expect, test } from "@playwright/test";
import { execFile } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const bridge = fileURLToPath(
  new URL(
    "../../src-tauri/target/debug/examples/settings_bridge",
    import.meta.url,
  ),
);

function invokeStore(root, command, args = {}) {
  return new Promise((resolve, reject) => {
    const child = execFile(
      bridge,
      [root],
      { timeout: 10_000 },
      (error, stdout) => {
        if (error) {
          reject(error);
          return;
        }
        try {
          const response = JSON.parse(stdout);
          if ("error" in response) reject(new Error(response.error));
          else if ("ok" in response) resolve(response.ok);
          else reject(new Error("Invalid Store bridge response"));
        } catch (cause) {
          reject(cause);
        }
      },
    );
    child.stdin.on("error", reject);
    child.stdin.end(JSON.stringify({ command, args }));
  });
}

test("adding a repository in Settings persists its canonical name after restart", async ({
  page,
}) => {
  const root = await mkdtemp(join(tmpdir(), "pr-sniper-settings-"));
  try {
    await invokeStore(root, "seed_settings", { launch_at_login: true });
    await page.exposeFunction("__settingsInvoke", (command, args) =>
      invokeStore(root, command, args),
    );
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = {
        invoke: (command, args) => window.__settingsInvoke(command, args),
      };
    });

    await test.step("existing Settings renders real persisted startup preference", async () => {
      await page.goto("/?view=settings");
      await expect(
        page.getByRole("heading", { name: "Settings", exact: true }),
      ).toBeVisible();
      await expect(page.getByLabel("Request launch at login")).toBeChecked();
      await expect(page.getByLabel("Request launch at login")).toBeDisabled();
      await expect(page.getByRole("alert")).toBeHidden();
    });

    await test.step("add one GitHub repository using Settings", async () => {
      await expect(
        page.getByRole("button", { name: "Add repository", exact: true }),
      ).toBeVisible();
      await page
        .getByLabel("GitHub repository", { exact: true })
        .fill("Octo/Hello-World");
      await page
        .getByRole("button", { name: "Add repository", exact: true })
        .click();
      await expect(
        page.getByText("octo/hello-world", { exact: true }),
      ).toBeVisible();
      await expect(page.getByRole("alert")).toBeHidden();
    });

    await test.step("fresh Store process reads canonical data without losing host settings", async () => {
      const saved = await invokeStore(root, "snapshot");
      expect(saved.settings.launch_at_login).toBe(true);
      expect(saved.settings.repositories).toEqual([
        expect.objectContaining({
          provider: "github",
          name: "octo/hello-world",
          enabled: true,
        }),
      ]);
    });

    await test.step("reloaded Settings reads the saved repository again", async () => {
      await page.reload();
      await expect(
        page.getByText("octo/hello-world", { exact: true }),
      ).toBeVisible();
      await expect(page.getByLabel("Request launch at login")).toBeChecked();
      await expect(page.getByRole("alert")).toBeHidden();
    });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
