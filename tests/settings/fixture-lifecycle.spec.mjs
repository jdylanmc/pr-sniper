import {
  mkdtemp,
  mkdir,
  writeFile,
  readFile,
  rm,
  stat,
} from "node:fs/promises";
import { join } from "node:path";
import { createStoreScope, expect, test } from "./fixtures.mjs";
import { target } from "./paths.mjs";

test("Store teardown waits for an accepted delayed write before removing its directory", async () => {
  await mkdir(target, { recursive: true });
  const root = await mkdtemp(join(target, "pr-sniper-store-lifecycle-"));
  const started = Promise.withResolvers();
  const release = Promise.withResolvers();
  let calls = 0;
  const scope = createStoreScope(async () => {
    calls++;
    started.resolve();
    await release.promise;
    await mkdir(join(root, "state"));
    await writeFile(join(root, "state/result.json"), "saved");
    return "saved";
  });
  try {
    const pending = scope.invoke();
    await started.promise;
    let closed = false;
    const closing = scope.close().then(() => {
      closed = true;
    });
    await Promise.resolve();
    expect(closed).toBe(false);
    await expect(scope.invoke()).rejects.toThrow("shutting down");
    release.resolve();
    expect(await pending).toBe("saved");
    await closing;
    expect(await readFile(join(root, "state/result.json"), "utf8")).toBe(
      "saved",
    );
    expect(calls).toBe(1);
    await rm(root, { recursive: true, force: true });
    await expect(stat(root)).rejects.toMatchObject({ code: "ENOENT" });
  } finally {
    release.resolve();
    await scope.close();
    await rm(root, { recursive: true, force: true });
  }
});

test("Store teardown preserves operation errors while draining them", async () => {
  const release = Promise.withResolvers();
  const failure = new Error("Native operation failed");
  const scope = createStoreScope(async () => {
    await release.promise;
    throw failure;
  });
  const operation = scope.invoke();
  const observed = operation.catch((error) => error);
  const closing = scope.close();
  release.resolve();
  expect(await observed).toBe(failure);
  await closing;
  await scope.close();
  await expect(scope.invoke()).rejects.toThrow("shutting down");
});

test("Store operations are FIFO and close drains queued work through original rejections", async () => {
  const started = Promise.withResolvers();
  const release = Promise.withResolvers();
  const lastStarted = Promise.withResolvers();
  const lastRelease = Promise.withResolvers();
  const calls = [];
  const failure = new Error("Synchronous native failure");
  const scope = createStoreScope((command, value) => {
    calls.push(command);
    if (command === "first") {
      started.resolve();
      return release.promise.then(() => value);
    }
    if (command === "rejected")
      return Promise.reject("Serialized native error");
    if (command === "throws") throw failure;
    lastStarted.resolve();
    return lastRelease.promise.then(() => value);
  });
  const first = scope.invoke("first", "first result");
  const rejected = scope.invoke("rejected").catch((error) => error);
  const thrown = scope.invoke("throws").catch((error) => error);
  const last = scope.invoke("last", "last result");
  let closed = false;
  try {
    await started.promise;
    expect(calls).toEqual(["first"]);
    const closing = scope.close().then(() => {
      closed = true;
    });
    await expect(scope.invoke("not accepted")).rejects.toThrow("shutting down");
    expect(closed).toBe(false);
    release.resolve();
    expect(await first).toBe("first result");
    expect(await rejected).toBe("Serialized native error");
    expect(await thrown).toBe(failure);
    await lastStarted.promise;
    expect(calls).toEqual(["first", "rejected", "throws", "last"]);
    expect(closed).toBe(false);
    lastRelease.resolve();
    expect(await last).toBe("last result");
    await closing;
    expect(closed).toBe(true);
    await scope.close();
  } finally {
    release.resolve();
    lastRelease.resolve();
    await scope.close();
  }
});

test("a Store rejection does not poison later submissions", async () => {
  const failure = { message: "Original native rejection" };
  let calls = 0;
  const scope = createStoreScope(() => {
    if (++calls === 1) return Promise.reject(failure);
    return "later result";
  });
  try {
    await expect(scope.invoke()).rejects.toBe(failure);
    expect(await scope.invoke()).toBe("later result");
  } finally {
    await scope.close();
  }
});

test("Store scopes for independent roots do not share their transaction queue", async ({
  dataRoot,
}) => {
  const firstRoot = join(dataRoot, "first");
  const secondRoot = join(dataRoot, "second");
  await Promise.all([mkdir(firstRoot), mkdir(secondRoot)]);
  const started = Promise.withResolvers();
  const release = Promise.withResolvers();
  const first = createStoreScope(async () => {
    started.resolve();
    await release.promise;
    await writeFile(join(firstRoot, "result"), "first");
  });
  const second = createStoreScope(() =>
    writeFile(join(secondRoot, "result"), "second"),
  );
  const pending = first.invoke();
  try {
    await started.promise;
    await second.invoke();
    await second.close();
    expect(await readFile(join(secondRoot, "result"), "utf8")).toBe("second");
    await expect(stat(join(firstRoot, "result"))).rejects.toMatchObject({
      code: "ENOENT",
    });
  } finally {
    release.resolve();
    await pending;
    await Promise.all([first.close(), second.close()]);
  }
});

test("held IPC replies release the Store queue after a first notification write and an error", async ({
  dataRoot,
  ipc,
  store,
}) => {
  expect((await store("notification_snapshot")).enabled).toBe(false);
  await expect(
    stat(join(dataRoot, "state", "notifications.json")),
  ).rejects.toMatchObject({
    code: "ENOENT",
  });
  const held = ipc.holdNext("set_notifications_enabled");
  let replied = false;
  const reply = ipc
    .invoke("set_notifications_enabled", { enabled: true })
    .then(() => {
      replied = true;
    });
  const concurrentRead = store("notification_snapshot");
  try {
    await held.arrived;
    expect((await concurrentRead).enabled).toBe(true);
    expect((await store("notification_snapshot")).enabled).toBe(true);
    expect(replied).toBe(false);
  } finally {
    held.release();
    await reply;
  }
  expect(replied).toBe(true);

  const expected = await store("fixture_unknown").catch((error) => error);
  expect(typeof expected).toBe("string");
  const heldError = ipc.holdNext("fixture_unknown");
  let rejected = false;
  const errorReply = ipc.invoke("fixture_unknown").catch((error) => {
    rejected = true;
    return error;
  });
  try {
    await heldError.arrived;
    expect((await store("notification_snapshot")).enabled).toBe(true);
    expect(rejected).toBe(false);
  } finally {
    heldError.release();
  }
  expect(await errorReply).toBe(expected);
});
