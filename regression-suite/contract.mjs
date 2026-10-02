import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { posix } from "node:path";

const sha = /^[a-f0-9]{64}$/;
const commit = /^[a-f0-9]{40}$/;
const token = /^[a-zA-Z0-9][a-zA-Z0-9_.:-]{0,127}$/;
export const digest = (bytes) =>
  createHash("sha256").update(bytes).digest("hex");

export function validateIdentity(identity) {
  assert.match(identity.sourceCommit, commit);
  assert.match(identity.testCommit, commit);
  assert.match(identity.testTreeSHA256, sha);
  assert.ok(typeof identity.buildCommand === "string" && identity.buildCommand);
  for (const artifact of [identity.original, identity.installed]) {
    assert.match(artifact.artifactSHA256, sha);
    assert.match(artifact.executableSHA256, sha);
    assert.ok(typeof artifact.bundleId === "string" && artifact.bundleId);
    assert.ok(
      typeof artifact.signatureQualification === "string" &&
        artifact.signatureQualification,
    );
  }
  const changed = [
    "artifactSHA256",
    "executableSHA256",
    "bundleId",
    "signatureQualification",
  ].some((field) => identity.original[field] !== identity.installed[field]);
  assert.ok(
    changed
      ? typeof identity.transformation === "string" && identity.transformation
      : identity.transformation === null,
    "Original and transformed identities must remain distinct",
  );
  assert.ok(posix.isAbsolute(identity.installedPath));
  assert.notEqual(posix.normalize(identity.installedPath), "/");
  assert.match(identity.instance.id, token);
  assert.ok(
    Number.isSafeInteger(identity.instance.pid) && identity.instance.pid > 0,
  );
  assert.ok(Number.isFinite(Date.parse(identity.instance.startedAt)));
  assert.ok(posix.isAbsolute(identity.isolation.dataDir));
  assert.notEqual(posix.normalize(identity.isolation.dataDir), "/");
  assert.match(
    identity.isolation.credentialNamespace,
    /^com\.jdylanmc\.pr-sniper\.tests\.[a-zA-Z0-9-]+$/,
  );
  return digest(JSON.stringify(identity));
}

export class Blocked extends Error {}
class ObservableMismatch extends Error {}

// Contract fixtures only. There is deliberately no switch that enables native PASS.
export async function exerciseFixture(entry, driver, context) {
  const observations = [];
  const assertions = [];
  const failures = [];
  let executed = false;
  let status = "BLOCKED";
  let reason = "unexecuted";
  let cleanup = "not-started";
  let sequence = 0;
  let identitySHA256;
  try {
    identitySHA256 = validateIdentity(context.identity);
    assert.match(context.runId, token);
    assert.match(context.attemptId, token);
    assert.ok(Number.isFinite(context.startedAt));
  } catch (error) {
    return {
      id: entry.id,
      status,
      reason: "invalid-identity",
      detail: error.message,
      executed,
      cleanup,
      evidenceKind: "fixture-contract",
      nativeStatus: "BLOCKED",
    };
  }
  try {
    await driver.prepare(entry.preconditions);
    executed = true;
    await entry.run({
      act: async (action, args = {}) => {
        if (typeof driver.act !== "function")
          throw new Blocked("interaction-unavailable");
        await driver.act(action, args);
      },
      observe: async (target) => {
        const requestedAt = context.now();
        const receipt = await driver.observe(target);
        const now = context.now();
        if (
          !receipt ||
          receipt.target !== target ||
          receipt.runId !== context.runId ||
          receipt.attemptId !== context.attemptId ||
          receipt.caseId !== entry.id ||
          receipt.identitySHA256 !== identitySHA256 ||
          !Number.isSafeInteger(receipt.sequence) ||
          receipt.sequence <= sequence ||
          !Number.isFinite(receipt.observedAt) ||
          receipt.observedAt < context.startedAt ||
          receipt.observedAt < requestedAt ||
          receipt.observedAt > now ||
          !sha.test(receipt.evidenceSHA256) ||
          receipt.evidenceSHA256 !== digest(JSON.stringify(receipt.value))
        )
          throw new Blocked("invalid-or-stale-observation");
        sequence = receipt.sequence;
        observations.push(receipt);
        return receipt.value;
      },
      equal: (actual, expected, expectation) => {
        assertions.push({ expectation, actual, expected });
        try {
          assert.deepEqual(actual, expected, expectation);
        } catch (error) {
          throw new ObservableMismatch(error.message);
        }
      },
    });
    if (!observations.length || !assertions.length)
      throw new Blocked("no-observable-assertions");
    status = "PASS";
    reason = "observable-expectations-met";
  } catch (error) {
    status = error instanceof ObservableMismatch ? "FAIL" : "BLOCKED";
    reason =
      status === "FAIL"
        ? "observable-mismatch"
        : error instanceof Blocked
          ? error.message
          : "harness-error";
    failures.push({ status, reason, detail: error.message });
  } finally {
    try {
      const restored = await driver.restore(entry.cleanup);
      if (
        restored?.baselineRestored !== true ||
        restored?.ownedInstanceTerminated !== true
      )
        throw new Blocked("restoration-or-termination-unverified");
      cleanup = "restored-and-terminated";
    } catch (error) {
      cleanup = "uncertain-retain-ownership";
      failures.push({
        status: "BLOCKED",
        reason: "cleanup-unverified",
        detail: error.message,
      });
      if (status !== "FAIL") {
        status = "BLOCKED";
        reason = "cleanup-unverified";
      }
    }
  }
  return {
    id: entry.id,
    runId: context.runId,
    attemptId: context.attemptId,
    identitySHA256,
    evidenceKind: "fixture-contract",
    nativeStatus: "BLOCKED",
    executed,
    status,
    reason,
    observations,
    assertions,
    failures,
    cleanup,
  };
}

export function summarizeAttempts(attempts) {
  return {
    evidenceKind: "fixture-contract",
    nativeStatus: "BLOCKED",
    attempts,
    counts: Object.fromEntries(
      ["PASS", "FAIL", "BLOCKED"].map((status) => [
        status,
        attempts.filter((attempt) => attempt.status === status).length,
      ]),
    ),
    // Retries never remove previous failures or cleanup uncertainty.
    status: attempts.some((attempt) => attempt.status === "FAIL")
      ? "FAIL"
      : !attempts.length ||
          attempts.some(
            (attempt) =>
              attempt.status !== "PASS" ||
              attempt.cleanup !== "restored-and-terminated",
          )
        ? "BLOCKED"
        : "PASS",
  };
}
