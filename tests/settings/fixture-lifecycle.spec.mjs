import {
  mkdtemp,
  mkdir,
  writeFile,
  readFile,
  rm,
  stat,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createStoreScope, expect, test } from "./fixtures.mjs";

test("Store teardown waits for an accepted delayed write before removing its directory", async () => {
  const root = await mkdtemp(join(tmpdir(), "pr-sniper-store-lifecycle-"));
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
