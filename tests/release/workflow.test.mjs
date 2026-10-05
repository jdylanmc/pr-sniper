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
    workflow_dispatch: null,
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

test("Rust caching retains dependencies, not application artifacts or PR-written cache entries", () => {
  const steps = windows.jobs.windows.steps;
  const cache = steps.find((step) =>
    step.uses?.startsWith("Swatinem/rust-cache@"),
  );
  assert.ok(cache);
  assert.match(cache.uses, /^Swatinem\/rust-cache@[a-f0-9]{40}$/);
  assert.ok(
    steps.indexOf(cache) >
      steps.findIndex((step) => step.run === "rustup show active-toolchain"),
  );
  assert.equal(cache.with.workspaces, "src-tauri -> target");
  assert.equal(cache.with["cache-targets"], "false");
  assert.equal(cache.with["cache-bin"], "false");
  assert.equal(cache.with["cache-workspace-crates"], "false");
  assert.equal(cache.with["cache-on-failure"], "false");
  assert.equal(
    cache.with["save-if"],
    "${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}",
  );
  assert.deepEqual(cache.with["cache-directories"].trim().split("\n"), [
    "src-tauri\\target\\debug\\.fingerprint",
    "src-tauri\\target\\debug\\build",
    "src-tauri\\target\\debug\\deps",
    "src-tauri\\target\\release\\.fingerprint",
    "src-tauri\\target\\release\\build",
    "src-tauri\\target\\release\\deps",
  ]);
  assert.equal(cache.with["cache-provider"], undefined);
  assert.equal(cache.with["add-rust-environment-hash-key"], undefined);
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
      "Swatinem/rust-cache",
      "actions/upload-artifact",
      "actions/upload-artifact",
      "actions/upload-artifact",
    ],
  );
  for (const step of actions) {
    assert.match(
      step.uses,
      /^(?:actions\/[a-z-]+|Swatinem\/rust-cache)@[a-f0-9]{40}$/,
    );
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
    "Create the installer before recording separate standalone and payload provenance",
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

test("hosted macOS WebKit adds a read-only pinned full-engine gate without native operations", () => {
  const webkit = parse(
    readFileSync(".github/workflows/macos-webkit.yml", "utf8"),
  );
  assert.deepEqual(webkit.on, {
    pull_request: null,
    push: { branches: ["main"] },
  });
  assert.deepEqual(webkit.permissions, { contents: "read" });
  assert.deepEqual(Object.keys(webkit.jobs), ["webkit"]);
  const job = webkit.jobs.webkit;
  assert.equal(job["runs-on"], "macos-15");
  assert.equal(job["runs-on"], ci.jobs.macos["runs-on"]);
  assert.equal(job["timeout-minutes"], 40);
  assert.equal(job.environment, undefined);
  assert.equal(job.permissions, undefined);
  assert.equal(job.needs, undefined);
  assert.equal(job.strategy, undefined);
  for (const scope of [webkit, job, ...job.steps]) {
    assert.equal(scope.env, undefined);
    assert.equal(scope.if, undefined);
    assert.equal(scope["continue-on-error"], undefined);
    assert.equal(scope.defaults, undefined);
  }
  assert.doesNotMatch(JSON.stringify(webkit), /secrets\.|github\.token/);
  const actions = job.steps.filter((step) => step.uses);
  assert.deepEqual(
    actions.map((step) => step.uses),
    [
      "actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
      "actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020",
    ],
  );
  for (const action of actions) {
    assert.ok(
      windows.jobs.windows.steps.some((step) => step.uses === action.uses),
      "Reuse the trusted Windows workflow's immutable action pins",
    );
  }
  assert.deepEqual(actions[0].with, { "persist-credentials": false });
  assert.deepEqual(actions[1].with, {
    "node-version-file": ".node-version",
    cache: "npm",
  });
  assert.equal(job.steps.length, 6);
  assert.deepEqual(
    job.steps.filter((step) => step.run).map((step) => step.run),
    [
      "rustup show active-toolchain",
      "npm ci",
      "npm exec playwright install webkit",
      "npm run test:settings -- --browser=webkit",
    ],
  );
  for (const step of job.steps.filter((step) => step.run)) {
    assert.ok(step["timeout-minutes"] > 0);
    assert.ok(step["timeout-minutes"] < job["timeout-minutes"]);
  }
  assert.equal(
    scripts["test:settings"],
    "npm run build && cargo build --manifest-path src-tauri/Cargo.toml --locked --example settings_bridge && playwright test --config tests/settings/playwright.config.mjs",
  );
  assert.match(
    readFileSync("rust-toolchain.toml", "utf8"),
    /\[toolchain\]\s+channel = "\d+\.\d+\.\d+"/,
  );
});
