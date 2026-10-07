import { expect, test } from "./fixtures.mjs";

function candidate(decision = null) {
  return {
    blocked: null,
    automatic_start: false,
    automatic_publication: false,
    human_gate: false,
    run: {
      id: "follow-up-1",
      phase:
        decision === "reply"
          ? "waiting_publication"
          : (decision ?? "waiting_start"),
      trigger_id: "101",
      cancelled: false,
      uncertain: false,
      receipt: null,
      error: null,
      analysis: decision
        ? {
            state: "completed",
            attempt_count: 1,
            retry_deadline: 1_800_000_900,
          }
        : null,
      publication: null,
      review: {
        job: {
          repository_name: "example/repo",
          number: 1,
          account_login: "repo-owner",
          account_id: "22",
          head_sha: "a".repeat(40),
        },
        selection: { agent: { name: "Reviewer", model: "configured-model" } },
      },
      thread: {
        id: "owned-thread",
        comments: [
          {
            id: "100",
            body: "Original machine finding",
            author_login: "repo-owner",
          },
          {
            id: "101",
            body: "What does the source return?",
            author_login: "author",
          },
        ],
      },
      result: decision
        ? {
            output: {
              decision,
              body: decision === "reply" ? "The function returns 42." : "",
              reason: "A human must decide or no new facts exist.",
              evidence:
                decision === "reply"
                  ? [
                      {
                        path: "source.rs",
                        side: "head",
                        line: 1,
                        quote: "return 42;",
                      },
                    ]
                  : [],
            },
            session_id: "session",
            model: "configured-model",
          }
        : null,
    },
  };
}
async function setup(page, candidates) {
  await page.addInitScript((followUps) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__followUpActions = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "monitoring_snapshot")
        return Promise.resolve({
          health: [],
          jobs: [],
          reviews: [],
          follow_ups: followUps,
        });
      if (["start_follow_up", "cancel_follow_up"].includes(command)) {
        window.__followUpActions.push({ command, args });
        return Promise.resolve();
      }
      return original(command, args);
    };
  }, candidates);
  await page.goto("/?view=queue");
}

test("follow-up analysis keeps job identity without legacy trust approval", async ({
  page,
}) => {
  await setup(page, [{ ...candidate(), trust_required: true }]);
  const root = page.locator("#thread-follow-ups");
  await expect(root.getByRole("checkbox", { name: /trust/i })).toHaveCount(0);
  await expect(root).toContainText("external comment 101");
  await expect(root).toContainText("GitHub: repo-owner (22)");
  expect(await page.evaluate(() => window.__followUpActions)).toEqual([]);
  await expect(
    root.getByRole("button", { name: "Start follow-up", exact: true }),
  ).toHaveCount(0);
  await expect(root).toContainText("Execution is automatic when eligible");
});

test("a validated response with Reply Comment off stays local without a manual permission bypass", async ({
  page,
}) => {
  await setup(page, [candidate("reply")]);
  const root = page.locator("#thread-follow-ups");
  await expect(root).toContainText("Local response: The function returns 42.");
  await expect(root).toContainText("source.rs, head line 1: return 42;");
  const publish = root.getByRole("button", {
    name: "Publish reply",
    exact: true,
  });
  await expect(publish).toHaveCount(0);
  await expect(
    root.getByRole("checkbox", { name: /Publish or reconcile/ }),
  ).toHaveCount(0);
  await expect(root).toContainText(
    "Reply Comment: off; analysis continues and qualifying responses remain local",
  );
  expect(await page.evaluate(() => window.__followUpActions)).toEqual([]);
});

test("quiet and human-judgment outcomes never offer publication", async ({
  page,
}) => {
  const human = candidate("human_input_required");
  human.human_gate = true;
  const quiet = candidate("quiet");
  quiet.run.id = "quiet-1";
  await setup(page, [human, quiet]);
  const root = page.locator("#thread-follow-ups");
  await expect(root).toContainText("Human input required. No automated reply.");
  await expect(root).toContainText(
    "Retry this follow-up only after the human decision",
  );
  await expect(root).toContainText("No reply needed.");
  await expect(root.getByRole("button")).toHaveCount(0);
});

