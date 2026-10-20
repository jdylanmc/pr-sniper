const Demo = globalThis.SniperDemo;
const storageKey = "pr-sniper.vnext.v2";
const tray = document.querySelector("#tray-trigger");
const popover = document.querySelector("#sniper-popover");
const stage = document.querySelector("#view-stage");
const announcement = document.querySelector("#lane-announcement");
const monitoringToggle = document.querySelector("#monitoring-toggle");
const notice = document.querySelector("#app-notice");
const noticeText = document.querySelector("#app-notice-text");
const genieButton = document.querySelector("#genie-button");
const pageTitle = document.querySelector("#page-title");
const backButton = document.querySelector("#back-button");
const toast = document.querySelector("#toast");
const themeKey = "pr-sniper.vnext.theme";
const themes = { light: "Light", dark: "Dark", os: "OS" };
const systemDark = matchMedia("(prefers-color-scheme: dark)");
let theme = (() => {
  const requested = new URLSearchParams(location.search).get("theme");
  if (requested in themes) return requested;
  try {
    const saved = localStorage.getItem(themeKey);
    if (saved in themes) return saved;
  } catch {
    // Blocked storage keeps the Light default.
  }
  return "light";
})();
function applyTheme() {
  document.documentElement.dataset.theme =
    theme === "dark" || (theme === "os" && systemDark.matches)
      ? "dark"
      : "light";
}
systemDark.addEventListener("change", applyTheme);
applyTheme();
let state = Demo.seed();
let savedRaw = null;
let storageBlocked = false;
let tab = "queue";
let queueFilter;
let historyFilter = "all";
// Reviewed rows drawn; grows by 50 per Show more.
let reviewedLimit = 50;
let dirty = false;
let toastTimer;
let noticeTimer;
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
        review.state === "queued" && repoFor(review)?.enabled && !review.nextId,
    )
    .sort(
      (a, b) => Number(b.restart) - Number(a.restart) || a.number - b.number,
    );
const human = () =>
  state.reviews.filter(
    (review) =>
      ["human", "question"].includes(review.state) ||
      (review.state === "stale" && review.dismissedHead !== review.head) ||
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
  noticeText.textContent = message;
  notice.hidden = false;
  announcement.textContent = message;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => {
    notice.hidden = true;
  }, 8000);
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
  if (savedRaw !== null)
    state = Demo.validate(Demo.migrate(JSON.parse(savedRaw)));
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
  if (focused?.dataset?.focusKey)
    route().returnFocus = focused.dataset.focusKey;
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

function navigate(page, id, keepEdits = false) {
  if (!keepEdits && !mayLeave()) return;
  if (keepEdits) {
    // The draft travels with this route; the nested editor starts clean.
    route().draftDirty = dirty;
    dirty = false;
  }
  remember();
  stacks[tab].push({ page, id });
  render(1, true);
}

function goBack() {
  const picker = stage.lastElementChild?.querySelector(
    '.agent-form:is([data-view="doctrines"], [data-view="voices"])',
  );
  if (picker) {
    setAgentView(picker, "");
    return;
  }
  const repoForm = stage.lastElementChild?.querySelector(
    '[data-form="repo"][data-view="admission"]',
  );
  if (repoForm) {
    popAdmission(repoForm);
    return;
  }
  if (stacks[tab].length === 1 || !mayLeave()) return;
  remember();
  stacks[tab].pop();
  if (route().draftDirty) dirty = true;
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
  genieButton.hidden = !(tab === "settings" && stacks[tab].length === 1);
}

function loadScenario(scenario) {
  try {
    if (!["fresh", "configured"].includes(scenario))
      throw new Error("Unknown desktop demo.");
    save(scenario === "fresh" ? Demo.fresh() : Demo.seed(), true);
    dirty = false;
    tab = "queue";
    queueFilter = undefined;
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
    notice.hidden = true;
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
      navigate(step === "ai" ? "copilot" : "github");
    else navigate("add-account", step);
  } else if (step === "agent") {
    if (state.agents.length === 1) navigate("agent", state.agents[0].id);
    else navigate(state.agents.length ? "agents" : "agent");
  } else {
    if (state.repos.length === 1) navigate("repo", state.repos[0].id);
    else navigate("repos");
  }
}

