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

test("repository browsing is scoped to a personal or organization owner", () => {
  const account = M.seed().accounts[0];
  assert.deepEqual(plain(M.repositoryOwners(account)), [
    { name: "alex-demo", kind: "Personal" },
    { name: "orbit", kind: "Organization" },
  ]);
  assert(
    M.repositoryCatalog(account, "alex-demo").every((name) =>
      name.startsWith("alex-demo/"),
    ),
  );
  assert(
    M.repositoryCatalog(account, "orbit").every((name) =>
      name.startsWith("orbit/"),
    ),
  );
  assert.throws(
    () => M.repositoryCatalog(account, "unrelated"),
    /Choose an owner/,
  );
  assert.throws(
    () => M.repositoryCatalog({ ...account, connected: false }, "orbit"),
    /Connect/,
  );
});

test("URL intake binds the chosen account and starts with no monitoring authority", () => {
  const before = M.seed();
  const next = M.update(before, "add-repo", {
    accountId: "github-personal",
    name: "https://github.com/alex-demo/new-repo.git/",
  });
  const repo = next.repos.at(-1);
  assert.equal(repo.name, "alex-demo/new-repo");
  assert.equal(repo.accountId, "github-personal");
  assert.equal(repo.enabled, false);
  assert.equal(repo.scope, false);
  assert.equal(repo.agentIds.length, 0);
  assert.deepEqual(plain(next.reviews), plain(before.reviews));
  assert.throws(
    () =>
      M.update(next, "add-repo", {
        accountId: repo.accountId,
        name: "Alex-Demo/NEW-REPO",
      }),
    /already added/,
  );
  assert.throws(
    () =>
      M.update(before, "add-repo", {
        accountId: "copilot-demo",
        name: "alex-demo/new-repo",
      }),
    /connected GitHub/,
  );
  for (const name of [
    "https://example.com/a/b",
    "/Users/demo/git/repo",
    "https://github.com/a/b/pull/1",
    "https://user:secret@github.com/a/b",
    "https://dev.azure.com/org/project/_git/repo",
    "a/..",
  ])
    assert.throws(() => M.repositoryName(name));
  assert.equal(M.repositoryName("owner/repo"), "owner/repo");
});

test("repository intake does not dispatch existing queued work into free capacity", () => {
  const state = M.seed();
  state.reviews
    .filter((review) => review.state === "running")
    .forEach((review) => {
      review.state = "queued";
      review.run = null;
      review.stage = 0;
    });
  M.validate(state);
  const next = M.update(state, "add-repo", {
    accountId: "github-personal",
    name: "alex-demo/new-repo",
  });
  assert.equal(count(next, "running"), 0);
  assert.deepEqual(plain(next.reviews), plain(state.reviews));
  assert.equal(next.repos.at(-1).enabled, false);
});

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

