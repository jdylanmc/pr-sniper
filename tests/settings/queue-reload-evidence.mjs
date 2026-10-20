import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, readdir, rename, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

const repository = fileURLToPath(new URL("../../", import.meta.url));
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

async function buildIdentity() {
  const assets = [];
  async function visit(directory, prefix = "") {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const name = `${prefix}${entry.name}`;
      if (entry.isDirectory())
        await visit(join(directory, entry.name), `${name}/`);
      else
        assets.push({
          file: name,
          sha256: sha256(await readFile(join(directory, entry.name))),
        });
    }
  }
  await visit(join(repository, "dist"));
  return {
    commit: execFileSync("git", ["rev-parse", "HEAD"], {
      cwd: repository,
      encoding: "utf8",
    }).trim(),
    dirty: Boolean(
      execFileSync("git", ["status", "--porcelain", "--untracked-files=no"], {
        cwd: repository,
        encoding: "utf8",
      }).trim(),
    ),
    version: JSON.parse(await readFile(join(repository, "package.json")))
      .version,
    lockSha256: sha256(await readFile(join(repository, "package-lock.json"))),
    assets: assets.sort((a, b) => a.file.localeCompare(b.file)),
  };
}

async function rendererState(page) {
  return page.evaluate(() => {
    const selectors = [
      ".panel-shell",
      ".panel-content",
      ".panel-context",
      "#handoff-queue",
      "[data-queue-ready-card]",
      "[data-queue-attention-card]",
      "[data-queue-filter][aria-pressed=true]",
    ];
    return {
      viewport: {
        width: innerWidth,
        height: innerHeight,
        deviceScaleFactor: devicePixelRatio,
      },
      readyState: document.readyState,
      visibility: document.visibilityState,
      fonts: document.fonts.status,
      media: Object.fromEntries(
        [
          "prefers-color-scheme: dark",
          "prefers-reduced-motion: reduce",
          "forced-colors: active",
        ].map((query) => [query, matchMedia(`(${query})`).matches]),
      ),
      activeTag: document.activeElement?.tagName,
      queue: {
        filter:
          ["ready", "attention"].find((filter) =>
            document.querySelector(
              `[data-queue-filter="${filter}"][aria-pressed=true]`,
            ),
          ) ?? null,
        rows: document.querySelectorAll("#handoff-queue article").length,
        visibleRows: [
          ...document.querySelectorAll("#handoff-queue article"),
        ].filter((element) => element.getClientRects().length > 0).length,
      },
      elements: selectors.map((selector) => {
        const element = document.querySelector(selector);
        if (!element) return { selector, present: false };
        const style = getComputedStyle(element);
        return {
          selector,
          present: true,
          bounds: element.getBoundingClientRect().toJSON(),
          scroll: {
            top: element.scrollTop,
            left: element.scrollLeft,
            height: element.scrollHeight,
            width: element.scrollWidth,
          },
          style: Object.fromEntries(
            [
              "display",
              "visibility",
              "opacity",
              "fontFamily",
              "fontSize",
              "color",
              "backgroundColor",
              "transform",
            ].map((key) => [key, style[key]]),
          ),
        };
      }),
    };
  });
}

// Independent decoding is forensic evidence; it does not replace the browser oracle.
export function decodeComparison(first, second) {
  const a = PNG.sync.read(first);
  const b = PNG.sync.read(second);
  if (a.width !== b.width || a.height !== b.height)
    return { dimensionsMatch: false, ratio: null };
  let changed = 0;
  for (let index = 0; index < a.data.length; index += 4)
    if (
      [0, 1, 2].some(
        (channel) =>
          Math.abs(a.data[index + channel] - b.data[index + channel]) > 16,
      )
    )
      changed++;
  return {
    dimensionsMatch: true,
    changed,
    pixels: a.width * a.height,
    ratio: changed / (a.width * a.height),
  };
}

export async function queueEvidenceFixture({ page }, use, testInfo) {
  const directory = testInfo.outputPath("queue-reload");
  await mkdir(directory, { recursive: true });
  const manifest = {
    schema: 1,
    test: {
      title: testInfo.title,
      retry: testInfo.retry,
      worker: testInfo.workerIndex,
    },
    startedAt: new Date().toISOString(),
    build: await buildIdentity(),
    run: {
      // Deliberate allowlist: never serialize process.env, logs or provider data.
      id: process.env.GITHUB_RUN_ID ?? null,
      attempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
      workflowSha: process.env.GITHUB_SHA ?? null,
      headSha: process.env.QUEUE_EVIDENCE_HEAD_SHA ?? null,
      os: process.platform,
      node: process.version,
      browser: page.context().browser().browserType().name(),
      browserVersion: page.context().browser().version(),
    },
    policy: { maximumChangedPixelRatio: 0.001, rgbChannelTolerance: 16 },
    phase: "setup",
    status: "in-progress",
    captures: { before: null, reload: null },
    comparison: null,
    pageErrors: 0,
  };
  const persist = async () => {
    await writeFile(
      join(directory, "manifest.pending"),
      JSON.stringify(manifest, null, 2),
    );
    await rename(
      join(directory, "manifest.pending"),
      join(directory, "manifest.json"),
    );
  };
  const pageError = () => {
    manifest.pageErrors++;
  };
  page.on("pageerror", pageError);
  await persist();
  const evidence = {
    async phase(phase) {
      manifest.phase = phase;
      await persist();
    },
    async capture(stage, bytes) {
      if (!["before", "reload"].includes(stage))
        throw new Error("Unknown capture stage.");
      // Commit PNG bytes before decoding or querying a potentially failed renderer.
      await writeFile(join(directory, `${stage}.png`), bytes);
      manifest.captures[stage] = {
        file: `${stage}.png`,
        sha256: sha256(bytes),
        bytes: bytes.length,
      };
      await persist();
      const image = PNG.sync.read(bytes);
      Object.assign(manifest.captures[stage], {
        width: image.width,
        height: image.height,
      });
      if (stage === "reload" && manifest.captures.before) {
        manifest.comparison = {
          browserRatio: null,
          independent: decodeComparison(
            await readFile(join(directory, "before.png")),
            bytes,
          ),
        };
      }
      await persist();
      manifest.captures[stage].renderer = await rendererState(page);
      await persist();
    },
    async compare(browserRatio) {
      manifest.comparison.browserRatio = browserRatio;
      await persist();
    },
  };
  try {
    await use(evidence);
  } finally {
    page.off("pageerror", pageError);
    manifest.status = testInfo.status;
    manifest.finishedAt = new Date().toISOString();
    manifest.errorContext = {
      phase: manifest.phase,
      expectedStatus: testInfo.expectedStatus,
      count: testInfo.errors.length,
      // Correlate errors without retaining arbitrary messages, stacks, URLs or secrets.
      fingerprints: testInfo.errors
        .slice(0, 8)
        .map((error) => sha256(error.message ?? error.value ?? "")),
    };
    await persist();
    await testInfo.attach("queue-reload-manifest", {
      path: join(directory, "manifest.json"),
      contentType: "application/json",
    });
  }
}
