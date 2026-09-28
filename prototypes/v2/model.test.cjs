const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { webcrypto } = require("node:crypto");
const context = vm.createContext({ crypto: webcrypto, Intl });
vm.runInContext(
  fs.readFileSync(path.join(__dirname, "model.js"), "utf8"),
  context,
);
const M = context.SniperDemo;
const count = (state, status) =>
  state.reviews.filter((review) => review.state === status).length;
const first = (state, status = "running") =>
  state.reviews.find((review) => review.state === status);
const plain = (value) => JSON.parse(JSON.stringify(value));

test("seed has six configurations, four active reviews, eighteen waiting and three human items", () => {
  const state = M.validate(M.seed());
  assert.equal(state.agents.length, 6);
  assert.equal(count(state, "running"), 4);
  assert.equal(count(state, "queued"), 18);
  assert.equal(count(state, "human") + count(state, "question"), 3);
});

test("global stop discards active attempts; resume restarts without duplicates", () => {
  let state = M.seed();
  const ids = state.reviews
    .map((review) => review.id)
    .sort()
    .join(",");
  const human = JSON.stringify(
    state.reviews.filter((review) =>
      ["human", "question"].includes(review.state),
    ),
  );
  for (let cycle = 0; cycle < 15; cycle++) {
    state = M.update(state, "monitor");
    assert.equal(count(state, "running"), 0);
    assert.equal(count(state, "queued"), 22);
    assert.equal(state.reviews.filter((review) => review.restart).length, 4);
    assert(
      state.reviews
        .filter((review) => review.restart)
        .every((review) => review.run === null && review.stage === 0),
    );
    assert.throws(() => M.update(state, "detect"), /Resume monitoring/);
    state = M.update(state, "monitor");
    assert.equal(count(state, "running"), 4);
    assert.equal(count(state, "queued"), 18);
    assert.equal(
      state.reviews
        .map((review) => review.id)
        .sort()
        .join(","),
      ids,
    );
    assert.equal(
      JSON.stringify(
        state.reviews.filter((review) =>
          ["human", "question"].includes(review.state),
        ),
      ),
      human,
    );
  }
});

test("capacity is global, positive, and independent of saved configurations", () => {
  let state = M.update(M.seed(), "capacity", { limit: 2 });
  assert.equal(count(state, "running"), 2);
  assert.equal(count(state, "queued"), 20);
  state = M.update(state, "capacity", { limit: 30 });
  assert.equal(count(state, "running"), 22);
  assert.equal(count(state, "queued"), 0);
  assert.equal(state.agents.length, 6);
  for (const limit of [0, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1])
    assert.throws(
      () => M.update(state, "capacity", { limit }),
      /positive whole/,
    );
});

test("cleared work respects approval versus human-handoff permissions and refills the slot", () => {
  for (const completion of ["approve", "human"]) {
    const state = M.seed();
    const review = first(state);
    state.agents.find((agent) => agent.id === review.agentId).completion =
      completion;
    review.run.completion = completion;
    const next = M.update(state, "finish", {
      reviewId: review.id,
      outcome: "clear",
    });
    assert.equal(
      next.reviews.find((item) => item.id === review.id).state,
      completion === "approve" ? "approved" : "human",
    );
    assert.equal(count(next, "running"), 4);
    assert.equal(count(next, "queued"), 17);
    assert.equal(review.state, "running");
  }
});

test("findings remain local unless both comment permission and publication gate allow it", () => {
  let state = M.seed();
  const review = first(state);
  state = M.update(state, "finish", {
    reviewId: review.id,
    outcome: "findings",
  });
  assert.equal(
    state.reviews.find((item) => item.id === review.id).state,
    "human",
  );
  const paused = M.update(state, "monitor");
  assert.throws(
    () => M.update(paused, "publish", { reviewId: review.id }),
    /paused/,
  );
  state = M.update(state, "publish", { reviewId: review.id });
  assert.equal(
    state.reviews.find((item) => item.id === review.id).state,
    "author",
  );
  let automatic = M.seed();
  automatic.preferences.automaticComments = true;
  const active = first(automatic);
  automatic = M.update(automatic, "finish", {
    reviewId: active.id,
    outcome: "findings",
  });
  assert.equal(
    automatic.reviews.find((item) => item.id === active.id).state,
    "author",
  );
});

