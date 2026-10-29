// Native MCP output (pinned by src-tauri control dispatcher tests) must satisfy
// the adapter's structured-output schemas, so the two cannot drift apart.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import * as s from "../../src-control/schemas.mjs";

const read = (name) =>
  JSON.parse(
    fs.readFileSync(new URL(`fixtures/${name}`, import.meta.url), "utf8"),
  );
const queue = read("native-queue.json");
const enums = read("native-enums.json");

test("schema enums are exactly the native ids", () => {
  assert.deepEqual(s.exitName.options, enums.exit);
  assert.deepEqual(s.progress.options, enums.progress);
  assert.deepEqual(s.mergeBlock.options, enums.merge_block);
  assert.deepEqual(s.disposition.options, enums.disposition);
  assert.deepEqual(s.modelRefusal.options, enums.model_refusal);
  assert.deepEqual(s.failureClass.options, enums.failure_class);
  assert.deepEqual(s.failedOperation.options, enums.failed_operation);
  assert.deepEqual(s.remedy.options, enums.remedy);
});

test("a parked model refusal parses with its Agent, model and status", () => {
  const [item] = queue.values;
  const parked = {
    ...item,
    state: "failed",
    failure_class: "model",
    failed_model: {
      agent_id: "agent-1",
      agent_name: "Reviewer",
      model: "example-model-fast",
      refusal: "rejected",
      status: 401,
    },
    failed_operation: "review",
    remedy: "change_model",
  };
  assert.deepEqual(s.queueItem.parse(parked), parked);
  for (const bad of [
    { ...parked, failed_model: { ...parked.failed_model, refusal: "denied" } },
    { ...parked, failed_model: { ...parked.failed_model, token: "x" } },
    { ...parked, remedy: "reconnect" },
  ])
    assert.equal(s.queueItem.safeParse(bad).success, false);
});

test("native queue output parses as the MCP queue page", () => {
  assert.deepEqual(s.page(s.queueItem).parse(queue), queue);
});

test("typed exit, progress and merge pending parse; unknown values do not", () => {
  const [item] = queue.values;
  const typed = {
    ...item,
    exit: "lgtm",
    progress: "merge_pending_conflicts",
    merge_pending: enums.merge_pending,
    reviews: item.reviews.map((r) => ({ ...r, exit: "lgtm" })),
  };
  assert.deepEqual(s.queueItem.parse(typed), typed);
  for (const bad of [
    { ...typed, progress: "merge_pending" },
    { ...typed, merge_pending: { reason: "behind", ci_not_run: false } },
    {
      ...typed,
      reviews: [{ ...typed.reviews[0], dispositions: { closed: 1 } }],
    },
  ])
    assert.equal(s.queueItem.safeParse(bad).success, false);
});
