const Demo = globalThis.SniperDemo;
const storageKey = "pr-sniper.vnext.v2";
const tray = document.querySelector("#tray-trigger");
const popover = document.querySelector("#sniper-popover");
const stage = document.querySelector("#view-stage");
const announcement = document.querySelector("#lane-announcement");
const monitoringToggle = document.querySelector("#monitoring-toggle");
const notice = document.querySelector("#app-notice");
const pageTitle = document.querySelector("#page-title");
const backButton = document.querySelector("#back-button");
const toast = document.querySelector("#toast");
let state = Demo.seed();
let savedRaw = null;
let storageBlocked = false;
let tab = "queue";
let lane = "human";
let historyFilter = "all";
let dirty = false;
let toastTimer;
const stacks = Object.fromEntries(
  ["queue", "running", "reviewed", "settings"].map((key) => [
    key,
    [{ page: key }],
  ]),
);
const route = () => stacks[tab].at(-1);
const esc = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ],
  );
const icon = (name) =>
  `<svg class="icon" aria-hidden="true"><use href="#${name}" /></svg>`;
const selected = (value, current) => (value === current ? "selected" : "");
const checked = (value) => (value ? "checked" : "");
const disabled = (value) => (value ? "disabled" : "");
const initials = (name) =>
  name
    .split(/\s+/)
    .slice(0, 2)
    .map((part) => part[0])
    .join("");
const agentFor = (review) =>
  state.agents.find((agent) => agent.id === review.agentId);
const repoFor = (review) =>
  state.repos.find((repo) => repo.id === review.repoId);
const running = () =>
  state.reviews.filter((review) => review.state === "running");
const queued = () =>
  state.reviews
    .filter(
      (review) =>
        ["queued", "failed", "stale"].includes(review.state) && !review.nextId,
    )
    .sort(
      (a, b) => Number(b.restart) - Number(a.restart) || a.number - b.number,
    );
const human = () =>
  state.reviews.filter(
    (review) =>
      ["human", "question"].includes(review.state) ||
      (review.state === "queued" && !review.trusted),
  );
const reviewed = () =>
  state.reviews.filter(
    (review) => !["queued", "running"].includes(review.state),
  );
const statusNames = {
  queued: "Queued",
  running: "Reviewing",
  human: "Ready for you",
  question: "Needs your input",
  approved: "Approved (demo)",
  author: "Waiting for author",
  done: "Human-reviewed",
  failed: "Review failed",
  stale: "Stale review",
};
const connectionProviders = [
  {
    id: "github",
    name: "GitHub",
    group: "Repositories",
    mark: "GH",
    available: true,
    description: "Repository access and publication identity",
  },
  {
    id: "azure-devops",
    name: "Azure DevOps",
    group: "Repositories",
    mark: "Az",
    available: false,
    description: "Organizations, projects, and repositories",
  },
  {
    id: "copilot",
    name: "GitHub Copilot",
    group: "AI providers",
    mark: "Co",
    available: true,
    description: "AI accounts for your review agents",
  },
  {
    id: "claude",
    name: "Claude",
    group: "AI providers",
    mark: "Cl",
    available: false,
    description: "Direct Anthropic integration",
  },
  {
    id: "codex",
    name: "Codex",
    group: "AI providers",
    mark: "Cx",
    available: false,
    description: "Direct OpenAI integration",
  },
  {
    id: "grok",
    name: "Grok",
    group: "AI providers",
    mark: "Gk",
    available: false,
    description: "Direct xAI integration",
  },
];

function showError(message) {
  notice.textContent = message;
  notice.hidden = false;
  announcement.textContent = message;
}

function tell(message) {
  toast.textContent = message;
  toast.hidden = false;
  announcement.textContent = message;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toast.hidden = true;
  }, 4500);
}

try {
  savedRaw = localStorage.getItem(storageKey);
  if (savedRaw !== null) state = Demo.validate(JSON.parse(savedRaw));
} catch (error) {
  storageBlocked = true;
  showError(
    `Saved prototype data could not be loaded. Sample data is shown without overwriting it. Use Settings > Advanced > Reset prototype. ${error.message}`,
  );
}
if (Demo.needsOnboarding(state)) stacks.queue = [{ page: "welcome" }];

function save(next, force = false) {
  if (storageBlocked && !force)
    throw new Error(
      "Saved prototype data is unavailable. Reset the prototype in Advanced before making changes.",
    );
  let currentRaw;
  try {
    currentRaw = localStorage.getItem(storageKey);
  } catch {
    throw new Error(
      "Browser storage is unavailable. Allow storage for this page before saving.",
    );
  }
  if (!force && currentRaw !== savedRaw)
    throw new Error(
      "Another tab changed this prototype. Reload before saving; your edit has not overwritten it.",
    );
  const nextRaw = JSON.stringify(Demo.validate(next));
  try {
    localStorage.setItem(storageKey, nextRaw);
  } catch {
    throw new Error(
      "Could not save prototype changes. Allow site storage or free browser storage, then try again.",
    );
  }
  savedRaw = nextRaw;
  storageBlocked = false;
  state = next;
  notice.hidden = true;
}

function perform(
  action,
  payload = {},
  message = "Saved in this browser.",
  redraw = true,
) {
  try {
    remember();
    save(Demo.update(state, action, payload));
    syncChrome();
    if (redraw) render();
    tell(message);
    return true;
  } catch (error) {
    showError(error.message);
    return false;
  }
}

function remember() {
  const current = stage.lastElementChild;
  if (!current) return;
  route().scrolls = [...current.querySelectorAll("[data-scroll]")].map(
    (element) => [element.dataset.scroll, element.scrollTop],
  );
  const focused = document.activeElement;
  if (focused?.dataset?.reviewId)
    route().returnReview = focused.dataset.reviewId;
}

function mayLeave() {
  if (!dirty) return true;
  if (
    !confirm(
      "Discard unsaved editor changes? Saved prototype data will be kept.",
    )
  )
    return false;
  dirty = false;
  return true;
}

function navigate(page, id) {
  if (!mayLeave()) return;
  remember();
  stacks[tab].push({ page, id });
  render(1, true);
}

function goBack() {
  if (stacks[tab].length === 1 || !mayLeave()) return;
  remember();
  stacks[tab].pop();
  render(-1, true);
}

function switchTab(next) {
  if (!mayLeave()) return;
  remember();
  const order = ["queue", "running", "reviewed", "settings"];
  const direction = Math.sign(order.indexOf(next) - order.indexOf(tab));
  tab = next;
  render(direction, true);
}

function syncChrome() {
  const setupNeeded = Demo.needsOnboarding(state);
  monitoringToggle.setAttribute("aria-checked", String(state.monitoring));
  monitoringToggle.disabled = setupNeeded;
  monitoringToggle.title = setupNeeded
    ? "Complete the Genie setup review before monitoring."
    : "Pause or resume all automation";
  popover.dataset.monitoring = String(state.monitoring);
  popover.dataset.lane = lane;
  tray.dataset.monitoring = String(state.monitoring);
  document.querySelector("#monitoring-label").textContent = setupNeeded
    ? "Setup needed"
    : state.monitoring
      ? "Monitoring"
      : "Paused";
  document.querySelectorAll("[data-scenario]").forEach((button) => {
    button.setAttribute(
      "aria-pressed",
      String(button.dataset.scenario === (state.scenario ?? "configured")),
    );
  });
  document.querySelector("#running-nav-count").textContent = running().length;
  document.querySelectorAll("[data-nav]").forEach((button) => {
    if (button.dataset.nav === tab) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  });
  backButton.hidden = stacks[tab].length < 2;
}

function loadScenario(scenario) {
  try {
    if (!["fresh", "configured"].includes(scenario))
      throw new Error("Unknown desktop demo.");
    save(scenario === "fresh" ? Demo.fresh() : Demo.seed(), true);
    dirty = false;
    tab = "queue";
    lane = "human";
    historyFilter = "all";
    Object.keys(stacks).forEach((key) => {
      stacks[key] = [
        {
          page:
            key === "queue" && Demo.needsOnboarding(state) ? "welcome" : key,
        },
      ];
    });
    clearTimeout(toastTimer);
    toast.hidden = true;
    render();
    openPopover();
    announcement.textContent = `${scenario === "fresh" ? "Fresh install" : "Configured"} demo reset. Previous prototype edits were replaced.`;
  } catch (error) {
    openPopover();
    showError(error.message);
  }
}

