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
  ipc: async ({ store }, use) => {
    const next = new Map();
    const gates = new Set();
    const holdNext = (command) => {
      if (next.has(command)) throw new Error(`Already holding ${command}`);
      const arrived = Promise.withResolvers();
      const gate = Promise.withResolvers();
      const hold = { arrived, gate };
      next.set(command, hold);
      gates.add(gate);
      return { arrived: arrived.promise, release: () => gate.resolve() };
    };
    const invoke = async (command, args) => {
      const hold = next.get(command);
      if (!hold) return store(command, args);
      next.delete(command);
      // Run the real Store first, then hold only the external IPC reply.
      const result = await store(command, args).then(
        (value) => ({ value }),
        (error) => ({ error }),
      );
      hold.arrived.resolve();
      await hold.gate.promise;
      gates.delete(hold.gate);
      if ("error" in result) throw result.error;
      return result.value;
    };
    try {
      await use({ holdNext, invoke });
    } finally {
      for (const gate of gates) gate.resolve();
    }
  },
  page: async ({ page, ipc }, use) => {
    await page.exposeFunction("__settingsInvoke", ipc.invoke);
    await page.addInitScript(() => {
      const pending = new Set();
      window.__TAURI_INTERNALS__ = {
        invoke: (command, args) => {
          if (["github_auth_state", "copilot_auth_state"].includes(command))
            return Promise.resolve({ accounts: [], flow: { state: "idle" } });
          const request = window.__settingsInvoke(command, args);
          const settled = request.finally(() => pending.delete(settled));
          pending.add(settled);
          return settled;
        },
      };
      window.__settingsIdle = async () => {
        while (pending.size) await Promise.allSettled([...pending]);
      };
    });
    await use(page);
  },
});

export { expect };
