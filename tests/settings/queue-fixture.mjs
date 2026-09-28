const repositoryId = "00000000-0000-4000-8000-000000000001";
const assignmentId = "00000000-0000-4000-8000-000000000002";
const agentId = "00000000-0000-4000-8000-000000000003";

export async function queueFixture(store) {
  const settings = (await store("snapshot")).settings;
  const agent = {
    id: agentId,
    name: "Correctness reviewer",
    model: "configured-model",
    ai_account: { provider: "copilot", account_id: "33" },
    prompt: "Review correctness.",
    signature: "stored",
  };
  settings.agents = [agent];
  settings.repositories = [
    {
      id: repositoryId,
      provider: "github",
      name: "example/repo",
      enabled: true,
      provider_account_id: "22",
      provider_repository_id: "100",
      watched_authors: [{ id: "11", login: "pr-author" }],
      assignments: [
        {
          id: assignmentId,
          agent_id: agentId,
          schedule: { kind: "interval", minutes: 5, timezone: "UTC" },
          comment: true,
        },
      ],
    },
  ];
  await store("seed_settings", settings);
  function review(number, decision = "machine_sign_off") {
    const job = {
      assignment_id: assignmentId,
      provider: "github",
      account_id: "22",
      account_login: "local-operator",
      configuration_id: repositoryId,
      repository_id: "100",
      repository_name: "example/repo",
      pull_request_id: String(number),
      number,
      title: number === 9 ? "Fix reconnect race" : "Review change",
      head_sha: "a".repeat(40),
      observed_base_sha: "b".repeat(40),
      trigger_policy: '[["11"],true]',
      author_id: "11",
      author_login: "pr-author",
      watched_author: true,
      requested_reviewer: false,
      waiting: "human_start",
      detected_at: 100,
    };
    return {
      key: JSON.stringify([
        job.provider,
        job.account_id,
        job.configuration_id,
        job.repository_id,
        job.pull_request_id,
        job.head_sha,
        job.trigger_policy,
        assignmentId,
      ]),
      assignment_id: assignmentId,
      job,
      selection: {
        agent,
        policy: settings.defaults,
        doctrine: null,
        preset: null,
      },
      operation: {
        id: `review-${number}`,
        provider: job.provider,
        account_id: job.account_id,
        configuration_id: job.configuration_id,
        repository_id: job.repository_id,
        pull_request_id: job.pull_request_id,
        head_sha: job.head_sha,
        trigger_policy: job.trigger_policy,
        operation_type: "review",
        state: "completed",
        attempt_count: 1,
        initial_attempt_at: 1_800_000_000,
        retry_deadline: 1_800_000_900,
        next_attempt_at: null,
        failure: null,
        attempted_mutation: null,
        pending_review_id: null,
        owned_thread_id: null,
        triggering_external_comment_id: null,
        confirmed_receipt: null,
      },
      manual_start: true,
      trust_confirmed: true,
      phase: "Automated review complete",
      error: null,
      result: {
        reviewed_base_sha: "b".repeat(40),
        output: {
          synopsis: "The change is ready for review.",
          files: [
            { path: "z-first.rs", order: 1, explanation: "Read this first." },
            { path: "a-second.rs", order: 2, explanation: "Then read this." },
          ],
          findings: [],
          decision,
        },
        session_id: `session-${number}`,
        model: agent.model,
        runtime_version: "fixture",
        input_tokens: 100,
        output_tokens: 20,
        tool_calls: 3,
      },
    };
  }
  function published(review) {
    return {
      id: `publication-${review.job.number}`,
      review,
      operation: {
        ...review.operation,
        id: `publication-op-${review.job.number}`,
        operation_type: "github_comment",
      },
      history: [],
      automatic: false,
      confirmed: true,
      cancelled: false,
      phase: "published",
      error: null,
      batch: {
        commit_id: review.job.head_sha,
        body: "Review summary",
        comments: [],
        unmappable: [],
      },
      mutation: "submit",
      uncertain: false,
      receipts: [
        {
          review_id: String(review.job.number + 100),
          state: "commented",
          comment_ids: [],
        },
      ],
    };
  }
  const ready = review(9);
  const author = review(1, "human_input_required");
  const operator = review(2, "human_input_required");
  const failed = review(3);
  const publications = [
    published(author),
    published(ready),
    published(operator),
    published(failed),
  ];
  publications[2].batch.unmappable = [0];
  publications[3].phase = "unresolved";
  publications[3].uncertain = true;
  publications[3].operation.state = "manual_retry";
  publications[3].error = "Provider response was lost.";
  publications[3].receipts = [];
  const state = {
    jobs: [author.job, ready.job, operator.job, failed.job],
    reviews: [author, ready, operator, failed],
    publications,
    follow_ups: [],
  };
  await store("seed_queue_state", state);
  return { state, settings, review, published };
}