test("disconnect blocks dependent work; reconnect restores eligibility", () => {
  let state = M.update(M.seed(), "account", { id: "copilot-demo" });
  assert.equal(count(state, "running"), 0);
  assert.equal(count(state, "queued"), 22);
  assert.match(
    M.blockReason(state, first(state, "queued")),
    /AI account disconnected/,
  );
  state = M.update(state, "account", { id: "copilot-demo" });
  assert.equal(count(state, "running"), 4);
});

test("manual start remains distinct from monitoring and automatic comment publication", () => {
  let state = M.seed();
  state = M.update(state, "preferences", {
    preferences: {
      ...state.preferences,
      automaticStart: false,
      automaticComments: true,
    },
  });
  assert.equal(count(state, "running"), 0);
  const review = first(state, "queued");
  state = M.update(state, "start", { reviewId: review.id });
  assert.equal(count(state, "running"), 1);
  assert.equal(
    state.reviews.find((item) => item.id === review.id).state,
    "running",
  );
});

test("untrusted detection requires explicit revision confirmation", () => {
  let state = M.seed();
  state.repos.forEach((repo) => {
    repo.watched = [];
  });
  state = M.update(state, "detect");
  const untrusted = state.reviews.filter((review) => review.number >= 500);
  assert(
    untrusted.length > 0 &&
      untrusted.every((review) => !review.trusted && review.state === "queued"),
  );
  assert.throws(
    () => M.update(state, "start", { reviewId: untrusted[0].id }),
    /Confirm trust/,
  );
  state = M.update(state, "trust", { reviewId: untrusted[0].id });
  assert.equal(
    state.reviews.find((review) => review.id === untrusted[0].id).trusted,
    true,
  );
});

test("agent and doctrine edits save shared references; referenced resources cannot disappear", () => {
  let state = M.seed();
  const agent = {
    ...plain(state.agents[0]),
    id: undefined,
    name: "Seventh reviewer",
    doctrineIds: [],
  };
  state = M.update(state, "save-agent", { agent });
  assert.equal(state.agents.length, 7);
  const newAgent = state.agents.at(-1);
  state = M.update(state, "save-agent", {
    agent: { ...plain(newAgent), name: "Renamed reviewer" },
  });
  assert.equal(state.agents.at(-1).name, "Renamed reviewer");
  state = M.update(state, "delete-agent", { id: newAgent.id });
  assert.equal(state.agents.length, 6);
  assert.throws(
    () => M.update(state, "delete-agent", { id: "agent-0" }),
    /Unassign/,
  );
  assert.throws(
    () => M.update(state, "delete-doctrine", { id: "correctness" }),
    /Remove this doctrine/,
  );
  assert.throws(
    () =>
      M.update(state, "save-agent", { agent: { ...agent, model: "made-up" } }),
    /demo model/,
  );
});

test("repository assignments and scope block work instead of silently remapping it", () => {
  let state = M.seed();
  const repo = { ...plain(state.repos[0]), agentIds: [], enabled: false };
  state = M.update(state, "save-repo", { repo });
  assert(
    state.reviews
      .filter(
        (review) => review.repoId === repo.id && review.state === "queued",
      )
      .every((review) => M.blockReason(state, review)),
  );
  assert.throws(
    () => M.update(state, "delete-repo", { id: repo.id }),
    /outstanding work/,
  );
  assert.throws(
    () => M.update(state, "save-repo", { repo: { ...repo, id: undefined } }),
    /already configured/,
  );
});

test("stale retry creates a new revision and preserves the previous evidence", () => {
  let state = M.seed();
  const original = first(state, "approved");
  state = M.update(state, "stale", { reviewId: original.id });
  state = M.update(state, "retry", { reviewId: original.id });
  const previous = state.reviews.find((review) => review.id === original.id);
  const next = state.reviews.find((review) => review.id === previous.nextId);
  assert.equal(previous.state, "stale");
  assert.equal(previous.head, original.head);
  assert.equal(previous.published, true);
  assert.notEqual(next.head, previous.head);
  assert.equal(next.published, false);
  assert.throws(
    () => M.update(state, "retry", { reviewId: original.id }),
    /already been queued/,
  );
});

