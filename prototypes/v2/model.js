/* Browser-only fixtures and deterministic demo transitions. No provider integration. */
(() => {
  const models = ["Balanced (demo)", "Reasoning (demo)", "Fast (demo)"];
  const stages = [
    "Reading changed files",
    "Checking surrounding code",
    "Preparing findings",
  ];
  const people = [
    "Sam Taylor",
    "Casey Lee",
    "Jordan Blake",
    "Morgan Chen",
    "Alex Rivera",
    "Jamie Park",
    "Riley Quinn",
    "Drew Ellis",
    "Avery Lane",
    "Robin Bell",
  ];
  const titles = [
    "Add keyboard shortcuts to the command menu",
    "Keep filters when switching projects",
    "Handle expired session links",
    "Show upload progress in the sidebar",
    "Reduce duplicate search requests",
    "Restore a dismissed notification",
    "Improve empty project guidance",
    "Make list selection keyboard accessible",
    "Persist the selected workspace",
    "Handle renamed shared folders",
    "Keep drafts after connection loss",
    "Add a compact repository picker",
    "Sort recent projects consistently",
    "Clarify permission error messages",
    "Respect reduced motion preferences",
    "Improve background sync recovery",
    "Show the current account in history",
    "Handle concurrent settings edits",
  ];
  const copy = (value) => JSON.parse(JSON.stringify(value));
  const requireValue = (condition, message) => {
    if (!condition) throw new Error(message);
  };
  const text = (value) => typeof value === "string" && value.trim().length > 0;
  const integer = (value) => Number.isSafeInteger(value) && value > 0;
  function accountName(value) {
    requireValue(
      typeof value === "string",
      "Enter a fictional account handle.",
    );
    const name = value.trim().toLowerCase();
    requireValue(
      name.length <= 39 && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name),
      "Use a fictional handle of up to 39 letters, numbers, and single hyphens. No passwords, tokens, or email addresses.",
    );
    return name;
  }
  const uid = (prefix) => `${prefix}-${crypto.randomUUID()}`;
  const find = (items, id, label) => {
    const result = items.find((item) => item.id === id);
    requireValue(result, `${label} is no longer available.`);
    return result;
  };

  function packet(state, title, index, number, status = "queued") {
    const repo = state.repos[index % state.repos.length];
    const agentId =
      repo.agentIds[
        Math.floor(index / state.repos.length) % repo.agentIds.length
      ];
    return {
      id: `review-${number}`,
      title,
      number,
      repoId: repo.id,
      repoName: repo.name,
      agentId,
      author: people[index % people.length],
      head: `${number.toString(16).padStart(6, "a")}c`,
      base: "b8a126f",
      state: status,
      stage: 0,
      attempt: status === "running" ? 1 : 0,
      restart: false,
      manual: false,
      trusted: true,
      decision: "",
      published: false,
      humanNote: "",
      failure: "",
      run: null,
      synopsis:
        "The change is consistent with the surrounding code. No blocking findings in this sample review.",
      files: [
        {
          path: "src/features/workspace.ts",
          note: "Start with the changed behavior and its caller.",
        },
        {
          path: "src/components/WorkspacePanel.ts",
          note: "Follow the user-facing state changes.",
        },
        {
          path: "tests/workspace.test.ts",
          note: "Inspect the intended edge cases. Tests were not executed.",
        },
      ],
      findings: [],
      conversation: [],
    };
  }

  function seed() {
    const names = [
      "Scout",
      "Sentinel",
      "Clarity",
      "Pathfinder",
      "Sentry",
      "Compass",
    ];
    const state = {
      version: 2,
      scenario: "configured",
      onboarded: true,
      monitoring: true,
      limit: 4,
      nextNumber: 500,
      preferences: {
        automaticStart: true,
        automaticComments: false,
        notifications: false,
        launchAtLogin: false,
      },
      accounts: [
        {
          id: "github-personal",
          name: "alex-demo",
          kind: "github",
          connected: true,
        },
        {
          id: "github-team",
          name: "orbit-review-demo",
          kind: "github",
          connected: true,
        },
        {
          id: "copilot-demo",
          name: "alex-ai-demo",
          kind: "copilot",
          connected: true,
        },
      ],
      doctrines: [
        {
          id: "correctness",
          name: "Correctness first",
          body: "Find concrete defects. Explain the failure path and keep speculative feedback out of the review.",
        },
        {
          id: "clarity",
          name: "Keep it understandable",
          body: "Prefer clear boundaries and understandable code. Ask for human judgment when the intended behavior is ambiguous.",
        },
        {
          id: "accessibility",
          name: "Accessible by default",
          body: "Consider keyboard access, focus, labels, contrast, and reduced motion.",
        },
      ],
      agents: names.map((name, index) => ({
        id: `agent-${index}`,
        name,
        accountId: "copilot-demo",
        model: models[index % models.length],
        prompt:
          "Review the complete change using read-only evidence. Explain concrete defects and preserve human judgment.",
        doctrineIds: [index % 2 ? "clarity" : "correctness"],
        completion: index === 1 ? "approve" : "human",
        comment: true,
      })),
      repos: ["orbit/console", "orbit/workspace", "orbit/storage"].map(
        (name, index) => ({
          id: `repo-${index}`,
          name,
          accountId: index === 1 ? "github-team" : "github-personal",
          enabled: true,
          scope: true,
          agentIds: [`agent-${index}`, `agent-${index + 3}`],
          watched: people.slice(0, 10),
          schedule: "interval",
          minutes: index === 1 ? 15 : 5,
          cron: "*/15 * * * *",
          zone: "America/New_York",
          start: "inherit",
          comments: "inherit",
        }),
      ),
      reviews: [],
    };
    state.reviews = titles.map((title, index) =>
      packet(state, title, index, 302 + index),
    );
    const running = [
      "Keep recent searches across sessions",
      "Improve large folder navigation",
      "Recover pending file transfers",
      "Add accessible status announcements",
    ].map((title, index) =>
      packet(state, title, index, 401 + index, "running"),
    );
    running.forEach((review, index) => {
      review.stage = index % stages.length;
      review.run = runSnapshot(state, review);
    });
    const human = [
      packet(state, "Make search feel instant", 3, 248, "human"),
      packet(state, "Remember where you left off", 4, 91, "human"),
      packet(state, "Retry interrupted uploads", 5, 173, "question"),
    ];
    human.forEach((review) => {
      review.attempt = 1;
      review.decision = review.state === "question" ? "question" : "clear";
      review.run = runSnapshot(state, review);
    });
    human[2].conversation = [
      {
        who: "Scout",
        body: "Should an interrupted upload retry automatically, or ask the person first?",
      },
    ];
    const history = [
      packet(state, "Keep project names consistent", 1, 82, "approved"),
      packet(state, "Recover failed attachment uploads", 2, 164, "author"),
      packet(state, "Simplify the account switcher", 0, 235, "done"),
    ];
    history.forEach((review, index) => {
      review.run = runSnapshot(state, review);
      review.attempt = 1;
      review.decision = index === 1 ? "findings" : "clear";
      review.published = index < 2;
    });
    history[1].findings = [
      {
        title: "Retry loses the original destination",
        severity: "Medium",
        path: "src/features/workspace.ts",
        line: 42,
        body: "The retry path rebuilds the request without retaining the selected folder. Preserve that destination in the retry state.",
        confidence: "High (sample)",
      },
    ];
    history[1].conversation = [
      {
        who: "Clarity",
        body: "Please preserve the selected destination when retrying.",
      },
      { who: "PR author", body: "I will update the retry state." },
    ];
    state.reviews.push(...running, ...human, ...history);
    return state;
  }

  function fresh() {
    const state = seed();
    state.scenario = "fresh";
    state.onboarded = false;
    state.monitoring = false;
    state.accounts = [];
    state.agents = [];
    state.repos = [];
    state.reviews = [];
    state.preferences.automaticStart = false;
    state.preferences.automaticComments = false;
    return state;
  }

  const needsOnboarding = (state) =>
    state.scenario === "fresh" && !state.onboarded;

  function setupStatus(state) {
    const aiAccounts = state.accounts.filter(
      (account) => account.kind === "copilot" && account.connected,
    );
    const githubAccounts = state.accounts.filter(
      (account) => account.kind === "github" && account.connected,
    );
    const agents = state.agents.filter(
      (agent) =>
        aiAccounts.some((account) => account.id === agent.accountId) &&
        models.includes(agent.model) &&
        text(agent.prompt),
    );
    const repos = state.repos.filter(
      (repo) =>
        repo.enabled &&
        repo.scope &&
        githubAccounts.some((account) => account.id === repo.accountId) &&
        repo.agentIds.some((id) => agents.some((agent) => agent.id === id)),
    );
    const steps = [
      aiAccounts.length > 0,
      githubAccounts.length > 0,
      agents.length > 0,
      repos.length > 0,
    ];
    return {
      aiAccounts,
      githubAccounts,
      agents,
      repos,
      steps,
      completed: steps.filter(Boolean).length,
      ready: steps.every(Boolean),
    };
  }

  function effective(state, repo, key) {
    const value = key === "start" ? repo.start : repo.comments;
    return value === "inherit"
      ? state.preferences[
          key === "start" ? "automaticStart" : "automaticComments"
        ]
      : value === "on";
  }

  function blockReason(state, review) {
    const repo = state.repos.find((item) => item.id === review.repoId);
    const agent = state.agents.find((item) => item.id === review.agentId);
    if (!repo) return "Repository removed";
    if (!repo.enabled) return "Repository monitoring disabled";
    if (!repo.scope) return "Monitoring scope needs confirmation";
    if (repo.watched.length && !repo.watched.includes(review.author))
      return "Author no longer matches the watch filter";
    if (
      !state.accounts.find((account) => account.id === repo.accountId)
        ?.connected
    )
      return "Repository account disconnected";
    if (!agent || !repo.agentIds.includes(review.agentId))
      return "Agent is no longer assigned";
    if (
      !state.accounts.find((account) => account.id === agent.accountId)
        ?.connected
    )
      return "AI account disconnected";
    if (!review.trusted) return "Confirm this revision for read-only review";
    if (!effective(state, repo, "start") && !review.manual)
      return "Waiting for manual start";
    return "";
  }

  function runSnapshot(state, review) {
    const agent = find(state.agents, review.agentId, "Agent");
    const repo = find(state.repos, review.repoId, "Repository");
    const account = find(state.accounts, repo.accountId, "Repository account");
    return {
      ...copy(agent),
      actingAccount: account.name,
      aiAccount: find(state.accounts, agent.accountId, "AI account").name,
    };
  }

  function requeue(review) {
    review.state = "queued";
    review.restart = true;
    review.stage = 0;
    review.run = null;
    review.failure = "";
  }

  function reconcile(state) {
    let active = 0;
    for (const review of state.reviews.filter(
      (item) => item.state === "running",
    )) {
      if (
        !state.monitoring ||
        blockReason(state, review) ||
        active >= state.limit
      )
        requeue(review);
      else active++;
    }
    if (!state.monitoring) return;
    const queue = state.reviews
      .filter((review) => review.state === "queued")
      .sort(
        (a, b) => Number(b.restart) - Number(a.restart) || a.number - b.number,
      );
    for (const review of queue) {
      if (active >= state.limit) break;
      if (blockReason(state, review)) continue;
      review.state = "running";
      review.stage = 0;
      review.attempt++;
      review.restart = false;
      review.run = runSnapshot(state, review);
      active++;
    }
  }

  function validate(state) {
    requireValue(
      state?.version === 2,
      "Saved prototype data has an unsupported version. Reset it to use the new demo.",
    );
    if (state.scenario !== undefined || state.onboarded !== undefined) {
      requireValue(
        ["configured", "fresh"].includes(state.scenario) &&
          typeof state.onboarded === "boolean",
        "Invalid saved desktop scenario.",
      );
      requireValue(
        !needsOnboarding(state) || state.monitoring === false,
        "Finish onboarding before starting monitoring.",
      );
    }
    requireValue(
      typeof state.monitoring === "boolean" &&
        integer(state.limit) &&
        integer(state.nextNumber),
      "Invalid monitoring or capacity settings.",
    );
    for (const key of [
      "automaticStart",
      "automaticComments",
      "notifications",
      "launchAtLogin",
    ])
      requireValue(
        typeof state.preferences?.[key] === "boolean",
        "Invalid saved preferences.",
      );
    for (const key of ["accounts", "agents", "repos", "doctrines", "reviews"]) {
      requireValue(Array.isArray(state[key]), `Invalid saved ${key}.`);
      requireValue(
        state[key].every(
          (item) => item && text(item.id) && /^[A-Za-z0-9_-]+$/.test(item.id),
        ),
        `Invalid saved ${key} identity.`,
      );
      requireValue(
        new Set(state[key].map((item) => item.id)).size === state[key].length,
        `Duplicate saved ${key} identities.`,
      );
    }
    state.accounts.forEach((item) =>
      requireValue(
        text(item.name) &&
          ["github", "copilot"].includes(item.kind) &&
          typeof item.connected === "boolean",
        "Invalid demo account.",
      ),
    );
    requireValue(
      new Set(
        state.accounts.map((item) => `${item.kind}/${item.name.toLowerCase()}`),
      ).size === state.accounts.length,
      "That demo identity is already connected for this provider.",
    );
    state.doctrines.forEach((item) =>
      requireValue(
        text(item.name) && text(item.body),
        "Doctrine name and principles are required.",
      ),
    );
    state.agents.forEach((item) => {
      requireValue(
        text(item.name) && text(item.prompt) && models.includes(item.model),
        "Agent name, prompt, and demo model are required.",
      );
      requireValue(
        state.accounts.some(
          (account) =>
            account.id === item.accountId && account.kind === "copilot",
        ),
        "Choose an AI account.",
      );
      requireValue(
        ["human", "approve"].includes(item.completion) &&
          typeof item.comment === "boolean",
        "Choose a valid completion permission.",
      );
      requireValue(
        Array.isArray(item.doctrineIds) &&
          item.doctrineIds.every((id) =>
            state.doctrines.some((doctrine) => doctrine.id === id),
          ),
        "An attached doctrine is missing.",
      );
    });
    state.repos.forEach((item) => {
      requireValue(
        typeof item.name === "string" && /^[\w.-]+\/[\w.-]+$/.test(item.name),
        "Use a repository name like owner/repository.",
      );
      requireValue(
        state.accounts.some(
          (account) =>
            account.id === item.accountId && account.kind === "github",
        ),
        "Choose a repository account.",
      );
      requireValue(
        typeof item.enabled === "boolean" && typeof item.scope === "boolean",
        "Invalid repository activation.",
      );
      requireValue(
        Array.isArray(item.agentIds) &&
          item.agentIds.every((id) =>
            state.agents.some((agent) => agent.id === id),
          ),
        "An assigned agent is missing.",
      );
      requireValue(
        Array.isArray(item.watched) && item.watched.every(text),
        "Invalid watched people.",
      );
      requireValue(
        ["inherit", "on", "off"].includes(item.start) &&
          ["inherit", "on", "off"].includes(item.comments),
        "Invalid automation overrides.",
      );
      requireValue(
        ["interval", "cron"].includes(item.schedule),
        "Choose a valid schedule type.",
      );
      if (item.schedule === "interval")
        requireValue(
          integer(item.minutes),
          "Choose a positive whole-minute interval.",
        );
      if (item.schedule === "cron") {
        requireValue(
          text(item.cron) && text(item.zone),
          "Cron and time zone must not be empty.",
        );
        const fields = item.cron.trim().split(/\s+/);
        requireValue(
          fields.length === 5 &&
            fields.every((field) => /^[\d*/, -]+$/.test(field)),
          "Enter a five-field cron expression. The prototype checks shape only, not real schedule semantics.",
        );
        try {
          new Intl.DateTimeFormat("en", { timeZone: item.zone });
        } catch {
          throw new Error(
            "Choose a valid IANA time zone, such as America/New_York.",
          );
        }
      }
    });
    requireValue(
      new Set(
        state.repos.map(
          (repo) => `${repo.accountId}/${repo.name.toLowerCase()}`,
        ),
      ).size === state.repos.length,
      "That repository is already configured for this account.",
    );
    state.reviews.forEach((item) => {
      requireValue(
        text(item.title) &&
          integer(item.number) &&
          text(item.repoId) &&
          text(item.repoName) &&
          text(item.agentId) &&
          text(item.author) &&
          text(item.head) &&
          text(item.base),
        "Invalid saved review identity.",
      );
      requireValue(
        [
          "queued",
          "running",
          "human",
          "question",
          "approved",
          "author",
          "done",
          "failed",
          "stale",
        ].includes(item.state),
        "Invalid saved review state.",
      );
      requireValue(
        Number.isInteger(item.stage) &&
          item.stage >= 0 &&
          item.stage < stages.length &&
          Number.isSafeInteger(item.attempt) &&
          item.attempt >= 0,
        "Invalid saved review attempt.",
      );
      for (const key of ["trusted", "manual", "restart", "published"])
        requireValue(
          typeof item[key] === "boolean",
          "Invalid saved review gate.",
        );
      for (const key of ["decision", "humanNote", "failure", "synopsis"])
        requireValue(
          typeof item[key] === "string",
          "Invalid saved review text.",
        );
      requireValue(
        Array.isArray(item.files) &&
          item.files.every((file) => text(file.path) && text(file.note)),
        "Invalid saved file guide.",
      );
      requireValue(
        Array.isArray(item.conversation) &&
          item.conversation.every(
            (entry) => text(entry.who) && text(entry.body),
          ),
        "Invalid saved conversation.",
      );
      requireValue(
        Array.isArray(item.findings) &&
          item.findings.every(
            (finding) =>
              text(finding.title) &&
              text(finding.path) &&
              text(finding.body) &&
              text(finding.severity) &&
              text(finding.confidence) &&
              integer(finding.line),
          ),
        "Invalid saved findings.",
      );
      if (item.run)
        requireValue(
          text(item.run.name) &&
            text(item.run.model) &&
            text(item.run.actingAccount) &&
            text(item.run.aiAccount) &&
            Array.isArray(item.run.doctrineIds) &&
            item.run.doctrineIds.every(text) &&
            ["human", "approve"].includes(item.run.completion) &&
            typeof item.run.comment === "boolean",
          "Invalid saved review configuration.",
        );
    });
    const active = state.reviews.filter((review) => review.state === "running");
    requireValue(
      active.length <= state.limit && (state.monitoring || !active.length),
      "Saved active reviews exceed the allowed capacity.",
    );
    requireValue(
      active.every((review) => review.run && !blockReason(state, review)),
      "A saved running review is no longer eligible.",
    );
    return state;
  }

  function update(original, action, payload = {}) {
    const state = copy(original);
    const review = payload.reviewId
      ? find(state.reviews, payload.reviewId, "Review")
      : null;
    switch (action) {
      case "monitor":
        requireValue(
          !needsOnboarding(state),
          "Complete the Genie setup review before starting monitoring.",
        );
        state.monitoring = !state.monitoring;
        break;
      case "finish-onboarding":
        requireValue(
          needsOnboarding(state),
          "This installation has already completed onboarding.",
        );
        requireValue(
          payload.confirmed === true,
          "Confirm the setup summary before starting monitoring.",
        );
        requireValue(
          setupStatus(state).ready,
          "Connect both account roles, create an available agent, and activate an assigned repository first.",
        );
        state.onboarded = true;
        state.monitoring = true;
        break;
      case "capacity":
        requireValue(
          integer(payload.limit),
          "Enter a positive whole number of concurrent reviews.",
        );
        state.limit = payload.limit;
        break;
      case "preference":
        requireValue(
          Object.hasOwn(state.preferences, payload.key) &&
            typeof payload.value === "boolean",
          "Unknown preference.",
        );
        state.preferences[payload.key] = payload.value;
        break;
      case "preferences":
        state.preferences = copy(payload.preferences);
        break;
      case "account": {
        const account = find(state.accounts, payload.id, "Account");
        account.connected = !account.connected;
        break;
      }
      case "add-account": {
        requireValue(
          ["github", "copilot"].includes(payload.kind),
          "That provider is coming soon; no account can be added yet.",
        );
        requireValue(
          payload.confirmed === true,
          "Confirm the returned demo identity before connecting.",
        );
        const name = accountName(payload.name);
        requireValue(
          !state.accounts.some(
            (account) =>
              account.kind === payload.kind &&
              account.name.toLowerCase() === name,
          ),
          "That identity is already listed for this provider. Use its existing connection or Reconnect.",
        );
        state.accounts.push({
          id: uid("account"),
          name,
          kind: payload.kind,
          connected: true,
        });
        // Connecting an unassigned account must not dispatch or change review work.
        return validate(state);
      }
      case "detect": {
        requireValue(
          state.monitoring,
          "Resume monitoring before simulating a check.",
        );
        let added = 0;
        for (const repo of state.repos) {
          if (
            !repo.enabled ||
            !repo.scope ||
            !state.accounts.find((account) => account.id === repo.accountId)
              ?.connected
          )
            continue;
          for (const agentId of repo.agentIds) {
            const number = state.nextNumber++;
            const item = packet(
              state,
              `Sample follow-up change #${number}`,
              state.repos.indexOf(repo),
              number,
            );
            item.agentId = agentId;
            if (repo.watched.length && !repo.watched.includes(item.author))
              continue;
            item.trusted = repo.watched.includes(item.author);
            state.reviews.push(item);
            added++;
          }
        }
        requireValue(
          added,
          "No sample PRs match the enabled repositories, confirmed scopes, watch filters, and assigned agents.",
        );
        break;
      }
      case "start":
        requireValue(
          state.monitoring &&
            ["queued", "failed", "stale"].includes(review.state),
          "Resume monitoring and select waiting work.",
        );
        requireValue(review.trusted, "Confirm trust for this revision first.");
        review.manual = true;
        review.state = "queued";
        review.failure = "";
        break;
      case "trust":
        requireValue(
          ["queued", "failed"].includes(review.state),
          "Only waiting work needs a new trust confirmation.",
        );
        review.trusted = true;
        break;
      case "retry":
        requireValue(
          ["failed", "stale"].includes(review.state),
          "Only failed or stale work can be retried.",
        );
        if (review.state === "stale") {
          requireValue(
            !review.nextId,
            "A fresh revision has already been queued for this sample.",
          );
          const next = copy(review);
          next.id = uid("review");
          next.head = `${(state.nextNumber++).toString(16).padStart(6, "c")}d`;
          next.attempt = 0;
          next.decision = "";
          next.published = false;
          next.findings = [];
          next.conversation = [];
          next.humanNote = "";
          next.trusted = find(
            state.repos,
            review.repoId,
            "Repository",
          ).watched.includes(review.author);
          delete next.nextId;
          requeue(next);
          review.nextId = next.id;
          state.reviews.push(next);
        } else requeue(review);
        break;
      case "interrupt":
        requireValue(review.state === "running", "This review is not running.");
        review.state = "failed";
        review.stage = 0;
        review.run = null;
        review.failure =
          "Demo interruption. Partial work discarded; retry starts a fresh attempt.";
        break;
      case "advance":
        requireValue(
          state.monitoring && review.state === "running",
          "This review is not running.",
        );
        review.stage = Math.min(stages.length - 1, review.stage + 1);
        break;
      case "finish": {
        requireValue(
          state.monitoring &&
            review.state === "running" &&
            !blockReason(state, review),
          "This review is not currently eligible to finish.",
        );
        requireValue(
          ["clear", "findings", "question", "failure"].includes(
            payload.outcome,
          ),
          "Choose a sample outcome.",
        );
        review.decision = payload.outcome;
        review.findings = [];
        review.conversation = [];
        review.failure = "";
        review.published = false;
        review.synopsis =
          payload.outcome === "clear"
            ? "The change is consistent with the surrounding code. No blocking findings in this sample review."
            : payload.outcome === "question"
              ? "One behavior decision needs human judgment before this review can be cleared."
              : payload.outcome === "failure"
                ? "The sample attempt failed. No validated review result is available."
                : "A retry-state issue needs attention before this change is ready.";
        if (payload.outcome === "failure") {
          review.state = "failed";
          review.failure =
            "The sample model response failed validation. Nothing was published.";
        } else if (payload.outcome === "question") {
          review.state = "question";
          review.conversation = [
            {
              who: review.run.name,
              body: "Should the new behavior retry automatically, or wait for a person's confirmation?",
            },
          ];
        } else if (payload.outcome === "findings") {
          review.findings = [
            {
              title: "Retry drops the selected destination",
              severity: "Medium",
              path: review.files[0].path,
              line: 42,
              body: "Preserve the selected destination when rebuilding the retry request.",
              confidence: "High (sample)",
            },
          ];
          review.synopsis =
            "A retry-state issue needs attention before this change is ready.";
          const repo = find(state.repos, review.repoId, "Repository");
          review.published =
            review.run.comment && effective(state, repo, "comments");
          review.state = review.published ? "author" : "human";
        } else {
          review.state =
            review.run.completion === "approve" ? "approved" : "human";
          review.published = review.state === "approved";
        }
        break;
      }
      case "publish":
        requireValue(
          state.monitoring &&
            review.state === "human" &&
            review.findings.length,
          "There are no pending findings to publish, or monitoring is paused.",
        );
        requireValue(
          find(state.agents, review.agentId, "Agent").comment,
          "This agent does not have comment permission.",
        );
        requireValue(
          !blockReason(state, { ...review, manual: true }),
          "Current account, repository, or trust gates block publication.",
        );
        review.published = true;
        review.state = "author";
        break;
      case "stale":
        requireValue(
          !["queued", "running"].includes(review.state),
          "Select a completed review to simulate a changed revision.",
        );
        review.state = "stale";
        review.failure =
          "The PR changed after this review. Saved evidence belongs to the earlier head.";
        break;
      case "save-agent": {
        const agent = {
          ...payload.agent,
          id: payload.agent.id || uid("agent"),
        };
        const index = state.agents.findIndex((item) => item.id === agent.id);
        if (index < 0) state.agents.push(agent);
        else {
          state.agents[index] = agent;
          state.reviews
            .filter(
              (item) => item.agentId === agent.id && item.state === "running",
            )
            .forEach(requeue);
        }
        break;
      }
      case "delete-agent":
        requireValue(
          !state.repos.some((repo) => repo.agentIds.includes(payload.id)) &&
            !state.reviews.some(
              (item) =>
                item.agentId === payload.id &&
                [
                  "queued",
                  "running",
                  "failed",
                  "stale",
                  "human",
                  "question",
                ].includes(item.state),
            ),
          "Unassign this agent and finish its outstanding work before deleting it.",
        );
        find(state.agents, payload.id, "Agent");
        state.agents = state.agents.filter((item) => item.id !== payload.id);
        break;
      case "save-repo": {
        const repo = { ...payload.repo, id: payload.repo.id || uid("repo") };
        const index = state.repos.findIndex((item) => item.id === repo.id);
        if (index < 0) state.repos.push(repo);
        else {
          state.repos[index] = repo;
          state.reviews
            .filter(
              (item) => item.repoId === repo.id && item.state === "running",
            )
            .forEach(requeue);
        }
        break;
      }
      case "delete-repo":
        requireValue(
          !state.reviews.some(
            (item) =>
              item.repoId === payload.id &&
              [
                "queued",
                "running",
                "failed",
                "stale",
                "human",
                "question",
              ].includes(item.state),
          ),
          "This repository has outstanding work. Disable monitoring instead, or finish the work before removing it.",
        );
        find(state.repos, payload.id, "Repository");
        state.repos = state.repos.filter((item) => item.id !== payload.id);
        break;
      case "save-doctrine": {
        const doctrine = {
          ...payload.doctrine,
          id: payload.doctrine.id || uid("doctrine"),
        };
        const index = state.doctrines.findIndex(
          (item) => item.id === doctrine.id,
        );
        if (index < 0) state.doctrines.push(doctrine);
        else state.doctrines[index] = doctrine;
        state.reviews
          .filter(
            (item) =>
              item.state === "running" &&
              item.run?.doctrineIds.includes(doctrine.id),
          )
          .forEach(requeue);
        break;
      }
      case "delete-doctrine":
        requireValue(
          !state.agents.some((agent) => agent.doctrineIds.includes(payload.id)),
          "Remove this doctrine from its agents before deleting it.",
        );
        find(state.doctrines, payload.id, "Doctrine");
        state.doctrines = state.doctrines.filter(
          (item) => item.id !== payload.id,
        );
        break;
      default:
        throw new Error("Unknown prototype action.");
    }
    reconcile(state);
    return validate(state);
  }

  globalThis.SniperDemo = Object.freeze({
    seed,
    fresh,
    needsOnboarding,
    setupStatus,
    validate,
    update,
    copy,
    models,
    accountName,
    stages,
    people,
    blockReason,
    effective,
  });
})();
