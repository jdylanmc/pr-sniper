import { test as base, expect } from "@playwright/test";
import { queueEvidenceFixture } from "../queue-reload-evidence.mjs";

const test = base.extend({ queueEvidence: queueEvidenceFixture });

test("controlled mismatch", async ({ page, queueEvidence }) => {
  await page.setContent("<style>html { background: white }</style>");
  await queueEvidence.capture("before", await page.screenshot());
  await page.setContent("<style>html { background: black }</style>");
  await queueEvidence.capture("reload", await page.screenshot());
  await queueEvidence.phase("reload-comparison");
  await queueEvidence.compare(1);
  expect(1).toBeLessThanOrEqual(0.001);
});

test("controlled step abort", async ({ page, queueEvidence }) => {
  await page.setContent("<style>html { background: white }</style>");
  await queueEvidence.capture("before", await page.screenshot());
  await queueEvidence.phase("reload-navigation");
  await test.step("abort before reload capture", async () => {
    throw new Error(
      "Synthetic step failure: secret-marker-must-not-be-exported",
    );
  });
});

test("controlled timeout", async ({ page, queueEvidence }) => {
  await page.setContent("<style>html { background: white }</style>");
  await queueEvidence.capture("before", await page.screenshot());
  await queueEvidence.phase("reload-navigation");
  // Expiry is the behavior under test, not a sleep or a retry-to-green.
  test.setTimeout(test.info().duration + 500);
  await new Promise(() => {});
});