function setupSteps(status) {
  return [
    {
      id: "ai",
      title: "Choose your AI provider",
      detail: status.steps[0]
        ? `${status.aiAccounts.length} AI account connected`
        : "Give your reviewers an AI account",
      enabled: true,
    },
    {
      id: "code",
      title: "Connect source control",
      detail: status.steps[1]
        ? `${status.githubAccounts.length} repository account connected`
        : "Choose who accesses your repositories",
      enabled: true,
    },
    {
      id: "agent",
      title: "Create your reviewer",
      detail: status.steps[2]
        ? `${status.agents.length} agent ready`
        : status.steps[0]
          ? "Pick a model, principles, and permissions"
          : "Connect an AI provider first",
      enabled: status.steps[0],
    },
    {
      id: "repo",
      title: "Choose what to watch",
      detail: status.steps[3]
        ? `${status.repos.length} repository ready`
        : status.steps[1] && status.steps[2]
          ? "Assign your agent and set its schedule"
          : "Connect source control and create an agent first",
      enabled: status.steps[1] && status.steps[2],
    },
  ];
}

function setupProgress(status) {
  return `<div class="setup-progress"><span>Your setup</span><strong>${status.completed} of 4 ready</strong><div class="setup-progress-track" role="progressbar" aria-label="Setup essentials ready" aria-valuemin="0" aria-valuemax="4" aria-valuenow="${status.completed}">${status.steps.map((complete) => `<i class="${complete ? "complete" : ""}"></i>`).join("")}</div></div>`;
}

function setupChecklist(status) {
  return `<ol class="setup-checklist">${setupSteps(status)
    .map(
      (step, index) =>
        `<li><button type="button" data-action="setup-step" data-step="${step.id}" ${disabled(!step.enabled)} class="${status.steps[index] ? "complete" : ""}"><span class="setup-step-symbol">${status.steps[index] ? icon("check") : index + 1}</span><span><strong>${step.title}</strong><small>${esc(step.detail)}</small></span>${icon("chevron")}</button></li>`,
    )
    .join("")}</ol>`;
}

function welcomePage() {
  const status = Demo.setupStatus(state);
  return `<div class="page-scroll" data-scroll="welcome">
    <section class="welcome-card"><span class="genie-symbol">${icon("spark")}</span><span class="first-launch-label">A fresh start</span><h2>A second set of eyes.<br />Let's set yours up.</h2><p>Connect your accounts, create a reviewer, and choose what to watch.</p><button class="genie-action" type="button" data-page="genie">${icon("spark")}${status.completed ? "Continue with Genie" : "Set up with Genie"}</button><button class="text-action manual-setup" data-nav="settings">I'll set it up myself</button></section>
    ${setupProgress(status)}${setupChecklist(status)}
    <p class="list-footnote">Nothing is running yet. Genie uses the same settings editors, one step at a time. ${state.doctrines.length} starter doctrines are included.</p>
  </div>`;
}

function geniePage() {
  const status = Demo.setupStatus(state);
  const next = status.steps.findIndex((complete) => !complete);
  const guidance = [
    [
      "Choose your AI provider",
      "Pick the provider that will power your reviewers, then connect an account. Repository access comes separately.",
      "Choose AI provider",
    ],
    [
      "Give it a place to look",
      "Choose your PR platform, then connect the identity that can access your repositories. Nothing gets selected automatically.",
      "Choose PR platform",
    ],
    [
      "Meet your first reviewer",
      "Choose the AI account and model, add review principles, and decide what the agent may do.",
      "Create a review agent",
    ],
    [
      "Point it at your work",
      "Choose a repository account, assign your agent, confirm scope, and set the schedule and automation gates.",
      "Configure a repository",
    ],
  ];
  const steps = setupSteps(status);
  return `<div class="page-scroll" data-scroll="genie"><section class="genie-card"><div class="genie-card-heading"><span class="genie-symbol">${icon("spark")}</span><span>Onboarding Genie<small>${status.ready ? "Ready for your final check" : `Step ${next + 1} of 4`}</small></span></div><h2>${status.ready ? "Your setup is ready." : guidance[next][0]}</h2><p>${status.ready ? "Review the accounts, permissions, and schedule before switching monitoring on." : guidance[next][1]}</p>
    <button class="genie-action" ${status.ready ? 'data-page="setup-review"' : `data-action="setup-step" data-step="${steps[next].id}"`}>${status.ready ? "Review & start" : guidance[next][2]}${icon("chevron")}</button></section>
    ${setupProgress(status)}${setupChecklist(status)}
    <p class="list-footnote">Each save updates shared settings. Back or Close keeps completed setup; monitoring stays off until you finish.</p></div>`;
}

function openSetupStep(step) {
  const status = Demo.setupStatus(state);
  const definition = setupSteps(status).find((item) => item.id === step);
  if (!definition?.enabled) {
    showError("Complete the prerequisite connections first.");
    return;
  }
  if (["ai", "code"].includes(step)) {
    const group = step === "ai" ? "AI providers" : "Repositories";
    if (
      state.accounts.some((account) =>
        connectionProviders.some(
          (provider) =>
            provider.group === group && provider.id === account.kind,
        ),
      )
    )
      navigate("accounts");
    else navigate("add-account", step);
  } else if (step === "agent") {
    if (state.agents.length === 1) navigate("agent", state.agents[0].id);
    else navigate(state.agents.length ? "agents" : "agent");
  } else {
    if (state.repos.length === 1) navigate("repo", state.repos[0].id);
    else navigate(state.repos.length ? "repos" : "repo");
  }
}

function setupReviewPage() {
  const status = Demo.setupStatus(state);
  return `<div class="page-scroll" data-scroll="setup-review"><section class="detail-hero"><span class="state-pill">${icon("spark")}Genie's final check</span><h2>Your app. Your call.</h2><p>Starting monitoring does not grant extra review or publication permissions.</p></section>
    ${!status.ready ? '<p class="inline-warning">Setup changed. Complete all four essentials before starting.</p>' : ""}
    ${state.repos
      .map((repo) => {
        const ready = status.repos.some((item) => item.id === repo.id);
        return `<section class="detail-section setup-repository"><h3>${esc(repo.name)}</h3>${!ready ? '<p class="inline-warning">Not ready: disabled, disconnected, unconfirmed scope, or no available assigned agent. This repository will not run.</p>' : ""}<dl class="context-list"><dt>GitHub identity</dt><dd>${esc(state.accounts.find((account) => account.id === repo.accountId)?.name)}</dd><dt>Detection</dt><dd>${repo.schedule === "interval" ? `Every ${repo.minutes} minutes` : `${esc(repo.cron)} / ${esc(repo.zone)}`}</dd><dt>Scope</dt><dd>${repo.scope ? "New PRs only (demo)" : "Not confirmed"}</dd><dt>Authors</dt><dd>${repo.watched.length ? `${repo.watched.length} watched people` : "All authors; untrusted revisions still need confirmation"}</dd><dt>Review start</dt><dd>${Demo.effective(state, repo, "start") ? "Automatic when eligible" : "Manual start required"}</dd><dt>Comments</dt><dd>${Demo.effective(state, repo, "comments") ? "Automatic if agent permits" : "Manual publication gate"}</dd></dl>
      ${repo.agentIds
        .map((id) => state.agents.find((agent) => agent.id === id))
        .filter(Boolean)
        .map(
          (agent) =>
            `<div class="setup-agent-summary"><strong>${esc(agent.name)}</strong><p>${esc(agent.model)} / Copilot: ${esc(state.accounts.find((account) => account.id === agent.accountId)?.name)}</p><small>${agent.completion === "approve" ? "May approve on GitHub (demo)" : "Sends cleared work to your human queue"} / ${agent.comment ? "May comment" : "No comment permission"}</small></div>`,
        )
        .join("")}
      <button class="text-action" data-page="repo" data-id="${repo.id}">Edit this repository</button></section>`;
      })
      .join("")}
    <section class="detail-section"><h3>Room to work</h3><p>At most <strong>${state.limit} reviews</strong> run at once. You can save as many agent configurations as you need.</p><button class="text-action" data-page="capacity">Adjust concurrent reviews</button></section>
    <form data-form="setup-finish" class="editor-form"><label class="checkbox-field"><input name="confirmSetup" type="checkbox" required /><span>I've reviewed these accounts, assignments, and automation permissions.</span></label><button type="submit" class="genie-action" disabled>Start monitoring</button><p class="section-note">Enters the app with your setup, not the busy sample fixture. No PRs are invented until you simulate a check.</p></form></div>`;
}

