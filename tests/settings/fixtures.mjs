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
  let tail = Promise.resolve();
  let closing = false;
  return {
    invoke(...args) {
      if (closing)
        return Promise.reject(new Error("Store fixture is shutting down."));
      // Match Host.store's mutex, independently for each fixture root.
      const request = tail.then(() => invoke(...args));
      tail = request.then(
        () => {},
        () => {},
      );
      return request;
    },
    async close() {
      closing = true;
      // The tail drains every accepted operation, even after rejection or
      // navigation. Each original request still carries its own result/error.
      await tail;
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
    const holdNext = (command, afterRequests = 0) => {
      if (next.has(command)) throw new Error(`Already holding ${command}`);
      const arrived = Promise.withResolvers();
      const gate = Promise.withResolvers();
      const hold = { arrived, gate, afterRequests };
      next.set(command, hold);
      gates.add(gate);
      return { arrived: arrived.promise, release: () => gate.resolve() };
    };
    const invoke = async (command, args) => {
      const hold = next.get(command);
      if (!hold) return store(command, args);
      if (hold.afterRequests > 0) {
        hold.afterRequests--;
        return store(command, args);
      }
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
      const callbacks = new Map();
      const listeners = new Map();
      let callbackId = 0;
      window.__panelEvent = (payload) => {
        for (const [id, { event, handler }] of listeners) {
          if (event === "pr-sniper:panel")
            callbacks.get(handler)?.({ event, id, payload });
        }
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (_event, id) => listeners.delete(id),
      };
      window.__TAURI_INTERNALS__ = {
        transformCallback: (handler) => {
          const id = ++callbackId;
          callbacks.set(id, handler);
          return id;
        },
        invoke: (command, args) => {
          if (command === "plugin:event|listen") {
            const id = ++callbackId;
            listeners.set(id, args);
            return Promise.resolve(id);
          }
          if (command === "plugin:event|unlisten") {
            listeners.delete(args.eventId);
            return Promise.resolve();
          }
          if (["github_auth_state", "copilot_auth_state"].includes(command))
            return Promise.resolve({ accounts: [], flow: { state: "idle" } });
          const request = window
            .__settingsInvoke(command, args)
            .then((value) => {
              if (
                [
                  "panel_navigate",
                  "hide_panel",
                  "open_settings",
                  "open_diagnostics",
                  "open_queue_item",
                  "fixture_show_panel",
                ].includes(command)
              )
                window.__panelEvent(value);
              return value;
            })
            .catch(async (error) => {
              if (command === "open_queue_item") {
                // Native routing emits the unavailable destination before returning its error.
                window.__panelEvent(
                  await window.__settingsInvoke("panel_snapshot", {}),
                );
              }
              throw error;
            });
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

export async function nativeCapacity(page, activeIds) {
  await page.addInitScript((activeIds) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__activeIds = activeIds;
    window.__stoppingIds = [];
    window.__capacityUnavailable = false;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "automation_snapshot") {
        if (window.__capacityUnavailable)
          return Promise.reject("Synthetic capacity failure");
        return original("fixture_capacity_snapshot", {
          activeIds: window.__activeIds,
          stoppingIds: window.__stoppingIds,
        });
      }
      return original(command, args);
    };
  }, activeIds);
}

export async function captureInspector(page, name) {
  const directory = join(target, "visual-correction-1");
  await mkdir(directory, { recursive: true });
  await page.setViewportSize({ width: 408, height: 744 });
  await page.locator(".panel-content").evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(page.locator("[data-panel-heading]")).toHaveText("Job details");
  await expect(page.locator(".panel-art")).toBeVisible();
  await expect(page.locator("[data-monitor-detail]")).toHaveCount(1);
  await expect(page.locator("[data-work-context]")).toHaveCount(1);
  await expect(page.locator(".job-provider-link")).toBeVisible();
  await page.screenshot({ path: join(directory, `${name}.png`) });
  const configuration = page.locator(
    "[data-work-context] > .work-configuration",
  );
  await configuration.evaluate((element) =>
    element.scrollIntoView({ block: "start" }),
  );
  await page.screenshot({ path: join(directory, `${name}-configuration.png`) });
  await page.locator(".panel-content").evaluate((element) => {
    element.scrollTop = 0;
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 320, height: 300 });
  await expect(page.locator(".job-hero")).toBeInViewport();
  await expect(page.locator("[data-panel-navigation]")).toBeInViewport();
  await expect(
    page.getByRole("button", { name: "Back", exact: true }),
  ).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  for (const spinner of await page.locator(".job-hero .work-spin").all())
    expect(
      await spinner.evaluate(
        (element) => getComputedStyle(element).animationName,
      ),
    ).toBe("none");
  await page.screenshot({
    path: join(directory, `${name}-320x300-reduced.png`),
  });
  await page
    .locator("[data-work-context] > .work-configuration summary")
    .first()
    .focus();
  await expect(
    page.locator("[data-work-context] > .work-configuration summary").first(),
  ).toBeInViewport();
  await page.setViewportSize({ width: 408, height: 744 });
}
