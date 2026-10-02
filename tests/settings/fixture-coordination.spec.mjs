import { spawn } from "node:child_process";
import {
  mkdir,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { expect, test as base } from "./fixtures.mjs";
import { bridge } from "./paths.mjs";

const sessionPath = (root) => join(root, "fixture-panel-session.json");
const temporaryFiles = async (root) =>
  (await readdir(root)).filter((name) =>
    name.startsWith(".fixture-panel-session-"),
  );

// Only this fixture owns these children. The normal Store/IPC helper is unchanged.
const test = base.extend({
  probe: async ({ dataRoot }, use) => {
    const children = [];
    const start = (command, args = {}, stage = "loaded", root = dataRoot) => {
      const child = spawn(bridge, [root, `--panel-probe=${stage}`], {
        stdio: ["pipe", "pipe", "pipe"],
      });
      const closed = Promise.withResolvers();
      let changed = Promise.withResolvers();
      const events = [];
      const inputErrors = [];
      let stdout = "";
      let stderr = "";
      let exit;
      let spawnError;
      child.on("error", (error) => {
        spawnError = error;
      });
      child.stdin.on("error", (error) => inputErrors.push(error));
      child.stdout.on("data", (chunk) => {
        stdout += chunk;
      });
      child.stderr.on("data", (chunk) => {
        stderr += chunk;
      });
      const lines = createInterface({ input: child.stderr });
      lines.on("line", (line) => {
        if (line.startsWith("fixture-panel:")) {
          events.push(line.slice("fixture-panel:".length));
          changed.resolve();
          changed = Promise.withResolvers();
        }
      });
      child.on("close", (code, signal) => {
        exit = { code, signal };
        lines.close();
        changed.resolve();
        closed.resolve();
      });
      const wait = async (stage) => {
        while (stage ? !events.includes(stage) : !events.length) {
          if (exit)
            throw new Error(
              `Probe exited before ${stage ?? "arrival"}: ${JSON.stringify({ exit, stdout, stderr, spawnError })}`,
            );
          await changed.promise;
        }
        return events[0];
      };
      const owned = {
        wait,
        release: () => child.stdin.end("continue\n"),
        abort: () => child.stdin.end(),
        async response() {
          await closed.promise;
          expect(spawnError).toBeUndefined();
          expect(inputErrors).toEqual([]);
          expect(exit, stderr).toEqual({ code: 0, signal: null });
          return JSON.parse(stdout);
        },
        async stop() {
          if (
            child.pid &&
            child.exitCode === null &&
            child.signalCode === null
          ) {
            expect(child.kill("SIGKILL")).toBe(true);
          }
          await closed.promise;
          return exit;
        },
      };
      children.push(owned);
      child.stdin.write(`${JSON.stringify({ command, args })}\n`);
      return owned;
    };
    try {
      await use(start);
    } finally {
      await Promise.all(children.map((child) => child.stop()));
    }
  },
});

test("overlapping panel mutations retain both revisions and the newer route", async ({
  store,
  probe,
}) => {
  const before = await store("fixture_show_panel");
  const navigate = probe("open_settings");
  await navigate.wait("loaded");
  const hide = probe("hide_panel");
  // A loaded stale reader (old bridge) and an OS-lock waiter both reach this barrier.
  await hide.wait();
  navigate.release();
  expect(await navigate.response()).toEqual({
    ok: {
      ...before,
      route: { tab: "settings" },
      revision: before.revision + 2,
    },
  });
  await hide.wait("loaded");
  hide.release();
  const expected = {
    ...before,
    route: { tab: "settings" },
    visible: false,
    revision: before.revision + 3,
  };
  expect(await hide.response()).toEqual({ ok: expected });
  expect(await store("panel_snapshot")).toEqual(expected);
});

test("partial panel writes never publish torn JSON and snapshots observe the completed mutation", async ({
  dataRoot,
  store,
  probe,
}) => {
  const before = await store("fixture_show_panel");
  const saved = JSON.parse(await readFile(sessionPath(dataRoot), "utf8"));
  const writer = probe("open_settings", {}, "staged");
  await writer.wait("staged");
  expect(JSON.parse(await readFile(sessionPath(dataRoot), "utf8"))).toEqual(
    saved,
  );
  const reader = probe("panel_snapshot");
  expect(await reader.wait()).toBe("waiting");
  writer.release();
  const expected = {
    ...before,
    route: { tab: "settings" },
    revision: before.revision + 2,
  };
  expect(await writer.response()).toEqual({ ok: expected });
  await reader.wait("loaded");
  reader.release();
  expect(await reader.response()).toEqual({ ok: expected });
  expect(await temporaryFiles(dataRoot)).toEqual([]);
});

test("ordinary concurrent bridge commands preserve every mutation and complete JSON", async ({
  store,
}) => {
  const before = await store("fixture_show_panel");
  const responses = await Promise.all(
    Array.from({ length: 24 }, async () => {
      const [hidden, snapshot] = await Promise.all([
        store("hide_panel"),
        store("panel_snapshot"),
      ]);
      expect(snapshot.route).toEqual(before.route);
      expect(snapshot.revision).toBeGreaterThanOrEqual(before.revision);
      expect(snapshot.revision).toBeLessThanOrEqual(before.revision + 24);
      return hidden;
    }),
  );
  expect(
    responses.map((response) => response.revision).sort((a, b) => a - b),
  ).toEqual(
    Array.from({ length: 24 }, (_, index) => before.revision + index + 1),
  );
  for (const response of responses)
    expect(response).toEqual({
      ...before,
      visible: false,
      revision: response.revision,
    });
  expect(await store("panel_snapshot")).toEqual({
    ...before,
    visible: false,
    revision: before.revision + 24,
  });
});

test("terminating an owned partial writer releases the lock and preserves the last complete session", async ({
  dataRoot,
  store,
  probe,
}) => {
  const before = await store("fixture_show_panel");
  const bytes = await readFile(sessionPath(dataRoot), "utf8");
  const writer = probe("open_settings", {}, "staged");
  await writer.wait("staged");
  const next = probe("hide_panel");
  expect(await next.wait()).toBe("waiting");
  const exit = await writer.stop();
  expect(exit.code !== 0 || exit.signal !== null).toBe(true);
  await next.wait("loaded");
  expect(await readFile(sessionPath(dataRoot), "utf8")).toBe(bytes);
  next.release();
  const expected = { ...before, visible: false, revision: before.revision + 1 };
  expect(await next.response()).toEqual({ ok: expected });
  expect(await store("panel_snapshot")).toEqual(expected);
  // Abrupt death cannot run tempfile's destructor; only the root's owner removes leftovers.
  const orphans = await temporaryFiles(dataRoot);
  expect(orphans).toHaveLength(1);
  for (const name of orphans) {
    const partial = await readFile(join(dataRoot, name), "utf8");
    expect(() => JSON.parse(partial)).toThrow(SyntaxError);
    await rm(join(dataRoot, name));
  }
  expect(await temporaryFiles(dataRoot)).toEqual([]);
});

test("a held panel owner does not block another root or non-panel Store operations", async ({
  dataRoot,
  store,
  probe,
}) => {
  await store("fixture_show_panel");
  const holder = probe("open_settings");
  await holder.wait("loaded");
  const otherRoot = join(dataRoot, "independent-root");
  await mkdir(otherRoot);
  const independent = probe("fixture_show_panel", {}, "loaded", otherRoot);
  await independent.wait("loaded");
  independent.release();
  expect((await independent.response()).ok).toMatchObject({
    route: { tab: "queue" },
    visible: true,
  });
  const saved = await store("save_repository", {
    repository: "example/parallel",
  });
  expect(saved.warning).toBeNull();
  expect(
    saved.settings.repositories.map((repository) => repository.name),
  ).toEqual(["example/parallel"]);
  expect((await store("snapshot")).settings.repositories).toEqual(
    saved.settings.repositories,
  );
  holder.release();
  expect((await holder.response()).ok.route).toEqual({ tab: "settings" });
});

test("held IPC replies retain concurrency after panel ownership ends", async ({
  store,
  ipc,
}) => {
  const before = await store("fixture_show_panel");
  const held = ipc.holdNext("panel_navigate");
  const reply = ipc.invoke("panel_navigate", { route: { tab: "settings" } });
  try {
    await held.arrived;
    const newer = await store("hide_panel");
    expect(newer).toEqual({
      ...before,
      route: { tab: "settings" },
      visible: false,
      revision: before.revision + 3,
    });
    const saved = await store("save_repository", {
      repository: "example/held-reply",
    });
    expect(saved.warning).toBeNull();
    expect(saved.settings.repositories[0].name).toBe("example/held-reply");
    held.release();
    expect(await reply).toEqual({
      ...before,
      route: { tab: "settings" },
      revision: before.revision + 2,
    });
    expect(await store("panel_snapshot")).toEqual(newer);
  } finally {
    held.release();
    await reply;
  }
});

test("malformed session bytes are refused without replacement or successful defaults", async ({
  dataRoot,
  store,
}) => {
  const before = await store("fixture_show_panel");
  const good = await readFile(sessionPath(dataRoot), "utf8");
  for (const bad of ["", "{", "null", "{}"]) {
    await writeFile(sessionPath(dataRoot), bad);
    for (const command of ["panel_snapshot", "hide_panel", "open_settings"]) {
      await expect(store(command)).rejects.toBe(
        "Invalid panel fixture session.",
      );
      expect(await readFile(sessionPath(dataRoot), "utf8")).toBe(bad);
    }
  }
  await writeFile(sessionPath(dataRoot), good);
  expect(await store("panel_snapshot")).toEqual(before);
});

test("session read and lock-open failures remain explicit and release resources", async ({
  dataRoot,
  store,
}) => {
  const path = sessionPath(dataRoot);
  await mkdir(path);
  await expect(store("panel_snapshot")).rejects.toBe(
    "Cannot read panel fixture session.",
  );
  await rm(path, { recursive: true });
  const lock = join(dataRoot, "fixture-panel-session.lock");
  await rm(lock, { force: true });
  await mkdir(lock);
  await expect(store("hide_panel")).rejects.toBe(
    "Cannot lock panel fixture session.",
  );
  await rm(lock, { recursive: true });
  expect(await store("fixture_show_panel")).toMatchObject({
    route: { tab: "queue" },
    visible: true,
  });
});

test("failed replacement propagates its error, removes temporary data and releases ownership", async ({
  dataRoot,
  store,
  probe,
}) => {
  const before = await store("fixture_show_panel");
  const path = sessionPath(dataRoot);
  const backup = join(dataRoot, "saved-panel.json");
  const writer = probe("open_settings", {}, "staged");
  await writer.wait("staged");
  await rename(path, backup);
  await mkdir(path);
  await writeFile(join(path, "sentinel"), "keep");
  writer.release();
  expect(await writer.response()).toEqual({
    error: "Cannot save panel fixture session.",
  });
  expect(await readFile(join(path, "sentinel"), "utf8")).toBe("keep");
  expect(await temporaryFiles(dataRoot)).toEqual([]);
  await rm(path, { recursive: true });
  await rename(backup, path);
  expect(await store("panel_snapshot")).toEqual(before);
});

for (const stage of ["loaded", "staged"]) {
  test(`closed ${stage} probe input reports failure and cleans up without a commit`, async ({
    dataRoot,
    store,
    probe,
  }) => {
    const before = await store("fixture_show_panel");
    const bytes = await readFile(sessionPath(dataRoot), "utf8");
    const writer = probe("open_settings", {}, stage);
    await writer.wait(stage);
    writer.abort();
    expect(await writer.response()).toEqual({
      error: "Panel fixture probe was not released.",
    });
    expect(await readFile(sessionPath(dataRoot), "utf8")).toBe(bytes);
    expect(await temporaryFiles(dataRoot)).toEqual([]);
    expect(await store("panel_snapshot")).toEqual(before);
  });
}

test("panel lock contention has a bounded explicit failure without stealing ownership", async ({
  store,
  probe,
}) => {
  const before = await store("fixture_show_panel");
  const holder = probe("panel_snapshot");
  await holder.wait("loaded");
  const contender = probe("hide_panel");
  expect(await contender.wait()).toBe("waiting");
  expect(await contender.response()).toEqual({
    error: "Timed out waiting for panel fixture session.",
  });
  holder.release();
  expect(await holder.response()).toEqual({ ok: before });
  expect(await store("hide_panel")).toEqual({
    ...before,
    visible: false,
    revision: before.revision + 1,
  });
});

test("native panel routes, visibility, revisions and missing-destination errors retain their contracts", async ({
  dataRoot,
  store,
}) => {
  const initial = await store("panel_snapshot");
  expect(initial).toEqual({
    route: { tab: "queue" },
    revision: 2,
    visible: true,
    missing: null,
    placement_warning: null,
  });
  await expect(readFile(sessionPath(dataRoot))).rejects.toMatchObject({
    code: "ENOENT",
  });
  let expected = { ...initial, revision: 4 };
  expect(await store("fixture_show_panel")).toEqual(expected);
  for (const [command, args, route, visible, increment] of [
    ["open_settings", {}, { tab: "settings" }, true, 2],
    ["hide_panel", {}, { tab: "settings" }, false, 1],
    ["fixture_show_panel", {}, { tab: "settings" }, true, 2],
    [
      "open_diagnostics",
      {},
      { tab: "settings", detail: { type: "diagnostics" } },
      true,
      2,
    ],
    [
      "panel_navigate",
      { route: { tab: "reviewed" } },
      { tab: "reviewed" },
      true,
      2,
    ],
  ]) {
    expected = {
      ...expected,
      route,
      visible,
      revision: expected.revision + increment,
    };
    expect(await store(command, args)).toEqual(expected);
    expect(await store("panel_snapshot")).toEqual(expected);
  }
  const bytes = await readFile(sessionPath(dataRoot), "utf8");
  await expect(
    store("panel_navigate", { route: { tab: "unknown" } }),
  ).rejects.toBe("Invalid panel route.");
  await expect(store("open_queue_item", { itemId: "\n" })).rejects.toBe(
    "Panel destination identity is invalid; no substitute was selected.",
  );
  expect(await readFile(sessionPath(dataRoot), "utf8")).toBe(bytes);
  const missing =
    "This exact saved destination is not available in the current evidence view. No other PR, iteration or job was selected.";
  await expect(
    store("open_queue_item", { itemId: "missing-exact-item" }),
  ).rejects.toBe(missing);
  expect(await store("panel_snapshot")).toEqual({
    ...expected,
    route: {
      tab: "queue",
      detail: { type: "item", item_id: "missing-exact-item" },
    },
    visible: true,
    revision: expected.revision + 2,
    missing,
  });
  expect(await store("queue_selection")).toBe("missing-exact-item");
});