test("failed output never publishes and human review is not performed in the app", () => {
  let state = M.seed();
  const active = first(state);
  state = M.update(state, "finish", {
    reviewId: active.id,
    outcome: "failure",
  });
  assert.equal(
    state.reviews.find((review) => review.id === active.id).published,
    false,
  );
  const question = first(state, "question");
  assert.throws(
    () =>
      M.update(state, "human", {
        reviewId: question.id,
        note: "A decision belongs on GitHub.",
      }),
    /Unknown prototype action/,
  );
  assert.equal(
    state.reviews.find((review) => review.id === question.id).state,
    "question",
  );
});

test("invalid saved state is rejected, not silently treated as success", () => {
  assert.throws(() => M.validate({ version: 1 }), /unsupported version/);
  const state = M.seed();
  state.reviews.push(state.reviews[0]);
  assert.throws(() => M.validate(state), /Duplicate saved reviews/);
});

test("watch-filter changes stop mismatched work before it starts or publishes", () => {
  let state = M.seed();
  state = M.update(state, "save-repo", {
    repo: { ...plain(state.repos[0]), watched: ["Robin Bell"] },
  });
  assert.equal(
    state.reviews.filter(
      (review) => review.repoId === "repo-0" && review.state === "running",
    ).length,
    0,
  );
  assert.match(
    M.blockReason(
      state,
      state.reviews.find((review) => review.repoId === "repo-0"),
    ),
    /watch filter/,
  );
  state.repos.forEach((repo) => {
    repo.watched = ["Robin Bell"];
  });
  assert.throws(() => M.update(state, "detect"), /No sample PRs match/);
});

test("a fresh revision's clear result does not reuse earlier findings or synopsis", () => {
  let state = M.seed();
  const earlier = first(state, "author");
  state = M.update(state, "stale", { reviewId: earlier.id });
  state = M.update(state, "retry", { reviewId: earlier.id });
  const nextId = state.reviews.find(
    (review) => review.id === earlier.id,
  ).nextId;
  state = M.update(state, "capacity", { limit: 30 });
  state = M.update(state, "finish", { reviewId: nextId, outcome: "clear" });
  const result = state.reviews.find((review) => review.id === nextId);
  assert.equal(result.findings.length, 0);
  assert.match(result.synopsis, /No blocking findings/);
  assert.equal(
    state.reviews.find((review) => review.id === earlier.id).findings.length,
    1,
  );
});

test("repository schedules validate only the selected mode without resetting inactive values", () => {
  const state = M.seed();
  for (const schedule of [
    { schedule: "cron", minutes: 0 },
    { schedule: "interval", cron: "", zone: "" },
  ]) {
    const repo = { ...plain(state.repos[0]), ...schedule };
    const next = M.update(state, "save-repo", { repo });
    assert.deepEqual(plain(next.repos[0]), repo);
    assert.doesNotThrow(() => M.validate(plain(next)));
  }
});

test("active schedule and saved configuration failures remain explicit", () => {
  const state = M.seed();
  for (const minutes of [
    0,
    -1,
    1.5,
    NaN,
    Infinity,
    Number.MAX_SAFE_INTEGER + 1,
  ])
    assert.throws(
      () =>
        M.update(state, "save-repo", {
          repo: { ...plain(state.repos[0]), minutes },
        }),
      /positive whole-minute/,
    );
  assert.throws(
    () =>
      M.update(state, "save-repo", {
        repo: { ...plain(state.repos[0]), schedule: "unknown" },
      }),
    /schedule/,
  );
  for (const field of ["cron", "zone"])
    assert.throws(
      () =>
        M.update(state, "save-repo", {
          repo: { ...plain(state.repos[0]), schedule: "cron", [field]: "" },
        }),
      /must not be empty/,
    );
  assert.throws(
    () =>
      M.update(state, "save-repo", {
        repo: { ...plain(state.repos[0]), schedule: "cron", cron: "* *" },
      }),
    /five-field/,
  );
  assert.throws(
    () =>
      M.update(state, "save-repo", {
        repo: {
          ...plain(state.repos[0]),
          schedule: "cron",
          zone: "Invalid/Place",
        },
      }),
    /time zone/,
  );
  const corrupt = M.seed();
  delete first(corrupt).run.doctrineIds;
  assert.throws(() => M.validate(corrupt), /saved review configuration/);
});

