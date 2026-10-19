import { test, expect } from "@playwright/test";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

test("failed assertion, aborted step and timeout retain Queue evidence", async ({
  browserName,
}, testInfo) => {
  const output = testInfo.outputPath("proof");
  const repository = fileURLToPath(new URL("../../", import.meta.url));
  const result = await promisify(execFile)(
    process.execPath,
    [
      join(repository, "node_modules/@playwright/test/cli.js"),
      "test",
      "--config",
      "tests/settings/evidence/playwright.config.mjs",
      `--browser=${browserName}`,
      "--output",
      output,
    ],
    { cwd: repository, timeout: 20_000 },
  ).then(
    () => ({ code: 0 }),
    (error) => ({ code: error.code, output: error.stdout + error.stderr }),
  );
  expect(result.code, result.output).toBe(1);
  const manifests = [];
  for (const entry of await readdir(output, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const directory = join(output, entry.name, "queue-reload");
    const text = await readFile(join(directory, "manifest.json"), "utf8");
    expect(text).not.toContain("secret-marker");
    const manifest = JSON.parse(text);
    manifests.push(manifest);
    expect(manifest.errorContext.count).toBeGreaterThan(0);
    expect(manifest.errorContext.phase).toMatch(/^reload-/);
    expect(manifest.build.commit).toMatch(/^[a-f0-9]{40}$/);
    expect(
      manifest.build.assets.some((asset) => asset.file.endsWith(".js")),
    ).toBe(true);
    expect(manifest.run.browser).toBe(browserName);
    for (const capture of Object.values(manifest.captures).filter(Boolean)) {
      const bytes = await readFile(join(directory, capture.file));
      expect(createHash("sha256").update(bytes).digest("hex")).toBe(
        capture.sha256,
      );
      const decoded = PNG.sync.read(bytes);
      expect([decoded.width, decoded.height]).toEqual([40, 30]);
      expect([capture.width, capture.height]).toEqual([
        decoded.width,
        decoded.height,
      ]);
      expect(capture.renderer.viewport).toEqual({
        width: 40,
        height: 30,
        deviceScaleFactor: 1,
      });
    }
    if (manifest.test.title === "controlled mismatch") {
      expect(manifest.status).toBe("failed");
      const before = PNG.sync.read(
        await readFile(join(directory, "before.png")),
      );
      const reload = PNG.sync.read(
        await readFile(join(directory, "reload.png")),
      );
      for (let index = 0; index < before.data.length; index += 4) {
        expect([...before.data.subarray(index, index + 3)]).toEqual([
          255, 255, 255,
        ]);
        expect([...reload.data.subarray(index, index + 3)]).toEqual([0, 0, 0]);
      }
      expect(manifest.comparison).toEqual({
        browserRatio: 1,
        independent: {
          dimensionsMatch: true,
          changed: 1200,
          pixels: 1200,
          ratio: 1,
        },
      });
    } else {
      expect(manifest.captures.reload).toBeNull();
      expect(manifest.comparison).toBeNull();
      expect(manifest.status).toBe(
        manifest.test.title === "controlled timeout" ? "timedOut" : "failed",
      );
    }
  }
  expect(manifests.map((manifest) => manifest.test.title).sort()).toEqual([
    "controlled mismatch",
    "controlled step abort",
    "controlled timeout",
  ]);
});
