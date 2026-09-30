import { test as base, expect } from "@playwright/test";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { bridge, target } from "./paths.mjs";

function invokeStore(root, command, args = {}) {
  return new Promise((resolve, reject) => {
    let inputError;
    const child = execFile(
      bridge,
      [root],
      { timeout: 10_000 },
      (error, stdout) => {
        if (error || inputError) {
          reject(error ?? inputError);
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
    child.stdin.on("error", (error) => {
      inputError = error;
    });
    child.stdin.end(JSON.stringify({ command, args }));
  });
}

export function createStoreScope(invoke) {
  const pending = new Set();
  let closing = false;
  return {
    invoke(...args) {
      if (closing)
        return Promise.reject(new Error("Store fixture is shutting down."));
      const request = Promise.resolve().then(() => invoke(...args));
      pending.add(request);
      void request.then(
        () => pending.delete(request),
        () => pending.delete(request),
      );
      return request;
    },
    async close() {
      closing = true;
      // Drain native operations, including calls whose browser already navigated
      // away. Their errors still go to the original callers.
      await Promise.allSettled([...pending]);
    },
  };
}

export const test = base.extend({
  dataRoot: async ({}, use) => {
    await mkdir(target, { recursive: true });
    const root = await mkdtemp(join(target, "pr-sniper-settings-"));
    try {
      await use(root);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  },
  store: async ({ dataRoot }, use) => {
    const scope = createStoreScope((command, args) =>
      invokeStore(dataRoot, command, args),
    );
    try {
      await use(scope.invoke);
    } finally {
      await scope.close();
    }
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
    try {
      await use(page);
    } finally {
      // Stop page timers before the dependent IPC/Store fixtures drain and the
      // dataRoot fixture removes their files.
      await page.close();
    }
  },
});

export { expect };