test("answered human input remains historical evidence without another decision request", async ({
  page,
}) => {
  const human = candidate("human_input_required");
  human.human_gate = false;
  human.superseded = true;
  await setup(page, [human]);
  const root = page.locator("#thread-follow-ups");
  await expect(root).toContainText(
    "Historical human-input outcome; this gate was answered by a later explicit assessment.",
  );
  await expect(root).not.toContainText(
    "Retry this follow-up only after the human decision",
  );
  await expect(root.getByRole("button")).toHaveCount(0);
});

test("uncertain replies require reconciliation while confirmed replies cannot be withdrawn", async ({
  page,
}) => {
  const unresolved = candidate("reply");
  unresolved.run.phase = "unresolved";
  unresolved.run.uncertain = true;
  unresolved.run.publication = {
    state: "manual_retry",
    attempt_count: 4,
    retry_deadline: 1_800_000_900,
  };
  const published = candidate("reply");
  published.run.id = "published-1";
  published.run.phase = "published";
  published.run.receipt = "200";
  published.run.publication = {
    state: "completed",
    attempt_count: 1,
    retry_deadline: 1_800_000_900,
  };
  await setup(page, [unresolved, published]);
  const root = page.locator("#thread-follow-ups");
  await expect(root).toContainText("never blindly posts a replacement");
  await expect(root).toContainText("Confirmed GitHub reply 200");
  await expect(
    root.getByRole("button", { name: "Cancel follow-up" }),
  ).toHaveCount(0);
  const retry = root.getByRole("button", { name: "Reconcile / retry reply" });
  await expect(retry).toBeEnabled();
  await expect(root.getByRole("checkbox")).toHaveCount(0);
  await retry.click();
  await expect
    .poll(() => page.evaluate(() => window.__followUpActions))
    .toEqual([
      {
        command: "start_follow_up",
        args: { id: "follow-up-1", publish: true },
      },
    ]);
});

test("running follow-up is cancellable and conversation text is never HTML", async ({
  page,
}) => {
  const active = candidate();
  active.run.phase = "analyzing";
  active.run.analysis = {
    state: "running",
    attempt_count: 1,
    retry_deadline: 1_800_000_900,
  };
  active.run.thread.comments = Array.from({ length: 105 }, (_, i) => ({
    id: String(i),
    author_login: "author",
    body: i === 104 ? "<img src=x onerror=alert(1)>" : `Comment ${i}`,
  }));
  await setup(page, [active]);
  const root = page.locator("#thread-follow-ups");
  await root.getByText("Conversation (105 comments)", { exact: true }).click();
  await expect(root.locator("li")).toHaveCount(105);
  await expect(root.locator("img")).toHaveCount(0);
  await root.getByRole("button", { name: "Cancel follow-up" }).click();
  await expect
    .poll(() => page.evaluate(() => window.__followUpActions))
    .toEqual([{ command: "cancel_follow_up", args: { id: "follow-up-1" } }]);
});

test("prior executed analysis keeps its recorded model and identity after a retry", async ({
  page,
}) => {
  const retried = candidate("quiet");
  const prior = structuredClone(retried.run.review);
  prior.selection.agent.name = "Original primary";
  prior.selection.agent.model = "actual-prior-model";
  retried.run.review.selection.agent.name = "Current primary";
  retried.run.review.selection.agent.model = "current-model";
  retried.run.analysis_history = [
    {
      context: prior,
      operation: {
        state: "failed",
        attempt_count: 1,
        retry_deadline: 1_800_000_900,
      },
    },
  ];
  await setup(page, [retried]);
  const history = page
    .locator("#thread-follow-ups")
    .getByText("Prior executed analysis attempts", { exact: true });
  await history.click();
  const details = history.locator("..");
  await expect(details).toContainText("Original primary");
  await expect(details).toContainText("actual-prior-model");
  await expect(details).not.toContainText("Current primary");
});
