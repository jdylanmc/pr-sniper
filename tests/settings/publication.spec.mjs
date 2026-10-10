import { expect, test } from "./fixtures.mjs";
import { queueFixture } from "./queue-fixture.mjs";
import {
  section,
  saveChanges,
  repositorySettings,
  closeDialog,
} from "./navigation.mjs";

test("publication failure stays on its PR with cause and action, not Settings", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const publication = fixture.state.publications.find(
    (p) => p.review.job.number === 3,
  );
  publication.phase = "pending";
  publication.error =
    "Publication verify_pending: remote comment body does not match the frozen batch. Open this PR on GitHub and inspect the review before retrying; no automatic submission or replacement is allowed.";
  await store("seed_queue_state", fixture.state);
  await page.goto("/");
  const navigation = page.getByRole("navigation", {
    name: "Application destinations",
  });
  await navigation
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await expect(page.locator("[data-panel-error]")).toBeHidden();
  await expect(
    page.getByText(publication.error, { exact: true }),
  ).not.toBeVisible();
  await navigation.getByRole("button", { name: "Queue", exact: true }).click();
  await page
    .locator("#handoff-queue")
    .getByRole("article", {
      name: "example/repo #3",
      exact: true,
    })
    .getByRole("button", { name: "Evidence and actions" })
    .click();
  const evidence = page.locator("#agent-reviews");
  await expect(evidence).toContainText("example/repo #3");
  await expect(evidence).toContainText(publication.error);
  await expect(
    evidence.getByRole("button", { name: "Reconcile / retry publication" }),
  ).toBeVisible();
  await navigation
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await expect(page.locator("[data-panel-error]")).toBeHidden();
});

test("retired publication defaults and repository overrides are not permission controls", async ({
  page,
  store,
}) => {
  await store("save_repository", { repository: "fixture/publication-policy" });
  const setup = (await store("snapshot")).settings;
  setup.repositories[0].enabled = false;
  await store("seed_settings", setup);
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  await expect(page.locator("#automatic-publication")).toHaveCount(0);
  await expect(
    page.getByText(
      "Publication permissions are set on each repository's Agent assignments.",
    ),
  ).toBeVisible();
  await section(page, "Integrations");
  let modal = await repositorySettings(page, "fixture/publication-policy");
  await expect(
    modal.getByText("Automation overrides", { exact: true }),
  ).toHaveCount(0);
  await expect(
    modal.getByLabel("Comment publication", { exact: true }),
  ).toHaveCount(0);
  await closeDialog(page);
  await page.reload();
  modal = await repositorySettings(page, "fixture/publication-policy");
  await expect(
    modal.getByLabel("Comment publication", { exact: true }),
  ).toHaveCount(0);
  await closeDialog(page);
  expect((await store("snapshot")).settings).toEqual(setup);
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
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

test("legacy result retries without renewed trust while publication stays blocked", async ({
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
  await expect(again).toBeEnabled();
  await expect(page.getByRole("checkbox", { name: /trust/i })).toHaveCount(0);
  await again.click();
  await expect
    .poll(() => page.evaluate(() => window.__publicationActions))
    .toEqual([
      {
        command: "start_review",
        args: { candidateKey: "exact-head" },
      },
    ]);
});
