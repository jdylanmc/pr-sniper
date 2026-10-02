import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { delimiter, join, resolve } from "node:path";
import test from "node:test";
import {
  Blocked,
  digest,
  exerciseFixture,
  summarizeAttempts,
  validateIdentity,
} from "../../regression-suite/contract.mjs";
import {
  canonicalRuntime,
  inspectReadiness,
  prepareReport,
  testSourceIdentity,
} from "../../regression-suite/macos.mjs";
import {
  cases,
  selectCases,
  validateRegistry,
} from "../../regression-suite/registry.mjs";

const hash = "a".repeat(64);
const identity = () => ({
  sourceCommit: "b".repeat(40),
  testCommit: "c".repeat(40),
  testTreeSHA256: hash,
  buildCommand: "fixture-only, no app build",
  original: {
    artifactSHA256: hash,
    executableSHA256: hash,
    bundleId: "com.jdylanmc.pr-sniper",
    signatureQualification: "synthetic-not-verified",
  },
  installed: {
    artifactSHA256: "d".repeat(64),
    executableSHA256: "e".repeat(64),
    bundleId: "com.jdylanmc.pr-sniper.tests.fixture",
    signatureQualification: "synthetic-test-copy-not-verified",
  },
  transformation: "synthetic fixture test-copy identity",
  installedPath: "/fixture/PR Sniper Test.app",
  instance: {
    id: "fixture-instance",
    pid: 123,
    startedAt: "2026-10-01T00:00:00Z",
  },
  isolation: {
    dataDir: "/fixture/profile",
    credentialNamespace: "com.jdylanmc.pr-sniper.tests.fixture",
  },
});

function fixtureDriver(entry, alter = (receipt) => receipt) {
  const context = {
    identity: identity(),
    runId: "fixture-run",
    attemptId: "fixture-attempt-1",
    startedAt: 100,
    now: () => 101,
  };
  let sequence = 0;
  let panel = {
    visible: false,
    windowId: "fixture-window",
    windowCount: 1,
    destination: "Queue",
    heading: "Your queue",
  };
  let draft;
  let restores = 0;
  const driver = {
    async prepare() {},
    async act(action, args) {
      if (action === "openPanel") panel.visible = true;
      else if (action === "dismissPanel") panel.visible = false;
      else if (action === "newDoctrineDraft") draft = { ...args };
      else if (action === "navigate") {
        panel.destination = args.destination;
        panel.heading = {
          Queue: "Your queue",
          Running: "Work queue",
          Reviewed: "Reviewed",
          Settings: "Settings",
        }[args.destination];
      } else throw new Blocked("unsupported-interaction");
    },
    async observe(target) {
      const value = structuredClone(target === "panel" ? panel : draft);
      return alter({
        target,
        value,
        identitySHA256: validateIdentity(context.identity),
        evidenceSHA256: digest(JSON.stringify(value)),
        runId: context.runId,
        attemptId: context.attemptId,
        caseId: entry.id,
        sequence: ++sequence,
        observedAt: 101,
      });
    },
    async restore() {
      restores++;
      return { baselineRestored: true, ownedInstanceTerminated: true };
    },
  };
  return { driver, context, restores: () => restores };
}

function inRepositoryFixture(body) {
  const parent = resolve("src-tauri/target");
  mkdirSync(parent, { recursive: true });
  const owned = mkdtempSync(join(parent, "regression-prep-contract-"));
  const primary = join(owned, "primary repository");
  mkdirSync(primary);
  const git = (...args) =>
    execFileSync("git", ["-C", primary, ...args], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        GIT_AUTHOR_NAME: "Fixture",
        GIT_AUTHOR_EMAIL: "fixture@example.invalid",
        GIT_COMMITTER_NAME: "Fixture",
        GIT_COMMITTER_EMAIL: "fixture@example.invalid",
      },
    });
  try {
    git("init", "--quiet");
    writeFileSync(join(primary, ".gitignore"), "/.regression-suite/\n");
    git("add", ".gitignore");
    const tree = git("write-tree").trim();
    const commit = git(
      "-c",
      "commit.gpgsign=false",
      "commit-tree",
      tree,
      "-m",
      "fixture",
    ).trim();
    git("update-ref", "HEAD", commit);
    body({ owned, primary: realpathSync(primary), git });
  } catch (error) {
    console.error(`Preserved failed fixture: ${owned}`);
    throw error;
  }
  rmSync(owned, { recursive: true });
}

