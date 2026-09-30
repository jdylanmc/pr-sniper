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
import { dirname, join } from "node:path";
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
  assert.deepEqual(workflow.jobs.sign.needs, ["gate", "tap-ready"]);
  assert.equal(workflow.jobs["tap-ready"].needs, "gate");
  assert.equal(workflow.jobs["tap-ready"].environment, "pr-sniper-tap");
  assert.equal(workflow.jobs.tap["runs-on"], "ubuntu-latest");
  assert.ok(!JSON.stringify(workflow.jobs.tap).includes("brew "));
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

test("Windows native application checks run on every PR and main push with read-only access", () => {
  assert.equal(windows.name, "Windows native application");
  assert.deepEqual(windows.on, {
    pull_request: null,
    push: { branches: ["main"] },
  });
  assert.deepEqual(windows.permissions, { contents: "read" });
  assert.deepEqual(Object.keys(windows.jobs), [
    "windows",
    "windows-installer-acceptance",
  ]);
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
    [
      "actions/checkout",
      "actions/setup-node",
      "actions/setup-python",
      "actions/upload-artifact",
      "actions/upload-artifact",
      "actions/upload-artifact",
    ],
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
      "npm run test:packaging:windows",
      "rustup show active-toolchain",
      "npm run format:check",
      "cargo check --manifest-path src-tauri\\Cargo.toml --locked --all-targets",
      "cargo test --manifest-path src-tauri\\Cargo.toml --locked --all-targets -- --nocapture",
      "cargo test --manifest-path src-tauri\\Cargo.toml --locked --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture",
      "cargo clippy --manifest-path src-tauri\\Cargo.toml --locked --all-targets -- -D warnings",
      "npm exec playwright install chromium",
      "npm run test:settings",
      "npm run build:windows",
      "npm run bundle:windows",
      ".\\scripts\\windows-artifact.ps1",
      ".\\scripts\\windows-installer-artifact.ps1",
      ".\\scripts\\windows-upgrade-fixture.ps1",
    ],
  );
  assert.equal(scripts.build, "tsc --noEmit && vite build");
});

test("unsigned installers and disposable upgrade fixtures cannot become public release assets", () => {
  assert.equal(
    scripts["bundle:windows"],
    "tauri bundle --ci --no-sign --bundles nsis",
  );
  const steps = windows.jobs.windows.steps;
  assert.ok(
    steps.findIndex((s) => s.run === "npm run bundle:windows") <
      steps.findIndex((s) => s.run === ".\\scripts\\windows-artifact.ps1"),
    "Tauri patches bundle-type bytes before recording the executable hash",
  );
  const uploads = steps.filter((s) =>
    s.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(
    uploads[1].with.name,
    "pr-sniper-windows-unsigned-installer-${{ github.sha }}",
  );
  assert.equal(
    uploads[1].with.path,
    "src-tauri/target/windows-installer-artifact/",
  );
  assert.equal(
    uploads[2].with.name,
    "pr-sniper-windows-upgrade-test-only-${{ github.sha }}",
  );
  assert.doesNotMatch(JSON.stringify(workflow), /windows|chocolatey/i);
});

test("real installer acceptance depends on native checks and a fresh hosted VM", () => {
  const job = windows.jobs["windows-installer-acceptance"];
  assert.equal(job.needs, "windows");
  assert.equal(job["runs-on"], "windows-2022");
  assert.equal(job.environment, undefined);
  assert.equal(job["continue-on-error"], undefined);
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

test("Windows artifact contains only the standalone app and exact-source provenance", () => {
  const upload = windows.jobs.windows.steps.find((step) =>
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
    "python3 -B -m unittest discover -s tests/release -p 'test_*.py' && node --test tests/release/workflow.test.mjs",
  );
  assert.equal(
    scripts["test:release:windows"],
    "python -B -m unittest discover -s tests/release -p test_release.py && node --test tests/release/workflow.test.mjs",
  );
});
