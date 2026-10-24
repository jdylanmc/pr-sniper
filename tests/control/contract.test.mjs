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