test("registry requires executable cases and selection keeps missing coverage visible", () => {
  validateRegistry();
  assert.throws(() => validateRegistry([]));
  assert.throws(() => validateRegistry([cases[0], cases[0]]));
  assert.throws(() => validateRegistry([{ ...cases[0], run: null }]));
  assert.throws(() =>
    validateRegistry([{ ...cases[0], preconditions: "not an array" }]),
  );
  assert.throws(() => validateRegistry([{ ...cases[0], cleanup: [""] }]));
  for (const request of [
    [],
    ["full", "panel-drafts"],
    ["panel-drafts", "panel-drafts"],
  ])
    assert.throws(() => selectCases(request));
  const subset = selectCases(["panel-navigation", "unknown-feature"]);
  assert.deepEqual(subset.selected, ["panel-destinations"]);
  assert.deepEqual(subset.notSelected, ["panel-unsaved-draft"]);
  assert.deepEqual(subset.missingFeatures, ["unknown-feature"]);
  assert.deepEqual(
    selectCases(["full"]).selected,
    cases.map((entry) => entry.id),
  );
  assert.ok(
    subset.coverageLimits.some((limit) => limit.includes("unregistered")),
  );
});

for (const entry of cases) {
  test(`${entry.id}: executable shared expectations pass fixtures, never native`, async () => {
    const fixture = fixtureDriver(entry);
    const result = await exerciseFixture(
      entry,
      fixture.driver,
      fixture.context,
    );
    assert.equal(result.status, "PASS");
    assert.equal(result.nativeStatus, "BLOCKED");
    assert.equal(result.evidenceKind, "fixture-contract");
    assert.ok(result.observations.length > 0);
    assert.ok(result.assertions.length > 0);
    assert.equal(fixture.restores(), 1);
  });
  test(`${entry.id}: wrong actual output remains raw FAIL (negative control)`, async () => {
    const fixture = fixtureDriver(entry, (receipt) => {
      receipt.value = { ...receipt.value, visible: false };
      receipt.evidenceSHA256 = digest(JSON.stringify(receipt.value));
      return receipt;
    });
    const result = await exerciseFixture(
      entry,
      fixture.driver,
      fixture.context,
    );
    assert.equal(result.status, "FAIL");
    assert.equal(result.reason, "observable-mismatch");
    assert.equal(fixture.restores(), 1);
    const good = fixtureDriver(entry);
    good.context.attemptId = "fixture-attempt-2";
    const retry = await exerciseFixture(entry, good.driver, good.context);
    const summary = summarizeAttempts([result, retry]);
    assert.equal(summary.status, "FAIL");
    assert.deepEqual(summary.counts, { PASS: 1, FAIL: 1, BLOCKED: 0 });
    assert.equal(summary.nativeStatus, "BLOCKED");
  });
}