test("adding an account requires a supported provider and explicit identity confirmation", () => {
  const state = M.seed();
  for (const kind of ["azure-devops", "claude", "codex", "grok", "unknown"]) {
    assert.throws(
      () =>
        M.update(state, "add-account", {
          kind,
          name: "demo-user",
          confirmed: true,
        }),
      /coming soon/,
    );
  }
  for (const confirmed of [undefined, false, "true"]) {
    assert.throws(
      () =>
        M.update(state, "add-account", {
          kind: "github",
          name: "demo-user",
          confirmed,
        }),
      /Confirm the returned/,
    );
  }
  for (const name of [
    "",
    " ",
    "name@example.com",
    "abc--def",
    "<script>",
    "a".repeat(40),
  ]) {
    assert.throws(
      () =>
        M.update(state, "add-account", {
          kind: "github",
          name,
          confirmed: true,
        }),
      /fictional handle/,
    );
  }
  assert.equal(state.accounts.length, 3);
});

test("confirmed identities persist independently without changing assignments or reviews", () => {
  const original = M.seed();
  const github = M.update(original, "add-account", {
    kind: "github",
    name: " New-Demo ",
    confirmed: true,
  });
  const copilot = M.update(github, "add-account", {
    kind: "copilot",
    name: "NEW-DEMO",
    confirmed: true,
  });
  assert.equal(github.accounts.at(-1).name, "new-demo");
  assert.equal(copilot.accounts.length, 5);
  assert.notEqual(copilot.accounts.at(-1).id, github.accounts.at(-1).id);
  for (const field of ["repos", "agents", "reviews", "preferences"]) {
    assert.deepEqual(plain(copilot[field]), plain(original[field]));
  }
  assert.equal(copilot.monitoring, original.monitoring);
  assert.equal(
    M.validate(JSON.parse(JSON.stringify(copilot))).accounts.length,
    5,
  );
});

test("duplicate identity confirmation cannot replace or reconnect an existing account", () => {
  let state = M.update(M.seed(), "account", { id: "github-personal" });
  assert.throws(
    () =>
      M.update(state, "add-account", {
        kind: "github",
        name: " ALEX-DEMO ",
        confirmed: true,
      }),
    /already listed/,
  );
  assert.equal(
    state.accounts.find((account) => account.id === "github-personal")
      .connected,
    false,
  );
  assert.equal(state.accounts.length, 3);
  const invalid = M.seed();
  invalid.accounts.push({
    ...plain(invalid.accounts[0]),
    id: "duplicate",
    name: "ALEX-DEMO",
  });
  assert.throws(() => M.validate(invalid), /already connected/);
});

test("account addition does not dispatch otherwise eligible queued work", () => {
  const state = M.seed();
  state.reviews
    .filter((review) => review.state === "running")
    .forEach((review) => {
      review.state = "queued";
      review.run = null;
      review.stage = 0;
    });
  M.validate(state);
  const next = M.update(state, "add-account", {
    kind: "copilot",
    name: "fresh-ai",
    confirmed: true,
  });
  assert.equal(count(next, "running"), 0);
  assert.deepEqual(plain(next.reviews), plain(state.reviews));
});

test("new accounts remain restricted to repository and AI roles", () => {
  let state = M.update(M.seed(), "add-account", {
    kind: "github",
    name: "new-repository-user",
    confirmed: true,
  });
  const githubId = state.accounts.at(-1).id;
  state = M.update(state, "add-account", {
    kind: "copilot",
    name: "new-ai-user",
    confirmed: true,
  });
  const copilotId = state.accounts.at(-1).id;
  assert.throws(
    () =>
      M.update(state, "save-agent", {
        agent: { ...plain(state.agents[0]), accountId: githubId },
      }),
    /AI account/,
  );
  assert.throws(
    () =>
      M.update(state, "save-repo", {
        repo: { ...plain(state.repos[0]), accountId: copilotId },
      }),
    /repository account/,
  );
  state = M.update(state, "save-agent", {
    agent: { ...plain(state.agents[0]), accountId: copilotId },
  });
  assert.equal(state.agents[0].accountId, copilotId);
});

