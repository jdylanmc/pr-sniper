import assert from "node:assert/strict";
import { execFile, execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { promisify } from "node:util";
import { parse } from "yaml";

const workflow = parse(readFileSync(".github/workflows/release.yml", "utf8"));
const ci = parse(readFileSync(".github/workflows/macos.yml", "utf8"));
const windows = parse(readFileSync(".github/workflows/windows.yml", "utf8"));
const { scripts } = JSON.parse(readFileSync("package.json", "utf8"));

const requiredChecks = [
  "macos",
  "webkit",
  "windows",
  "windows-installer-acceptance",
];
const aggregators = {
  macos: [ci, ["macos-native", "browser"]],
  webkit: [ci, ["browser"]],
  windows: [
    windows,
    [
      "windows-checks",
      "windows-native-lib",
      "windows-native-lib-rest",
      "windows-browser",
      "windows-package",
      "windows-upgrade-fixture",
      "windows-control-acceptance",
    ],
  ],
};
const aggregateGate =
  'jq -r \'to_entries[] | "\\(.key): \\(.value.result)"\' <<<"$NEEDS"\n' +
  'jq -e \'length > 0 and all(.[]; .result == "success")\' <<<"$NEEDS" >/dev/null\n';
const acceptanceGate = [
  "$needs = $env:NEEDS | ConvertFrom-Json",
  "$names = @($needs.PSObject.Properties.Name)",
  'foreach ($name in $names) { "${name}: $($needs.$name.result)" }',
  "if ($names.Count -eq 0 -or @($names | Where-Object { $needs.$_.result -cne 'success' }).Count -ne 0) {",
  "  throw 'Every needed job must succeed.'",
  "}",
  "",
].join("\n");
const settle = "node scripts/ci-settle-codegen.mjs";
const bridgeBuild =
  "cargo build --manifest-path src-tauri\\Cargo.toml --locked --example settings_bridge";
const controlAcceptance =
  "node tests/control/native-acceptance.mjs --executable src-tauri/target/release/pr-sniper.exe";
const windowsCargo =
  "cargo test --manifest-path src-tauri\\Cargo.toml --locked";
const isolated = "cancelled_or_failed_startup_does_not_leave_a_child";
const libPartition = ["retention::", "actions::", "control::installer::"];
const libFirst = `${windowsCargo} --lib -- ${libPartition.join(" ")} --nocapture --test-threads=2 --skip ${isolated}`;
const libRest = `${windowsCargo} --lib -- --nocapture --test-threads=2 --skip ${isolated} ${libPartition.map((filter) => `--skip ${filter}`).join(" ")}`;
const runs = (job) => job.steps.filter((step) => step.run).map((s) => s.run);
const isCargoTest = (run) =>
  run === "npm test" || run.startsWith("cargo test ");
const allJobs = () => [
  ...Object.entries(ci.jobs).map(([name, job]) => [ci, name, job]),
  ...Object.entries(windows.jobs).map(([name, job]) => [windows, name, job]),
];

test("canonical doctrine checkout preserves exact bytes with core.autocrlf=true", () => {
  const directory = join(".agents", "skills", "doctrine", "doctrines");
  const documents = readdirSync(directory)
    .filter((name) => name.endsWith(".md"))
    .map((name) => [name, readFileSync(join(directory, name))]);
  assert.ok(documents.some(([name]) => name === "manifest.md"));
  const samples = [
    "src/main.ts",
    "src/style.css",
    "index.html",
    "package.json",
    ".github/workflows/windows.yml",
    "src-tauri/icons/icon.png",
    "src-tauri/icons/icon.ico",
    "src-tauri/icons/icon.icns",
  ].map((path) => [path, readFileSync(path)]);
  const fixture = join(process.cwd(), `.pr-sniper-checkout-${randomUUID()}`);
  const git = (...args) =>
    execFileSync("git", ["-C", fixture, ...args], { stdio: "pipe" });
  mkdirSync(join(fixture, directory), { recursive: true });
  try {
    writeFileSync(
      join(fixture, ".gitattributes"),
      readFileSync(".gitattributes"),
    );
    for (const [name, bytes] of documents) {
      assert.ok(
        !bytes.includes(13),
        `${name} must be canonical LF in the source checkout`,
      );
      writeFileSync(join(fixture, directory, name), bytes);
    }
    for (const [path, bytes] of samples) {
      mkdirSync(dirname(join(fixture, path)), { recursive: true });
      writeFileSync(join(fixture, path), bytes);
    }
    git("init", "--quiet");
    git("config", "core.autocrlf", "true");
    git(
      "-c",
      "core.autocrlf=false",
      "add",
      "--",
      ".gitattributes",
      directory,
      ...samples.map(([path]) => path),
    );
    rmSync(join(fixture, directory), { recursive: true });
    for (const [path] of samples) rmSync(join(fixture, path));

    git("checkout-index", "--all", "--force");

    assert.equal(
      git("config", "--get", "core.autocrlf").toString().trim(),
      "true",
    );
    for (const [name, bytes] of documents) {
      assert.deepEqual(
        readFileSync(join(fixture, directory, name)),
        bytes,
        name,
      );
    }
    for (const [path, bytes] of samples) {
      assert.deepEqual(readFileSync(join(fixture, path)), bytes, path);
    }
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});

test("only explicit stable tag release events can enter the secret-bearing pipeline", () => {
  assert.deepEqual(Object.keys(workflow.on).sort(), [
    "push",
    "workflow_dispatch",
  ]);
  assert.deepEqual(workflow.on.push, { tags: ["v*"] });
  assert.deepEqual(workflow.permissions, {});
  assert.equal(workflow.concurrency["cancel-in-progress"], false);
  assert.equal(workflow.jobs.sign.environment, "pr-sniper-release");
  assert.equal(workflow.jobs.tap.environment, "pr-sniper-tap");
  assert.equal(workflow.jobs.chocolatey.environment, "pr-sniper-chocolatey");
  assert.deepEqual(workflow.jobs.sign.needs, ["gate", "tap-ready"]);
  assert.equal(workflow.jobs["tap-ready"].needs, "gate");
  assert.equal(workflow.jobs["tap-ready"].environment, "pr-sniper-tap");
  assert.equal(workflow.jobs.tap["runs-on"], "ubuntu-latest");
  assert.ok(!JSON.stringify(workflow.jobs.tap).includes("brew "));
  assert.deepEqual(workflow.jobs.publish.needs, ["gate", "sign"]);
  assert.deepEqual(workflow.jobs.tap.needs, ["gate", "publish"]);
  assert.equal(workflow.jobs["windows-build"].needs, "gate");
  assert.deepEqual(workflow.jobs["windows-publish"].needs, [
    "gate",
    "publish",
    "tap",
    "windows-build",
  ]);
  assert.deepEqual(workflow.jobs.chocolatey.needs, ["gate", "windows-publish"]);
});

test("all release actions have immutable refs and source checkout credentials are not retained", () => {
  for (const [name, job] of Object.entries(workflow.jobs)) {
    for (const step of job.steps) {
      if (step.uses)
        assert.match(step.uses, /^[A-Za-z0-9-]+\/[a-z0-9-]+@[a-f0-9]{40}$/);
      if (step.uses?.startsWith("actions/checkout@")) {
        assert.equal(step.with["persist-credentials"], false);
        if (name !== "gate")
          assert.equal(step.with.ref, "${{ needs.gate.outputs.sha }}");
      }
    }
  }
});

test("signing secrets are absent from install/build/upload and tap credential is isolated", () => {
  const sign = workflow.jobs.sign;
  assert.deepEqual(sign.permissions, { contents: "read" });
  assert.deepEqual(workflow.jobs.publish.permissions, { contents: "write" });
  assert.deepEqual(workflow.jobs.tap.permissions, { contents: "read" });
  const secretSteps = sign.steps.filter((step) =>
    JSON.stringify(step).includes("secrets."),
  );
  assert.equal(secretSteps.length, 1);
  assert.equal(secretSteps[0].run, "python3 -B scripts/sign_release.py");
  assert.equal(
    secretSteps[0].env.APPLE_SIGNING_IDENTITY,
    "${{ vars.APPLE_SIGNING_IDENTITY }}",
  );
  assert.equal(
    sign.steps.find((s) => s.run === "npm run bundle").env,
    undefined,
  );
  assert.ok(!JSON.stringify(sign).includes("HOMEBREW_TAP_TOKEN"));
  const tapSecrets = workflow.jobs.tap.steps.filter((step) =>
    JSON.stringify(step).includes("secrets."),
  );
  assert.equal(tapSecrets.length, 1);
  assert.equal(tapSecrets[0].run, "python3 -B scripts/release.py tap");
  assert.ok(!JSON.stringify(workflow.jobs.publish).includes("APPLE_"));
  assert.ok(!JSON.stringify(ci).includes("secrets."));
  const windowsBuild = workflow.jobs["windows-build"];
  const windowsPublish = workflow.jobs["windows-publish"];
  const chocolatey = workflow.jobs.chocolatey;
  assert.deepEqual(windowsBuild.permissions, {
    contents: "read",
    actions: "read",
  });
  assert.deepEqual(windowsPublish.permissions, {
    contents: "write",
    actions: "read",
  });
  assert.deepEqual(chocolatey.permissions, {
    contents: "write",
    actions: "read",
  });
  assert.equal(windowsBuild.environment, undefined);
  assert.equal(windowsPublish.environment, undefined);
  assert.equal(chocolatey.environment, "pr-sniper-chocolatey");
  assert.ok(!JSON.stringify(windowsBuild).includes("secrets."));
  assert.ok(!JSON.stringify(windowsPublish).includes("secrets."));
  const chocoSecrets = chocolatey.steps.filter((step) =>
    JSON.stringify(step).includes("secrets."),
  );
  assert.equal(chocoSecrets.length, 1);
  assert.equal(
    chocoSecrets[0].run,
    "python -B scripts/release.py choco-publish",
  );
  assert.deepEqual(chocoSecrets[0].env, {
    GITHUB_TOKEN: "${{ github.token }}",
    CHOCOLATEY_API_KEY: "${{ secrets.CHOCOLATEY_API_KEY }}",
  });
  assert.ok(
    !chocolatey.steps.some((step) => step.run?.includes("choco pack")),
    "chocolatey job reuses the pre-packed nupkg artifact across retries",
  );
  for (const [name, job] of Object.entries(workflow.jobs)) {
    if (name !== "chocolatey") {
      assert.ok(!JSON.stringify(job).includes("CHOCOLATEY_API_KEY"), name);
    }
  }
});

test("publication consumes only this run's verified artifact and contract tests run in normal CI", () => {
  const upload = workflow.jobs.sign.steps.find((s) =>
    s.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(upload.if, undefined);
  assert.equal(
    upload.with.path,
    "${{ runner.temp }}/pr-sniper-release-assets/",
  );
  assert.equal(upload.with["if-no-files-found"], "error");
  for (const name of ["publish", "tap"]) {
    const download = workflow.jobs[name].steps.find((s) =>
      s.uses?.startsWith("actions/download-artifact@"),
    );
    assert.equal(download.with.name, upload.with.name);
    assert.equal(download.with["run-id"], undefined);
    assert.equal(
      workflow.jobs[name].env.SOURCE_SHA,
      "${{ needs.gate.outputs.sha }}",
    );
  }
  const winUpload = workflow.jobs["windows-build"].steps.find((s) =>
    s.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(winUpload.if, undefined);
  assert.equal(winUpload.with.name, "windows-release-assets");
  assert.equal(
    winUpload.with.path,
    "${{ runner.temp }}/pr-sniper-windows-release-assets/",
  );
  assert.equal(winUpload.with["if-no-files-found"], "error");
  assert.equal(winUpload.with["retention-days"], 14);
  assert.equal(winUpload.with["compression-level"], 0);
  assert.equal(
    workflow.jobs["windows-build"].env.SOURCE_SHA,
    "${{ needs.gate.outputs.sha }}",
  );
  for (const name of ["windows-publish", "chocolatey"]) {
    const download = workflow.jobs[name].steps.find((s) =>
      s.uses?.startsWith("actions/download-artifact@"),
    );
    assert.equal(download.with.name, winUpload.with.name);
    assert.equal(
      download.with.path,
      "${{ runner.temp }}/pr-sniper-windows-release-assets",
    );
    assert.equal(download.with["run-id"], undefined);
    assert.equal(
      workflow.jobs[name].env.SOURCE_SHA,
      "${{ needs.gate.outputs.sha }}",
    );
  }
  const chocoUpload = workflow.jobs["windows-publish"].steps.find((s) =>
    s.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(chocoUpload.if, undefined);
  assert.equal(chocoUpload.with.name, "chocolatey-release-package");
  assert.equal(
    chocoUpload.with.path,
    "${{ runner.temp }}/pr-sniper-windows-release-assets/pr-sniper.*.nupkg",
  );
  assert.equal(chocoUpload.with["if-no-files-found"], "error");
  assert.equal(chocoUpload.with["retention-days"], 14);
  assert.equal(chocoUpload.with["compression-level"], 0);
  const chocoDownloads = workflow.jobs.chocolatey.steps.filter((s) =>
    s.uses?.startsWith("actions/download-artifact@"),
  );
  assert.deepEqual(
    chocoDownloads.map((s) => [s.with.name, s.with.path, s.with["run-id"]]),
    [
      [
        "windows-release-assets",
        "${{ runner.temp }}/pr-sniper-windows-release-assets",
        undefined,
      ],
      [
        "chocolatey-release-package",
        "${{ runner.temp }}/pr-sniper-windows-release-assets",
        undefined,
      ],
    ],
  );
  assert.ok(
    ci.jobs["macos-native"].steps.some((s) => s.run === "npm run test:release"),
  );
});

test("Windows native application checks run on every PR and main push with read-only access", () => {
  assert.equal(windows.name, "Windows native application");
  assert.deepEqual(windows.on, {
    workflow_dispatch: null,
    pull_request: null,
    push: { branches: ["main"] },
  });
  assert.deepEqual(windows.permissions, { contents: "read" });
  assert.deepEqual(Object.keys(windows.jobs), [
    "windows-checks",
    "windows-native-lib",
    "windows-native-lib-rest",
    "windows-browser",
    "windows-package",
    "windows-upgrade-fixture",
    "windows-control-acceptance",
    "windows-installer-acceptance",
    "windows",
  ]);
  for (const [name, job] of Object.entries(windows.jobs)) {
    if (name === "windows") continue;
    assert.equal(job["runs-on"], "windows-2022", name);
    assert.equal(job.defaults.run.shell, "pwsh", name);
    assert.ok(job["timeout-minutes"] > 0, name);
    assert.ok(job["timeout-minutes"] <= 45, name);
  }
  assert.equal(
    windows.jobs["windows-installer-acceptance"]["timeout-minutes"],
    15,
  );
});

test("Windows cancels superseded PR runs without cancelling main release evidence", () => {
  assert.equal(
    windows.concurrency.group,
    "${{ github.workflow }}-${{ github.event.pull_request.number || github.run_id }}",
  );
  assert.equal(
    windows.concurrency["cancel-in-progress"],
    "${{ github.event_name == 'pull_request' }}",
  );
  for (const job of Object.values(windows.jobs)) {
    for (const step of job.steps.filter((step) => step.run)) {
      assert.ok(step["timeout-minutes"] > 0, step.name || step.run);
      assert.ok(step["timeout-minutes"] < job["timeout-minutes"]);
    }
  }
});

test("every PR workflow cancels superseded runs without cancelling non-PR runs", () => {
  const names = new Set();
  const groups = new Set([
    "${{ github.workflow }}-${{ github.event.pull_request.number || github.run_id }}",
    "${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}",
  ]);
  for (const file of readdirSync(".github/workflows").filter((file) =>
    /\.ya?ml$/.test(file),
  )) {
    const current = parse(
      readFileSync(join(".github/workflows", file), "utf8"),
    );
    const events = current.on;
    const handlesPR =
      events === "pull_request" ||
      (Array.isArray(events)
        ? events.includes("pull_request")
        : events && Object.hasOwn(events, "pull_request"));
    if (!handlesPR) continue;
    assert.ok(current.name && !names.has(current.name), file);
    names.add(current.name);
    assert.ok(groups.has(current.concurrency?.group), file);
    assert.equal(
      current.concurrency["cancel-in-progress"],
      "${{ github.event_name == 'pull_request' }}",
      file,
    );
  }
  assert.ok(names.size > 0, "No PR-triggered workflows inspected");
});

test("Site tests run in the Playwright image matching @playwright/test", () => {
  const job = parse(readFileSync(".github/workflows/pages.yml", "utf8")).jobs
    .test;
  const { devDependencies } = JSON.parse(readFileSync("package.json", "utf8"));
  assert.equal(
    job.container.image,
    `mcr.microsoft.com/playwright:v${devDependencies["@playwright/test"]}-noble`,
  );
  const commands = job.steps.map((step) => step.run ?? "").join("\n");
  assert.doesNotMatch(commands, /playwright install|apt-get|sudo /);
});

test("Windows browser shards keep the full suite and bounded per-shard headroom", () => {
  const job = windows.jobs["windows-browser"];
  assert.equal(job.name, "windows-browser (${{ matrix.shard }}/3)");
  assert.deepEqual(job.strategy, {
    "fail-fast": false,
    matrix: { shard: [1, 2, 3] },
  });
  assert.equal(job["timeout-minutes"], 45);
  const suite = job.steps.filter((step) =>
    step.run?.startsWith("npm run test:settings"),
  );
  assert.equal(suite.length, 1);
  assert.equal(
    suite[0].run,
    "npm run test:settings -- --shard=${{ matrix.shard }}/3 --workers=2",
  );
  assert.equal(suite[0]["timeout-minutes"], 20);
  // npm appends the shard arguments to the final Playwright command.
  assert.match(
    scripts["test:settings"],
    / && playwright test --config tests\/settings\/playwright\.config\.mjs$/,
  );
});

test("Rust caching retains dependencies, not application artifacts or PR-written cache entries", () => {
  const caches = allJobs().flatMap(([, name, job]) =>
    job.steps
      .filter((step) => step.uses?.startsWith("Swatinem/rust-cache@"))
      .map((step) => [name, job, step]),
  );
  assert.deepEqual(
    caches.map(([name]) => name),
    [
      "macos-native",
      "browser",
      "windows-checks",
      "windows-native-lib",
      "windows-native-lib-rest",
      "windows-browser",
      "windows-package",
      "windows-upgrade-fixture",
    ],
  );
  const mainOnly =
    "${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}";
  const directories = {
    "macos-15": ["debug", "release"].flatMap((profile) =>
      [".fingerprint", "build", "deps"].map(
        (dir) => `src-tauri/target/${profile}/${dir}`,
      ),
    ),
    "windows-2022-debug": [".fingerprint", "build", "deps"].map(
      (dir) => `src-tauri\\target\\debug\\${dir}`,
    ),
    "windows-2022-release": [".fingerprint", "build", "deps"].map(
      (dir) => `src-tauri\\target\\release\\${dir}`,
    ),
  };
  const savers = {
    "macos-15": "macos-native",
    "windows-2022-debug": "windows-native-lib",
    "windows-2022-release": "windows-package",
  };
  for (const [name, job, cache] of caches) {
    assert.equal(job.steps.filter((s) => s === cache).length, 1);
    assert.match(cache.uses, /^Swatinem\/rust-cache@[a-f0-9]{40}$/);
    const index = job.steps.indexOf(cache);
    assert.equal(
      job.steps[index - 1].run,
      "rustup show active-toolchain",
      `${name} restores after the pinned toolchain is active`,
    );
    assert.ok(
      job.steps
        .slice(0, index)
        .every(
          (step) =>
            !/cargo|npm (?:test|run (?:lint|bundle|build:windows|test:settings))/.test(
              step.run ?? "",
            ),
        ),
      `${name} restores before compiling`,
    );
    const key = cache.with["shared-key"];
    assert.ok(Object.hasOwn(directories, key), name);
    assert.deepEqual(cache.with, {
      workspaces: "src-tauri -> target",
      "shared-key": key,
      "cache-bin": "false",
      "cache-targets": "false",
      "cache-directories": `${directories[key].join("\n")}\n`,
      "cache-workspace-crates": "false",
      "cache-on-failure": "false",
      "save-if": savers[key] === name ? mainOnly : "false",
    });
    assert.equal(
      cache.name,
      savers[key] === name
        ? "Cache Rust dependencies (main writes only)"
        : "Restore Rust dependencies (never writes)",
    );
  }
  for (const [key, saver] of Object.entries(savers))
    assert.ok(
      caches.some(
        ([name, , cache]) => name === saver && cache.with["shared-key"] === key,
      ),
    );
  // The single saver of each key compiles a superset of the restoring jobs' dependencies.
  assert.ok(
    runs(windows.jobs["windows-native-lib"]).some((run) =>
      run.startsWith("cargo clippy "),
    ),
  );
  assert.ok(
    runs(windows.jobs["windows-native-lib"]).some((run) =>
      run.startsWith("cargo test "),
    ),
  );
  assert.ok(
    ["npm run lint", "npm test", "npm run bundle"].every((run) =>
      runs(ci.jobs["macos-native"]).includes(run),
    ),
  );
});

test("Windows checks have no release credentials, privileged environment or failure bypass", () => {
  assert.doesNotMatch(JSON.stringify(windows), /secrets\.|github\.token/);
  for (const [name, job] of Object.entries(windows.jobs)) {
    assert.equal(job.env, undefined, name);
    assert.equal(job.environment, undefined, name);
    assert.equal(job.permissions, undefined, name);
    assert.equal(job["continue-on-error"], undefined, name);
    assert.equal(
      job.if,
      ["windows", "windows-installer-acceptance"].includes(name)
        ? "${{ always() }}"
        : undefined,
      name,
    );
    for (const step of job.steps) {
      assert.equal(step["continue-on-error"], undefined, name);
      assert.equal(
        step.if,
        [
          "Retain native installer diagnostics",
          "Retain native control acceptance receipts",
        ].includes(step.name)
          ? "${{ always() }}"
          : undefined,
        name,
      );
      if (step.env !== undefined)
        assert.deepEqual(step.env, { NEEDS: "${{ toJSON(needs) }}" }, name);
      if (step.uses?.startsWith("actions/checkout@"))
        assert.equal(step.with["persist-credentials"], false, name);
    }
  }
});

test("Windows uses pinned Node and real fail-fast frontend and portable release checks", () => {
  const pin =
    /^(?:actions\/(?:checkout|setup-node|setup-python|upload-artifact|download-artifact)|Swatinem\/rust-cache)@[a-f0-9]{40}$/;
  const actions = Object.fromEntries(
    Object.entries(windows.jobs).map(([name, job]) => [
      name,
      job.steps
        .filter((step) => step.uses)
        .map((step) => {
          assert.match(step.uses, pin);
          return step.uses.split("@")[0];
        }),
    ]),
  );
  const prepare = ["actions/checkout", "actions/setup-node"];
  assert.deepEqual(actions, {
    "windows-checks": [
      ...prepare,
      "actions/setup-python",
      "Swatinem/rust-cache",
      "actions/upload-artifact",
    ],
    "windows-native-lib": [...prepare, "Swatinem/rust-cache"],
    "windows-native-lib-rest": [...prepare, "Swatinem/rust-cache"],
    "windows-browser": [...prepare, "Swatinem/rust-cache"],
    "windows-package": [
      ...prepare,
      "actions/setup-python",
      "Swatinem/rust-cache",
      "actions/upload-artifact",
      "actions/upload-artifact",
    ],
    "windows-upgrade-fixture": [
      ...prepare,
      "Swatinem/rust-cache",
      "actions/upload-artifact",
    ],
    "windows-control-acceptance": [
      ...prepare,
      "actions/download-artifact",
      "actions/download-artifact",
      "actions/upload-artifact",
    ],
    "windows-installer-acceptance": [
      "actions/checkout",
      "actions/download-artifact",
      "actions/download-artifact",
      "actions/upload-artifact",
      "actions/upload-artifact",
    ],
    windows: [],
  });
  for (const job of Object.values(windows.jobs)) {
    for (const node of job.steps.filter((s) =>
      s.uses?.startsWith("actions/setup-node@"),
    )) {
      assert.deepEqual(node.with, {
        "node-version-file": ".node-version",
        cache: "npm",
      });
    }
    for (const python of job.steps.filter((s) =>
      s.uses?.startsWith("actions/setup-python@"),
    )) {
      assert.deepEqual(python.with, { "python-version": "3.13" });
    }
  }
  const lanes = Object.fromEntries(
    Object.entries(windows.jobs).map(([name, job]) => [name, runs(job)]),
  );
  const targets = "--bins --examples --test '*'";
  const clippy =
    "cargo clippy --manifest-path src-tauri\\Cargo.toml --locked --all-targets -- -D warnings";
  assert.deepEqual(lanes, {
    "windows-checks": [
      "npm ci",
      "npm run test:skills",
      "npm run build",
      "npm run test:release:windows",
      "npm run test:packaging:windows",
      "rustup show active-toolchain",
      "npm run format:check",
      `${windowsCargo} ${targets} --no-run`,
      settle,
      `${windowsCargo} ${targets} -- --nocapture --test-threads=2`,
      bridgeBuild,
    ],
    "windows-native-lib": [
      "npm ci",
      "npm run build",
      "rustup show active-toolchain",
      clippy,
      `${windowsCargo} --lib --no-run`,
      settle,
      `${windowsCargo} --lib copilot::runtime::tests::${isolated} -- --exact --nocapture --test-threads=1`,
      libFirst,
      `${windowsCargo} --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture`,
      `${windowsCargo} --lib review::runtime::tests::bundled_runtime_intelligence_offline -- --exact --ignored --nocapture --test-threads=1`,
    ],
    "windows-native-lib-rest": [
      "npm ci",
      "npm run build",
      "rustup show active-toolchain",
      `${windowsCargo} --lib --no-run`,
      settle,
      libRest,
    ],
    "windows-browser": [
      "npm ci",
      "rustup show active-toolchain",
      "npm exec playwright install chromium",
      "npm run test:settings -- --shard=${{ matrix.shard }}/3 --workers=2",
    ],
    "windows-package": [
      "npm ci",
      "rustup show active-toolchain",
      "npm run build:windows",
      "npm run bundle:windows",
      ".\\scripts\\windows-artifact.ps1",
      ".\\scripts\\windows-installer-artifact.ps1",
    ],
    "windows-upgrade-fixture": [
      "npm ci",
      "rustup show active-toolchain",
      ".\\scripts\\windows-upgrade-fixture.ps1",
    ],
    "windows-control-acceptance": ["npm ci", controlAcceptance],
    "windows-installer-acceptance": [
      acceptanceGate,
      ".\\scripts\\windows-installer-acceptance.ps1 -Candidate src-tauri\\target\\candidate -Upgrade src-tauri\\target\\upgrade-fixture",
    ],
    windows: [aggregateGate],
  });
  // Every command of the former single Windows job still runs. Its
  // `cargo check --all-targets` is subsumed by `cargo clippy --all-targets`,
  // and its `--all-targets` test run is the lib/targets lane split above.
  const lanesRun = new Set(Object.values(lanes).flat());
  for (const run of [
    "npm ci",
    "npm run test:skills",
    "npm run build",
    "npm run test:release:windows",
    "npm run test:packaging:windows",
    "rustup show active-toolchain",
    "npm run format:check",
    `${windowsCargo} --lib copilot::runtime::tests::${isolated} -- --exact --nocapture --test-threads=1`,
    `${windowsCargo} --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture`,
    `${windowsCargo} --lib review::runtime::tests::bundled_runtime_intelligence_offline -- --exact --ignored --nocapture --test-threads=1`,
    clippy,
    "npm exec playwright install chromium",
    "npm run build:windows",
    "npm run bundle:windows",
    ".\\scripts\\windows-artifact.ps1",
    ".\\scripts\\windows-installer-artifact.ps1",
    ".\\scripts\\windows-upgrade-fixture.ps1",
  ])
    assert.ok(lanesRun.has(run), run);
  assert.equal(
    scripts.build,
    "npm run build:control-mcp && tsc --noEmit && vite build",
  );
  assert.equal(
    scripts["build:control-mcp"],
    "node scripts/build-control-mcp.mjs",
  );
});

test("both native workflows execute exact offline Intelligence acceptance without skip or live tests", () => {
  for (const [job, platform, manifest, predecessors] of [
    [
      ci.jobs["macos-native"],
      "macOS",
      "src-tauri/Cargo.toml",
      ["npm test", settle],
    ],
    [
      windows.jobs["windows-native-lib"],
      "Windows",
      "src-tauri\\Cargo.toml",
      [
        `${windowsCargo} --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture`,
      ],
    ],
  ]) {
    const matches = job.steps.filter(
      (step) => step.id === "agent-intelligence-offline",
    );
    assert.equal(matches.length, 1);
    const step = matches[0];
    assert.equal(
      step.name,
      `Verify bundled ${platform} Agent Intelligence (offline, no credentials or inference)`,
    );
    assert.equal(
      step.run,
      `cargo test --manifest-path ${manifest} --locked --lib review::runtime::tests::bundled_runtime_intelligence_offline -- --exact --ignored --nocapture --test-threads=1`,
    );
    assert.equal(step["timeout-minutes"], 2);
    assert.equal(step.if, undefined);
    assert.equal(step["continue-on-error"], undefined);
    assert.equal(step.env, undefined);
    const index = job.steps.indexOf(step);
    assert.deepEqual(
      job.steps
        .slice(index - predecessors.length, index)
        .map((before) => before.run),
      predecessors,
    );
  }
  assert.equal(
    allJobs().filter(([, , job]) =>
      job.steps.some((step) => step.id === "agent-intelligence-offline"),
    ).length,
    2,
  );
});

test("unsigned installers and disposable upgrade fixtures cannot become public release assets", () => {
  assert.equal(
    scripts["bundle:windows"],
    "tauri bundle --ci --no-sign --bundles nsis",
  );
  const steps = windows.jobs["windows-package"].steps;
  assert.ok(
    steps.findIndex((s) => s.run === "npm run bundle:windows") <
      steps.findIndex((s) => s.run === ".\\scripts\\windows-artifact.ps1"),
    "Create the installer before recording separate standalone and payload provenance",
  );
  // Control acceptance receipts are diagnostics, never installer assets.
  const uploads = steps.filter(
    (s) =>
      s.uses?.startsWith("actions/upload-artifact@") &&
      s.id !== "control-native-artifacts",
  );
  assert.equal(uploads.length, 2);
  assert.equal(
    uploads[1].with.name,
    "pr-sniper-windows-unsigned-installer-${{ github.sha }}",
  );
  assert.equal(
    uploads[1].with.path,
    "src-tauri/target/windows-installer-artifact/",
  );
  const fixture = windows.jobs["windows-upgrade-fixture"].steps.filter((s) =>
    s.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(fixture.length, 1);
  assert.equal(
    fixture[0].with.name,
    "pr-sniper-windows-upgrade-test-only-${{ github.sha }}",
  );
  assert.equal(
    fixture[0].with.path,
    "src-tauri/target/windows-upgrade-fixture/upgrade-test-only.exe\nsrc-tauri/target/windows-upgrade-fixture/fixture.json\n",
  );
  assert.equal(fixture[0].with["if-no-files-found"], "error");
  assert.doesNotMatch(
    JSON.stringify(workflow),
    /pr-sniper-windows-unsigned-installer|pr-sniper-windows-upgrade-test-only|upgrade-test-only\.exe|pr-sniper-localtest/,
  );
  const winBuild = workflow.jobs["windows-build"];
  assert.equal(winBuild["runs-on"], "windows-2022");
  assert.deepEqual(winBuild.defaults, { run: { shell: "pwsh" } });
  const winBuildRuns = runs(winBuild);
  assert.deepEqual(winBuildRuns, [
    "npm ci",
    "rustup show active-toolchain",
    "python -B scripts/release.py check-windows-release",
    "npm run build:windows",
    "npm run bundle:windows",
    ".\\scripts\\windows-artifact.ps1",
    ".\\scripts\\windows-installer-artifact.ps1",
    "python -B scripts/release.py stage-windows-release",
  ]);
  const winCache = winBuild.steps.find((s) =>
    s.uses?.startsWith("Swatinem/rust-cache@"),
  );
  assert.deepEqual(winCache.with, {
    workspaces: "src-tauri -> target",
    "shared-key": "windows-2022-release",
    "cache-bin": "false",
    "cache-targets": "false",
    "cache-directories":
      "src-tauri\\target\\release\\.fingerprint\nsrc-tauri\\target\\release\\build\nsrc-tauri\\target\\release\\deps\n",
    "cache-workspace-crates": "false",
    "cache-on-failure": "false",
    "save-if": "false",
  });
  const winPublish = workflow.jobs["windows-publish"];
  assert.equal(winPublish["runs-on"], "windows-2022");
  assert.deepEqual(winPublish.defaults, { run: { shell: "pwsh" } });
  const winPublishRuns = runs(winPublish);
  assert.equal(winPublishRuns.length, 2);
  assert.equal(
    winPublishRuns[0],
    "python -B scripts/release.py publish-windows",
  );
  assert.match(
    winPublishRuns[1],
    /\.\\scripts\\windows-chocolatey\.ps1 -Mode PublicUnsigned/,
  );
  assert.match(winPublishRuns[1], /choco pack/);
  assert.doesNotMatch(winPublishRuns[1], /CHOCOLATEY_API_KEY|choco push/);
  const chocoJob = workflow.jobs.chocolatey;
  assert.equal(chocoJob["runs-on"], "windows-2022");
  assert.deepEqual(chocoJob.defaults, { run: { shell: "pwsh" } });
  const chocoRuns = runs(chocoJob);
  assert.deepEqual(chocoRuns, ["python -B scripts/release.py choco-publish"]);
});

test("base and upgrade metadata hash the extracted payload, not Tauri's restored standalone executable", () => {
  for (const path of [
    "scripts/windows-installer-artifact.ps1",
    "scripts/windows-upgrade-fixture.ps1",
  ]) {
    const source = readFileSync(path, "utf8");
    assert.match(
      source,
      /Get-PrSniperInstallerPayload -Installer \$installer -Version \$version/,
    );
    assert.match(source, /application_sha256 = \$payload\.sha256/);
    assert.match(source, /standalone_application_sha256 = /);
    assert.doesNotMatch(
      source,
      /(?<!standalone_)application_sha256 = \(Get-FileHash \$app\)/,
    );
  }
  const reader = readFileSync("scripts/windows-installer-payload.ps1", "utf8");
  assert.match(reader, /Get-FileHash -LiteralPath \$path -Algorithm SHA256/);
  assert.doesNotMatch(
    reader,
    /__TAURI_BUNDLE_TYPE_VAR_|WriteAllBytes|Set-Content|Start-Process/,
  );
  const acceptance = readFileSync(
    "scripts/windows-installer-acceptance.ps1",
    "utf8",
  );
  assert.match(
    acceptance,
    /\$observedHash -ine \$Metadata\.application_sha256/,
  );
  assert.match(acceptance, /observed_pe_version = \$observedVersion/);
  assert.match(
    acceptance,
    /observed_registration_version = \$registeredVersion/,
  );
});

test("real installer acceptance gates on both installer builds and a fresh hosted VM", () => {
  const job = windows.jobs["windows-installer-acceptance"];
  assert.deepEqual(job.needs, ["windows-package", "windows-upgrade-fixture"]);
  assert.equal(job.if, "${{ always() }}");
  assert.equal(job["runs-on"], "windows-2022");
  assert.equal(job.environment, undefined);
  assert.equal(job["continue-on-error"], undefined);
  const [gate] = job.steps;
  assert.equal(
    gate.name,
    "Require successful installer and upgrade fixture builds",
  );
  assert.deepEqual(gate.env, { NEEDS: "${{ toJSON(needs) }}" });
  assert.equal(gate.run, acceptanceGate);
  assert.ok(gate["timeout-minutes"] < job["timeout-minutes"]);
  const diagnostics = job.steps.find(
    (step) => step.name === "Retain native installer diagnostics",
  );
  assert.ok(diagnostics);
  assert.equal(diagnostics.if, "${{ always() }}");
  assert.equal(
    diagnostics.with.path,
    "src-tauri/target/windows-acceptance/installer-diagnostics/",
  );
  assert.equal(diagnostics.with["if-no-files-found"], "warn");
  for (const step of job.steps) {
    assert.equal(step.if, step === diagnostics ? "${{ always() }}" : undefined);
    assert.equal(step["continue-on-error"], undefined);
    if (step.uses) assert.match(step.uses, /^actions\/[a-z-]+@[a-f0-9]{40}$/);
    if (step.uses?.startsWith("actions/download-artifact@")) {
      assert.match(step.with.name, /\$\{\{ github.sha \}\}$/);
      assert.equal(step.with["run-id"], undefined);
    }
  }
  const produced = job.needs.flatMap((name) =>
    windows.jobs[name].steps
      .filter((step) => step.uses?.startsWith("actions/upload-artifact@"))
      .map((step) => step.with.name),
  );
  const downloads = job.steps
    .filter((step) => step.uses?.startsWith("actions/download-artifact@"))
    .map((step) => step.with.name);
  assert.deepEqual(downloads, [
    "pr-sniper-windows-unsigned-installer-${{ github.sha }}",
    "pr-sniper-windows-upgrade-test-only-${{ github.sha }}",
  ]);
  for (const name of downloads) assert.ok(produced.includes(name), name);
  // The fixture is built in parallel, so it binds itself to the same clean
  // commit and checked-in version that acceptance compares with installer.json.
  const fixture = readFileSync("scripts/windows-upgrade-fixture.ps1", "utf8");
  assert.doesNotMatch(fixture, /installer\.json/);
  assert.match(fixture, /\$commit -cne \$env:GITHUB_SHA/);
  assert.match(fixture, /status --porcelain[\s\S]*npm exec tauri build/);
  assert.match(
    fixture,
    /version = \(Get-Content \(Join-Path \$repository 'package\.json'\) -Raw \| ConvertFrom-Json\)\.version/,
  );
  const acceptance = readFileSync(
    "scripts/windows-installer-acceptance.ps1",
    "utf8",
  );
  assert.match(acceptance, /\$base\.commit -cne \$env:GITHUB_SHA/);
  assert.match(acceptance, /\$next\.commit -cne \$base\.commit/);
  assert.match(acceptance, /\$next\.base_version -cne \$base\.version/);
  const config = JSON.parse(
    readFileSync("src-tauri/tauri.windows.conf.json", "utf8"),
  );
  assert.equal(config.bundle.windows.nsis.installMode, "currentUser");
  assert.deepEqual(config.bundle.windows.webviewInstallMode, { type: "skip" });
  assert.equal(config.bundle.useLocalToolsDir, true);
  const template = readFileSync("src-tauri/windows/installer.nsi", "utf8");
  assert.doesNotMatch(template, /KillProcess|ExecWait|ExecShell|RmDir\s+\/r/i);
  assert.doesNotMatch(template, /DeleteRegKey\s+(?!\/ifempty)/i);
  assert.doesNotMatch(
    template,
    /WriteReg\w+\s+HKLM|CurrentVersion\\Run|Software\\Classes/i,
  );
  assert.match(template, /RequestExecutionLevel user/);
  assert.match(
    template,
    /Install Microsoft Edge WebView2 Evergreen Runtime first/,
  );
});

test("native diagnostics retain the refusal without weakening operation status or exposing app data", () => {
  const template = readFileSync("src-tauri/windows/installer.nsi", "utf8");
  const trace = template.match(/!macro Trace message([\s\S]*?)!macroend/)[1];
  assert.doesNotMatch(
    trace,
    /ClearErrors|SetErrorLevel|Abort|ReadReg|ReadEnvStr/,
  );
  assert.match(trace, /Push \$0[\s\S]*Push \$1[\s\S]*Pop \$1[\s\S]*Pop \$0/);
  assert.match(
    template,
    /!macro Fail message\s+!insertmacro Trace "refusal: \$\{message\}"\s+SetErrorLevel 2/,
  );
  assert.match(
    template,
    /CreateFileW\(w "\$DiagnosticPath", i 0x40000000, i 1, p 0, i 1,/,
  );
  assert.match(
    template,
    /ReadEnvStr \$DiagnosticDirectory PR_SNIPER_INSTALLER_DIAGNOSTICS/,
  );
  const acceptance = readFileSync(
    "scripts/windows-installer-acceptance.ps1",
    "utf8",
  );
  assert.match(
    acceptance,
    /\$failure = \$_[\s\S]*preserving original acceptance failure/,
  );
  assert.match(acceptance, /-Filter 'nsis-\*\.txt' -File/);
  assert.doesNotMatch(
    acceptance,
    /chocolatey\.log|Start-Transcript|Get-ChildItem Env:/,
  );
});

test("expected Chocolatey failures are labelled and logged without hiding real failures", () => {
  const helper = readFileSync("scripts/windows-choco-test.ps1", "utf8");
  assert.match(
    helper,
    /& choco @Arguments[\s\S]*?> \$log\s+\$code = \$LASTEXITCODE/,
  );
  assert.match(helper, /--execution-timeout=\$TimeoutSeconds/);
  assert.match(helper, /\$ExpectFailure -and \$code -eq 0/);
  assert.match(helper, /-not \$ExpectFailure -and \$code -ne 0/);
  assert.match(helper, /Get-Content -LiteralPath \$log -Tail 80/);
  assert.match(helper, /expected exit \$expectation/);
  for (const path of [
    "scripts/windows-installer-acceptance.ps1",
    "scripts/windows-installer-faults.ps1",
    "scripts/windows-removal-retry.ps1",
  ]) {
    const source = readFileSync(path, "utf8");
    assert.match(source, /windows-choco-test\.ps1/);
    assert.match(source, /Invoke-PrSniperChocoTest/);
    assert.doesNotMatch(source, /& choco (?:install|upgrade|uninstall)\b/);
  }
  const retry = readFileSync("scripts/windows-removal-retry.ps1", "utf8");
  assert.match(retry, /\$failedExit = \$injected\.exit_code/);
  assert.match(retry, /-ExpectFailure/);
  assert.match(retry, /-not \$completed/);
  assert.match(retry, /Assert-BusyCleanupRefusal \$true/);
  assert.match(retry, /Assert-BusyCleanupRefusal \$false/);
});

test("registry string data and byte counts use paired System register sources", () => {
  const template = readFileSync("src-tauri/windows/installer.nsi", "utf8");
  const setter = template.match(
    /!macro SetString name value([\s\S]*?)!macroend/,
  )[1];
  assert.match(
    setter,
    /Push \$R8\s+Push \$R9\s+StrCpy \$R9 "\$\{value\}"\s+StrLen \$R8 "\$R9"/,
  );
  assert.match(setter, /IntOp \$R8 \$R8 \+ 1\s+IntOp \$R8 \$R8 \* 2/);
  assert.match(
    setter,
    /RegSetValueExW\(p \$Registry, w "\$\{name\}", i 0, i 1, w R9, i R8\) i\.r0/,
  );
  assert.doesNotMatch(setter, /System::Call[^\n]*\$\{value\}/);
  assert.match(
    setter,
    /Pop \$R9\s+Pop \$R8\s+\$\{If\} \$0 != 0[\s\S]*StrCpy \$OperationFailed 1[\s\S]*Return/,
  );
  // These quoted payloads are data, not descriptor syntax. Cover all writers
  // sharing SetString, including rollback's same registration-writing function.
  for (const name of [
    "DisplayIcon",
    "UninstallString",
    "QuietUninstallString",
  ]) {
    assert.match(
      template,
      new RegExp(`!insertmacro SetString "${name}" '\\$\\\\"`),
    );
  }
  assert.match(
    template,
    /StrCpy \$Registry \$0\s+\$\{If\} \$1 = 1\s+StrCpy \$RegistryCreated 1\s+\$\{EndIf\}\s+!insertmacro Trace "install:registration-open"/,
  );
});

test("registry-denial fixtures retain only DACL restoration rights before injection", () => {
  const faults = readFileSync("scripts/windows-installer-faults.ps1", "utf8");
  assert.match(faults, /OpenBaseKey\('CurrentUser', 'Registry64'\)/);
  assert.match(
    faults,
    /OpenSubKey\([\s\S]*RegistryKeyPermissionCheck\]::ReadWriteSubTree,[\s\S]*RegistryRights\]::ReadPermissions -bor \[Security\.AccessControl\.RegistryRights\]::ChangePermissions\)/,
  );
  assert.doesNotMatch(faults, /^\s*Set-Acl\b/m);
  assert.match(faults, /-Exercise \{ Require-FailedAndPreserved \}/);
  assert.match(faults, /if \(\$restoreKey\) \{ \$restoreKey\.Dispose\(\) \}/);
  const fixture = readFileSync(
    "scripts/windows-registry-acl-fixture.ps1",
    "utf8",
  );
  assert.doesNotMatch(
    fixture,
    /OpenSubKey|Get-Acl|Set-Acl|FullControl|TakeOwnership/,
  );
  assert.match(
    fixture,
    /SetSecurityDescriptorBinaryForm\(\$original, \$section\)/,
  );
  assert.match(fixture, /restoration readback differs from the original/);
  assert.match(fixture, /Preserving original fixture failure/);
});

test("immutable completion survives outer rollback and is left for package-owned cleanup", () => {
  const install = readFileSync(
    "packaging/chocolatey/chocolateyinstall.ps1",
    "utf8",
  );
  const uninstall = readFileSync(
    "packaging/chocolatey/chocolateyuninstall.ps1",
    "utf8",
  );
  const state = readFileSync("packaging/chocolatey/removal-state.ps1", "utf8");
  assert.match(install, /Initialize-PrSniperRemovalState \$tools/);
  assert.match(install, /installation_id = \[guid\]::NewGuid\(\)/);
  assert.match(
    uninstall,
    /Complete-PrSniperNativeRemoval \$tools \$receipt \$receiptHash \$durablePath/,
  );
  assert.match(
    uninstall,
    /Get-PrSniperDurableRemovalPath \$tools \$env:ChocolateyInstall \$receipt \$receiptHash/,
  );
  assert.doesNotMatch(
    uninstall,
    /CreateNew|Set-Content|WriteAllText|Remove-Item[^\n]*native-removal/,
  );
  assert.match(
    state,
    /foreach \(\$phase in @\('native-removal-pending', 'native-removal-state'\)\)/,
  );
  assert.match(
    state,
    /Remove-Item -LiteralPath \(Join-Path \$Tools 'native-removal\.pending\.json'\)/,
  );
  assert.doesNotMatch(state, /Remove-Item[^\n]*'native-removal\.json'/);
  assert.doesNotMatch(state, /Remove-Item[^\n]*\$DurablePath/);
  assert.match(
    readFileSync("scripts/windows-installer-acceptance.ps1", "utf8"),
    /Same-feed reinstall inherited stale native-completion state/,
  );
  assert.match(
    readFileSync("scripts/windows-removal-retry.ps1", "utf8"),
    /Outer package cleanup left tracked completion state/,
  );
});

test("Windows artifact contains only the standalone app and exact-source provenance", () => {
  const upload = windows.jobs["windows-package"].steps.find((step) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(upload.with.name, "pr-sniper-windows-x64-${{ github.sha }}");
  assert.deepEqual(upload.with.path.trim().split("\n"), [
    "src-tauri/target/windows-artifact/pr-sniper.exe",
    "src-tauri/target/windows-artifact/build.json",
    "src-tauri/target/windows-artifact/SHA256SUMS",
  ]);
  assert.equal(upload.with["if-no-files-found"], "error");
  assert.equal(scripts["build:windows"], "tauri build --no-bundle -- --locked");
  assert.equal(scripts.bundle, "tauri build --bundles app");
});

test("both repository release runners execute workflow contracts without weakening macOS coverage", () => {
  assert.equal(
    scripts["test:release"],
    "python3 -B -m unittest discover -s tests/release -p 'test_*.py' && node --test tests/release/workflow.test.mjs tests/release/changesets.test.mjs",
  );
  assert.equal(
    scripts["test:release:windows"],
    "python -B -m unittest discover -s tests/release -p test_release.py && node --test tests/release/workflow.test.mjs tests/release/changesets.test.mjs",
  );
});

test("macOS browser shards run the full Chromium and WebKit suites read-only with pinned actions", async () => {
  assert.equal(existsSync(".github/workflows/macos-webkit.yml"), false);
  assert.equal(ci.name, "macOS foundation");
  assert.deepEqual(ci.on, {
    workflow_dispatch: null,
    pull_request: null,
    push: { branches: ["main", "feat/macos-foundation"] },
  });
  assert.deepEqual(ci.permissions, { contents: "read" });
  assert.deepEqual(Object.keys(ci.jobs), [
    "macos-native",
    "browser",
    "macos",
    "webkit",
  ]);
  const job = ci.jobs.browser;
  assert.equal(job.name, "browser (${{ matrix.shard }}/4)");
  assert.equal(job["runs-on"], "macos-15");
  assert.equal(job["runs-on"], ci.jobs["macos-native"]["runs-on"]);
  assert.equal(job["timeout-minutes"], 45);
  assert.deepEqual(job.strategy, {
    "fail-fast": false,
    matrix: { shard: [1, 2, 3, 4] },
  });
  assert.equal(job.environment, undefined);
  assert.equal(job.permissions, undefined);
  assert.equal(job.needs, undefined);
  const suite = job.steps.find((step) => step.id === "browser-tests");
  const upload = job.steps.find((step) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  for (const scope of [job, ...job.steps]) {
    if (scope !== suite) assert.equal(scope.env, undefined);
    if (scope !== upload) assert.equal(scope.if, undefined);
    assert.equal(scope["continue-on-error"], undefined);
    assert.equal(scope.defaults, undefined);
  }
  assert.doesNotMatch(JSON.stringify(ci), /secrets\.|github\.token/);
  const actions = job.steps.filter((step) => step.uses);
  assert.deepEqual(
    actions.map((step) => step.uses),
    [
      "actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
      "actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020",
      "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
      "actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02",
    ],
  );
  for (const action of actions) {
    assert.ok(
      Object.values(windows.jobs).some((other) =>
        other.steps.some((step) => step.uses === action.uses),
      ),
      "Reuse the trusted Windows workflow's immutable action pins",
    );
  }
  assert.deepEqual(actions[0].with, { "persist-credentials": false });
  assert.deepEqual(actions[1].with, {
    "node-version-file": ".node-version",
    cache: "npm",
  });
  assert.deepEqual(suite.env, {
    QUEUE_EVIDENCE_HEAD_SHA:
      "${{ github.event.pull_request.head.sha || github.sha }}",
  });
  assert.equal(
    upload.if,
    "${{ always() && steps.browser-tests.outcome != 'skipped' }}",
  );
  assert.equal(job.steps.indexOf(upload), job.steps.indexOf(suite) + 1);
  assert.equal(upload["timeout-minutes"], 5);
  assert.deepEqual(upload.with, {
    name: "queue-reload-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.shard }}",
    path: "src-tauri/target/settings-browser/**/queue-reload/*.png\nsrc-tauri/target/settings-browser/**/queue-reload/manifest.json\n",
    "if-no-files-found": "warn",
    "retention-days": 14,
    "include-hidden-files": false,
  });
  assert.equal(job.steps.length, 10);
  const build =
    "npm run build && cargo build --manifest-path src-tauri/Cargo.toml --locked --example settings_bridge";
  const playwright =
    "npm exec playwright -- test --config tests/settings/playwright.config.mjs";
  assert.deepEqual(runs(job), [
    "rustup show active-toolchain",
    "npm ci",
    "npm exec playwright install chromium webkit",
    build,
    `${playwright} --shard=\${{ matrix.shard }}/4 --workers=2`,
    `${playwright} --browser=webkit --shard=\${{ matrix.shard }}/4 --workers=2`,
  ]);
  for (const step of job.steps) {
    assert.ok(step["timeout-minutes"] > 0);
    assert.ok(step["timeout-minutes"] < job["timeout-minutes"]);
  }
  const buildStep = job.steps.find((step) => step.run === build);
  const chromium = job.steps.find(
    (step) => step.name === "Run this Chromium shard of the full suite",
  );
  assert.equal(buildStep["timeout-minutes"], 30);
  assert.equal(chromium["timeout-minutes"], 20);
  assert.equal(suite.name, "Run this WebKit shard of the full suite");
  assert.equal(suite["timeout-minutes"], 20);
  assert.equal(job.steps.indexOf(chromium), job.steps.indexOf(buildStep) + 1);
  assert.equal(job.steps.indexOf(suite), job.steps.indexOf(chromium) + 1);
  assert.equal(
    scripts["test:settings"],
    "npm run build && cargo build --manifest-path src-tauri/Cargo.toml --locked --example settings_bridge && playwright test --config tests/settings/playwright.config.mjs",
  );
  assert.match(
    readFileSync("rust-toolchain.toml", "utf8"),
    /\[toolchain\]\s+channel = "\d+\.\d+\.\d+"/,
  );
  const { default: browser } =
    await import("../settings/playwright.config.mjs");
  assert.equal(browser.fullyParallel, false);
  assert.equal(browser.workers, 1);
  assert.equal(browser.retries, 0);
  assert.equal(browser.timeout, undefined);
  assert.equal(browser.use.browserName, "chromium");
});

test("macOS native checks run every former macOS command except the sharded browser suite", () => {
  const job = ci.jobs["macos-native"];
  assert.equal(job["runs-on"], "macos-15");
  assert.equal(job["timeout-minutes"], 60);
  assert.equal(job.strategy, undefined);
  assert.equal(job.if, undefined);
  assert.deepEqual(runs(job).slice(0, 10), [
    "rustup show active-toolchain",
    "npm ci",
    "npm run test:skills",
    "npm run format:check",
    "npm run lint",
    "npm test",
    settle,
    "cargo test --manifest-path src-tauri/Cargo.toml --locked --lib review::runtime::tests::bundled_runtime_intelligence_offline -- --exact --ignored --nocapture --test-threads=1",
    "npm run test:release",
    "npm run bundle",
  ]);
  assert.deepEqual(
    job.steps.slice(-7).map((step) => step.name ?? step.uses),
    [
      "Verify native notification bundle identity",
      "Compile native acceptance harness",
      "Archive bundle with executable permissions",
      "actions/upload-artifact@v4",
      "Build separate-identifier native control acceptance app",
      "Exercise installed MCP and CLI against the real isolated native app",
      "Retain native control acceptance receipts",
    ],
  );
  assert.ok(!runs(job).some((run) => /playwright|test:settings/.test(run)));
});

test("required checks are fail-closed aggregators over every CI lane", () => {
  for (const [name, [file, needs]] of Object.entries(aggregators)) {
    const job = file.jobs[name];
    assert.deepEqual(job.needs, needs, name);
    assert.equal(job.if, "${{ always() }}", name);
    assert.equal(job["runs-on"], "ubuntu-24.04", name);
    assert.equal(job.name, undefined, name);
    assert.equal(job.strategy, undefined, name);
    assert.equal(job.environment, undefined, name);
    assert.equal(job.permissions, undefined, name);
    assert.equal(job["continue-on-error"], undefined, name);
    assert.equal(job.steps.length, 1, name);
    const [gate] = job.steps;
    assert.equal(gate.name, "Require every needed job to succeed");
    assert.deepEqual(gate.env, { NEEDS: "${{ toJSON(needs) }}" });
    assert.equal(gate.run, aggregateGate);
    assert.equal(gate.if, undefined);
    assert.equal(gate["continue-on-error"], undefined);
    assert.ok(gate["timeout-minutes"] < job["timeout-minutes"]);
  }
  const acceptance = windows.jobs["windows-installer-acceptance"];
  const covered = new Set([
    ...Object.values(aggregators).flatMap(([, needs]) => needs),
    ...acceptance.needs,
  ]);
  for (const [, name, job] of allJobs()) {
    if (requiredChecks.includes(name)) continue;
    assert.ok(covered.has(name), `${name} must gate a required check`);
    assert.ok(
      !requiredChecks.includes(job.name ?? name),
      `${name} must not report a required check name`,
    );
  }
  assert.deepEqual(
    allJobs()
      .map(([, name]) => name)
      .filter((name) => requiredChecks.includes(name))
      .sort(),
    [...requiredChecks].sort(),
  );
  const release = readFileSync("scripts/release.py", "utf8");
  assert.match(release, /^WORKFLOW = "\.github\/workflows\/macos\.yml"$/m);
  assert.match(release, /job\.get\("name"\) == "macos"/);
  assert.match(release, /"\.github\/workflows\/windows\.yml"/);
  assert.match(
    release,
    /for name in \("windows", "windows-installer-acceptance"\)/,
  );
});

test("aggregate gates pass only when every needed job succeeded", () => {
  const cases = [
    [{ a: { result: "success" }, b: { result: "success" } }, true],
    [{ a: { result: "success" }, b: { result: "failure" } }, false],
    [{ a: { result: "success" }, b: { result: "skipped" } }, false],
    [{ a: { result: "cancelled" } }, false],
    [{ a: { result: "Success" } }, false],
    [{}, false],
  ];
  // Each platform's release runner exercises the gate its own jobs execute.
  const gate =
    process.platform === "win32"
      ? ["pwsh", ["-NoProfile", "-NonInteractive", "-Command", acceptanceGate]]
      : [
          "bash",
          ["--noprofile", "--norc", "-eo", "pipefail", "-c", aggregateGate],
        ];
  for (const [needs, passes] of cases) {
    let passed = true;
    try {
      execFileSync(gate[0], gate[1], {
        env: { ...process.env, NEEDS: JSON.stringify(needs) },
        stdio: "pipe",
      });
    } catch (error) {
      assert.equal(typeof error.status, "number", "gate interpreter must run");
      passed = false;
    }
    assert.equal(passed, passes, JSON.stringify(needs));
  }
});

test("browser shards partition every test file and case exactly once per engine", async () => {
  const run = promisify(execFile);
  const list = async (args) => {
    const { stdout } = await run(
      process.execPath,
      [
        "node_modules/@playwright/test/cli.js",
        "test",
        "--config",
        "tests/settings/playwright.config.mjs",
        "--list",
        "--reporter=json",
        ...args,
      ],
      { maxBuffer: 64 * 1024 * 1024 },
    );
    const specs = [];
    const walk = (suite) => {
      for (const spec of suite.specs ?? [])
        for (const one of spec.tests)
          specs.push({ file: spec.file, id: `${spec.id}:${one.projectName}` });
      for (const child of suite.suites ?? []) walk(child);
    };
    for (const suite of JSON.parse(stdout).suites) walk(suite);
    return specs;
  };
  const shardRuns = [];
  for (const [, name, job] of allJobs()) {
    for (const step of job.steps) {
      const match = step.run?.match(/--shard=\$\{\{ matrix\.shard \}\}\/(\d+)/);
      if (!match) continue;
      const total = Number(match[1]);
      assert.deepEqual(
        job.strategy.matrix.shard,
        Array.from({ length: total }, (_, index) => index + 1),
      );
      assert.equal(job.strategy["fail-fast"], false);
      assert.doesNotMatch(
        step.run,
        /--grep|-g |--project|--last-failed|--only-changed|--max-failures|-x\b|--retries|--repeat-each|--timeout|--pass-with-no-tests|--forbid-only=false/,
      );
      assert.match(step.run, / --workers=2$/);
      shardRuns.push([
        name,
        step.run.includes("--browser=webkit") ? "webkit" : "chromium",
        total,
      ]);
    }
  }
  assert.deepEqual(shardRuns, [
    ["browser", "chromium", 4],
    ["browser", "webkit", 4],
    ["windows-browser", "chromium", 3],
  ]);
  for (const [, browser, total] of shardRuns) {
    const engine = browser === "webkit" ? ["--browser=webkit"] : [];
    // Sequential: concurrent Playwright processes race on the shared transform
    // cache and fail with EPERM on Windows runners.
    const full = await list(engine);
    const shards = [];
    for (let index = 0; index < total; index += 1)
      shards.push(await list([...engine, `--shard=${index + 1}/${total}`]));
    assert.ok(full.length > 900, `${browser} suite unexpectedly small`);
    const fullIds = full.map((spec) => spec.id);
    assert.equal(new Set(fullIds).size, fullIds.length);
    const owner = new Map();
    const fileOwner = new Map();
    shards.forEach((specs, index) => {
      assert.ok(specs.length > 0, `${browser} shard ${index + 1} is empty`);
      for (const { file, id } of specs) {
        assert.ok(!owner.has(id), `${id} runs in more than one shard`);
        owner.set(id, index);
        assert.equal(
          fileOwner.get(file) ?? index,
          index,
          `${file} is split across shards`,
        );
        fileOwner.set(file, index);
      }
    });
    assert.deepEqual([...owner.keys()].sort(), [...fullIds].sort());
    assert.deepEqual(
      [...fileOwner.keys()].sort(),
      [...new Set(full.map((spec) => spec.file))].sort(),
    );
    assert.ok(
      readdirSync("tests/settings").filter((name) => name.endsWith(".spec.mjs"))
        .length === fileOwner.size,
      "every settings spec file is listed",
    );
  }
});

test("Windows native lanes run every Cargo test target once with only the isolated SDK exclusion", () => {
  const manifest = readFileSync("src-tauri/Cargo.toml", "utf8");
  assert.doesNotMatch(
    manifest,
    /\[\[(?:bench|test|example|bin)\]\]|auto(?:benches|tests|examples|bins)|harness\s*=|^test\s*=/m,
  );
  assert.equal(existsSync("src-tauri/benches"), false);
  const tests = Object.entries(windows.jobs).flatMap(([name, job]) =>
    runs(job)
      .filter(isCargoTest)
      .map((run) => [name, run]),
  );
  const targets = "--bins --examples --test '*'";
  // --lib plus bins, examples and every integration test is --all-targets
  // when the package declares no benches.
  assert.deepEqual(tests, [
    ["windows-checks", `${windowsCargo} ${targets} --no-run`],
    [
      "windows-checks",
      `${windowsCargo} ${targets} -- --nocapture --test-threads=2`,
    ],
    ["windows-native-lib", `${windowsCargo} --lib --no-run`],
    [
      "windows-native-lib",
      `${windowsCargo} --lib copilot::runtime::tests::${isolated} -- --exact --nocapture --test-threads=1`,
    ],
    ["windows-native-lib", libFirst],
    [
      "windows-native-lib",
      `${windowsCargo} --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture`,
    ],
    [
      "windows-native-lib",
      `${windowsCargo} --lib review::runtime::tests::bundled_runtime_intelligence_offline -- --exact --ignored --nocapture --test-threads=1`,
    ],
    ["windows-native-lib-rest", `${windowsCargo} --lib --no-run`],
    ["windows-native-lib-rest", libRest],
  ]);
  for (const [, run] of tests)
    assert.doesNotMatch(run, /--include-ignored|--all-targets|--features/);
  // libtest runs a case when it matches any filter (or none are given) and no
  // --skip. The two partitions use the same substrings as filters and skips,
  // so they are disjoint and together cover every non-ignored lib test except
  // the isolated SDK case, which both skip and which runs once with --exact.
  const args = (run) => run.split(" -- ")[1].split(" ");
  const parts = (run) => {
    const list = args(run);
    const skips = list.flatMap((arg, index) =>
      list[index - 1] === "--skip" ? [arg] : [],
    );
    const filters = list.filter(
      (arg, index) => !arg.startsWith("--") && list[index - 1] !== "--skip",
    );
    return {
      filters,
      skips,
      flags: list.filter((arg) => arg.startsWith("--") && arg !== "--skip"),
    };
  };
  const first = parts(libFirst);
  const rest = parts(libRest);
  assert.deepEqual(first.filters, libPartition);
  assert.deepEqual(first.skips, [isolated]);
  assert.deepEqual(rest.filters, []);
  assert.deepEqual(rest.skips, [isolated, ...first.filters]);
  assert.deepEqual(first.flags, rest.flags);
  assert.deepEqual(first.flags, ["--nocapture", "--test-threads=2"]);
  const sources = (directory) =>
    readdirSync(directory, { recursive: true })
      .filter((path) => path.endsWith(".rs"))
      .map((path) => [
        join(directory, path),
        readFileSync(join(directory, path), "utf8"),
      ]);
  const named = [
    ...sources("src-tauri/src"),
    ...sources("src-tauri/tests"),
    ...sources("src-tauri/examples"),
  ].flatMap(([path, source]) =>
    (source.match(new RegExp(`fn \\w*${isolated}\\w*`, "g")) ?? []).map(
      (name) => [path.replaceAll("\\", "/"), name],
    ),
  );
  assert.deepEqual(named, [
    ["src-tauri/src/copilot/runtime.rs", `fn ${isolated}`],
  ]);
  assert.equal(
    scripts.test,
    "cargo test --manifest-path src-tauri/Cargo.toml --locked",
  );
  assert.equal(
    runs(ci.jobs["macos-native"]).filter((r) => r === "npm test").length,
    1,
  );
});

test("each job compiles its Cargo test binaries once before reusing them", () => {
  const selection = (run) =>
    run === "npm test"
      ? "default"
      : run.match(/--lib|--bins --examples --test '\*'/)[0];
  let multiple = 0;
  for (const [, name, job] of allJobs()) {
    const commands = runs(job);
    const tests = commands.filter(isCargoTest);
    const settled = commands.filter((run) => run === settle);
    if (tests.length <= 1) {
      assert.equal(settled.length, 0, name);
      continue;
    }
    multiple += 1;
    assert.equal(settled.length, 1, name);
    assert.equal(
      commands.indexOf(settle),
      commands.indexOf(tests[0]) + 1,
      `${name} settles generated assets right after its first compile`,
    );
    const first = selection(tests[0]);
    if (first !== "default") assert.match(tests[0], / --no-run$/);
    for (const run of tests.slice(1)) {
      assert.ok(
        first === "default"
          ? selection(run) === "--lib"
          : selection(run) === first,
        `${name}: ${run} reuses the binaries compiled by ${tests[0]}`,
      );
    }
  }
  assert.equal(multiple, 4);
});

test("codegen settling backdates only this crate's post-build-script outputs", () => {
  const root = join(process.cwd(), `.pr-sniper-settle-${randomUUID()}`);
  const finished = new Date("2026-01-01T00:00:00Z");
  const before = new Date("2025-12-31T00:00:00Z");
  const during = new Date("2026-01-01T00:00:05Z");
  const write = (path, time) => {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, "x");
    utimesSync(path, time, time);
  };
  const settleIn = () =>
    execFileSync(process.execPath, ["scripts/ci-settle-codegen.mjs"], {
      env: { ...process.env, CARGO_TARGET_DIR: root },
      stdio: "pipe",
    }).toString();
  const mtime = (path) => statSync(path).mtime.getTime();
  try {
    const own = join(root, "debug", "build", "pr-sniper-0123");
    const other = join(root, "debug", "build", "serde-0123");
    write(join(own, "output"), finished);
    write(join(own, "out", "acl-manifests.json"), before);
    write(join(own, "out", "4e5fe447"), during);
    write(join(other, "output"), finished);
    write(join(other, "out", "generated"), during);
    assert.match(
      settleIn(),
      /^Settled 1 generated file\(s\) in 1 build script run\(s\)\.$/m,
    );
    assert.equal(mtime(join(own, "out", "4e5fe447")), finished.getTime());
    assert.equal(
      mtime(join(own, "out", "acl-manifests.json")),
      before.getTime(),
    );
    assert.equal(mtime(join(other, "out", "generated")), during.getTime());
    assert.equal(readFileSync(join(own, "out", "4e5fe447"), "utf8"), "x");
    rmSync(own, { recursive: true });
    assert.throws(settleIn);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("CI keeps line tables but drops full debuginfo without changing local profiles", () => {
  for (const file of [ci, windows])
    assert.deepEqual(file.env, { CARGO_PROFILE_DEV_DEBUG: "line-tables-only" });
  for (const [, name, job] of allJobs()) {
    for (const scope of [job, ...job.steps]) {
      for (const key of Object.keys(scope.env ?? {}))
        assert.doesNotMatch(key, /^(?:CARGO|RUST)/, name);
    }
  }
  assert.doesNotMatch(
    readFileSync("src-tauri/Cargo.toml", "utf8"),
    /^\[profile/m,
  );
  assert.equal(existsSync(".cargo/config.toml"), false);
  assert.equal(existsSync("src-tauri/.cargo/config.toml"), false);
});

test("both native lanes run bounded real-app MCP acceptance and retain only safe receipts", () => {
  const lanes = [
    ci.jobs["macos-native"],
    windows.jobs["windows-control-acceptance"],
  ];
  for (const job of lanes) {
    const check = job.steps.find(
      (step) => step.id === "control-native-acceptance",
    );
    assert.match(
      check.run,
      /^node tests\/control\/native-acceptance\.mjs --executable /,
    );
    assert.equal(check.if, undefined);
    assert.equal(check["continue-on-error"], undefined);
    assert.equal(check["timeout-minutes"], 4);
    const artifacts = job.steps.find(
      (step) => step.id === "control-native-artifacts",
    );
    assert.equal(artifacts.if, "${{ always() }}");
    assert.equal(
      artifacts.with.path,
      "src-tauri/target/control-native-acceptance/**/receipt.json",
    );
    assert.equal(artifacts.with["if-no-files-found"], "warn");
    assert(job.steps.indexOf(artifacts) > job.steps.indexOf(check));
  }
  // Both lanes gate their required checks.
  assert(aggregators.macos[1].includes("macos-native"));
  assert(aggregators.windows[1].includes("windows-control-acceptance"));
  // Windows runs the exact packaged executable and the checks lane's Store
  // bridge on a fresh hosted VM, never a rebuild.
  const control = windows.jobs["windows-control-acceptance"];
  assert.deepEqual(control.needs, ["windows-checks", "windows-package"]);
  assert.equal(control.if, undefined);
  const downloads = control.steps.filter((step) =>
    step.uses?.startsWith("actions/download-artifact@"),
  );
  assert.deepEqual(
    downloads.map((step) => [step.with.name, step.with.path]),
    [
      ["pr-sniper-windows-x64-${{ github.sha }}", "src-tauri/target/release"],
      [
        "pr-sniper-windows-settings-bridge-test-only-${{ github.sha }}",
        "src-tauri/target/debug/examples",
      ],
    ],
  );
  assert.ok(
    !runs(control).some((run) =>
      /^(?:cargo |npm run (?:build|tauri))/.test(run),
    ),
  );
  const bridge = windows.jobs["windows-checks"].steps.find((step) =>
    step.with?.name?.startsWith("pr-sniper-windows-settings-bridge-test-only-"),
  );
  assert.equal(
    bridge.with.path,
    "src-tauri/target/debug/examples/settings_bridge.exe",
  );
  assert.equal(bridge.with["retention-days"], 1);
  const mac = ci.jobs["macos-native"].steps;
  const macBuild = mac.find((step) =>
    step.run?.includes("--config tests/control/tauri.acceptance.conf.json"),
  );
  assert(macBuild.run.includes("--debug --bundles app"));
  assert.equal(macBuild["timeout-minutes"], 10);
  assert(
    mac.indexOf(macBuild) <
      mac.findIndex((step) => step.id === "control-native-acceptance"),
  );
  assert.equal(
    JSON.parse(readFileSync("tests/control/tauri.acceptance.conf.json"))
      .identifier,
    "com.jdylanmc.pr-sniper.tests.control-ci",
  );
});

test("changesets validate PRs, consume via a bot PR and tag releases only after exact main CI", () => {
  const changesets = parse(
    readFileSync(".github/workflows/changesets.yml", "utf8"),
  );
  const tagger = parse(
    readFileSync(".github/workflows/release-tag.yml", "utf8"),
  );
  const helper = readFileSync("scripts/changesets-pr.sh", "utf8");
  assert.deepEqual(Object.keys(changesets.on), [
    "pull_request",
    "push",
    "workflow_dispatch",
  ]);
  assert.deepEqual(changesets.on.push, { branches: ["main"] });
  assert.deepEqual(changesets.permissions, { contents: "read" });
  assert.equal(
    changesets.jobs.changeset.if,
    "${{ github.event_name == 'pull_request' }}",
  );
  assert.equal(changesets.jobs.changeset.permissions, undefined);
  assert.match(
    changesets.jobs.changeset.steps.at(-1).run,
    /^node scripts\/changesets\.mjs validate /,
  );
  for (const name of ["consume", "release"]) {
    const run = changesets.jobs[name].steps.map((s) => s.run || "").join("\n");
    assert.match(
      run,
      /bash scripts\/changesets-pr\.sh "(\$BRANCH|changesets\/)/,
      name,
    );
    assert.doesNotMatch(run, /HEAD:main|origin main\b/, name);
  }
  assert.equal(changesets.jobs.consume.env.BRANCH, "changesets/consume");
  assert.match(helper, /push -q --force origin "HEAD:refs\/heads\/\$branch"/);
  assert.match(helper, /gh workflow run macos\.yml --ref "\$branch"/);
  assert.match(helper, /gh workflow run windows\.yml --ref "\$branch"/);
  assert.doesNotMatch(helper, /skip ci/i);
  assert.deepEqual(ci.on.workflow_dispatch, null);
  assert.deepEqual(tagger.on, {
    workflow_run: {
      workflows: [ci.name, windows.name],
      types: ["completed"],
      branches: ["main"],
    },
  });
  assert.deepEqual(tagger.permissions, {});
  assert.equal(tagger.concurrency["cancel-in-progress"], false);
  assert.deepEqual(tagger.jobs.tag.permissions, {
    contents: "write",
    actions: "write",
  });
  const run = tagger.jobs.tag.steps.at(-1).run;
  for (const pattern of [
    /git show "\$SHA\^1:package\.json"/,
    /refs\/tags\/\$tag/,
    /for workflow in macos\.yml windows\.yml/,
    /event=push&branch=main/,
    /gh workflow run release\.yml --ref "\$tag"/,
    /test "\$\(git rev-parse "\$tag\^\{commit\}"\)" = "\$SHA"/,
    /if \[ "\$\(runs\)" = 0 \]; then dispatch;/,
  ])
    assert.match(run, pattern);
  assert.ok(run.indexOf("max_by") < run.indexOf("git tag -a"));
});