function capacity() {
  const count = running().length;
  return `<div class="capacity-strip">
    <div class="capacity-ring" role="img" aria-label="${count} of ${state.limit} review slots in use">
      <svg viewBox="0 0 44 44" aria-hidden="true"><circle class="ring-track" cx="22" cy="22" r="18" /><circle class="ring-fill" cx="22" cy="22" r="18" style="stroke-dashoffset:${113.1 * (1 - count / state.limit)}" /></svg>
      <span><b id="running-count">${count}</b><span>/${state.limit}</span></span>
    </div>
    <div><strong>${state.monitoring ? (count ? "All hands on deck" : "Ready when work arrives") : "Everything is paused"}</strong>
    <span>${state.monitoring ? `${count} reviews running` : "Active work discarded and returned to queue"}</span></div>
    <span class="working-dots" aria-hidden="true"><i></i><i></i><i></i><i></i></span>
  </div>`;
}

function pill(review) {
  const blocked = ["queued", "failed"].includes(review.state)
    ? Demo.blockReason(state, review)
    : "";
  const label = !review.trusted
    ? "Needs your confirmation"
    : blocked && review.state === "queued"
      ? "Blocked"
      : review.state === "human" && review.findings.length
        ? "Findings need attention"
        : statusNames[review.state];
  const warning =
    ["question", "failed", "stale"].includes(review.state) ||
    blocked ||
    review.findings.length;
  const symbol = warning
    ? "question"
    : review.state === "running"
      ? "activity"
      : review.state === "queued"
        ? "queue"
        : review.state === "author"
          ? "mail"
          : "check";
  return `<span class="state-pill ${warning ? "question" : ""}">${icon(symbol)}${label}</span>`;
}

function reference(review) {
  return `<span class="pr-reference">${icon("pull-request")}<span>${esc(review.repoName)} #${review.number}</span></span>`;
}

function reviewCard(review, index, compact = false) {
  const agent = review.run?.name ?? agentFor(review)?.name ?? "Unassigned";
  const reason = Demo.blockReason(state, review);
  return `<button type="button" class="review-card review-open ${compact ? "agent-card" : ""}" data-action="review" data-review-id="${esc(review.id)}">
    ${compact ? `<span class="agent-title-row"><span class="agent-position">${String(index + 1).padStart(2, "0")}</span><span><strong class="review-title">${esc(review.title)}</strong>${reference(review)}</span></span>` : `<span class="review-card-header">${pill(review)}<span class="review-age">${review.state === "running" ? `Attempt ${review.attempt}` : "Sample review"}</span></span><strong class="review-title">${esc(review.title)}</strong>${reference(review)}`}
    ${review.state === "running" ? `<span class="stage-label">${icon("activity")}${Demo.stages[review.stage]}</span>` : ""}
    ${review.restart ? '<span class="requeued-note">Interrupted / queued for a fresh start</span>' : ""}
    ${compact && reason ? `<span class="requeued-note">${esc(reason)}</span>` : ""}
    ${review.state === "question" ? `<span class="review-question">${esc(review.conversation.at(-1)?.body)}</span>` : ""}
    <span class="review-card-footer"><span class="avatar" data-color="${index % 2 ? "orange" : "teal"}">${esc(initials(review.author))}</span><span class="person-name">${esc(review.author)}</span><span class="agent-credit"><strong>${esc(agent)}</strong> / ${compact ? "queued" : `${review.files.length} files`}</span>${icon("chevron")}</span>
  </button>`;
}

function empty(title, description, symbol = "queue") {
  return `<div class="empty-state">${icon(symbol)}<h2>${esc(title)}</h2><p>${esc(description)}</p></div>`;
}

function queuePage() {
  const waiting = queued();
  const attention = human();
  const ready = attention.filter(
    (review) => review.state === "human" && !review.findings.length,
  ).length;
  return `<div class="queue-tabs" role="tablist" aria-label="Review queues">
    ${[
      ["human", "For you", attention.length, "need your eyes", "queue"],
      [
        "agent",
        "For agents",
        waiting.length,
        "waiting to start",
        "pull-request",
      ],
    ]
      .map(
        ([
          key,
          label,
          count,
          caption,
          symbol,
        ]) => `<button id="${key}-tab" class="queue-tab" role="tab" type="button" aria-selected="${lane === key}" aria-controls="${key}-panel" tabindex="${lane === key ? 0 : -1}" data-lane="${key}">
      <span class="tab-label">${label}${icon(symbol)}</span><span class="tab-metric"><strong id="${key}-count">${count}</strong><span>${caption}</span></span></button>`,
      )
      .join("")}
  </div>${capacity()}
  <div class="queue-viewport"><div class="lane-track">
    <section id="human-panel" class="queue-panel" data-scroll="human" role="tabpanel" aria-labelledby="human-tab" tabindex="0" ${lane !== "human" ? 'inert aria-hidden="true"' : ""}>
      <div class="list-heading"><h2>Over to you</h2><span>${ready} ready, ${attention.length - ready} need attention</span></div>
      <div class="review-list" id="human-list">${attention.map((review, index) => reviewCard(review, index)).join("") || empty("You're all caught up", "We'll put the next human handoff here.")}</div>
      <p class="list-footnote">Machine-reviewed. The final call is yours.</p>
    </section>
    <section id="agent-panel" class="queue-panel" data-scroll="agent" role="tabpanel" aria-labelledby="agent-tab" tabindex="0" ${lane !== "agent" ? 'inert aria-hidden="true"' : ""}>
      <div class="list-heading"><h2>Next up</h2><span id="packet-count">${waiting.length} review packets</span></div>
      <p class="agent-intro" id="agent-intro">${state.monitoring ? "A free slot picks up the next eligible review." : "All checks and automation are off. Interrupted reviews start over when you resume."}</p>
      <button class="quiet-action check-demo" data-action="detect" ${disabled(!state.monitoring)}>Simulate a repository check</button>
      <div class="review-list agent-list" id="agent-list">${waiting.map((review, index) => reviewCard(review, index, true)).join("") || empty("No waiting work", "Simulate a repository check to add sample review packets.")}</div>
      <p class="list-footnote">Sample ordering, not a production scheduling policy.</p>
    </section>
  </div></div>`;
}

function runningPage() {
  return `<div class="page-scroll" data-scroll="running">
    <div class="feature-summary"><span class="summary-icon">${icon("activity")}</span><div><strong>${running().length}<small> / ${state.limit}</small></strong><p>review slots in use</p></div><span class="summary-tail">${queued().length} waiting</span></div>
    <p class="section-note">Each slot handles one PR revision. Open a review to step through its sample activity.</p>
    <div class="review-list">${
      running()
        .map((review, index) => reviewCard(review, index))
        .join("") ||
      empty(
        Demo.needsOnboarding(state)
          ? "No reviews yet"
          : state.monitoring
            ? "Nothing running right now"
            : "Taking a breather",
        Demo.needsOnboarding(state)
          ? "Finish setup with Genie to start watching your repositories."
          : state.monitoring
            ? "Check the backlog for blockers, or simulate a repository check."
            : "Reviews are back in the queue. Resume monitoring for a fresh start.",
        "activity",
      )
    }</div>
    <button class="quiet-action wide" data-action="detect" ${disabled(!state.monitoring)}>Simulate a repository check</button>
    <p class="list-footnote">No timers or fake percentages. Activity advances only when you choose a demo action.</p>
  </div>`;
}

function reviewedPage() {
  const items = reviewed().filter(
    (review) =>
      historyFilter === "all" ||
      (historyFilter === "approved"
        ? review.state === "approved"
        : historyFilter === "attention"
          ? ["human", "question", "author", "failed", "stale"].includes(
              review.state,
            )
          : review.state === "done"),
  );
  return `<div class="page-scroll" data-scroll="reviewed">
    <div class="feature-summary"><span class="summary-icon">${icon("reviewed")}</span><div><strong>${reviewed().length}</strong><p>review outcomes</p></div><span class="summary-tail">Sample history</span></div>
    <div class="filter-row" aria-label="Filter review history">${[
      ["all", "All"],
      ["approved", "Approved"],
      ["attention", "Follow-up"],
      ["done", "Human-reviewed"],
    ]
      .map(
        ([key, label]) =>
          `<button class="filter-chip" data-filter="${key}" aria-pressed="${historyFilter === key}">${label}</button>`,
      )
      .join("")}</div>
    <div class="review-list">${items.map((review, index) => reviewCard(review, index)).join("") || empty("Nothing in this view", "Choose another filter or complete a sample review.", "reviewed")}</div>
    <p class="list-footnote">An approval is not a merge. All provider outcomes here are simulated.</p>
  </div>`;
}