test("disconnect forgets the account and blocks dependents; adding it again restores eligibility", () => {
  let state = M.update(M.seed(), "account", { id: "copilot-demo" });
  assert.equal(
    state.accounts.some((account) => account.id === "copilot-demo"),
    false,
  );
  M.validate(JSON.parse(JSON.stringify(state)));
  assert.equal(count(state, "running"), 0);
  assert.equal(count(state, "queued"), 22);
  assert.match(
    M.blockReason(state, first(state, "queued")),
    /AI account disconnected/,
  );
  state = M.update(state, "add-account", {
    kind: "copilot",
    name: "alex-ai-demo",
    confirmed: true,
  });
  assert.equal(
    state.accounts.find((account) => account.name === "alex-ai-demo").id,
    "copilot-demo",
  );
  assert.equal(state.forgotten.length, 0);
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
    repo.admission = plain(M.defaultAdmission());
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
    /outstanding work/,
  );
  const idle = M.update(M.fresh(), "add-account", {
    kind: "copilot",
    name: "idle-ai",
    confirmed: true,
  });
  const assignedAgent = M.update(idle, "save-agent", {
    agent: {
      ...plain(M.seed().agents[0]),
      id: "idle-agent",
      accountId: idle.accounts[0].id,
    },
  });
  const github = M.update(assignedAgent, "add-account", {
    kind: "github",
    name: "idle-gh",
    confirmed: true,
  });
  const withRepo = M.update(github, "add-repo", {
    name: "owner/idle",
    accountId: github.accounts[1].id,
  });
  const assigned = M.update(withRepo, "save-repo", {
    repo: { ...plain(withRepo.repos[0]), agentIds: ["idle-agent"] },
  });
  const removed = M.update(assigned, "delete-agent", { id: "idle-agent" });
  assert.deepEqual(plain(removed.repos[0].agentIds), []);
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

const only = (...users) => {
  const admission = plain(M.defaultAdmission(users));
  admission.mode = "filtered";
  admission.rules.forEach((rule) => {
    if (rule.kind === "assigned" || rule.kind === "mentioned")
      rule.enabled = false;
  });
  return admission;
};

test("filter changes stop mismatched work before it starts or publishes", () => {
  let state = M.seed();
  state = M.update(state, "save-repo", {
    repo: { ...plain(state.repos[0]), admission: only("Robin Bell") },
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
    /pull request filters/,
  );
  state.repos.forEach((repo) => {
    repo.admission = only("Robin Bell");
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

test("a connected identity cannot be duplicated, but a disconnected one can be added again at once", () => {
  assert.throws(
    () =>
      M.update(M.seed(), "add-account", {
        kind: "github",
        name: " ALEX-DEMO ",
        confirmed: true,
      }),
    /already connected/,
  );
  let state = M.update(M.seed(), "account", { id: "github-personal" });
  assert.equal(state.accounts.length, 2);
  state = M.update(state, "add-account", {
    kind: "github",
    name: " ALEX-DEMO ",
    confirmed: true,
  });
  assert.equal(state.accounts.length, 3);
  assert.equal(
    state.accounts.some((account) => account.id === "github-personal"),
    true,
  );
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

test("agents carry context window, reasoning effort and a default signature that follows name and model", () => {
  assert.equal(
    M.defaultSignature("Solid Snake", "GPT-5.6 Sol Fast"),
    "\u{1F52B} pr-sniper(solid-snake:gpt-5-6-sol-fast)",
  );
  assert.equal(
    M.defaultSignature("  ", ""),
    "\u{1F52B} pr-sniper(agent:model)",
  );
  const agent = M.seed().agents[0];
  assert.equal(agent.signature, M.defaultSignature(agent.name, agent.model));
  assert.deepEqual(plain(M.promptTokens.map(([token]) => token)), [
    "pr_title",
    "pr_number",
    "pr_author",
    "head_sha",
    "base_sha",
    "changed_files",
    "changed_file_count",
    "agent_name",
  ]);
  const state = M.seed();
  state.agents[0].reasoning = "Extreme";
  assert.throws(() => M.validate(state), /reasoning effort/);
  const legacy = M.seed();
  for (const key of ["contextWindow", "reasoning", "signature"])
    delete legacy.agents[0][key];
  M.validate(legacy);
});

test("repositories may be unbound and keep their pull request filters", () => {
  let state = M.seed();
  const admission = plain(state.repos[0].admission);
  admission.rules[0].enabled = false;
  state = M.update(state, "save-repo", {
    repo: { ...plain(state.repos[0]), accountId: "", admission },
  });
  assert.equal(state.repos[0].admission.rules[0].enabled, false);
  assert.equal(state.repos[0].accountId, "");
  assert.match(M.blockReason(state, state.reviews[0]) || "x", /./);
  const invalid = M.seed();
  invalid.repos[0].admission.mode = "maybe";
  assert.throws(() => M.validate(invalid), /pull request filters/);
});

const rule = (admission, kind) => admission.rules.find((r) => r.kind === kind);
const order = (admission, kinds) => {
  admission.rules = kinds.map((kind) => rule(admission, kind));
  return admission;
};
const pr = (over = {}) => ({
  author: "Casey Lee",
  reviewRequested: false,
  mentioned: false,
  files: [{ path: "src/a.ts" }, { path: "docs/readme.md" }],
  ...over,
});
const filtered = (setup) => {
  const admission = plain(M.defaultAdmission());
  admission.mode = "filtered";
  admission.rules.forEach((r) => {
    r.enabled = false;
  });
  setup(admission);
  return { admission };
};

test("mode all admits every pull request regardless of rules", () => {
  const repo = { admission: plain(M.defaultAdmission()) };
  rule(repo.admission, "exclude_users").users = ["Casey Lee"];
  rule(repo.admission, "exclude_users").enabled = true;
  assert.equal(repo.admission.mode, "all");
  assert.equal(M.admits(repo, pr()), true);
});

test("filtered mode rejects when no enabled rule matches", () => {
  assert.equal(
    M.admits(
      filtered(() => {}),
      pr(),
    ),
    false,
  );
  const repo = filtered((a) => {
    rule(a, "assigned").enabled = true;
    rule(a, "mentioned").enabled = true;
  });
  assert.equal(M.admits(repo, pr()), false);
  assert.equal(M.admits(repo, pr({ reviewRequested: true })), true);
  assert.equal(M.admits(repo, pr({ mentioned: true })), true);
});

test("disabled rules never decide", () => {
  const repo = filtered((a) => {
    rule(a, "include_users").users = ["Casey Lee"];
    rule(a, "exclude_users").users = ["Casey Lee"];
    rule(a, "exclude_users").enabled = true;
  });
  assert.equal(M.admits(repo, pr()), false);
  rule(repo.admission, "include_users").enabled = true;
  assert.equal(M.admits(repo, pr()), true);
});

test("the first matching rule wins in every ordering of include and exclude users", () => {
  const base = (kinds) =>
    filtered((a) => {
      rule(a, "include_users").users = ["Casey Lee"];
      rule(a, "exclude_users").users = ["Casey Lee"];
      rule(a, "include_users").enabled = rule(a, "exclude_users").enabled =
        true;
      order(a, [...kinds, ...M.ruleKinds.filter((k) => !kinds.includes(k))]);
    });
  assert.equal(M.admits(base(["include_users", "exclude_users"]), pr()), true);
  assert.equal(M.admits(base(["exclude_users", "include_users"]), pr()), false);
});

test("precedence permutations across assigned, include users and exclude paths", () => {
  const kinds = ["assigned", "include_users", "exclude_paths"];
  const permutations = (items) =>
    items.length < 2
      ? [items]
      : items.flatMap((item, i) =>
          permutations([...items.slice(0, i), ...items.slice(i + 1)]).map(
            (rest) => [item, ...rest],
          ),
        );
  assert.equal(permutations(kinds).length, 6);
  for (const permutation of permutations(kinds)) {
    const repo = filtered((a) => {
      rule(a, "assigned").enabled = true;
      rule(a, "include_users").enabled = true;
      rule(a, "include_users").users = ["Casey Lee"];
      rule(a, "exclude_paths").enabled = true;
      rule(a, "exclude_paths").paths = ["src/", "docs/"];
      order(a, [
        ...permutation,
        ...M.ruleKinds.filter((k) => !kinds.includes(k)),
      ]);
    });
    // Every changed file is excluded, so the outcome depends on what comes first.
    const expected =
      permutation.indexOf("exclude_paths") >
      Math.min(
        permutation.indexOf("assigned"),
        permutation.indexOf("include_users"),
      );
    assert.equal(
      M.admits(repo, pr({ reviewRequested: true })),
      expected,
      permutation.join(">"),
    );
  }
});

test("exclude paths rejects only when every changed file matches", () => {
  const repo = filtered((a) => {
    rule(a, "exclude_paths").enabled = true;
    rule(a, "exclude_paths").paths = ["docs/", "**/*.md"];
    rule(a, "include_users").enabled = true;
    rule(a, "include_users").users = ["Casey Lee"];
  });
  rule(repo.admission, "include_users").enabled = true;
  order(repo.admission, [
    "exclude_paths",
    ...M.ruleKinds.filter((k) => k !== "exclude_paths"),
  ]);
  assert.equal(
    M.admits(repo, pr({ files: [{ path: "docs/a.md" }, { path: "x.md" }] })),
    false,
  );
  assert.equal(M.admits(repo, pr()), true);
  assert.equal(M.admits(repo, pr({ files: [] })), true);
});

test("include paths admits when any changed file matches", () => {
  const repo = filtered((a) => {
    rule(a, "include_paths").enabled = true;
    rule(a, "include_paths").paths = ["tests/", "src/**/*.ts"];
  });
  assert.equal(M.admits(repo, pr({ files: [{ path: "src/x/y.ts" }] })), true);
  assert.equal(M.admits(repo, pr({ files: [{ path: "tests/a.js" }] })), true);
  assert.equal(M.admits(repo, pr({ files: [{ path: "lib/a.ts" }] })), false);
  rule(repo.admission, "include_paths").paths = [];
  assert.equal(M.admits(repo, pr()), false);
});

test("path globs: directory prefix, star, double star and question mark", () => {
  const cases = [
    ["docs/", "docs/a/b.md", true],
    ["docs/", "mydocs/a.md", false],
    ["docs", "docs/a.md", false],
    ["docs", "docs", true],
    ["src/*.ts", "src/a.ts", true],
    ["src/*.ts", "src/a/b.ts", false],
    ["src/**/*.ts", "src/a.ts", true],
    ["src/**/*.ts", "src/a/b/c.ts", true],
    ["**/*.md", "a/b/c.md", true],
    ["a?c.txt", "abc.txt", true],
    ["a?c.txt", "a/c.txt", false],
    ["a.b", "axb", false],
  ];
  for (const [pattern, file, expected] of cases)
    assert.equal(M.globMatches(pattern, file), expected, `${pattern} ${file}`);
});

test("path patterns are validated", () => {
  assert.match(M.pathProblem(""), /Enter a path/);
  assert.match(M.pathProblem("   "), /Enter a path/);
  assert.match(M.pathProblem("a".repeat(257)), /repository-relative/);
  assert.equal(M.pathProblem("a".repeat(256)), "");
  assert.match(M.pathProblem("/src"), /repository-relative/);
  assert.match(M.pathProblem("src/../x"), /repository-relative/);
  assert.match(M.pathProblem("..", []), /repository-relative/);
  assert.match(M.pathProblem("docs/", ["docs/"]), /Already added/);
  assert.equal(M.pathProblem("docs/"), "");
});

test("old repository data migrates deterministically to admission", () => {
  const legacy = (extra) => {
    const state = M.seed();
    state.repos = state.repos.map((repo) => {
      const { admission, ...rest } = plain(repo);
      return { ...rest, ...extra };
    });
    return state;
  };
  const watched = M.migrate(
    legacy({ watched: ["Robin Bell", "Casey Lee"], reviewerRequests: "on" }),
  );
  const a = watched.repos[0].admission;
  assert.equal(a.mode, "filtered");
  assert.deepEqual(plain(rule(a, "include_users")), {
    kind: "include_users",
    enabled: true,
    users: ["Robin Bell", "Casey Lee"],
  });
  assert.equal(rule(a, "assigned").enabled, true);
  assert.equal(rule(a, "mentioned").enabled, false);
  assert.equal("watched" in watched.repos[0], false);
  assert.equal("reviewerRequests" in watched.repos[0], false);
  M.validate(
    M.migrate(legacy({ watched: [...M.people], reviewerRequests: "on" })),
  );

  const none = M.migrate(legacy({ watched: [], reviewerRequests: "off" }));
  assert.equal(none.repos[0].admission.mode, "all");
  assert.equal(rule(none.repos[0].admission, "assigned").enabled, false);
  assert.equal(rule(none.repos[0].admission, "include_users").enabled, false);
  assert.equal(
    rule(M.migrate(legacy({ watched: [] })).repos[0].admission, "assigned")
      .enabled,
    true,
  );
  const again = M.migrate(plain(watched));
  assert.deepEqual(plain(again), plain(watched));
  assert.throws(() => M.validate(legacy({ watched: [] })), /outdated|filters/);
});

test("admission validation is strict", () => {
  const mutate = (change) => {
    const state = M.seed();
    change(state.repos[0].admission);
    return state;
  };
  for (const change of [
    (a) => (a.mode = "some"),
    (a) => a.rules.pop(),
    (a) => (a.rules[1].kind = "assigned"),
    (a) => (a.rules[0].enabled = "yes"),
    (a) => (rule(a, "include_users").users = [""]),
    (a) => (rule(a, "include_users").users = ["A", "A"]),
    (a) => delete rule(a, "exclude_users").users,
    (a) => (rule(a, "include_paths").paths = ["/abs"]),
    (a) => (rule(a, "include_paths").paths = ["x/../y"]),
    (a) => (rule(a, "include_paths").paths = ["a", "a"]),
    (a) => (rule(a, "exclude_paths").paths = ["a".repeat(257)]),
    (a) => (rule(a, "assigned").users = []),
  ])
    assert.throws(() => M.validate(mutate(change)), /pull request filters/);
  // Rules may be reordered freely.
  M.validate(mutate((a) => a.rules.reverse()));
});

test("detection and trust honor admission rules", () => {
  let state = M.seed();
  state.repos.forEach((repo) => {
    repo.admission = filtered((a) => {
      rule(a, "exclude_paths").enabled = true;
      rule(a, "exclude_paths").paths = ["src/", "tests/"];
      rule(a, "include_users").enabled = true;
      rule(a, "include_users").users = [];
    }).admission;
  });
  assert.throws(() => M.update(state, "detect"), /No sample PRs match/);
  state.repos.forEach((repo) => {
    rule(repo.admission, "include_paths").enabled = true;
    rule(repo.admission, "include_paths").paths = ["src/"];
    order(repo.admission, [
      "include_paths",
      ...M.ruleKinds.filter((k) => k !== "include_paths"),
    ]);
  });
  state = M.update(state, "detect");
  assert(state.reviews.some((review) => review.number >= 500));
});

test("notification types default on, migrate older saves and validate", () => {
  const state = plain(M.seed());
  assert.deepEqual(
    Object.keys(state.preferences.notificationTypes),
    plain(M.notificationTypeKeys),
  );
  assert.ok(Object.values(state.preferences.notificationTypes).every(Boolean));
  delete state.preferences.notificationTypes;
  assert.throws(() => M.validate(plain(state)), /Invalid saved preferences/);
  const migrated = M.validate(M.migrate(plain(state)));
  assert.equal(migrated.preferences.notificationTypes.failure, true);
  const muted = M.update(migrated, "preferences", {
    preferences: {
      ...migrated.preferences,
      notificationTypes: {
        ...migrated.preferences.notificationTypes,
        failure: false,
      },
    },
  });
  assert.equal(muted.preferences.notificationTypes.failure, false);
  assert.equal(muted.preferences.notificationTypes.ready, true);
});

test("voices seed once, save the guard flag and cannot disappear while used", () => {
  let state = M.seed();
  assert.deepEqual(
    plain(state.voices.map((voice) => [voice.name, voice.guardrails])),
    [
      ["Caveman", true],
      ["Gilfoyle", true],
      ["Merlin", true],
    ],
  );
  state = M.update(state, "save-voice", {
    voice: { name: "Pirate", body: "Arr.", guardrails: false },
  });
  const pirate = state.voices.at(-1);
  assert.equal(pirate.guardrails, false);
  state = M.update(state, "save-agent", {
    agent: { ...plain(state.agents[0]), voiceId: pirate.id },
  });
  assert.throws(
    () => M.update(state, "delete-voice", { id: pirate.id }),
    /Remove this voice/,
  );
  state = M.update(state, "save-agent", {
    agent: { ...plain(state.agents[0]), voiceId: "" },
  });
  for (const voice of plain(state.voices))
    state = M.update(state, "delete-voice", { id: voice.id });
  assert.deepEqual(plain(state.voices), []);
  assert.deepEqual(plain(M.migrate(plain(state)).voices), []);
  const legacy = plain(M.seed());
  delete legacy.voices;
  assert.equal(M.migrate(legacy).voices.length, 3);
});