for (const [name, mutate] of [
  [
    "candidate binding",
    (r) => {
      r.identitySHA256 = "f".repeat(64);
    },
  ],
  [
    "case",
    (r) => {
      r.caseId = "other-case";
    },
  ],
  [
    "run",
    (r) => {
      r.runId = "other-run";
    },
  ],
  [
    "attempt",
    (r) => {
      r.attemptId = "old-attempt";
    },
  ],
  [
    "target",
    (r) => {
      r.target = "other-target";
    },
  ],
  [
    "stale time",
    (r) => {
      r.observedAt = 100;
    },
  ],
  [
    "future time",
    (r) => {
      r.observedAt = 102;
    },
  ],
  [
    "sequence",
    (r) => {
      r.sequence = 0;
    },
  ],
  [
    "payload digest",
    (r) => {
      r.evidenceSHA256 = "f".repeat(64);
    },
  ],
  ["absent observation", () => null],
]) {
  test(`invalid ${name} blocks rather than passing`, async () => {
    const fixture = fixtureDriver(cases[0], (r) =>
      mutate(r) === null ? null : r,
    );
    const result = await exerciseFixture(
      cases[0],
      fixture.driver,
      fixture.context,
    );
    assert.equal(result.status, "BLOCKED");
    assert.equal(result.reason, "invalid-or-stale-observation");
    assert.equal(fixture.restores(), 1);
  });
}

test("identity failures block before any fixture action", async () => {
  for (const mutate of [
    (i) => {
      delete i.sourceCommit;
    },
    (i) => {
      i.testTreeSHA256 = "missing";
    },
    (i) => {
      delete i.installed.signatureQualification;
    },
    (i) => {
      i.instance.pid = 0;
    },
    (i) => {
      i.isolation.dataDir = "relative";
    },
    (i) => {
      i.isolation.credentialNamespace = "production";
    },
    (i) => {
      i.transformation = null;
    },
  ]) {
    const fixture = fixtureDriver(cases[0]);
    mutate(fixture.context.identity);
    const result = await exerciseFixture(
      cases[0],
      fixture.driver,
      fixture.context,
    );
    assert.equal(result.reason, "invalid-identity");
    assert.equal(result.executed, false);
    assert.equal(fixture.restores(), 0);
  }
});

test("unsupported actions, preparation failure and driver assertions are BLOCKED", async () => {
  for (const phase of ["prepare", "act", "observe"]) {
    const fixture = fixtureDriver(cases[0]);
    fixture.driver[phase] = async () => {
      assert.fail("fixture driver failure");
    };
    const result = await exerciseFixture(
      cases[0],
      fixture.driver,
      fixture.context,
    );
    assert.equal(result.status, "BLOCKED");
    assert.equal(result.reason, "harness-error");
    assert.equal(result.executed, phase !== "prepare");
    assert.equal(fixture.restores(), 1);
  }
  const fixture = fixtureDriver(cases[0]);
  fixture.driver.act = undefined;
  assert.equal(
    (await exerciseFixture(cases[0], fixture.driver, fixture.context)).reason,
    "interaction-unavailable",
  );
});

test("cleanup failure cannot pass and does not erase a primary FAIL", async () => {
  for (const wrong of [false, true]) {
    const fixture = fixtureDriver(cases[0], (receipt) => {
      if (wrong) {
        receipt.value.visible = false;
        receipt.evidenceSHA256 = digest(JSON.stringify(receipt.value));
      }
      return receipt;
    });
    fixture.driver.restore = async () => ({
      baselineRestored: false,
      ownedInstanceTerminated: false,
    });
    const result = await exerciseFixture(
      cases[0],
      fixture.driver,
      fixture.context,
    );
    assert.equal(result.status, wrong ? "FAIL" : "BLOCKED");
    assert.equal(result.cleanup, "uncertain-retain-ownership");
    assert.equal(result.failures.at(-1).reason, "cleanup-unverified");
  }
  assert.equal(summarizeAttempts([]).status, "BLOCKED");
});

test("primary and linked feature worktrees share one absent canonical runtime", () => {
  inRepositoryFixture(({ owned, primary, git }) => {
    const feature = join(owned, "feature worktree");
    git("worktree", "add", "--quiet", "-b", "fixture-feature", feature);
    assert.deepEqual(canonicalRuntime(feature), canonicalRuntime(primary));
    assert.equal(
      canonicalRuntime(feature).runtime,
      join(primary, ".regression-suite"),
    );
    assert.equal(inspectReadiness(feature, "darwin").environment, "absent");
    assert.ok(
      inspectReadiness(feature, "darwin").blocks.includes("setup-missing"),
    );
    assert.equal(existsSync(join(primary, ".regression-suite")), false);
    assert.equal(existsSync(join(feature, ".regression-suite")), false);
    assert.equal(git("status", "--porcelain"), "");
  });
});

