import { expect, test } from "./fixtures.mjs";
import {
  section,
  saveChanges,
  repositorySettings,
  closeDialog,
} from "./navigation.mjs";

test("publication permission is independent, inherited and persisted", async ({
  page,
  store,
}) => {
  await store("save_repository", { repository: "fixture/publication-policy" });
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  const automatic = page.getByRole("switch", {
    name: /^Publish review comments automatically/,
  });
  await expect(automatic).not.toBeChecked();
  await automatic.check();
  expect(
    (await store("snapshot")).settings.defaults.automatic_comment_publication,
  ).toBe(false);
  await saveChanges(page);
  const saved = (await store("snapshot")).settings;
  expect(saved.defaults.automatic_comment_publication).toBe(true);
  expect(saved.defaults.automatic_agent_start).toBe(false);
  await section(page, "Integrations");
  let modal = await repositorySettings(page, "fixture/publication-policy");
  await expect(
    modal.getByLabel("Comment publication", { exact: true }),
  ).toHaveValue("inherit");
  await modal
    .getByLabel("Comment publication", { exact: true })
    .selectOption("manual");
  await closeDialog(page);
  await saveChanges(page);
  expect(
    (await store("snapshot")).settings.repositories[0].overrides
      .automatic_comment_publication,
  ).toBe(false);
  await page.reload();
  modal = await repositorySettings(page, "fixture/publication-policy");
  await expect(
    modal.getByLabel("Comment publication", { exact: true }),
  ).toHaveValue("manual");
  await modal
    .getByLabel("Comment publication", { exact: true })
    .selectOption("inherit");
  await closeDialog(page);
  await saveChanges(page);
  expect(
    (await store("snapshot")).settings.repositories[0].overrides
      ?.automatic_comment_publication,
  ).toBeUndefined();
});

function snapshot(phase = null) {
  const operation = {
    id: "review-1",
    state: "completed",
    attempt_count: 1,
    retry_deadline: 1_800_000_900,
  };
  return {
    health: [],
    jobs: [],
    reviews: [
      {
        key: "exact-head",
        assignment_id: "assignment",
        agent_name: "Reviewer",
        trust_required: false,
        blocked: null,
        job: {
          account_id: "22",
          account_login: "repo-owner",
          repository_name: "example/repo",
          number: 1,
          head_sha: "a".repeat(40),
        },
        run: {
          operation,
          phase: "Automated review complete",
          error: null,
          selection: {
            agent: {
              model: "configured-model",
              ai_account: { account_id: "33" },
            },
          },
          result: {
            reviewed_base_sha: "b".repeat(40),
            output: {
              synopsis: "A change needs human review.",
              decision: "human_input_required",
              files: [
                {
                  path: "source.rs",
                  explanation: "Changed implementation.",
                  order: 1,
                },
              ],
              findings: [
                {
                  path: "source.rs",
                  side: "head",
                  line: 100,
                  severity: "high",
                  title: "Defect",
                  explanation: "Evidence.",
                  confidence: 90,
                },
              ],
            },
            session_id: "session",
            model: "configured-model",
            runtime_version: "fixture",
            input_tokens: 10,
            output_tokens: 10,
            tool_calls: 1,
          },
        },
      },
    ],
    publications: [
      {
        review_operation_id: "review-1",
        automatic: false,
        blocked: null,
        publication: phase
          ? {
              id: "publication-1",
              phase,
              error:
                phase === "unresolved" ? "Provider response was lost." : null,
              operation: {
                ...operation,
                id: "publication-op",
                state:
                  phase === "pending"
                    ? "running"
                    : phase === "unresolved"
                      ? "manual_retry"
                      : "completed",
              },
              uncertain: phase === "unresolved",
              cancelled: false,
              history: [],
              batch: { unmappable: [0] },
              receipts:
                phase === "unresolved"
                  ? []
                  : [
                      {
                        review_id: "42",
                        state: phase === "pending" ? "pending" : "commented",
                        comment_ids: [],
                      },
                    ],
            }
          : null,
      },
    ],
  };
}

async function setup(page, state) {
  await page.addInitScript((snapshot) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__publicationActions = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "monitoring_snapshot") return Promise.resolve(snapshot);
      if (
        ["publish_review", "cancel_publication", "start_review"].includes(
          command,
        )
      ) {
        window.__publicationActions.push({ command, args });
        return Promise.resolve();
      }
      return original(command, args);
    };
  }, state);
  await page.goto("/?view=queue");
}