function detailPage(review) {
  const agent = review.run ?? agentFor(review);
  const repo = repoFor(review);
  const acting =
    review.run?.actingAccount ??
    state.accounts.find((account) => account.id === repo?.accountId)?.name ??
    "Account unavailable";
  const ai =
    review.run?.aiAccount ??
    state.accounts.find((account) => account.id === agent?.accountId)?.name ??
    "AI account unavailable";
  const reason = Demo.blockReason(state, review);
  const resultAvailable = !["queued", "running", "failed"].includes(
    review.state,
  );
  return `<div class="page-scroll" data-scroll="detail">
    <div class="detail-hero">${pill(review)}<h2>${esc(review.title)}</h2>${reference(review)}<p>By ${esc(review.author)}</p><a class="primary-action wide platform-link" href="https://github.com/${review.repoName.split("/").map(encodeURIComponent).join("/")}/pull/${review.number}" target="_blank" rel="noopener noreferrer">Open pull request on GitHub ${icon("chevron")}</a><p class="platform-link-note">Review, comment, approve, or merge on GitHub. Demo destinations may not exist.</p></div>
    <div class="identity-strip"><span>${icon("crosshair")} ${esc(agent?.name ?? "Unassigned")}</span><span>${esc(agent?.model ?? "No model")}</span></div>
    ${review.failure ? `<div class="inline-warning">${esc(review.failure)}</div>` : ""}
    ${["queued", "failed"].includes(review.state) && reason ? `<div class="inline-warning">${esc(reason)}</div>` : ""}
    ${
      review.state === "running"
        ? `<section class="detail-section"><h3>Review activity</h3><ol class="activity-steps">${Demo.stages.map((label, index) => `<li class="${index < review.stage ? "complete" : index === review.stage ? "current" : ""}"><span>${index < review.stage ? icon("check") : index + 1}</span>${label}</li>`).join("")}</ol><p class="section-note">Read-only review. Repository code, builds, and tests are not executed.</p>
      <button class="secondary-action wide" data-action="advance" data-id="${review.id}" ${disabled(review.stage === Demo.stages.length - 1)}>Advance sample activity</button>
      <form data-form="complete" data-id="${review.id}" class="compact-form"><label>Sample outcome<select name="outcome"><option value="clear">No blocking findings</option><option value="findings">Findings to address</option><option value="question">Needs human judgment</option><option value="failure">Invalid output / failure</option></select></label><button class="primary-action" type="submit">Complete sample review</button></form>
      <button class="text-action danger" data-action="interrupt" data-id="${review.id}">Stop this sample review</button></section>`
        : ""
    }
    ${
      resultAvailable
        ? `<section class="detail-section"><h3>Review synopsis</h3><p>${esc(review.synopsis)}</p>
      <div class="outcome-note">${review.state === "approved" ? "Provider approval simulated as " + esc(acting) + ". No merge was performed." : review.state === "author" ? "Sample findings published. Waiting for the PR author." : review.state === "done" ? "Human review shown in sample history. Continue the real workflow on GitHub." : "Machine assessment only. Continue the human review on GitHub."}</div></section>
      <section class="detail-section"><h3>Findings <span class="count-badge">${review.findings.length}</span></h3>${review.findings.map((finding) => `<article class="finding"><span class="state-pill question">${esc(finding.severity)}</span><h4>${esc(finding.title)}</h4><code>${esc(finding.path)}:${finding.line}</code><p>${esc(finding.body)}</p><small>Confidence: ${esc(finding.confidence)}</small></article>`).join("") || '<p class="success-line">No blocking findings in this sample.</p>'}</section>
      <details class="detail-section file-guide" open><summary>Changed-file guide <span class="count-badge">${review.files.length}</span></summary><ol>${review.files.map((file) => `<li><code>${esc(file.path)}</code><p>${esc(file.note)}</p></li>`).join("")}</ol></details>`
        : ""
    }
    ${review.conversation.length ? `<section class="detail-section"><h3>Conversation</h3><div class="conversation">${review.conversation.map((entry) => `<article><strong>${esc(entry.who)}</strong><p>${esc(entry.body)}</p></article>`).join("")}</div></section>` : ""}
    <section class="detail-section"><h3>Review context</h3><dl class="context-list"><dt>Repository account</dt><dd>${esc(acting)}</dd><dt>Copilot AI account</dt><dd>${esc(ai)}</dd><dt>Head / base</dt><dd><code>${esc(review.head)} / ${esc(review.base)}</code></dd><dt>Completion permission</dt><dd>${agent?.completion === "approve" ? "Provider approval (demo)" : "Send to human queue"}</dd><dt>Attempt</dt><dd>${review.attempt || "Not started"}</dd></dl></section>
    ${reviewActions(review)}
    ${resultAvailable && review.state !== "stale" ? `<details class="scenario-details"><summary>Prototype scenarios</summary><button class="text-action" data-action="stale" data-id="${review.id}">Simulate a changed PR revision</button></details>` : ""}
    <p class="list-footnote">Evidence is synthetic. The PR link opens GitHub; this prototype never submits reviews, comments, approvals, or merges.</p>
  </div>`;
}

function reviewActions(review) {
  if (review.nextId)
    return `<button class="primary-action wide" data-action="review" data-review-id="${esc(review.nextId)}">View the newer revision</button>`;
  if (["failed", "stale"].includes(review.state))
    return `<button class="primary-action wide" data-action="retry" data-id="${review.id}">Queue a fresh review</button>`;
  if (review.state === "queued")
    return `<section class="detail-section"><h3>Ready to start?</h3>${!review.trusted ? `<p>Confirm this exact revision for a read-only review. Monitoring scope is not trust.</p><button class="secondary-action wide" data-action="trust" data-id="${review.id}">Confirm trust for this revision</button>` : ""}
    <button class="primary-action wide" data-action="start" data-id="${review.id}" ${disabled(!state.monitoring || !review.trusted)}>Request a review slot</button><p class="section-note">Uses the next available slot. Never exceeds your concurrency limit.</p></section>`;
  return "";
}

function settingsRow(title, subtitle, value, symbol, page) {
  return `<button class="settings-row" data-page="${page}"><span class="row-symbol">${icon(symbol)}</span><span class="row-copy"><strong>${title}</strong><small>${esc(subtitle)}</small></span><span class="row-value">${esc(value)}</span>${icon("chevron")}</button>`;
}

function settingsPage() {
  return `<div class="page-scroll" data-scroll="settings">
    <div class="settings-intro"><span class="summary-icon">${icon("settings")}</span><div><strong>Your review setup</strong><p>Shared agents. Your rules.</p></div></div>
    <div class="settings-list">
      ${settingsRow("Accounts", "GitHub and Copilot, kept separate", `${state.accounts.filter((account) => account.connected).length} connected`, "people", "accounts")}
      ${settingsRow("Agents", "Reusable review configurations", state.agents.length, "crosshair", "agents")}
      ${settingsRow("Repositories", "Assignments, people, and schedules", state.repos.length, "folder", "repos")}
      ${settingsRow("Doctrines", "The principles behind each review", state.doctrines.length, "book", "doctrines")}
    </div>
    <div class="settings-list">${settingsRow("Concurrent reviews", "A limit on running work, not saved agents", state.limit, "activity", "capacity")}${settingsRow("Preferences", "Automation, notifications, startup", "", "settings", "preferences")}${settingsRow("Advanced", "Local demo data and reset", "", "terminal", "advanced")}</div>
    <p class="section-note">Changes stay in this browser. No real accounts, credentials, or application settings are read or changed.</p>
  </div>`;
}