function firstInstallSetup() {
  let state = M.fresh();
  assert.equal(M.setupStatus(state).completed, 0);
  state = M.update(state, "add-account", {
    kind: "copilot",
    name: "first-ai",
    confirmed: true,
  });
  assert.equal(M.setupStatus(state).completed, 1);
  const ai = state.accounts.at(-1).id;
  state = M.update(state, "add-account", {
    kind: "github",
    name: "first-code",
    confirmed: true,
  });
  assert.equal(M.setupStatus(state).completed, 2);
  const github = state.accounts.at(-1).id;
  state = M.update(state, "save-agent", {
    agent: {
      ...plain(M.seed().agents[0]),
      id: undefined,
      name: "My reviewer",
      accountId: ai,
    },
  });
  assert.equal(M.setupStatus(state).completed, 3);
  const agentId = state.agents.at(-1).id;
  state = M.update(state, "save-repo", {
    repo: {
      ...plain(M.seed().repos[0]),
      id: undefined,
      name: "sample/first-repo",
      accountId: github,
      agentIds: [agentId],
      start: "on",
    },
  });
  assert.equal(M.setupStatus(state).completed, 4);
  return state;
}

test("fresh install has no configured accounts, agents, repositories, reviews, or running automation", () => {
  const state = M.validate(M.fresh());
  for (const key of ["accounts", "agents", "repos", "reviews"])
    assert.equal(state[key].length, 0);
  assert.equal(state.limit, 4);
  assert.equal(state.monitoring, false);
  assert.equal(state.preferences.automaticStart, false);
  assert.equal(state.preferences.automaticComments, false);
  assert.equal(state.doctrines.length, 3);
  assert.equal(M.needsOnboarding(state), true);
  assert.throws(() => M.update(state, "monitor"), /Genie setup review/);
  assert.throws(
    () => M.update(state, "finish-onboarding", { confirmed: true }),
    /Connect both account roles/,
  );
  assert.equal(M.seed().reviews.length, 28);
});

test("Genie progress comes from shared setup; completion requires a separate confirmation", () => {
  const state = firstInstallSetup();
  assert.equal(state.monitoring, false);
  assert.equal(state.reviews.length, 0);
  assert.equal(M.needsOnboarding(state), true);
  assert.throws(
    () => M.update(state, "finish-onboarding", { confirmed: false }),
    /Confirm the setup/,
  );
  const started = M.update(state, "finish-onboarding", { confirmed: true });
  assert.equal(started.monitoring, true);
  assert.equal(started.onboarded, true);
  assert.equal(started.scenario, "fresh");
  assert.equal(M.needsOnboarding(started), false);
  assert.equal(started.reviews.length, 0);
  assert.equal(started.agents.length, 1);
  assert.equal(started.accounts.length, 2);
  const checked = M.update(started, "detect");
  assert.equal(checked.reviews.length, 1);
  assert.equal(count(checked, "running"), 1);
});

test("Genie cannot finish with disconnected accounts, missing scope, or unassigned repositories", () => {
  const state = firstInstallSetup();
  const disconnected = M.update(state, "account", { id: state.accounts[0].id });
  assert.equal(M.setupStatus(disconnected).ready, false);
  assert.throws(
    () => M.update(disconnected, "finish-onboarding", { confirmed: true }),
    /Connect both/,
  );
  for (const change of [
    { scope: false },
    { enabled: false },
    { agentIds: [] },
  ]) {
    const incomplete = M.update(state, "save-repo", {
      repo: { ...plain(state.repos[0]), ...change },
    });
    assert.equal(M.setupStatus(incomplete).ready, false);
    assert.throws(
      () => M.update(incomplete, "finish-onboarding", { confirmed: true }),
      /activate an assigned repository/,
    );
  }
});

test("fresh progress persists, desktop resets produce clean fixtures, and prior v2 data stays valid", () => {
  const partial = M.update(M.fresh(), "add-account", {
    kind: "copilot",
    name: "kept-ai",
    confirmed: true,
  });
  const restored = M.validate(JSON.parse(JSON.stringify(partial)));
  assert.equal(M.setupStatus(restored).completed, 1);
  assert.equal(M.fresh().accounts.length, 0);
  assert.equal(M.seed().accounts.length, 3);
  const legacy = M.seed();
  delete legacy.scenario;
  delete legacy.onboarded;
  M.validate(legacy);
  assert.equal(M.needsOnboarding(legacy), false);
  const invalid = M.fresh();
  invalid.monitoring = true;
  assert.throws(() => M.validate(invalid), /Finish onboarding/);
});