test("source hashes include added case files and are independent of enumeration order", () => {
  inRepositoryFixture(({ owned }) => {
    const source = join(owned, "test-sources");
    mkdirSync(join(source, "cases"), { recursive: true });
    writeFileSync(join(source, "registry.mjs"), "fixture registry");
    const before = testSourceIdentity(source);
    writeFileSync(join(source, "cases", "new.mjs"), "fixture case");
    const added = testSourceIdentity(source);
    assert.notEqual(added.testTreeSHA256, before.testTreeSHA256);
    assert.deepEqual(Object.keys(added.testFiles), [
      "cases/new.mjs",
      "registry.mjs",
    ]);
    writeFileSync(join(source, "cases", "new.mjs"), "changed fixture case");
    assert.notEqual(
      testSourceIdentity(source).testTreeSHA256,
      added.testTreeSHA256,
    );
    symlinkSync(source, join(source, "linked"), "junction");
    assert.throws(() => testSourceIdentity(source), /must not link/);
  });
});

test("presence is unverified; busy/uncertain claims block and are never changed", () => {
  inRepositoryFixture(({ primary }) => {
    const runtime = join(primary, ".regression-suite");
    mkdirSync(runtime);
    assert.equal(
      inspectReadiness(primary, "darwin").environment,
      "present-but-unverified",
    );
    writeFileSync(join(runtime, "guest.lock"), "unknown-owner-do-not-remove");
    for (let request = 0; request < 2; request++) {
      const report = prepareReport(primary, ["full"]);
      assert.equal(report.status, "BLOCKED");
      assert.equal(report.executed, 0);
      assert.ok(report.blocks.includes("guest-busy-or-ownership-uncertain"));
      assert.equal(
        readFileSync(join(runtime, "guest.lock"), "utf8"),
        "unknown-owner-do-not-remove",
      );
    }
    assert.ok(
      inspectReadiness(primary, "win32").blocks.includes("macos-required"),
    );
  });
});

test("linked runtime is refused without inspecting its target", () => {
  inRepositoryFixture(({ primary, owned }) => {
    const outside = join(owned, "unrelated");
    mkdirSync(outside);
    symlinkSync(outside, join(primary, ".regression-suite"), "junction");
    assert.ok(
      inspectReadiness(primary, "darwin").blocks.includes(
        "unsafe-runtime-path",
      ),
    );
    assert.equal(existsSync(join(outside, "guest.lock")), false);
  });
});

test("bare repository and invalid identity produce visible blockers", () => {
  inRepositoryFixture(({ owned, primary }) => {
    const bare = join(owned, "bare");
    execFileSync("git", ["init", "--quiet", "--bare", bare]);
    assert.throws(
      () => canonicalRuntime(bare),
      /Primary non-bare worktree is required|cannot use bare repository/,
    );
    assert.ok(
      prepareReport(bare, ["full"]).blocks.includes(
        "environment-or-test-inspection-failed",
      ),
    );
    const invalid = prepareReport(primary, ["unknown-feature"], {});
    assert.equal(invalid.candidateQualification, "invalid");
    assert.ok(invalid.blocks.includes("invalid-candidate-identity"));
    assert.ok(invalid.blocks.includes("missing-feature-coverage"));
    const declared = prepareReport(primary, ["full"], identity());
    assert.equal(
      declared.candidateQualification,
      "declared-only-not-artifact-verification",
    );
    assert.ok(declared.blocks.includes("test-identity-mismatch"));
    assert.ok(declared.blocks.includes("source-repository-mismatch"));
    assert.equal(declared.status, "BLOCKED");
  });
});