function collectionPage(kind) {
  const configuration = {
    agents: [
      "Agents",
      "New agent",
      "agent",
      "Unlimited configurations. Concurrency is set separately.",
    ],
    repos: [
      "Repositories",
      "Add repository",
      "repo",
      "Each repository has its own acting GitHub account and review assignments.",
    ],
    doctrines: [
      "Doctrines",
      "New doctrine",
      "doctrine",
      "Reusable principles, shared across agents. Plain text, never commands.",
    ],
  };
  const [, button, editor, hint] = configuration[kind];
  return `<div class="page-scroll" data-scroll="${kind}"><div class="section-toolbar"><p>${hint}</p><button class="small-primary" data-page="${editor}">${icon("plus")}${button}</button></div><div class="configuration-list">
    ${
      state[kind]
        .map((item) => {
          const detail =
            kind === "agents"
              ? `${item.model} / ${item.completion === "approve" ? "May approve" : "Human handoff"}`
              : kind === "repos"
                ? `${item.enabled ? "Monitoring enabled" : "Disabled"} / ${item.agentIds.length} assigned`
                : `${item.body.trim().split(/\s+/).length} words`;
          return `<button class="configuration-card" data-page="${editor}" data-id="${esc(item.id)}"><span class="row-symbol">${icon(kind === "agents" ? "crosshair" : kind === "repos" ? "folder" : "book")}</span><span class="row-copy"><strong>${esc(item.name)}</strong><small>${esc(detail)}</small>${kind === "agents" ? `<span class="small-description">${esc(item.prompt)}</span>` : ""}</span>${icon("chevron")}</button>`;
        })
        .join("") ||
      empty(`No ${kind} yet`, `Use "${button}" to create your first one.`)
    }
    </div></div>`;
}

function field(label, control, hint = "") {
  return `<label class="form-field"><span>${label}</span>${control}${hint ? `<small>${hint}</small>` : ""}</label>`;
}
const input = (name, value = "", required = true) =>
  `<input name="${name}" value="${esc(value)}" ${required ? "required" : ""} autocomplete="off" />`;
const option = (value, label, current) =>
  `<option value="${esc(value)}" ${selected(value, current)}>${esc(label)}</option>`;
function accountSelect(kind, current) {
  return `<select name="accountId" required>${option("", "Choose an account", current)}${state.accounts
    .filter((account) => account.kind === kind)
    .map((account) =>
      option(
        account.id,
        `${account.name}${account.connected ? "" : " (disconnected)"}`,
        current,
      ),
    )
    .join("")}</select>`;
}
function editorFooter(kind, id) {
  return `<div class="editor-actions"><button class="primary-action" type="submit">Save ${kind}</button><button class="secondary-action" type="button" data-action="back">Cancel</button></div>${id ? `<button class="text-action danger" type="button" data-action="delete-${kind}" data-id="${esc(id)}">Delete ${kind}</button>` : ""}<p class="section-note">Sample text only. Save writes to this browser, not PR Sniper.</p>`;
}
function agentEditor(id) {
  const agent = state.agents.find((item) => item.id === id) ?? {
    name: "",
    accountId: "",
    model: "",
    prompt: "",
    doctrineIds: [],
    completion: "human",
    comment: false,
  };
  return `<div class="page-scroll" data-scroll="agent-editor"><form data-form="agent" data-id="${esc(id ?? "")}" class="editor-form">
    ${field("Agent name", input("name", agent.name))}
    <div class="form-group"><h2>Intelligence</h2>${field("Copilot AI account", accountSelect("copilot", agent.accountId), "Separate from the GitHub account that publishes comments or approvals.")}
    ${field("Model", `<select name="model" required>${option("", "Choose a demo model", agent.model)}${Demo.models.map((model) => option(model, model, agent.model)).join("")}</select>`, "Synthetic model choices; no subscription or model discovery is performed.")}</div>
    ${field("Review prompt", `<textarea name="prompt" required rows="4">${esc(agent.prompt)}</textarea>`)}
    <fieldset class="check-group"><legend>Doctrines</legend>${state.doctrines.map((doctrine) => `<label><input type="checkbox" name="doctrineIds" value="${esc(doctrine.id)}" ${checked(agent.doctrineIds.includes(doctrine.id))} /><span>${esc(doctrine.name)}</span></label>`).join("") || "<p>No doctrines yet. Add one in Settings.</p>"}</fieldset>
    <div class="form-group"><h2>Permissions</h2>${field("When a review clears", `<select name="completion">${option("human", "Send to human queue", agent.completion)}${option("approve", "Approve on GitHub (simulated)", agent.completion)}</select>`, "Provider approval is distinct from machine sign-off. Neither option merges a PR.")}
    <label class="checkbox-field"><input name="comment" type="checkbox" ${checked(agent.comment)} /><span>May publish review comments</span></label><p class="section-note">Repository publication gates still apply. Approval uses the repository's acting identity, not the AI account.</p></div>
    ${id ? '<p class="inline-warning">Saving a changed agent restarts its active mock reviews with the new configuration.</p>' : ""}
    ${editorFooter("agent", id)}</form></div>`;
}

function repoEditor(id) {
  const repo = state.repos.find((item) => item.id === id) ?? {
    name: "",
    accountId: "",
    enabled: true,
    scope: false,
    agentIds: [],
    watched: [],
    schedule: "interval",
    minutes: 5,
    cron: "*/15 * * * *",
    zone: "America/New_York",
    start: "inherit",
    comments: "inherit",
  };
  return `<div class="page-scroll" data-scroll="repo-editor"><form data-form="repo" data-id="${esc(id ?? "")}" class="editor-form">
    ${field("Repository", input("name", repo.name), "Use a fictional owner/repository; no provider lookup occurs.")}
    ${field("Acting GitHub account", accountSelect("github", repo.accountId))}
    <label class="checkbox-field"><input type="checkbox" name="enabled" ${checked(repo.enabled)} /><span>Enable repository monitoring</span></label>
    <label class="checkbox-field"><input type="checkbox" name="scope" ${checked(repo.scope)} /><span>Confirm new-PR-only scope (demo)</span></label><p class="section-note">This mock does not import existing PRs. Scope activation is not trust or permission to start a review.</p>
    <fieldset class="check-group"><legend>Assigned agents</legend>${state.agents.map((agent) => `<label><input type="checkbox" name="agentIds" value="${esc(agent.id)}" ${checked(repo.agentIds.includes(agent.id))} /><span>${esc(agent.name)}<small>${agent.completion === "approve" ? "May approve" : "Human handoff"}</small></span></label>`).join("")}</fieldset>
    <details class="check-group"><summary>People you watch <span class="count-badge">${repo.watched.length}</span></summary>${Demo.people.map((person) => `<label><input type="checkbox" name="watched" value="${esc(person)}" ${checked(repo.watched.includes(person))} /><span>${esc(person)}</span></label>`).join("")}<p class="section-note">Empty means all authors after activation, not blanket trust. Untrusted sample revisions require confirmation.</p></details>
    <div class="form-group"><h2>Detection schedule</h2>${field("Schedule type", `<select name="schedule">${option("interval", "Fixed interval", repo.schedule)}${option("cron", "Cron (advanced)", repo.schedule)}</select>`)}
      <div data-schedule="interval" ${repo.schedule !== "interval" ? "hidden" : ""}>${field("Check every (minutes)", `<input name="minutes" type="number" min="1" step="1" value="${repo.minutes}" required />`)}</div>
      <div data-schedule="cron" ${repo.schedule !== "cron" ? "hidden" : ""}>${field("Five-field cron", input("cron", repo.cron))}${field("Time zone", input("zone", repo.zone))}<p class="section-note">Shape and time-zone validation only. No real timer or cron execution.</p></div>
    </div>
    <div class="form-group"><h2>Automation overrides</h2>${["start", "comments"].map((key) => field(key === "start" ? "Start reviews automatically" : "Publish comments automatically", `<select name="${key}">${option("inherit", `Use global default (${state.preferences[key === "start" ? "automaticStart" : "automaticComments"] ? "on" : "off"})`, repo[key])}${option("on", "On for this repository", repo[key])}${option("off", "Off for this repository", repo[key])}</select>`)).join("")}</div>
    ${id ? '<p class="inline-warning">Changed assignments do not silently reassign existing packets. Active reviews restart; unassigned packets stay visibly blocked.</p>' : ""}
    ${editorFooter("repo", id)}</form></div>`;
}