test("publication-off normal output is local-only with no invented manual-publication task", async ({
  page,
}) => {
  const state = snapshot();
  state.publications[0].local_only = true;
  await setup(page, state);
  await expect(
    page.getByRole("button", {
      name: "Publish review",
      exact: true,
    }),
  ).toHaveCount(0);
  await expect(page.locator("#agent-reviews")).toContainText("repo-owner (22)");
  await expect(page.locator("#agent-reviews")).toContainText(
    "Local-only evidence",
  );
  await expect(
    page.getByRole("checkbox", {
      name: /Publish or reconcile this exact revision/,
    }),
  ).toHaveCount(0);
  await expect
    .poll(() => page.evaluate(() => window.__publicationActions))
    .toEqual([]);
  await expect(page.getByRole("button", { name: /^Approve/ })).toHaveCount(0);
});

test("pending publication exposes withdrawal without claiming it can undo published comments", async ({
  page,
}) => {
  await setup(page, snapshot("pending"));
  await page
    .getByRole("button", { name: "Withdraw publication confirmation" })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.__publicationActions))
    .toEqual([
      {
        command: "cancel_publication",
        args: { publicationId: "publication-1" },
      },
    ]);
  await expect(page.locator("#agent-reviews")).toContainText(
    "Confirmed GitHub review 42: pending",
  );
});

test("uncertain publication exposes reconciliation and retains unmappable findings", async ({
  page,
}) => {
  await setup(page, snapshot("unresolved"));
  const root = page.locator("#agent-reviews");
  await expect(root).toContainText("GitHub outcome is unresolved");
  await expect(root).toContainText("1 finding(s) could not be mapped");
  await expect(root).toContainText("Defect (source.rs");
  const retry = page.getByRole("button", {
    name: "Reconcile / retry publication",
  });
  await expect(retry).toBeDisabled();
  await page.getByRole("checkbox").check();
  await retry.click();
  await expect
    .poll(() => page.evaluate(() => window.__publicationActions))
    .toEqual([
      { command: "publish_review", args: { reviewOperationId: "review-1" } },
    ]);
});

test("stale-after-publication preserves its receipt and offers no second batch", async ({
  page,
}) => {
  const state = snapshot("stale_after_publication");
  state.publications[0].publication.error =
    "<img src=x onerror=alert(1)> Head changed.";
  await setup(page, state);
  const root = page.locator("#agent-reviews");
  await expect(root).toContainText("stale after publication");
  await expect(root).toContainText("Confirmed GitHub review 42: commented");
  await expect(root).toContainText("This is not GitHub approval.");
  await expect(root.locator("img")).toHaveCount(0);
  await expect(root.getByRole("button")).toHaveCount(0);
});

test("confirmed publication with failed final checks offers reconciliation, never withdrawal", async ({
  page,
}) => {
  const state = snapshot("published");
  state.publications[0].publication.operation.state = "manual_retry";
  state.publications[0].publication.error =
    "Post-publication verification timed out.";
  await setup(page, state);
  await expect(page.locator("#agent-reviews")).toContainText(
    "Confirmed GitHub review 42: commented",
  );
  await expect(
    page.getByRole("button", { name: "Withdraw publication confirmation" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Reconcile / retry publication" }),
  ).toBeDisabled();
});

test("legacy untrusted result requires renewed trust before another review", async ({
  page,
}) => {
  const state = snapshot();
  state.reviews[0].run.result.reviewed_base_sha = null;
  state.reviews[0].trust_required = true;
  state.publications[0].blocked =
    "This older result has no reviewed base. Run a new review before publication.";
  await setup(page, state);
  await expect(
    page.getByRole("button", { name: "Publish review", exact: true }),
  ).toHaveCount(0);
  const again = page.getByRole("button", { name: "Review again" });
  await expect(again).toBeDisabled();
  await page
    .getByRole("checkbox", { name: /I trust this exact revision for another/ })
    .check();
  await again.click();
  await expect
    .poll(() => page.evaluate(() => window.__publicationActions))
    .toEqual([
      {
        command: "start_review",
        args: { candidateKey: "exact-head", confirmTrust: true },
      },
    ]);
});