function setupReviewPage() {
  const status = Demo.setupStatus(state);
  return `<div class="page-scroll" data-scroll="setup-review"><section class="detail-hero"><span class="state-pill">${icon("spark")}Genie's final check</span><h2>Your app. Your call.</h2><p>Starting monitoring does not grant extra review or publication permissions.</p></section>
    ${!status.ready ? '<p class="inline-warning">Setup changed. Complete all four essentials before starting.</p>' : ""}
    ${state.repos
      .map((repo) => {
        const ready = status.repos.some((item) => item.id === repo.id);
        return `<section class="detail-section setup-repository"><h3>${esc(repo.name)}</h3>${!ready ? '<p class="inline-warning">Not ready: disabled, disconnected, unconfirmed scope, or no available assigned agent. This repository will not run.</p>' : ""}<dl class="context-list"><dt>GitHub identity</dt><dd>${esc(state.accounts.find((account) => account.id === repo.accountId)?.name)}</dd><dt>Detection</dt><dd>${repo.schedule === "interval" ? `Every ${repo.minutes} minutes` : `${esc(repo.cron)} / ${esc(repo.zone)}`}</dd><dt>Scope</dt><dd>${repo.scope ? "New PRs only (demo)" : "Not confirmed"}</dd><dt>Pull requests</dt><dd>${repo.admission.mode === "all" ? "All pull requests; untrusted revisions still need confirmation" : `Filtered (${repo.admission.rules.filter((rule) => rule.enabled).length} rules on)`}</dd><dt>Review start</dt><dd>${Demo.effective(state, repo, "start") ? "Automatic when eligible" : "Manual start required"}</dd><dt>Comments</dt><dd>${Demo.effective(state, repo, "comments") ? "Automatic if agent permits" : "Manual publication gate"}</dd></dl>
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

function workRow(review, kind) {
  const blocked = ["queued", "failed"].includes(review.state)
    ? Demo.blockReason(state, review)
    : "";
  const display =
    kind === "reviewed"
      ? statusNames[review.state]
      : review.state === "running"
        ? "Running"
        : review.state === "failed"
          ? "Failed"
          : review.state === "stale"
            ? "Stale"
            : blocked
              ? "Blocked"
              : "Queued";
  const symbol =
    kind === "reviewed"
      ? "reviewed"
      : review.state === "failed"
        ? "failed"
        : blocked || review.state === "stale"
          ? "blocked"
          : "clock";
  const marker =
    review.state === "running" && kind !== "reviewed"
      ? '<span class="work-spin" aria-hidden="true"></span>'
      : `<span class="work-marker" aria-hidden="true">${icon(symbol)}</span>`;
  const agent = review.run?.name ?? agentFor(review)?.name ?? "Unassigned";
  return `<li><button type="button" class="work-row review-open" data-action="review" data-review-id="${esc(review.id)}" data-display="${display.toLowerCase().replaceAll(" ", "-")}" title="${esc(`${review.title}; ${display}${blocked ? `. ${blocked}` : ""}`)}">${marker}<span class="work-copy"><span class="work-heading"><strong>${esc(kind === "reviewed" ? review.title : agent)}</strong><span class="work-state">${esc(display)}</span></span>${kind === "reviewed" ? "" : `<span class="work-subject">${esc(review.title)}</span>`}<span class="work-reference">${esc(review.repoName)} #${review.number}</span></span><span class="work-chevron" aria-hidden="true">${icon("chevron")}</span></button></li>`;
}

function quietEmpty(symbol, label) {
  return `<div class="queue-empty">${icon(symbol)}<span class="sr-only">${esc(label)}</span></div>`;
}

function empty(title, description, symbol = "queue") {
  return `<div class="empty-state">${icon(symbol)}<h2>${esc(title)}</h2><p>${esc(description)}</p></div>`;
}

function queuePage() {
  const attention = human();
  const ready = attention.filter(
    (review) => review.state === "human" && !review.findings.length,
  );
  const waiting = attention.filter((review) => !ready.includes(review));
  const view = queueFilter ?? (ready.length ? "ready" : "attention");
  const items = view === "ready" ? ready : waiting;
  return `<div class="queue-metrics" role="group" aria-label="Human handoffs">
    ${[
      ["ready", "Ready for you", ready.length, "need your eyes", "queue"],
      [
        "attention",
        "Needs attention",
        waiting.length,
        "need your input",
        "question",
      ],
    ]
      .map(
        ([key, label, count, caption, symbol]) =>
          `<button type="button" class="queue-metric" data-queue-filter="${key}" aria-pressed="${view === key}"><span class="queue-metric-label">${label}${icon(symbol)}</span><span class="queue-metric-value"><strong id="${key}-count">${count}</strong><span>${caption}</span></span></button>`,
      )
      .join("")}
  </div>
  <div class="page-scroll queue-scroll" data-scroll="queue" id="human-panel">${
    items.length
      ? `<div class="queue-group" data-queue-group="${view}" id="human-list">${items
          .map(
            (review) =>
              `<div class="queue-entry"><button type="button" class="queue-row review-open" data-action="review" data-review-id="${esc(review.id)}"><span class="queue-row-copy">${pill(review)}<strong class="review-title">${esc(review.title)}</strong>${reference(review)}</span>${icon("chevron")}</button>${review.state === "stale" ? `<button type="button" class="queue-dismiss" data-action="dismiss-stale" data-id="${esc(review.id)}" aria-label="Dismiss stale review" title="Dismiss stale review"><svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="m6 6 12 12M6 18 18 6"/></svg></button>` : ""}</div>`,
          )
          .join("")}</div>`
      : quietEmpty("inbox", "Nothing here")
  }</div>`;
}

function runningPage() {
  const rows = [...running(), ...queued()];
  return `<div class="page-scroll" data-scroll="running">${
    rows.length
      ? `<ol class="work-list" aria-label="Agent work queue">${rows.map((review) => workRow(review, "running")).join("")}</ol>`
      : quietEmpty("activity", "No agent work")
  }
    <button class="quiet-action wide check-demo" data-action="detect" ${disabled(!state.monitoring)}>Simulate a repository check</button>
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
    ${
      items.length
        ? `<ol class="work-list" aria-label="Reviewed work">${items
            .slice(0, reviewedLimit)
            .map((review) => workRow(review, "reviewed"))
            .join("")}</ol>${
            items.length > reviewedLimit
              ? `<button class="quiet-action work-more" data-action="reviewed-more" aria-label="Show ${Math.min(50, items.length - reviewedLimit)} more">+${items.length - reviewedLimit}</button>`
              : ""
          }`
        : quietEmpty("reviewed", "Nothing in this view")
    }
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
    <nav class="settings-list" aria-labelledby="settings-group-general"><h2 class="settings-list-heading" id="settings-group-general">General</h2>
      ${settingsRow("Accounts", "GitHub and Copilot, kept separate", `${state.accounts.filter((account) => account.connected).length} connected`, "people", "accounts")}
      ${settingsRow("Repositories", "Assignments, people, and schedules", state.repos.length, "folder", "repos")}
      ${settingsRow("Preferences", "Automation, notifications, startup", "", "settings", "preferences")}
      ${settingsRow("Advanced", "Local demo data and reset", "", "terminal", "advanced")}
    </nav>
    <nav class="settings-list" aria-labelledby="settings-group-behavior"><h2 class="settings-list-heading" id="settings-group-behavior">Review behavior</h2>
      ${settingsRow("Agents", "Reusable review configurations", state.agents.length, "crosshair", "agents")}
      ${settingsRow("Doctrines", "The principles behind each review", state.doctrines.length, "book", "doctrines")}
      ${settingsRow("Voices", "How agents sound to people", state.voices.length, "spark", "voices")}
    </nav>
    <p class="section-note">Changes stay in this browser. No real accounts, credentials, or application settings are read or changed.</p>
  </div>`;
}

function collectionPage(kind) {
  if (kind === "repos") return repositoriesPage();
  const editor =
    kind === "agents" ? "agent" : kind === "voices" ? "voice" : "doctrine";
  const label = `New ${editor}`;
  const symbol =
    kind === "agents" ? "crosshair" : kind === "voices" ? "spark" : "book";
  const warning = (item) => {
    if (kind !== "agents") return "";
    if (!item.accountId || !item.prompt?.trim()) return "Unconfigured";
    return state.accounts.some(
      (account) => account.id === item.accountId && account.connected,
    )
      ? ""
      : "Reconnect required";
  };
  const cards = state[kind].map((item) => {
    const detail =
      kind === "agents"
        ? `${item.model} / ${countLabel(state.repos.filter((repo) => repo.agentIds.includes(item.id)).length, "repository", "repositories")}`
        : kind === "voices"
          ? `${countLabel(item.body.trim().split(/\s+/).length, "word", "words")} / ${countLabel(state.agents.filter((agent) => agent.voiceId === item.id).length, "agent", "agents")}`
          : `${item.body.trim().split(/\s+/).length} words`;
    const problem = warning(item);
    return `<div class="configuration-card"><button type="button" class="card-open" data-page="${editor}" data-id="${esc(item.id)}"><span class="row-symbol">${icon(symbol)}</span><span class="row-copy"><strong>${esc(item.name)}</strong><small>${esc(detail)}</small>${problem ? `<span class="row-warning">${problem}</span>` : ""}</span></button><button type="button" class="row-trash" data-action="delete-${editor}" data-id="${esc(item.id)}" aria-label="Delete ${esc(item.name)}" title="Delete">${icon("trash")}</button></div>`;
  });
  return `<div class="page-scroll has-fab" data-scroll="${kind}">${cards.length ? `<div class="configuration-list">${cards.join("")}</div>` : quietEmpty(symbol, `No ${kind} yet`)}</div>
    <button type="button" class="floating-pill" data-page="${editor}">${icon("plus")}${label}</button>`;
}

function repositoriesPage() {
  const accounts = state.accounts.filter(
    (account) => account.kind === "github",
  );
  const connected = (repo) =>
    state.accounts.some(
      (account) => account.id === repo.accountId && account.connected,
    );
  const accountName = (repo) =>
    state.accounts.find((item) => item.id === repo.accountId)?.name ??
    state.forgotten.find((item) => item.id === repo.accountId)?.name ??
    "unbound";
  const row = (repo, live) =>
    `<div class="repo-row"><button type="button" class="settings-row repo-open" data-page="repo" data-id="${esc(repo.id)}" data-focus-key="repo-${esc(repo.id)}"><span class="row-copy"><strong>${esc(repo.name)}</strong><small>GitHub / ${esc(accountName(repo))}</small></span>${icon("chevron")}</button><button type="button" class="mini-switch" role="switch" aria-checked="${repo.enabled}" aria-label="Monitor ${esc(repo.name)}" data-action="toggle-repo" data-id="${esc(repo.id)}" data-focus-key="toggle-${esc(repo.id)}" ${disabled(!live)}></button></div>`;
  const live = state.repos.filter(connected);
  const lost = state.repos.filter((repo) => !connected(repo));
  return `<div class="page-scroll has-fab" data-scroll="repos">
    ${live.length ? `<div class="settings-list repository-items">${live.map((repo) => row(repo, true)).join("")}</div>` : ""}
    <div class="settings-list repository-accounts">${accounts.map((account) => `<div class="repository-account"><span class="provider-mark" data-brand="github">GH</span><span class="row-copy"><strong>${esc(account.name)}</strong></span><button class="repository-browse" data-page="repo-browser" data-id="${esc(account.id)}" data-focus-key="browse-${esc(account.id)}" aria-label="Browse repositories as ${esc(account.name)}" title="Browse repositories" ${disabled(!account.connected)}>${icon("search")}</button></div>`).join("") || `<button class="settings-row" data-page="github"><span class="row-copy"><strong>Connect a repository account</strong></span>${icon("chevron")}</button>`}</div>
    ${lost.length ? `<div class="settings-list repository-items repository-disconnected">${lost.map((repo) => row(repo, false)).join("")}</div>` : ""}
  </div>
  <button type="button" class="floating-pill" data-page="repo-url" data-focus-key="repo-url" aria-label="Add repository by URL" title="Add repository by URL">${icon("plus")}URL</button>`;
}

function repositoryBrowserPage() {
  const account = state.accounts.find((item) => item.id === route().id);
  if (!account?.connected || account.kind !== "github")
    return empty(
      "Account unavailable",
      "Reconnect in Accounts before browsing.",
    );
  const owners = Demo.repositoryOwners(account);
  route().owner ??= owners[0].name;
  return `<div class="page-scroll" data-scroll="repo-browser">
    ${field("Repository owner", `<select id="repository-owner">${owners.map((owner) => option(owner.name, `${owner.name} (${owner.kind.toLowerCase()})`, route().owner)).join("")}</select>`)}
    ${field("Find a repository", `<input type="search" id="repository-filter" value="${esc(route().query ?? "")}" placeholder="Search this owner..." autocomplete="off" />`)}
    <div class="settings-list" id="repository-results">${repositoryResults(account)}</div>
  </div>`;
}

function repositoryResults(account) {
  const names = Demo.repositoryCatalog(account, route().owner).filter((name) =>
    name.toLowerCase().includes((route().query ?? "").toLowerCase()),
  );
  return (
    names
      .map((name) => {
        const existing = state.repos.find(
          (repo) =>
            repo.accountId === account.id &&
            repo.name.toLowerCase() === name.toLowerCase(),
        );
        return `<button class="settings-row" data-action="pick-repo" data-name="${esc(name)}" data-id="${esc(account.id)}" data-focus-key="pick-${esc(name)}"><span class="row-copy"><strong>${esc(name.split("/")[1])}</strong><small>${esc(name)}</small></span><span class="row-value">${existing ? "Added" : "Add"}</span>${icon(existing ? "chevron" : "plus")}</button>`;
      })
      .join("") ||
    empty(
      "No matching repositories",
      "Try another name or choose a different owner.",
    )
  );
}

function repositoryUrlPage() {
  const accounts = state.accounts.filter(
    (account) => account.kind === "github" && account.connected,
  );
  if (!accounts.length)
    return `<div class="page-scroll" data-scroll="repo-url">${empty("Connect GitHub first", "A repository needs an acting account.")}<button class="primary-action wide" data-page="accounts">Open Accounts</button></div>`;
  return `<div class="page-scroll" data-scroll="repo-url"><form class="editor-form" data-form="repo-url">
    ${field("Repository URL", '<input name="repository" placeholder="https://github.com/owner/repository" required autocomplete="off" spellcheck="false" />')}
    ${field("Acting GitHub account", `<select name="accountId" required>${option("", "Choose an account", "")}${accounts.map((account) => option(account.id, account.name, "")).join("")}</select>`)}
    <div class="editor-actions"><button class="primary-action" type="submit">Add & configure</button><button class="secondary-action" type="button" data-action="back">Cancel</button></div>
  </form></div>`;
}

function addRepository(name, accountId) {
  const normalized = Demo.repositoryName(name);
  let repo = state.repos.find(
    (item) =>
      item.accountId === accountId &&
      item.name.toLowerCase() === normalized.toLowerCase(),
  );
  if (!repo) {
    if (
      !perform(
        "add-repo",
        { name: normalized, accountId },
        "Repository added. Configure it before monitoring.",
        false,
      )
    )
      return;
    repo = state.repos.at(-1);
  }
  dirty = false;
  const index = stacks[tab].findLastIndex((item) => item.page === "repos");
  if (index >= 0) stacks[tab] = stacks[tab].slice(0, index + 1);
  stacks[tab].push({ page: "repo", id: repo.id });
  render(1, true);
}

function field(label, control, hint = "") {
  return `<label class="form-field"><span>${label}</span>${control}${hint ? `<small>${hint}</small>` : ""}</label>`;
}
const countLabel = (count, one, many) => `${count} ${count === 1 ? one : many}`;
const input = (name, value = "", required = true) =>
  `<input name="${name}" value="${esc(value)}" ${required ? "required" : ""} autocomplete="off" />`;
const option = (value, label, current) =>
  `<option value="${esc(value)}" ${selected(value, current)}>${esc(label)}</option>`;
function accountSelect(kind, current, name = "accountId", required = true) {
  return `<select name="${name}" ${required ? "required" : ""}>${option("", required ? "Choose an account" : "Unbound", current)}${state.accounts
    .filter((account) => account.kind === kind)
    .map((account) =>
      option(
        account.id,
        `${account.name}${account.connected ? "" : " (disconnected)"}`,
        current,
      ),
    )
    .join("")}${state.forgotten
    .filter((account) => account.kind === kind)
    .map((account) =>
      option(account.id, `${account.name} (disconnected)`, current),
    )
    .join("")}</select>`;
}

function editorFooter(kind, id, save = `Save ${kind}`) {
  return `<div class="editor-actions"><button class="primary-action" type="submit">${save}</button><button class="secondary-action" type="button" data-action="back">Cancel</button></div>${id && kind !== "repo" ? `<button class="text-action danger" type="button" data-action="delete-${kind}" data-id="${esc(id)}">Delete ${kind}</button>` : ""}`;
}

function agentEditor(id) {
  const saved = state.agents.find((item) => item.id === id);
  const base = saved ?? {
    name: "",
    accountId: "",
    model: "",
    prompt: "Review this pull request for correctness, risk, and readability.",
    doctrineIds: [],
    voiceId: "",
    completion: "human",
    comment: false,
  };
  const draft = route().draft;
  const agent = {
    contextWindow: "Standard",
    reasoning: "Medium",
    ...base,
    ...draft?.values,
  };
  agent.signature ??= Demo.defaultSignature(agent.name, agent.model);
  const lastSignature =
    draft?.lastSignature ?? Demo.defaultSignature(agent.name, agent.model);
  const tokens = Demo.promptTokens
    .map(
      ([token, hint]) =>
        `<button type="button" data-action="insert-token" data-token="${token}"><code>{{${token}}}</code><span>${esc(hint)}</span></button>`,
    )
    .join("");
  const row = (label, control) =>
    `<label class="kv-row"><span>${label}</span>${control}</label>`;
  const select = (name, values, current, required = false) =>
    `<select name="${name}" ${required ? "required" : ""}>${values.map((value) => option(value, value, current)).join("")}</select>`;
  return `<div class="page-scroll" data-scroll="agent-editor"><form data-form="agent" data-id="${esc(id ?? "")}" data-last-signature="${esc(lastSignature)}" class="editor-form agent-form">
    <div class="agent-main">
      <section class="group-card">
        ${row("Name", `<input name="name" value="${esc(agent.name)}" required maxlength="80" autocomplete="off" placeholder="e.g. The Nitpicker" />`)}
        ${row("AI account", accountSelect("copilot", agent.accountId))}
      </section>
      <section class="group-card">
        ${row('<span class="model-label">Model<button type="button" class="icon-button" data-action="refresh-models" aria-label="Refresh model list" title="Refresh model list">' + icon("refresh") + "</button></span>", `<select name="model" required>${option("", "Choose a demo model", agent.model)}${Demo.models.map((model) => option(model, model, agent.model)).join("")}</select>`)}
        ${row("Context window", select("contextWindow", Demo.contextWindows, agent.contextWindow))}
        ${row("Reasoning effort", select("reasoning", Demo.reasoningEfforts, agent.reasoning))}
      </section>
      <section class="group-card">
        <button type="button" class="kv-row kv-link" data-action="open-doctrines" aria-label="Choose doctrines"><span>Doctrines</span><span class="kv-value" data-doctrine-summary>${agent.doctrineIds.length} selected</span>${icon("chevron")}</button>
        <button type="button" class="kv-row kv-link" data-action="open-voices" aria-label="Choose voice"><span>Voice</span><span class="kv-value" data-voice-summary>${esc(state.voices.find((voice) => voice.id === agent.voiceId)?.name ?? "None")}</span>${icon("chevron")}</button>
        <div class="prompt-block"><div class="prompt-head"><span>Prompt</span><button type="button" class="icon-button info-tip" data-action="prompt-tip" aria-label="Prompt tips" aria-expanded="false" aria-controls="prompt-tips">${icon("info")}</button></div>
          <div class="tip-body" id="prompt-tips" role="group" aria-label="Prompt tokens and tips" hidden><strong>Tokens</strong><span class="tip-note">Click to insert. Replaced when the review runs.</span><span class="token-list">${tokens}</span><strong>Tips</strong><ul><li>Open with a role and goal.</li><li>Say what to flag, what to ignore, and the severity bar.</li><li>Ask for file, line and a suggested fix.</li></ul></div>
          <textarea name="prompt" aria-label="Prompt" required rows="4">${esc(agent.prompt)}</textarea></div>
        ${row('<span class="model-label">Signature<button type="button" class="icon-button" data-action="reset-signature" aria-label="Reset signature to default" title="Reset to default">' + icon("reset") + "</button></span>", `<input name="signature" value="${esc(agent.signature)}" required maxlength="200" autocomplete="off" />`)}
      </section>
      <section class="group-card">
        ${row("When a review clears", `<select name="completion">${option("human", "Human queue", agent.completion)}${option("approve", "Approve (demo)", agent.completion)}</select>`)}
        ${row("May comment", `<input type="checkbox" class="switch" name="comment" ${checked(agent.comment)} />`)}
      </section>
      ${editorFooter("agent", id)}
    </div>
    <div class="agent-doctrines" hidden>
      <input type="search" id="doctrine-filter" aria-label="Filter doctrines" placeholder="Filter doctrines" autocomplete="off" />
      <div class="group-card doctrine-choices">${
        state.doctrines
          .map(
            (doctrine) =>
              `<div class="doctrine-choice" data-doctrine-choice data-title="${esc(doctrine.name.toLowerCase())}"><label><input type="checkbox" name="doctrineIds" value="${esc(doctrine.id)}" ${checked(agent.doctrineIds.includes(doctrine.id))} /><span>${esc(doctrine.name)}</span></label><button type="button" class="icon-button" data-action="edit-doctrine" data-id="${esc(doctrine.id)}" aria-label="Edit doctrine ${esc(doctrine.name)}" title="Edit doctrine">${icon("pencil")}</button></div>`,
          )
          .join("") || quietEmpty("book", "No doctrines yet")
      }</div>
    </div>
    <div class="agent-voices" hidden>
      <div class="group-card doctrine-choices" role="radiogroup" aria-label="Available voices">${[
        { id: "", name: "None" },
        ...state.voices,
      ]
        .map(
          (voice) =>
            `<div class="doctrine-choice" data-voice-choice><label><input type="radio" name="voiceId" value="${esc(voice.id)}" ${checked((agent.voiceId ?? "") === voice.id)} /><span>${esc(voice.name)}</span></label>${voice.id ? `<button type="button" class="icon-button" data-action="edit-voice" data-id="${esc(voice.id)}" aria-label="Edit voice ${esc(voice.name)}" title="Edit voice">${icon("pencil")}</button>` : ""}</div>`,
        )
        .join("")}</div>
    </div></form></div>`;
}

function repoEditor(id) {
  const saved = state.repos.find((item) => item.id === id);
  const base = saved ?? {
    name: "",
    accountId: "",
    enabled: true,
    scope: false,
    agentIds: [],
    admission: Demo.defaultAdmission(),
    schedule: "interval",
    minutes: 5,
    cron: "*/15 * * * *",
    zone: "America/New_York",
    start: "inherit",
    comments: "inherit",
  };
  const repo = { ...base, ...route().draft?.values };
  const row = (label, control) =>
    `<label class="kv-row"><span>${label}</span>${control}</label>`;
  const plain = (action, label, extra = "") =>
    `<button type="button" class="plain-row ${extra}" data-action="${action}" data-id="${esc(id ?? "")}" ${extra.includes("danger") && !id ? "hidden" : ""}>${label}</button>`;
  return `<div class="page-scroll" data-scroll="repo-editor"><form data-form="repo" data-id="${esc(id ?? "")}" class="editor-form repo-form">
    <section class="group-card">${row("Monitor", `<input type="checkbox" class="switch" name="enabled" aria-label="Monitor this repository" ${checked(repo.enabled)} />`)}${row("Review drafts", `<input type="checkbox" class="switch" name="reviewDrafts" aria-label="Review drafts" ${checked(repo.reviewDrafts)} />`)}</section>
    <section class="group-card admission-group">${row("All pull requests", `<input type="checkbox" class="switch" name="admissionAll" aria-label="All pull requests" ${checked(repo.admission.mode === "all")} />`)}<button type="button" class="kv-row kv-link" data-action="open-filters"><span>Filtered pull requests</span><span class="kv-value" data-filter-summary></span>${icon("chevron")}</button></section>
    <section class="group-card"><div class="card-title"><h2>Agents on this repository</h2><button type="button" class="pill-small" data-action="assign-agent">${icon("plus")}Assign agent</button></div>
      <select class="picker" data-picker="agents" aria-label="Assign agent" hidden></select><div data-list="agents"></div></section>
    <details class="group-card repo-disclosure"><summary>Repository</summary>
      <div class="repo-fields" hidden>${row("Name", input("name", repo.name))}${row("Account", accountSelect("github", repo.accountId, "accountId", false))}</div>
      ${plain("edit-repo-fields", "Edit repository")}
      ${plain("unbind-account", "Unbind account")}
      ${plain("delete-repo", "Remove repository", "danger")}
      ${plain("verify-repo", "Verify GitHub connection")}
      ${plain("read-prs", "Read PR metadata")}
    </details>
    <details class="group-card repo-disclosure schedule-details"><summary>Schedule and scope (prototype)</summary>
      ${row("New-PR-only scope", `<input type="checkbox" class="switch" name="scope" ${checked(repo.scope)} />`)}
      ${row("Schedule", `<select name="schedule">${option("interval", "Every", repo.schedule)}${option("cron", "Advanced", repo.schedule)}</select>`)}
      <div data-schedule="interval">${row("Every (minutes)", `<input name="minutes" type="number" min="1" step="1" value="${repo.minutes}" required />`)}</div>
      <div data-schedule="cron">${row("Cron", input("cron", repo.cron))}${row("Time zone", input("zone", repo.zone))}</div>
      ${["start", "comments"].map((key) => row(key === "start" ? "Start reviews" : "Publish comments", `<select name="${key}">${option("inherit", `Default (${state.preferences[key === "start" ? "automaticStart" : "automaticComments"] ? "on" : "off"})`, repo[key])}${option("on", "On", repo[key])}${option("off", "Off", repo[key])}</select>`)).join("")}
    </details>
    <div class="admission-view" data-admission-view hidden></div>
    ${editorFooter("repo", id, "Save repository")}</form></div>`;
}

function agentPermissions(agent) {
  return `${agent.completion === "approve" ? "May approve" : "Human handoff"} / ${agent.comment ? "May comment" : "No comments"}`;
}

function renderRepoLists(form) {
  const agents = form._agents.map((id) =>
    state.agents.find((a) => a.id === id),
  );
  form.querySelector('[data-list="agents"]').innerHTML = agents
    .filter(Boolean)
    .map(
      (agent) =>
        `<div class="assign-row"><button type="button" class="assign-open" data-action="edit-assigned-agent" data-id="${esc(agent.id)}"><strong>${esc(agent.name)}</strong><small>${agentPermissions(agent)}</small></button><button type="button" class="row-trash" data-action="unassign-agent" data-id="${esc(agent.id)}" aria-label="Remove ${esc(agent.name)} from this repository" title="Remove">${icon("trash")}</button><input type="hidden" name="agentIds" value="${esc(agent.id)}" /></div>`,
    )
    .join("");
  const fill = (name, label, entries) => {
    const picker = form.querySelector(`[data-picker="${name}"]`);
    picker.innerHTML =
      `<option value="">${label}</option>` +
      entries
        .map(
          ([value, text]) =>
            `<option value="${esc(value)}">${esc(text)}</option>`,
        )
        .join("");
    picker.hidden = true;
  };
  fill(
    "agents",
    "Choose an agent",
    state.agents
      .filter((agent) => !form._agents.includes(agent.id))
      .map((agent) => [agent.id, agent.name]),
  );
  form.querySelector('[data-action="assign-agent"]').disabled =
    form.querySelector('[data-picker="agents"]').options.length < 2;
}

function initRepoForm(form) {
  const saved = state.repos.find((repo) => repo.id === form.dataset.id);
  const values = route().draft?.values;
  form._agents = [...(values?.agentIds ?? saved?.agentIds ?? [])];
  form._admission = Demo.copy(
    values?.admission ?? saved?.admission ?? Demo.defaultAdmission(),
  );
  form._route = [...(route().draft?.admissionRoute ?? [])];
  renderRepoLists(form);
  syncAdmission(form);
  syncSchedule(form);
}

const ruleLabels = {
  assigned: "Assigned to me for review",
  mentioned: "Mentioning me in comments",
  include_users: "Include users",
  exclude_users: "Exclude users",
  include_paths: "Include paths",
  exclude_paths: "Exclude paths",
};
const valueRule = (rule) =>
  rule.kind.endsWith("_users") || rule.kind.endsWith("_paths");
const handle = (person) => `@${person.toLowerCase().replaceAll(" ", "-")}`;

function syncAdmission(form) {
  const admission = form._admission;
  const all = admission.mode === "all";
  form.elements.namedItem("admissionAll").checked = all;
  const open = form.querySelector('[data-action="open-filters"]');
  open.disabled = all;
  form.querySelector("[data-filter-summary]").textContent = all
    ? ""
    : `${admission.rules.filter((rule) => rule.enabled).length} on`;
  renderAdmission(form);
}

function currentRule(form) {
  const kind = form._route.at(-1)?.replace("rule:", "");
  return form._admission.rules.find((rule) => rule.kind === kind);
}

function renderAdmission(form) {
  const view = form.querySelector("[data-admission-view]");
  const name = form._route.at(-1);
  form.dataset.view = name ? "admission" : "";
  view.hidden = !name;
  if (!name) {
    view.replaceChildren();
    if (form._baseTitle) pageTitle.textContent = form._baseTitle;
    return;
  }
  form._baseTitle ??= pageTitle.textContent;
  if (name === "filters") {
    pageTitle.textContent = "Filtered pull requests";
    const rules = form._admission.rules;
    view.innerHTML = `<section class="group-card admission-rules">${rules
      .map((rule, index) => {
        const label = ruleLabels[rule.kind];
        const values = valueRule(rule);
        const count = (rule.users ?? rule.paths ?? []).length;
        return `<div class="admission-rule" data-rule="${rule.kind}"><button type="button" class="admission-open" data-action="open-rule" data-kind="${rule.kind}" ${disabled(!values)} aria-label="${esc(label)}${values ? ": edit values" : ""}"><span class="admission-label">${esc(label)}</span><span class="kv-value">${values ? count : ""}</span>${values ? icon("chevron") : ""}</button><button type="button" class="admission-move" data-action="move-rule" data-kind="${rule.kind}" data-dir="-1" aria-label="Move ${esc(label)} up" title="Move up" ${disabled(index === 0)}>${icon("up")}</button><button type="button" class="admission-move" data-action="move-rule" data-kind="${rule.kind}" data-dir="1" aria-label="Move ${esc(label)} down" title="Move down" ${disabled(index === rules.length - 1)}>${icon("down")}</button><input type="checkbox" class="switch" data-rule-switch="${rule.kind}" aria-label="${esc(label)}" ${checked(rule.enabled)} /></div>`;
      })
      .join("")}</section>`;
    return;
  }
  const rule = currentRule(form);
  pageTitle.textContent = ruleLabels[rule.kind];
  const users = rule.kind.endsWith("_users");
  const values = users ? rule.users : rule.paths;
  const list = `<section class="group-card admission-values">${values
    .map(
      (value, index) =>
        `<div class="assign-row"><span class="person-handle" title="${esc(value)}">${users ? esc(handle(value)) : `<code>${esc(value)}</code>`}</span><button type="button" class="row-trash" data-action="remove-value" data-index="${index}" aria-label="Remove ${esc(users ? handle(value) : value)}" title="Remove">${icon("trash")}</button></div>`,
    )
    .join("")}</section>`;
  if (users) {
    const choices = Demo.people.filter((person) => !values.includes(person));
    view.innerHTML = `${list}<select class="picker" data-picker="people" aria-label="Add people" hidden><option value="">Choose a person</option>${choices.map((person) => `<option value="${esc(person)}">${esc(person)}</option>`).join("")}</select><button type="button" class="floating-pill" data-action="add-people" ${disabled(!choices.length)}>${icon("plus")}Add people</button>`;
  } else
    view.innerHTML = `${list}<div class="admission-path-form"><input data-path-input aria-label="Path pattern" placeholder="docs/ or src/**/*.ts" autocomplete="off" /><button type="button" class="primary-action" data-action="add-path">Add</button><p role="alert" class="inline-warning" hidden></p></div>`;
}

function pushAdmission(form, name) {
  form._route.push(name);
  renderAdmission(form);
  form
    .querySelector(
      "[data-admission-view] button:not(:disabled), [data-path-input]",
    )
    ?.focus({ preventScroll: true });
}

function popAdmission(form) {
  const left = form._route.pop();
  syncAdmission(form);
  const target = form._route.length
    ? form.querySelector(
        `[data-action="open-rule"][data-kind="${left.replace("rule:", "")}"]`,
      )
    : form.querySelector('[data-action="open-filters"]');
  target?.focus({ preventScroll: true });
}

function addAdmissionValue(form, value) {
  const rule = currentRule(form);
  const users = rule.kind.endsWith("_users");
  (users ? rule.users : rule.paths).push(value);
  rule.enabled = true;
  dirty = true;
  renderAdmission(form);
}

function setAgentView(form, view) {
  form.dataset.view = view;
  const opener = form.dataset.opener || "open-doctrines";
  if (view) form.dataset.opener = `open-${view}`;
  form.querySelector(".agent-main").hidden = !!view;
  form.querySelector(".agent-doctrines").hidden = view !== "doctrines";
  form.querySelector(".agent-voices").hidden = view !== "voices";
  const saved = state.agents.find((agent) => agent.id === form.dataset.id);
  pageTitle.textContent =
    view === "doctrines"
      ? "Doctrines"
      : view === "voices"
        ? "Voice"
        : saved
          ? `Editing ${saved.name}`
          : "New agent";
  if (!form.isConnected) return;
  (view === "doctrines"
    ? form.querySelector("#doctrine-filter")
    : view === "voices"
      ? form.querySelector('[name="voiceId"]:checked')
      : form.querySelector(`[data-action="${opener}"]`)
  ).focus({ preventScroll: true });
}

function growPrompt(root) {
  const area = root.querySelector('textarea[name="prompt"]');
  if (!area) return;
  area.style.height = "auto";
  area.style.height = `${area.scrollHeight}px`;
}

function syncSignature(form) {
  const signature = form.elements.namedItem("signature");
  const next = Demo.defaultSignature(
    form.elements.namedItem("name").value,
    form.elements.namedItem("model").value,
  );
  if (signature.value === form.dataset.lastSignature) signature.value = next;
  form.dataset.lastSignature = next;
}

function loadModels(form) {
  const button = form.querySelector('[data-action="refresh-models"]');
  button.classList.add("is-loading");
  button.setAttribute("aria-busy", "true");
  setTimeout(() => {
    button.classList.remove("is-loading");
    button.removeAttribute("aria-busy");
  }, 700);
}

function captureDraft() {
  const form = stage.lastElementChild?.querySelector("form[data-form]");
  if (!form || !["agent", "repo"].includes(form.dataset.form)) return;
  const get = (name) => form.elements.namedItem(name);
  const text = (name) => get(name)?.value ?? "";
  const values =
    form.dataset.form === "agent"
      ? {
          name: text("name"),
          accountId: text("accountId"),
          model: text("model"),
          contextWindow: text("contextWindow"),
          reasoning: text("reasoning"),
          prompt: text("prompt"),
          signature: text("signature"),
          completion: text("completion"),
          comment: get("comment").checked,
          doctrineIds: [
            ...form.querySelectorAll('[name="doctrineIds"]:checked'),
          ].map((item) => item.value),
          voiceId: form.querySelector('[name="voiceId"]:checked')?.value ?? "",
        }
      : {
          name: text("name"),
          accountId: text("accountId"),
          enabled: get("enabled").checked,
          scope: get("scope").checked,
          reviewDrafts: get("reviewDrafts").checked,
          agentIds: [...form._agents],
          admission: Demo.copy(form._admission),
          schedule: text("schedule"),
          minutes: Number(text("minutes")),
          cron: text("cron"),
          zone: text("zone"),
          start: text("start"),
          comments: text("comments"),
        };
  route().draft = {
    values,
    admissionRoute: form._route ? [...form._route] : undefined,
    view: form.dataset.view ?? "",
    lastSignature: form.dataset.lastSignature,
  };
}

function syncSchedule(form) {
  const schedule = form.elements.namedItem("schedule").value;
  form.querySelectorAll("[data-schedule]").forEach((element) => {
    element.hidden = element.dataset.schedule !== schedule;
    element.querySelectorAll("input").forEach((control) => {
      control.disabled = element.hidden;
    });
  });
}

function doctrineEditor(id) {
  const item = state.doctrines.find((doctrine) => doctrine.id === id) ?? {
    name: "",
    body: "",
  };
  return `<div class="page-scroll" data-scroll="doctrine-editor"><form data-form="doctrine" data-id="${esc(id ?? "")}" class="editor-form">${field("Doctrine name", input("name", item.name))}${field("Review principles", `<textarea name="body" rows="10" required>${esc(item.body)}</textarea>`)}${editorFooter("doctrine", id)}</form></div>`;
}

function voiceEditor(id) {
  const item = state.voices.find((voice) => voice.id === id) ?? {
    name: "",
    body: "",
    guardrails: true,
  };
  return `<div class="page-scroll" data-scroll="voice-editor"><form data-form="voice" data-id="${esc(id ?? "")}" class="editor-form">${field("Voice name", input("name", item.name))}${field("Style", `<textarea name="body" rows="8" required>${esc(item.body)}</textarea>`)}<label class="voice-guardrails"><input type="checkbox" name="guardrails" ${checked(item.guardrails)} /><span>Fixed guardrails: tone only; critique code, never people</span></label>${editorFooter("voice", id)}</form></div>`;
}

function accountsPage() {
  const count = (kind) =>
    state.accounts.filter((account) => account.kind === kind).length;
  return `<div class="page-scroll" data-scroll="accounts"><div class="settings-list">
    ${settingsRow("GitHub Copilot", "AI access", `${count("copilot")} connected`, "spark", "copilot")}
    ${settingsRow("GitHub", "Repository access", `${count("github")} connected`, "pull-request", "github")}
  </div></div>`;
}

function accountKindPage(kind) {
  const accounts = state.accounts.filter((account) => account.kind === kind);
  const label =
    kind === "github" ? "Add GitHub account" : "Connect Copilot account";
  return `<div class="page-scroll has-fab" data-scroll="${kind}">${
    accounts.length
      ? `<div class="settings-list account-list">${accounts
          .map(
            (account) =>
              `<div class="account-row"><div><strong>${esc(account.name)}</strong><small>\u2713 Connected</small></div><button type="button" class="account-disconnect" data-action="account" data-id="${esc(account.id)}" aria-label="Disconnect ${esc(account.name)}" title="Disconnect">${icon("plug-off")}</button></div>`,
          )
          .join("")}</div>`
      : quietEmpty("plug-off", "No accounts connected")
  }</div>
    <button type="button" class="floating-pill" data-page="account-signin" data-id="${kind}">${icon("plus")}${label}</button>`;
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
  return `<div class="page-scroll" data-scroll="account-signin"><div class="connection-heading"><span class="provider-mark" data-brand="${provider.id}">${provider.mark}</span><div><h2>${provider.name}</h2></div></div>
    <form data-form="account-signin" data-id="${provider.id}" class="editor-form">
      ${field("Fictional account handle", `<input name="accountName" value="${esc(route().draftName ?? "")}" placeholder="e.g. dev-demo" required maxlength="39" pattern="[a-zA-Z0-9]+(-[a-zA-Z0-9]+)*" autocomplete="off" spellcheck="false" />`)}
      <button class="primary-action wide" type="submit">Simulate browser sign-in</button>
      <button class="quiet-action wide" type="button" data-action="cancel-account">Cancel</button>
      <details class="scenario-details"><summary>Prototype scenarios</summary><button class="text-action" type="button" data-action="account-signin-failed">Simulate sign-in failure</button></details>
    </form></div>`;
}

function returnToAccounts() {
  dirty = false;
  const index = stacks[tab].findLastIndex((item) =>
    ["accounts", "copilot", "github", "genie", "welcome"].includes(item.page),
  );
  stacks[tab] =
    index >= 0
      ? stacks[tab].slice(0, index + 1)
      : [{ page: "settings" }, { page: "accounts" }];
  render(-1, true);
}

function capacityPage() {
  const duck = state.duck ?? { calls: 8, timeout: 180, window: 15 };
  return `<div class="page-scroll" data-scroll="capacity"><div class="feature-summary"><span class="summary-icon">${icon("activity")}</span><div><strong>${state.limit}</strong><p>maximum simultaneous reviews</p></div></div>
    <form data-form="capacity" class="editor-form"><p class="section-note">Save as many agents as you like. This controls how many review packets can run at once across all repositories.</p>${field("Concurrent reviews", `<input name="limit" type="number" min="1" step="1" value="${state.limit}" required />`)}
    <fieldset class="duck-limits"><legend>Rubber duck</legend>${field("Calls", `<input name="duckCalls" type="number" min="1" max="32" step="1" value="${duck.calls}" required />`)}${field("Timeout (s)", `<input name="duckTimeout" type="number" min="30" max="600" step="1" value="${duck.timeout}" required />`)}${field("Primary window (min)", `<input name="duckWindow" type="number" min="5" max="120" step="1" value="${duck.window}" required />`)}</fieldset>
    <p class="inline-warning">Lowering the limit stops surplus sample reviews, discards their partial work, and puts them back in the queue.</p><button class="primary-action wide" type="submit">Save review limit</button></form></div>`;
}

const notificationTypeLabels = {
  confirmation: "Confirmation",
  human_input: "Needs input",
  ready: "Ready",
  failure: "Failures",
  approved: "Approved",
  merged: "Merged",
  final_review_cleared: "Final review cleared",
};

function notificationTypeRows() {
  return `<fieldset class="notification-types" data-notification-types ${state.preferences.notifications ? "" : "hidden"}><legend>Notification types</legend>${Demo.notificationTypeKeys
    .map(
      (key) =>
        `<label class="preference-row"><span><strong>${notificationTypeLabels[key]}</strong></span><input type="checkbox" class="switch" name="type-${key}" ${checked(state.preferences.notificationTypes[key])} /></label>`,
    )
    .join("")}</fieldset>`;
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
  return `<div class="page-scroll" data-scroll="preferences"><div class="settings-list">${settingsRow("AI capacity", "A limit on running work, not saved agents", state.limit, "activity", "capacity")}</div><div class="form-group"><label class="preference-row"><span><strong>Theme</strong></span><select data-theme-select aria-label="Theme">${Object.entries(
    themes,
  )
    .map(
      ([value, label]) =>
        `<option value="${value}" ${value === theme ? "selected" : ""}>${label}</option>`,
    )
    .join(
      "",
    )}</select></label></div><form data-form="preferences" class="editor-form"><div class="form-group">${Object.entries(
    labels,
  )
    .map(
      ([key, [label, description]]) =>
        `<label class="preference-row" title="${esc(description)}"><span><strong>${label}</strong></span><input type="checkbox" class="switch" name="${key}" ${checked(state.preferences[key])} /></label>${key === "notifications" ? notificationTypeRows() : ""}`,
    )
    .join(
      "",
    )}</div><div class="form-group"><button type="button" class="quiet-action copy-diagnostics" data-action="copy-diagnostics" aria-label="Copy diagnostics" title="Copy diagnostics">${icon("copy")}<span>Copy diagnostics</span></button></div><button class="primary-action wide" type="submit">Save preferences</button></form></div>`;
}

function diagnosticsText() {
  return `Storage: ${storageBlocked ? "Unavailable / reset required" : "Browser localStorage"}\nSchema: v${state.version}\nReview packets: ${state.reviews.length}\nNetwork operations: None\n`;
}

const changelogSource = `# Changelog

## Unreleased

## 0.2.1 - 2026-10-11

### Fixed

- Relaunching opens the panel; it opens under the tray icon of the clicked screen.

## 0.2.0 - 2026-10-10

### Added

- Expand {{token}} placeholders (pr_title, pr_number, pr_author, head_sha, base_sha, changed_files, changed_file_count, agent_name) in Agent prompts when a review runs. The Agent editor lists them and inserts on click.
- Default Agent signature 🔫 pr-sniper(<agent>:<model>) with a reset icon.
- In-app Changelog from the footer version.

### Changed

- Redesign the panel to be icon-first with minimal prose and consistent navy-band overlap: pinned Queue title with toggle cards and grouped lists, drill-in titles with the back arrow in the band, 8px rounded window corners, floating primary buttons, slim switches, a three-card Agent editor with a Doctrines drill-in, compact repository editor, and error toasts.
- Copilot and GitHub accounts connect without a manual confirm step and disconnect with an icon. A disconnected account is forgotten, so it can be reconnected without a duplicate-account error.

## 0.1.2 - 2026-10-01

### Added

- Sample entries stand in for the rendered CHANGELOG.md in this prototype.
`;

// Escape first, then mark up **bold** and `code`; never raw HTML.
const inlineMarkdown = (text) =>
  esc(text).replace(/\*\*([^*]+)\*\*|`([^`]+)`/g, (_match, bold, code) =>
    bold === undefined ? `<code>${code}</code>` : `<strong>${bold}</strong>`,
  );

function changelogPage() {
  let html = "";
  let open = false;
  for (const raw of changelogSource.split("\n").slice(1)) {
    const heading = /^(##|###) (.+)$/.exec(raw);
    if (heading) {
      if (open) html += "</ul>";
      open = false;
      const tag = heading[1] === "##" ? "h2" : "h3";
      html += `<${tag}>${inlineMarkdown(heading[2])}</${tag}>`;
    } else if (raw.startsWith("- ")) {
      if (!open) html += "<ul>";
      open = true;
      html += `<li>${inlineMarkdown(raw.slice(2))}</li>`;
    }
  }
  if (open) html += "</ul>";
  return `<div class="page-scroll" data-scroll="changelog"><section class="changelog">${html}</section></div>`;
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
    copilot: "GitHub Copilot",
    github: "GitHub",
    changelog: "Changelog",
    "repo-browser": "Browse",
    "repo-url": "Add by URL",
    doctrines: "Doctrines",
    doctrine: "Doctrine",
    voices: "Voices",
    voice: "Voice",
    accounts: "Accounts",
    "add-account": "Add account",
    "account-signin": "Sign in",
    capacity: "Capacity",
    preferences: "Preferences",
    advanced: "Advanced",
    welcome: "Welcome",
    genie: "Genie",
    "setup-review": "Review setup",
  };
  pageTitle.textContent = titles[current.page] ?? "PR Sniper";
  const record = (list) => list.find((item) => item.id === current.id);
  if (current.page === "agent")
    pageTitle.textContent = record(state.agents)
      ? `Editing ${record(state.agents).name}`
      : "New agent";
  else if (current.page === "doctrine")
    pageTitle.textContent = record(state.doctrines)
      ? `Editing ${record(state.doctrines).name}`
      : "New doctrine";
  else if (current.page === "voice")
    pageTitle.textContent = record(state.voices)
      ? `Editing ${record(state.voices).name}`
      : "New voice";
  else if (current.page === "repo")
    pageTitle.textContent = record(state.repos)?.name ?? "New repository";
  popover.dataset.editor = String(
    ["agent", "repo", "doctrine", "voice"].includes(current.page),
  );
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
    case "voices":
      html = collectionPage(current.page);
      break;
    case "agent":
      html = agentEditor(current.id);
      break;
    case "repo":
      html = repoEditor(current.id);
      break;
    case "repo-browser":
      html = repositoryBrowserPage();
      break;
    case "repo-url":
      html = repositoryUrlPage();
      break;
    case "doctrine":
      html = doctrineEditor(current.id);
      break;
    case "voice":
      html = voiceEditor(current.id);
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
    case "copilot":
    case "github":
      html = accountKindPage(current.page);
      break;
    case "changelog":
      html = changelogPage();
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
  if (current.page === "repo")
    initRepoForm(next.querySelector('[data-form="repo"]'));
  if (current.page === "agent") {
    const form = next.querySelector('[data-form="agent"]');
    if (["doctrines", "voices"].includes(current.draft?.view))
      setAgentView(form, current.draft.view);
    form.dataset.view ||= "";
  }
  if (previous) {
    previous.inert = true;
    previous.setAttribute("aria-hidden", "true");
  }
  stage.replaceChildren(...(previous ? [previous] : []), next);
  growPrompt(next);
  if (current.page === "agent" && current.draft?.view === "doctrines")
    pageTitle.textContent = "Doctrines";
  if (current.page === "agent" && current.draft?.view === "voices")
    pageTitle.textContent = "Voice";
  if (current.page === "repo") {
    const repoForm = next.querySelector('[data-form="repo"]');
    if (repoForm._route.length) {
      repoForm._baseTitle = pageTitle.textContent;
      renderAdmission(repoForm);
    }
  }
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
    const returnControl =
      direction < 0 &&
      [...next.querySelectorAll("[data-focus-key]")].find(
        (element) => element.dataset.focusKey === current.returnFocus,
      );
    (returnControl || returnCard || pageTitle).focus({ preventScroll: true });
  }
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
            contextWindow: value("contextWindow"),
            reasoning: value("reasoning"),
            signature: value("signature"),
            prompt: value("prompt"),
            doctrineIds: all("doctrineIds"),
            voiceId: value("voiceId"),
            completion: value("completion"),
            comment: bool("comment"),
          },
        },
      ];
    case "repo": {
      const schedule = value("schedule");
      const saved = state.repos.find((repo) => repo.id === id) ?? {
        minutes: Number(form.elements.namedItem("minutes").defaultValue),
        cron: form.elements.namedItem("cron").defaultValue,
        zone: form.elements.namedItem("zone").defaultValue,
      };
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
            admission: Demo.copy(form._admission),
            schedule,
            minutes:
              schedule === "interval"
                ? Number(value("minutes"))
                : saved.minutes,
            cron: schedule === "cron" ? value("cron") : saved.cron,
            zone: schedule === "cron" ? value("zone") : saved.zone,
            start: value("start"),
            comments: value("comments"),
          },
        },
      ];
    }
    case "doctrine":
      return [
        "save-doctrine",
        { doctrine: { id, name: value("name"), body: value("body") } },
      ];
    case "voice":
      return [
        "save-voice",
        {
          voice: {
            id,
            name: value("name"),
            body: value("body"),
            guardrails: bool("guardrails"),
          },
        },
      ];
    case "capacity":
      return [
        "capacity",
        {
          limit: Number(value("limit")),
          duck: {
            calls: Number(value("duckCalls")),
            timeout: Number(value("duckTimeout")),
            window: Number(value("duckWindow")),
          },
        },
      ];
    case "preferences":
      return [
        "preferences",
        {
          preferences: {
            ...Object.fromEntries(
              Object.keys(state.preferences)
                .filter((key) => key !== "notificationTypes")
                .map((key) => [key, bool(key)]),
            ),
            notificationTypes: bool("notifications")
              ? Object.fromEntries(
                  Demo.notificationTypeKeys.map((key) => [
                    key,
                    bool(`type-${key}`),
                  ]),
                )
              : { ...state.preferences.notificationTypes },
          },
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
    if (form.dataset.form === "repo-url") {
      const data = new FormData(form);
      addRepository(data.get("repository"), data.get("accountId"));
      return;
    }
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
      if (
        perform(
          "add-account",
          { kind, name, confirmed: true },
          `Connected @${name} in the demo.`,
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
      "voice",
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
stage.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && event.target.id === "doctrine-filter")
    event.preventDefault();
});
stage.addEventListener("input", (event) => {
  if (event.target.id === "doctrine-filter") {
    const query = event.target.value.trim().toLowerCase();
    stage.querySelectorAll("[data-doctrine-choice]").forEach((item) => {
      item.hidden = !item.dataset.title.includes(query);
    });
    return;
  }
  if (event.target.id === "repository-filter") {
    route().query = event.target.value;
    const account = state.accounts.find((item) => item.id === route().id);
    stage.querySelector("#repository-results").innerHTML =
      repositoryResults(account);
    return;
  }
  const form = event.target.closest("form");
  if (form?.dataset.form === "account-signin") {
    route().draftName = event.target.value;
    return;
  }
  if (form && form.dataset.form !== "complete") dirty = true;
  if (form?.dataset.form === "agent") {
    if (event.target.name === "name") syncSignature(form);
    if (event.target.name === "prompt") growPrompt(form);
  }
});
stage.addEventListener("change", (event) => {
  if (event.target.matches("[data-theme-select]")) {
    theme = event.target.value;
    try {
      localStorage.setItem(themeKey, theme);
    } catch {
      // The choice still applies for this page.
    }
    applyTheme();
    return;
  }
  if (event.target.id === "repository-owner") {
    route().owner = event.target.value;
    route().query = "";
    render();
    stage.querySelector("#repository-owner").focus();
    return;
  }
  if (
    event.target.name === "notifications" &&
    event.target.closest('[data-form="preferences"]')
  ) {
    event.target
      .closest("form")
      .querySelector("[data-notification-types]").hidden =
      !event.target.checked;
  }
  if (event.target.name === "confirmSetup") {
    event.target
      .closest("form")
      .querySelector('button[type="submit"]').disabled =
      !event.target.checked || !Demo.setupStatus(state).ready;
  }
  if (event.target.name === "voiceId") {
    const form = event.target.closest("form");
    form.querySelector("[data-voice-summary]").textContent =
      state.voices.find((voice) => voice.id === event.target.value)?.name ??
      "None";
  }
  if (event.target.name === "doctrineIds") {
    const form = event.target.closest("form");
    form.querySelector("[data-doctrine-summary]").textContent =
      `${form.querySelectorAll('[name="doctrineIds"]:checked').length} selected`;
  }
  if (event.target.closest('[data-form="agent"]')) {
    const form = event.target.closest("form");
    if (event.target.name === "model") syncSignature(form);
    if (event.target.name === "model" || event.target.name === "accountId")
      loadModels(form);
  }
  if (event.target.matches("[data-picker]")) {
    const form = event.target.closest("form");
    const value = event.target.value;
    if (value) {
      if (event.target.dataset.picker === "agents") {
        form._agents.push(value);
        dirty = true;
        renderRepoLists(form);
      } else {
        addAdmissionValue(form, value);
        form
          .querySelector('[data-action="add-people"]')
          ?.focus({ preventScroll: true });
      }
    }
  }
  if (event.target.name === "schedule")
    syncSchedule(event.target.closest("form"));
  if (event.target.name === "admissionAll") {
    const form = event.target.closest("form");
    form._admission.mode = event.target.checked ? "all" : "filtered";
    dirty = true;
    syncAdmission(form);
  }
  if (event.target.dataset.ruleSwitch) {
    const form = event.target.closest("form");
    form._admission.rules.find(
      (rule) => rule.kind === event.target.dataset.ruleSwitch,
    ).enabled = event.target.checked;
    dirty = true;
  }
});
stage.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && event.target.matches("[data-path-input]")) {
    event.preventDefault();
    event.target
      .closest("form")
      .querySelector('[data-action="add-path"]')
      .click();
  }
});

popover.addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button || button.disabled) return;
  if (button.dataset.nav) {
    switchTab(button.dataset.nav);
    return;
  }
  if (button.dataset.queueFilter) {
    remember();
    queueFilter = button.dataset.queueFilter;
    render();
    stage
      .querySelector(`[data-queue-filter="${queueFilter}"]`)
      ?.focus({ preventScroll: true });
    return;
  }
  if (button.dataset.filter) {
    remember();
    historyFilter = button.dataset.filter;
    render();
    return;
  }
  if (button.id === "notice-dismiss") {
    notice.hidden = true;
    clearTimeout(noticeTimer);
    return;
  }
  if (button.dataset.page) {
    navigate(button.dataset.page, button.dataset.id);
    return;
  }
  const action = button.dataset.action;
  const id = button.dataset.id;
  if (!action) return;
  if (action === "reviewed-more") {
    reviewedLimit += 50;
    render();
    return;
  }
  if (action === "pick-repo") {
    addRepository(button.dataset.name, id);
    return;
  }
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
    const assigned =
      action === "delete-agent"
        ? state.repos
            .filter((repo) => repo.agentIds.includes(id))
            .map((repo) => repo.name)
        : [];
    if (
      !confirm(
        `Delete this shared prototype configuration?${assigned.length ? ` It will be removed from ${assigned.join(", ")} first.` : ""} Existing history will be kept.`,
      )
    )
      return;
    if (perform(action, { id }, "Prototype configuration deleted.", false)) {
      dirty = false;
      if (["agent", "repo", "doctrine", "voice"].includes(route().page))
        goBack();
      else render();
    }
    return;
  }
  if (action === "account") {
    perform("account", { id }, "Account disconnected.");
    return;
  }
  if (action === "copy-diagnostics") {
    const done = (ok) => {
      button.dataset.state = ok ? "copied" : "failed";
      button.querySelector("svg").outerHTML = icon(ok ? "check" : "copy");
    };
    navigator.clipboard.writeText(diagnosticsText()).then(
      () => done(true),
      () => done(false),
    );
    return;
  }
  if (action === "changelog") {
    if (route().page === "changelog") goBack();
    else navigate("changelog");
    return;
  }
  if (action === "toggle-repo") {
    const repo = state.repos.find((item) => item.id === id);
    if (
      perform("save-repo", {
        repo: { ...Demo.copy(repo), enabled: !repo.enabled },
      })
    )
      stage
        .querySelector(`[data-focus-key="toggle-${id}"]`)
        ?.focus({ preventScroll: true });
    return;
  }
  const form = button.closest("form");
  const editorActions = {
    "open-doctrines": () => setAgentView(form, "doctrines"),
    "open-voices": () => setAgentView(form, "voices"),
    "prompt-tip": () => {
      const tip = form.querySelector(".tip-body");
      tip.hidden = !tip.hidden;
      button.setAttribute("aria-expanded", String(!tip.hidden));
    },
    "insert-token": () => {
      const area = form.querySelector('textarea[name="prompt"]');
      area.setRangeText(
        `{{${button.dataset.token}}}`,
        area.selectionStart,
        area.selectionEnd,
        "end",
      );
      area.dispatchEvent(new Event("input", { bubbles: true }));
      area.focus();
    },
    "reset-signature": () => {
      const signature = form.elements.namedItem("signature");
      form.dataset.lastSignature = signature.value = Demo.defaultSignature(
        form.elements.namedItem("name").value,
        form.elements.namedItem("model").value,
      );
      dirty = true;
      signature.focus();
    },
    "refresh-models": () => loadModels(form),
    "edit-doctrine": () => {
      captureDraft();
      navigate("doctrine", id, true);
    },
    "edit-voice": () => {
      captureDraft();
      navigate("voice", id, true);
    },
    "assign-agent": () => {
      const picker = form.querySelector('[data-picker="agents"]');
      picker.hidden = !picker.hidden;
      if (!picker.hidden) picker.focus();
    },
    "add-people": () => {
      const picker = form.querySelector('[data-picker="people"]');
      picker.hidden = !picker.hidden;
      if (!picker.hidden) picker.focus();
    },
    "open-filters": () => pushAdmission(form, "filters"),
    "open-rule": () => pushAdmission(form, `rule:${button.dataset.kind}`),
    "move-rule": () => {
      const rules = form._admission.rules;
      const index = rules.findIndex(
        (rule) => rule.kind === button.dataset.kind,
      );
      const target = index + Number(button.dataset.dir);
      [rules[index], rules[target]] = [rules[target], rules[index]];
      dirty = true;
      renderAdmission(form);
      form
        .querySelector(
          `[data-rule="${button.dataset.kind}"] [data-dir="${button.dataset.dir}"]`,
        )
        ?.focus({ preventScroll: true });
    },
    "remove-value": () => {
      const rule = currentRule(form);
      const values = rule.users ?? rule.paths;
      values.splice(Number(button.dataset.index), 1);
      dirty = true;
      renderAdmission(form);
    },
    "add-path": () => {
      const input = form.querySelector("[data-path-input]");
      const alert = form.querySelector('[data-admission-view] [role="alert"]');
      const value = input.value.trim();
      const problem = Demo.pathProblem(value, currentRule(form).paths);
      if (problem) {
        alert.textContent = problem;
        alert.hidden = false;
        input.focus();
        return;
      }
      addAdmissionValue(form, value);
      form.querySelector("[data-path-input]").focus();
    },
    "unassign-agent": () => {
      form._agents = form._agents.filter((item) => item !== id);
      dirty = true;
      renderRepoLists(form);
    },
    "edit-assigned-agent": () => {
      captureDraft();
      navigate("agent", id, true);
    },
    "edit-repo-fields": () => {
      const fields = form.querySelector(".repo-fields");
      fields.hidden = !fields.hidden;
      if (!fields.hidden) fields.querySelector("input").focus();
    },
    "unbind-account": () => {
      form.elements.namedItem("accountId").value = "";
      button.disabled = true;
      dirty = true;
      tell("Account unbound. Save the repository to apply.");
    },
    "verify-repo": () => {
      const account = state.accounts.find(
        (item) =>
          item.id === form.elements.namedItem("accountId").value &&
          item.connected,
      );
      if (account)
        tell(`Verified as ${account.name} (demo). No request was made.`);
      else
        showError("No connected GitHub account is bound to this repository.");
    },
    "read-prs": () => {
      const account = state.accounts.find(
        (item) =>
          item.id === form.elements.namedItem("accountId").value &&
          item.connected,
      );
      if (account) tell("Read PR metadata (demo). Sample data only.");
      else
        showError("No connected GitHub account is bound to this repository.");
    },
  };
  if (editorActions[action] && form) {
    editorActions[action]();
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
    stage.querySelector('[data-queue-filter][aria-pressed="true"]') || pageTitle
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
  if (event.key === "Escape") {
    event.preventDefault();
    const tip = stage.querySelector(".tip-body:not([hidden])");
    if (tip) {
      tip.hidden = true;
      tip.parentElement
        .querySelector(".info-tip")
        .setAttribute("aria-expanded", "false");
      return;
    }
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