function doctrineEditor(id) {
  const item = state.doctrines.find((doctrine) => doctrine.id === id) ?? {
    name: "",
    body: "",
  };
  return `<div class="page-scroll" data-scroll="doctrine-editor"><form data-form="doctrine" data-id="${esc(id ?? "")}" class="editor-form">${field("Doctrine name", input("name", item.name))}${field("Review principles", `<textarea name="body" rows="10" required>${esc(item.body)}</textarea>`, "Plain text, shared by every agent using this doctrine. No commands or secrets.")}${editorFooter("doctrine", id)}</form></div>`;
}

function accountsPage() {
  return `<div class="page-scroll" data-scroll="accounts"><div class="section-toolbar"><p>Keep repository and AI identities separate. Demo connections only.</p><button class="small-primary" data-page="add-account">${icon("plus")}Add account</button></div>
    ${["github", "copilot"]
      .filter((kind) => state.accounts.some((account) => account.kind === kind))
      .map(
        (kind) =>
          `<section class="detail-section"><h2>${kind === "github" ? "GitHub / repository access" : "Copilot / AI access"}</h2>${state.accounts
            .filter((account) => account.kind === kind)
            .map(
              (account) =>
                `<div class="account-row"><span class="avatar">${esc(initials(account.name.replaceAll("-", " ")))}</span><div><strong>${esc(account.name)}</strong><small>${account.connected ? "Connected (demo)" : "Disconnected / dependent work blocked"}</small></div><button class="quiet-action" data-action="account" data-id="${account.id}">${account.connected ? "Disconnect" : "Reconnect"}</button></div>`,
            )
            .join("")}</section>`,
      )
      .join("")}
    ${state.accounts.length ? "" : empty("No accounts yet", "Add your repository and AI identities to get started.", "people")}
    <section class="detail-section upcoming-connections"><h2>Coming soon</h2><div class="upcoming-tags">${connectionProviders
      .filter((provider) => !provider.available)
      .map((provider) => `<span>${provider.name}</span>`)
      .join("")}</div></section>
    <p class="section-note">Mock sign-in requests no credentials. Sign-in never guarantees model access, and AI credentials never act on GitHub.</p></div>`;
}

function addAccountPage() {
  const group =
    route().id === "ai"
      ? "AI providers"
      : route().id === "code"
        ? "Repositories"
        : null;
  return `<div class="page-scroll" data-scroll="add-account"><div class="settings-intro"><span class="summary-icon">${icon("people")}</span><div><strong>${group === "AI providers" ? "Choose your AI provider" : group === "Repositories" ? "Choose your PR platform" : "Choose a connection"}</strong><p>One account, one explicit role.</p></div></div>
    ${(group ? [group] : ["Repositories", "AI providers"])
      .map(
        (group) =>
          `<section class="provider-group"><h2>${group}</h2><div class="provider-list">${connectionProviders
            .filter((provider) => provider.group === group)
            .map(
              (provider) =>
                `<button class="provider-option" type="button" data-provider="${provider.id}" ${provider.available ? `data-page="account-signin" data-id="${provider.id}"` : 'disabled aria-disabled="true"'} aria-label="${provider.name}${provider.available ? ", add demo account" : ", coming soon"}"><span class="provider-mark" data-brand="${provider.id}">${provider.mark}</span><span class="row-copy"><strong>${provider.name}</strong><small>${provider.description}</small></span>${provider.available ? icon("chevron") : '<span class="coming-soon">Coming soon</span>'}</button>`,
            )
            .join("")}</div></section>`,
      )
      .join("")}
    <p class="section-note">${group === "AI providers" ? "Copilot is the first available AI integration, not a preselected provider. Direct integrations are distinct from models offered through Copilot." : group === "Repositories" ? "GitHub is available in this mock. Azure DevOps is coming soon." : "Only GitHub and Copilot can be added in this mock. Direct AI integrations are distinct from models offered through Copilot."}</p>
    <button class="quiet-action wide" type="button" data-action="cancel-account">Cancel</button></div>`;
}

function accountSignInPage(kind) {
  const provider = connectionProviders.find(
    (item) => item.id === kind && item.available,
  );
  if (!provider)
    return empty(
      "Connection unavailable",
      "Choose GitHub or Copilot. Other providers are coming soon.",
      "people",
    );
  return `<div class="page-scroll" data-scroll="account-signin"><div class="connection-heading"><span class="provider-mark" data-brand="${provider.id}">${provider.mark}</span><div><h2>Connect ${provider.name}</h2><p>${provider.description}</p></div></div>
    <form data-form="account-signin" data-id="${provider.id}" class="editor-form">
      <p class="demo-signin-notice">${icon("compass")}Simulated browser sign-in</p>
      <p class="section-note">Production would open sign-in in your browser. This demo stays here and never requests credentials.</p>
      ${field("Fictional account handle", `<input name="accountName" value="${esc(route().draftName ?? "")}" placeholder="e.g. dev-demo" required maxlength="39" pattern="[a-zA-Z0-9]+(-[a-zA-Z0-9]+)*" autocomplete="off" spellcheck="false" />`, "Letters, numbers, and single hyphens only. Do not enter a real password or token.")}
      ${provider.id === "github" ? '<p class="connection-scope">Repository identity only. A real GitHub connection requests broad repository access; this mock requests none. Repositories and permissions are configured separately.</p>' : '<p class="connection-scope">AI identity only. Connecting Copilot does not connect a repository, verify a subscription, or grant permission to publish.</p>'}
      <button class="primary-action wide" type="submit">Simulate browser sign-in</button>
      <button class="quiet-action wide" type="button" data-action="cancel-account">Cancel</button>
      <details class="scenario-details"><summary>Prototype scenarios</summary><button class="text-action" type="button" data-action="account-signin-failed">Simulate sign-in failure</button></details>
    </form></div>`;
}

function accountConfirmPage(kind) {
  const provider = connectionProviders.find(
    (item) => item.id === kind && item.available,
  );
  const name = route().pendingName;
  if (!provider || !name)
    return empty(
      "No pending identity",
      "Go back and simulate sign-in before connecting an account.",
      "people",
    );
  return `<div class="page-scroll" data-scroll="account-confirm"><section class="confirmation-card"><span class="state-pill">${icon("check")}Mock identity returned</span><span class="confirmation-avatar">${esc(initials(name.replaceAll("-", " ")))}</span><h2>@${esc(name)}</h2><p>${provider.name}</p><small>${provider.description}</small></section>
    <form data-form="account-confirm" class="editor-form">
      <h2 class="form-title">Is this the account you want?</h2>
      <p class="section-note">Nothing is saved until you confirm. This adds a local demo connection, not a real provider account.</p>
      <label class="checkbox-field"><input type="checkbox" name="confirmIdentity" required /><span>Use <strong>@${esc(name)}</strong> as this ${kind === "github" ? "GitHub repository" : "Copilot AI"} identity.</span></label>
      <button class="primary-action wide" type="submit" disabled>Confirm and connect</button>
      <button class="secondary-action wide" type="button" data-action="back">Use a different identity</button>
      <button class="text-action wide" type="button" data-action="cancel-account">Cancel connection</button>
      <p class="section-note">No repositories or agents will be assigned automatically. No reviews, comments, or approvals will run.</p>
    </form></div>`;
}

function returnToAccounts() {
  dirty = false;
  const index = stacks[tab].findLastIndex((item) =>
    ["accounts", "genie", "welcome"].includes(item.page),
  );
  stacks[tab] =
    index >= 0
      ? stacks[tab].slice(0, index + 1)
      : [{ page: "settings" }, { page: "accounts" }];
  render(-1, true);
}

function capacityPage() {
  return `<div class="page-scroll" data-scroll="capacity"><div class="feature-summary"><span class="summary-icon">${icon("activity")}</span><div><strong>${state.limit}</strong><p>maximum simultaneous reviews</p></div></div>
    <form data-form="capacity" class="editor-form"><p class="section-note">Save as many agents as you like. This controls how many review packets can run at once across all repositories.</p>${field("Concurrent reviews", `<input name="limit" type="number" min="1" step="1" value="${state.limit}" required />`)}
    <p class="inline-warning">Lowering the limit stops surplus sample reviews, discards their partial work, and puts them back in the queue.</p><button class="primary-action wide" type="submit">Save review limit</button></form></div>`;
}