test("CLI run refuses native execution even with fake VM tools present", () => {
  inRepositoryFixture(({ owned, primary }) => {
    const tools = join(owned, "fake-tools");
    const marker = join(owned, "forbidden-call");
    mkdirSync(tools);
    for (const name of [
      "tart",
      "ssh",
      "open",
      "osascript",
      "swift",
      "xcrun",
      "launchctl",
      "codesign",
    ]) {
      writeFileSync(
        join(tools, name),
        `#!/bin/sh\nprintf forbidden > "$REGRESSION_TRAP"\nexit 99\n`,
        { mode: 0o700 },
      );
      writeFileSync(
        join(tools, `${name}.cmd`),
        "@echo forbidden>%REGRESSION_TRAP%\r\n@exit /b 99\r\n",
      );
    }
    for (const args of [
      ["run", "full"],
      ["prepare", "missing-feature"],
      ["run"],
      ["prepare", "full", "panel-drafts"],
    ]) {
      const child = spawnSync(
        process.execPath,
        [resolve("regression-suite/macos.mjs"), ...args],
        {
          cwd: primary,
          env: {
            ...process.env,
            PATH: `${tools}${delimiter}${process.env.PATH}`,
            REGRESSION_TRAP: marker,
          },
          encoding: "utf8",
        },
      );
      assert.equal(child.status, 20, child.stderr);
      const report = JSON.parse(child.stdout);
      assert.equal(report.status, "BLOCKED");
      assert.equal(report.executed, 0);
      assert.equal(existsSync(marker), false);
      assert.equal(existsSync(join(primary, ".regression-suite")), false);
    }
  });
});

test("three owned skills preserve approved clauses and all local Markdown links resolve", () => {
  const discovery = readFileSync(
    "docs/agent/discovery/pr-sniper-vm-regression.md",
    "utf8",
  );
  const nano = readFileSync(
    "docs/agent/specs/pr-sniper-vm-regression.nano.md",
    "utf8",
  );
  for (const [name, criteria] of [
    ["setup-regression-suite-mac", ["D01", "D02", "D03", "D06", "AC-003"]],
    ["regression-test", ["D04", "D06", "AC-005"]],
    ["regression-suite-mac", ["D05", "D07", "AC-006"]],
  ]) {
    const directory = `.github/skills/${name}`;
    const skill = readFileSync(`${directory}/SKILL.md`, "utf8");
    const intent = readFileSync(`${directory}/intent.md`, "utf8");
    assert.ok(skill.startsWith(`---\nname: ${name}\n`));
    assert.ok(
      skill.includes(
        `disable-model-invocation: ${name === "setup-regression-suite-mac"}`,
      ),
    );
    assert.ok(!skill.includes("allowed-tools:"));
    for (const id of criteria) {
      const source = id.startsWith("D") ? discovery : nano;
      const clause = source
        .split("\n")
        .find(
          (line) => line.startsWith(`- ${id} `) || line.startsWith(`- ${id}:`),
        );
      assert.ok(clause, id);
      assert.ok(intent.includes(clause), `${name}: exact ${id} provenance`);
    }
    for (const document of [skill, intent]) {
      for (const [, link] of document.matchAll(/\]\(([^)]+)\)/g))
        assert.ok(existsSync(resolve(directory, link)), link);
    }
  }
});

test("fixture gate is additive in both platform workflows and formatting", () => {
  const { scripts } = JSON.parse(readFileSync("package.json", "utf8"));
  assert.equal(
    scripts["test:regression"],
    "node --test tests/regression/*.test.mjs",
  );
  for (const platform of ["macos", "windows"]) {
    const workflow = readFileSync(`.github/workflows/${platform}.yml`, "utf8");
    assert.ok(workflow.includes("- run: npm run test:regression"));
    assert.ok(workflow.includes("- run: npm run test:settings"));
  }
  for (const command of ["format", "format:check"])
    assert.ok(scripts[command].includes("regression-suite tests/regression"));
  assert.ok(
    readFileSync(".gitignore", "utf8")
      .split("\n")
      .includes("/.regression-suite/"),
  );
});
