import { execFileSync } from "node:child_process";
import { lstatSync, readFileSync, readdirSync, realpathSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { digest, validateIdentity } from "./contract.mjs";
import { cases, selectCases } from "./registry.mjs";

const sourceRoot = fileURLToPath(new URL("../", import.meta.url));
const git = (cwd, ...args) =>
  execFileSync("git", ["-C", cwd, ...args], {
    encoding: "utf8",
    timeout: 10_000,
    stdio: ["ignore", "pipe", "pipe"],
  });

export function canonicalRuntime(cwd) {
  const common = realpathSync(
    git(cwd, "rev-parse", "--path-format=absolute", "--git-common-dir").trim(),
  );
  const first = git(cwd, "worktree", "list", "--porcelain", "-z").split(
    "\0\0",
  )[0];
  const fields = first.split("\0");
  if (!fields[0].startsWith("worktree ") || fields.includes("bare"))
    throw new Error("Primary non-bare worktree is required");
  const root = realpathSync(fields[0].slice("worktree ".length));
  if (
    realpathSync(
      git(
        root,
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
      ).trim(),
    ) !== common ||
    realpathSync(git(root, "rev-parse", "--show-toplevel").trim()) !== root
  )
    throw new Error("Canonical repository identity mismatch");
  return { root, runtime: join(root, ".regression-suite") };
}

function kind(path) {
  try {
    const stat = lstatSync(path);
    if (stat.isSymbolicLink()) return "symlink";
    return stat.isDirectory() ? "directory" : "file";
  } catch (error) {
    if (error.code === "ENOENT") return "absent";
    throw error;
  }
}

export function inspectReadiness(cwd, platform = process.platform) {
  const location = canonicalRuntime(cwd);
  const blocks = ["guest-native-driver-unimplemented"];
  const runtimeKind = kind(location.runtime);
  let environment = runtimeKind;
  if (platform !== "darwin") blocks.push("macos-required");
  if (runtimeKind === "absent") blocks.push("setup-missing");
  else if (runtimeKind !== "directory") blocks.push("unsafe-runtime-path");
  else if (kind(join(location.runtime, "guest.lock")) !== "absent") {
    environment = "busy-or-ownership-uncertain";
    blocks.push("guest-busy-or-ownership-uncertain");
  } else {
    environment = "present-but-unverified";
    blocks.push("setup-unverified");
  }
  return {
    ...location,
    environment,
    readiness: "BLOCKED",
    regressionStatus: "UNEXECUTED",
    blocks,
    cleanup: "not-started-no-runtime-writes",
  };
}

export function testSourceIdentity(
  directory = fileURLToPath(new URL("./", import.meta.url)),
) {
  const entries = readdirSync(directory, {
    recursive: true,
    withFileTypes: true,
  });
  if (entries.some((entry) => entry.isSymbolicLink()))
    throw new Error("Test sources must not link outside the submitted tree");
  const files = entries
    .filter((entry) => entry.isFile())
    .map((entry) =>
      relative(directory, join(entry.parentPath, entry.name))
        .split(sep)
        .join("/"),
    )
    .sort();
  if (!files.length) throw new Error("Test source tree is empty");
  const testFiles = Object.fromEntries(
    files.map((name) => [name, digest(readFileSync(join(directory, name)))]),
  );
  return { testFiles, testTreeSHA256: digest(JSON.stringify(testFiles)) };
}

export function prepareReport(cwd, requested, identity = null) {
  const selection = selectCases(requested);
  const report = {
    schemaVersion: 1,
    kind: "native-preparation",
    status: "BLOCKED",
    executed: 0,
    ...selection,
    candidate: identity,
    candidateQualification: "not-supplied",
    independence: "unassigned-not-verified",
    cases: selection.selected.map((id) => ({
      id,
      status: "BLOCKED",
      executed: false,
      reason: "guest-native-driver-unimplemented",
    })),
    potentialBugs: [],
    blocks: [
      "guest-native-driver-unimplemented",
      "independent-worker-unassigned",
    ],
    cleanup: "not-started-no-runtime-writes",
  };
  if (identity) {
    try {
      report.identitySHA256 = validateIdentity(identity);
      report.candidateQualification = "declared-only-not-artifact-verification";
    } catch (error) {
      report.candidateQualification = "invalid";
      report.blocks.push("invalid-candidate-identity");
      report.identityError = error.message;
    }
  } else report.blocks.push("candidate-identity-missing");
  if (selection.missingFeatures.length)
    report.blocks.push("missing-feature-coverage");
  try {
    report.environment = inspectReadiness(cwd);
    report.blocks.push(...report.environment.blocks);
    if (canonicalRuntime(sourceRoot).root !== report.environment.root)
      report.blocks.push("source-repository-mismatch");
    report.testCommit = git(sourceRoot, "rev-parse", "HEAD").trim();
    const status = git(
      sourceRoot,
      "status",
      "--porcelain",
      "--",
      "regression-suite",
    ).trim();
    report.testsDirty = Boolean(status);
    Object.assign(report, testSourceIdentity());
    if (
      identity &&
      (identity.testCommit !== report.testCommit ||
        identity.testTreeSHA256 !== report.testTreeSHA256)
    )
      report.blocks.push("test-identity-mismatch");
    if (status) report.blocks.push("test-worktree-dirty");
  } catch (error) {
    report.blocks.push("environment-or-test-inspection-failed");
    report.inspectionError = error.message;
  }
  report.blocks = [...new Set(report.blocks)];
  return report;
}

function main(args) {
  const [command, ...selection] = args;
  if (command === "list" && !selection.length) {
    console.log(JSON.stringify(cases, null, 2));
    return 0;
  }
  if (!["prepare", "run"].includes(command) || !selection.length)
    throw new Error(
      "Usage: node regression-suite/macos.mjs list | prepare|run full|FEATURE...",
    );
  // `run` is a fail-closed placeholder, not a launcher or fixture fallback.
  console.log(JSON.stringify(prepareReport(process.cwd(), selection), null, 2));
  return 20;
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.log(
      JSON.stringify({
        status: "BLOCKED",
        executed: 0,
        reason: "invalid-preparation-request",
        detail: error.message,
      }),
    );
    process.exitCode = 20;
  }
}