function preferencesPage() {
  const labels = {
    automaticStart: [
      "Start eligible reviews automatically",
      "Trust and repository scope still apply. Off means explicit start.",
    ],
    automaticComments: [
      "Publish comments automatically",
      "Separate from starting reviews; agents also need comment permission.",
    ],
    notifications: [
      "Notify me when attention is needed",
      "A saved preference only. This prototype sends no system notifications.",
    ],
    launchAtLogin: [
      "Open PR Sniper at login",
      "A saved preference only. No operating-system login item is changed.",
    ],
  };
  return `<div class="page-scroll" data-scroll="preferences"><form data-form="preferences" class="editor-form"><div class="form-group">${Object.entries(
    labels,
  )
    .map(
      ([key, [label, description]]) =>
        `<label class="preference-row"><span><strong>${label}</strong><small>${description}</small></span><input type="checkbox" name="${key}" ${checked(state.preferences[key])} /></label>`,
    )
    .join(
      "",
    )}</div><button class="primary-action wide" type="submit">Save preferences</button></form></div>`;
}

function advancedPage() {
  return `<div class="page-scroll" data-scroll="advanced"><section class="detail-section"><h2>Local prototype data</h2><p>Saved agents, assignments, review outcomes, and pause state stay in this browser under one prototype-only key.</p><dl class="context-list"><dt>Storage</dt><dd>${storageBlocked ? "Unavailable / reset required" : "Browser localStorage"}</dd><dt>Schema</dt><dd>v${state.version}</dd><dt>Review packets</dt><dd>${state.reviews.length}</dd><dt>Network operations</dt><dd>None</dd><dt>Production access</dt><dd>None</dd></dl></section><section class="detail-section"><h2>Start fresh</h2><p>Reset only this prototype's sample data. Your real repositories, accounts, and PR Sniper settings are never touched.</p><button class="danger-action wide" data-action="reset">Reset prototype</button></section><p class="section-note">Mocks do not prove native tray behavior, authentication, review quality, provider approval, or Windows support.</p></div>`;
}

function render(direction = 0, focus = false) {
  const current = route();
  const titles = {
    queue: "Your queue",
    running: "On the case",
    reviewed: "Reviewed",
    settings: "Settings",
    review: "Review",
    agents: "Agents",
    agent: "Agent",
    repos: "Repositories",
    repo: "Repository",
    doctrines: "Doctrines",
    doctrine: "Doctrine",
    accounts: "Accounts",
    "add-account": "Add account",
    "account-signin": "Sign in",
    "account-confirm": "Confirm",
    capacity: "Capacity",
    preferences: "Preferences",
    advanced: "Advanced",
    welcome: "Welcome",
    genie: "Genie",
    "setup-review": "Review setup",
  };
  pageTitle.textContent = titles[current.page] ?? "PR Sniper";
  let html;
  switch (current.page) {
    case "welcome":
      html = welcomePage();
      break;
    case "genie":
      html = geniePage();
      break;
    case "setup-review":
      html = setupReviewPage();
      break;
    case "queue":
      html = queuePage();
      break;
    case "running":
      html = runningPage();
      break;
    case "reviewed":
      html = reviewedPage();
      break;
    case "settings":
      html = settingsPage();
      break;
    case "agents":
    case "repos":
    case "doctrines":
      html = collectionPage(current.page);
      break;
    case "agent":
      html = agentEditor(current.id);
      break;
    case "repo":
      html = repoEditor(current.id);
      break;
    case "doctrine":
      html = doctrineEditor(current.id);
      break;
    case "accounts":
      html = accountsPage();
      break;
    case "add-account":
      html = addAccountPage();
      break;
    case "account-signin":
      html = accountSignInPage(current.id);
      break;
    case "account-confirm":
      html = accountConfirmPage(current.id);
      break;
    case "capacity":
      html = capacityPage();
      break;
    case "preferences":
      html = preferencesPage();
      break;
    case "advanced":
      html = advancedPage();
      break;
    case "review": {
      const review = state.reviews.find((item) => item.id === current.id);
      html = review
        ? detailPage(review)
        : empty(
            "Review unavailable",
            "This exact item is no longer in the prototype. Go back to choose another.",
          );
      break;
    }
    default:
      html = empty(
        "Page unavailable",
        "Use the bottom navigation to return to the prototype.",
      );
  }
  const previous = stage.lastElementChild;
  const next = document.createElement("div");
  next.className = `app-view ${current.page === "queue" ? "queue-view" : ""}`;
  next.dataset.page = current.page;
  next.innerHTML = html;
  if (previous) {
    previous.inert = true;
    previous.setAttribute("aria-hidden", "true");
  }
  stage.replaceChildren(...(previous ? [previous] : []), next);
  syncChrome();
  for (const [key, scrollTop] of current.scrolls ?? []) {
    const element = [...next.querySelectorAll("[data-scroll]")].find(
      (item) => item.dataset.scroll === key,
    );
    if (element) element.scrollTop = scrollTop;
  }
  const moving =
    direction && !matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (moving && previous) {
    const options = { duration: 220, easing: "cubic-bezier(.22,.7,.25,1)" };
    previous.animate(
      [
        { transform: "translateX(0)", opacity: 1 },
        { transform: `translateX(${-direction * 28}%)`, opacity: 0 },
      ],
      options,
    );
    const arrival = next.animate(
      [
        { transform: `translateX(${direction * 100}%)` },
        { transform: "translateX(0)" },
      ],
      options,
    );
    arrival.onfinish = () => previous.remove();
  } else previous?.remove();
  if (focus) {
    const returnCard =
      direction < 0 &&
      [...next.querySelectorAll("[data-review-id]")].find(
        (element) => element.dataset.reviewId === current.returnReview,
      );
    (returnCard || pageTitle).focus({ preventScroll: true });
  }
}

function selectLane(nextLane, focus = false) {
  lane = nextLane;
  popover.dataset.lane = lane;
  stage.querySelectorAll('[role="tab"]').forEach((button) => {
    const active = button.dataset.lane === lane;
    button.setAttribute("aria-selected", String(active));
    button.tabIndex = active ? 0 : -1;
    if (active && focus) button.focus({ preventScroll: true });
  });
  stage.querySelectorAll('[role="tabpanel"]').forEach((panel) => {
    const active = panel.id === `${lane}-panel`;
    panel.inert = !active;
    panel.setAttribute("aria-hidden", String(!active));
  });
  announcement.textContent =
    lane === "human"
      ? `${human().length} items need human attention.`
      : `${queued().length} review packets waiting. ${running().length} of ${state.limit} slots in use.`;
}

function readForm(form) {
  const data = new FormData(form);
  const value = (name) => String(data.get(name) ?? "").trim();
  const bool = (name) => data.has(name);
  const all = (name) => data.getAll(name).map(String);
  const id = form.dataset.id || undefined;
  switch (form.dataset.form) {
    case "agent":
      return [
        "save-agent",
        {
          agent: {
            id,
            name: value("name"),
            accountId: value("accountId"),
            model: value("model"),
            prompt: value("prompt"),
            doctrineIds: all("doctrineIds"),
            completion: value("completion"),
            comment: bool("comment"),
          },
        },
      ];
    case "repo":
      return [
        "save-repo",
        {
          repo: {
            id,
            name: value("name"),
            accountId: value("accountId"),
            enabled: bool("enabled"),
            scope: bool("scope"),
            agentIds: all("agentIds"),
            watched: all("watched"),
            schedule: value("schedule"),
            minutes: Number(value("minutes")),
            cron: value("cron"),
            zone: value("zone"),
            start: value("start"),
            comments: value("comments"),
          },
        },
      ];
    case "doctrine":
      return [
        "save-doctrine",
        { doctrine: { id, name: value("name"), body: value("body") } },
      ];
    case "capacity":
      return ["capacity", { limit: Number(value("limit")) }];
    case "preferences":
      return [
        "preferences",
        {
          preferences: Object.fromEntries(
            Object.keys(state.preferences).map((key) => [key, bool(key)]),
          ),
        },
      ];
    case "complete":
      return ["finish", { reviewId: id, outcome: value("outcome") }];
    default:
      throw new Error("Unknown prototype form.");
  }
}

