import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { parse } from "yaml";

const workflow = parse(readFileSync(".github/workflows/release.yml", "utf8"));
const ci = parse(readFileSync(".github/workflows/macos.yml", "utf8"));
const windows = parse(readFileSync(".github/workflows/windows.yml", "utf8"));
const { scripts } = JSON.parse(readFileSync("package.json", "utf8"));

test("canonical doctrine checkout preserves exact bytes with core.autocrlf=true", () => {
  const directory = join(".agents", "skills", "doctrine", "doctrines");
  const documents = readdirSync(directory)
    .filter((name) => name.endsWith(".md"))
    .map((name) => [name, readFileSync(join(directory, name))]);
  assert.ok(documents.some(([name]) => name === "manifest.md"));
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
    git("init", "--quiet");
    git("config", "core.autocrlf", "true");
    git("-c", "core.autocrlf=false", "add", "--", ".gitattributes", directory);
    rmSync(join(fixture, directory), { recursive: true });

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
  assert.deepEqual(workflow.jobs.sign.needs, ["gate", "tap-ready"]);
  assert.equal(workflow.jobs["tap-ready"].needs, "gate");
  assert.equal(workflow.jobs["tap-ready"].environment, "pr-sniper-tap");
  assert.deepEqual(workflow.jobs.publish.needs, ["gate", "sign"]);
  assert.deepEqual(workflow.jobs.tap.needs, ["gate", "publish"]);
});

test("all release actions have immutable refs and source checkout credentials are not retained", () => {
  for (const [name, job] of Object.entries(workflow.jobs)) {
    for (const step of job.steps) {
      if (step.uses)
        assert.match(step.uses, /^[a-z0-9-]+\/[a-z0-9-]+@[a-f0-9]{40}$/);
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
  assert.ok(ci.jobs.macos.steps.some((s) => s.run === "npm run test:release"));
});

test("Windows frontend/shared checks run on every PR and main push with read-only access", () => {
  assert.equal(windows.name, "Windows frontend and shared checks");
  assert.deepEqual(windows.on, {
    pull_request: null,
    push: { branches: ["main"] },
  });
  assert.deepEqual(windows.permissions, { contents: "read" });
  assert.deepEqual(Object.keys(windows.jobs), ["windows"]);
  assert.equal(windows.jobs.windows["runs-on"], "windows-2022");
  assert.ok(windows.jobs.windows["timeout-minutes"] > 0);
});

test("Windows checks have no release credentials, privileged environment or failure bypass", () => {
  const job = windows.jobs.windows;
  assert.doesNotMatch(JSON.stringify(windows), /secrets\.|github\.token/);
  assert.equal(windows.env, undefined);
  assert.equal(job.env, undefined);
  assert.equal(job.environment, undefined);
  assert.equal(job.permissions, undefined);
  for (const scope of [job, ...job.steps]) {
    assert.equal(scope.if, undefined);
    assert.equal(scope["continue-on-error"], undefined);
    assert.equal(scope.env, undefined);
  }
  const checkout = job.steps.find((step) =>
    step.uses?.startsWith("actions/checkout@"),
  );
  assert.equal(checkout.with["persist-credentials"], false);
});

test("Windows uses pinned Node and real fail-fast frontend and portable release checks", () => {
  const job = windows.jobs.windows;
  assert.equal(job.defaults.run.shell, "pwsh");
  const actions = job.steps.filter((step) => step.uses);
  assert.deepEqual(
    actions.map((step) => step.uses.split("@")[0]),
    ["actions/checkout", "actions/setup-node", "actions/setup-python"],
  );
  for (const step of actions) {
    assert.match(step.uses, /^actions\/[a-z-]+@[a-f0-9]{40}$/);
  }
  const node = job.steps.find((step) =>
    step.uses?.startsWith("actions/setup-node@"),
  );
  assert.equal(node.with["node-version-file"], ".node-version");
  assert.equal(node.with["node-version"], undefined);
  assert.equal(node.with.cache, "npm");
  const python = job.steps.find((step) =>
    step.uses?.startsWith("actions/setup-python@"),
  );
  assert.equal(python.with["python-version"], "3.13");
  assert.deepEqual(
    job.steps.filter((step) => step.run).map((step) => step.run),
    [
      "npm ci",
      "npm run build",
      "npm run test:release:windows",
      "rustup show active-toolchain",
      "cargo fmt --manifest-path src-tauri\\Cargo.toml --all --check",
      "cargo test --manifest-path src-tauri\\foundations\\Cargo.toml --locked",
      "cargo test --manifest-path src-tauri\\foundations\\Cargo.toml --locked --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture",
      "cargo clippy --manifest-path src-tauri\\foundations\\Cargo.toml --locked --all-targets -- -D warnings",
    ],
  );
  assert.equal(scripts.build, "tsc --noEmit && vite build");
});

test("both repository release runners execute workflow contracts without weakening macOS coverage", () => {
  assert.equal(
    scripts["test:release"],
    "python3 -B -m unittest discover -s tests/release -p 'test_*.py' && node --test tests/release/workflow.test.mjs",
  );
  assert.equal(
    scripts["test:release:windows"],
    "python -B -m unittest discover -s tests/release -p test_release.py && node --test tests/release/workflow.test.mjs",
  );
});
