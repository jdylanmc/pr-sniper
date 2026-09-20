import { test as base, expect } from "@playwright/test";
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
          // Tauri rejects with the command's serialized error, not a JS Error.
          if ("error" in response) reject(response.error);
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

export const test = base.extend({
  store: async ({}, use) => {
    const root = await mkdtemp(join(tmpdir(), "pr-sniper-settings-"));
    try {
      await use((command, args) => invokeStore(root, command, args));
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  },
  page: async ({ page, store }, use) => {
    await page.exposeFunction("__settingsInvoke", store);
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = {
        invoke: (command, args) => window.__settingsInvoke(command, args),
      };
    });
    await use(page);
  },
});

export { expect };