stage.addEventListener("submit", (event) => {
  event.preventDefault();
  const form = event.target;
  if (!(form instanceof HTMLFormElement)) return;
  try {
    if (form.dataset.form === "setup-finish") {
      if (
        perform(
          "finish-onboarding",
          { confirmed: new FormData(form).has("confirmSetup") },
          "You're set up. Your queue will fill after an eligible repository check.",
          false,
        )
      ) {
        dirty = false;
        tab = "queue";
        stacks.queue = [{ page: "queue" }];
        render(1, true);
      }
      return;
    }
    if (form.dataset.form === "account-signin") {
      const kind = form.dataset.id;
      if (
        !connectionProviders.some(
          (provider) => provider.id === kind && provider.available,
        )
      )
        throw new Error("That provider is coming soon.");
      const name = Demo.accountName(new FormData(form).get("accountName"));
      route().draftName = name;
      remember();
      stacks[tab].push({
        page: "account-confirm",
        id: kind,
        pendingName: name,
      });
      if (!storageBlocked) notice.hidden = true;
      render(1, true);
      return;
    }
    if (form.dataset.form === "account-confirm") {
      const current = route();
      if (
        perform(
          "add-account",
          {
            kind: current.id,
            name: current.pendingName,
            confirmed: new FormData(form).has("confirmIdentity"),
          },
          `Connected @${current.pendingName} in the demo. Choose assignments separately.`,
          false,
        )
      )
        returnToAccounts();
      return;
    }
    const [action, payload] = readForm(form);
    const editor = [
      "agent",
      "repo",
      "doctrine",
      "capacity",
      "preferences",
    ].includes(form.dataset.form);
    if (
      perform(
        action,
        payload,
        editor
          ? "Saved in this browser."
          : "Sample review updated. Nothing was sent to GitHub.",
        false,
      )
    ) {
      dirty = false;
      if (editor) goBack();
      else render();
    }
  } catch (error) {
    showError(error.message);
  }
});
stage.addEventListener("input", (event) => {
  const form = event.target.closest("form");
  if (form?.dataset.form === "account-signin") {
    route().draftName = event.target.value;
    return;
  }
  if (form?.dataset.form === "account-confirm") return;
  if (form && form.dataset.form !== "complete") dirty = true;
});
stage.addEventListener("change", (event) => {
  if (event.target.name === "confirmSetup") {
    event.target
      .closest("form")
      .querySelector('button[type="submit"]').disabled =
      !event.target.checked || !Demo.setupStatus(state).ready;
  }
  if (event.target.name === "confirmIdentity") {
    event.target
      .closest("form")
      .querySelector('button[type="submit"]').disabled = !event.target.checked;
  }
  if (event.target.name === "schedule") {
    const form = event.target.closest("form");
    form.querySelectorAll("[data-schedule]").forEach((element) => {
      element.hidden = element.dataset.schedule !== event.target.value;
    });
  }
});

popover.addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button || button.disabled) return;
  if (button.dataset.nav) {
    switchTab(button.dataset.nav);
    return;
  }
  if (button.dataset.lane) {
    selectLane(button.dataset.lane);
    return;
  }
  if (button.dataset.filter) {
    remember();
    historyFilter = button.dataset.filter;
    render();
    return;
  }
  if (button.dataset.page) {
    navigate(button.dataset.page, button.dataset.id);
    return;
  }
  const action = button.dataset.action;
  const id = button.dataset.id;
  if (!action) return;
  if (action === "review") {
    navigate("review", button.dataset.reviewId);
    return;
  }
  if (action === "back") {
    goBack();
    return;
  }
  if (action === "setup-step") {
    openSetupStep(button.dataset.step);
    return;
  }
  if (action === "cancel-account") {
    if (!storageBlocked) notice.hidden = true;
    returnToAccounts();
    return;
  }
  if (action === "account-signin-failed") {
    showError(
      "Simulated sign-in failed. No identity was saved. Try the mock sign-in again or cancel.",
    );
    return;
  }
  if (action === "reset") {
    if (
      !confirm(
        "Reset all prototype-only settings and sample reviews in this browser? Real PR Sniper data is not affected.",
      )
    )
      return;
    loadScenario(state.scenario ?? "configured");
    return;
  }
  if (action.startsWith("delete-")) {
    if (
      !confirm(
        "Delete this shared prototype configuration? Existing history will be kept.",
      )
    )
      return;
    if (perform(action, { id }, "Prototype configuration deleted.", false)) {
      dirty = false;
      goBack();
    }
    return;
  }
  if (action === "account") {
    perform(
      "account",
      { id },
      "Demo connection updated; dependent work was reconciled.",
    );
    return;
  }
  const messages = {
    detect: "Sample detection added packets; available slots filled.",
    advance: "Sample review activity advanced.",
    interrupt:
      "Review stopped. Partial work discarded; explicit retry required.",
    retry: "Queued for a fresh review; saved earlier evidence is retained.",
    start: "Review requested. Capacity and eligibility gates still apply.",
    trust: "Trust confirmed for this sample revision.",
    stale:
      "Changed-revision scenario applied; earlier evidence is marked stale.",
  };
  perform(action, { reviewId: id }, messages[action] ?? "Prototype updated.");
});

monitoringToggle.addEventListener("click", () => {
  // Keep an editor's unsaved fields intact while the global stop remains usable.
  const stopping = state.monitoring;
  const count = running().length;
  if (
    perform(
      "monitor",
      {},
      stopping
        ? `Paused everywhere. ${count} active reviews discarded and requeued.`
        : "Monitoring resumed. Eligible reviews start fresh.",
      !dirty,
    )
  )
    syncChrome();
});
backButton.addEventListener("click", goBack);

function positionPopover() {
  const anchor = tray.getBoundingClientRect();
  const width = popover.getBoundingClientRect().width;
  const left = Math.max(
    12,
    Math.min(anchor.right - width + 24, window.innerWidth - width - 12),
  );
  popover.style.left = `${left}px`;
  popover.style.right = "auto";
  popover.style.setProperty(
    "--anchor-x",
    `${anchor.left + anchor.width / 2 - left}px`,
  );
}
function openPopover() {
  popover.hidden = false;
  tray.setAttribute("aria-expanded", "true");
  tray.setAttribute("aria-label", "Close PR Sniper");
  positionPopover();
  popover.classList.remove("is-opening");
  void popover.offsetWidth;
  popover.classList.add("is-opening");
  (
    stage.querySelector('[role="tab"][aria-selected="true"]') || pageTitle
  ).focus({ preventScroll: true });
}
function closePopover() {
  remember();
  popover.hidden = true;
  tray.setAttribute("aria-expanded", "false");
  tray.setAttribute("aria-label", "Open PR Sniper");
  tray.focus({ preventScroll: true });
}
tray.addEventListener("click", () =>
  popover.hidden ? openPopover() : closePopover(),
);
document.querySelectorAll("[data-scenario]").forEach((button) => {
  button.addEventListener("click", () => loadScenario(button.dataset.scenario));
});
document
  .querySelector("#close-popover")
  .addEventListener("click", closePopover);
document.addEventListener("pointerdown", (event) => {
  if (
    !popover.hidden &&
    !popover.contains(event.target) &&
    !tray.contains(event.target)
  )
    closePopover();
});
document.addEventListener("keydown", (event) => {
  if (popover.hidden) return;
  if (
    event.target.matches('[role="tab"]') &&
    ["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)
  ) {
    event.preventDefault();
    selectLane(
      event.key === "Home"
        ? "human"
        : event.key === "End"
          ? "agent"
          : lane === "human"
            ? "agent"
            : "human",
      true,
    );
    return;
  }
  if (event.key === "Escape") {
    event.preventDefault();
    closePopover();
    return;
  }
  if (event.key !== "Tab") return;
  const focusable = [
    ...popover.querySelectorAll(
      'button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary, [tabindex="0"]',
    ),
  ].filter(
    (element) =>
      element.tabIndex >= 0 &&
      !element.closest("[inert], [hidden]") &&
      element.getClientRects().length,
  );
  const first = focusable[0];
  const last = focusable.at(-1);
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
});
window.addEventListener("resize", () => {
  if (!popover.hidden) positionPopover();
});
window.addEventListener("storage", (event) => {
  if (event.key === storageKey)
    showError(
      "Prototype data changed in another tab. Reload before saving to avoid overwriting it.",
    );
});
function updateClock() {
  const now = new Date();
  const clock = document.querySelector("#desktop-clock");
  clock.dateTime = now.toISOString();
  clock.textContent = new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  }).format(now);
}
updateClock();
setInterval(updateClock, 30_000);
render();
