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
  dataRoot: async ({}, use) => {
    const root = await mkdtemp(join(tmpdir(), "pr-sniper-settings-"));
    try {
      await use(root);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  },
  store: async ({ dataRoot }, use) => {
    await use((command, args) => invokeStore(dataRoot, command, args));
  },
  page: async ({ page, store }, use) => {
    await page.exposeFunction("__settingsInvoke", store);
    await page.addInitScript(() => {
      const pending = new Set();
      window.__TAURI_INTERNALS__ = {
        invoke: (command, args) => {
          const request = window.__settingsInvoke(command, args);
          const settled = request.finally(() => pending.delete(settled));
          pending.add(settled);
          return settled;
        },
      };
      window.__settingsIdle = () => Promise.all([...pending]);
    });
    await use(page);
  },
});

export { expect };
