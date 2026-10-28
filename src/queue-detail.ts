import { invoke } from "@tauri-apps/api/core";
import { renderActivity } from "./activity";
import type { MonitoringSnapshot, PublicationCandidate } from "./monitoring";
import {
  compactDuration,
  failureChip,
  openDestination,
  type QueueItem,
} from "./queue";
import { progressPresentation } from "./exits";

export type FixTarget = "ai" | "repository-account";

export interface QueueDetailContext {
  snapshot: MonitoringSnapshot;
  showError: (value: string) => void;
  refresh: () => Promise<void>;
  fix?: (target: FixTarget) => void;
}

const stateLabels: Record<QueueItem["state"], string> = {
  machine_signed_off: "Ready",
  waiting_for_human: "Your input",
  waiting_for_author: "Author",
  waiting_on_ci: "Waiting on CI",
  waiting_on_draft: "Waiting on draft",
  waiting_on_reviews: "Waiting on reviews",
  confirmation_required: "Confirm",
  awaiting_publication: "Publishing",
  queued: "Queued",
  reviewing: "Reviewing",
  failed: "Failed",
  stale: "Stale",
  stale_after_publication: "Stale",
  approved: "Approved",
  closed: "Closed",
  merged: "Merged",
};

// Machine cause -> short label and the corrective route.
const causes: Record<string, [string, "fix" | "retry"]> = {
  account_disconnected: ["GitHub account disconnected", "fix"],
  account_binding_required: ["GitHub account required", "fix"],
  signed_out: ["Reconnect required", "fix"],
  wrong_identity: ["Reconnect required", "fix"],
  missing_scope: ["Reconnect required", "fix"],
  authentication_changed: ["Reconnect required", "fix"],
  scope_confirmation_required: ["Reconnect required", "fix"],
  network: ["Network error", "retry"],
  rate_limited: ["Rate limited", "retry"],
  timeout: ["Timed out", "retry"],
  permission: ["No access", "retry"],
  forbidden: ["No access", "retry"],
  provider_unavailable: ["Provider unavailable", "retry"],
  interrupted: ["Interrupted", "retry"],
  invalid_schedule: ["Invalid schedule", "retry"],
  invalid_global_cron: ["Invalid schedule", "retry"],
  configuration: ["Configuration changed", "retry"],
  configuration_changed: ["Configuration changed", "retry"],
  repository_configuration_required: ["Repository setup", "retry"],
  settings_unavailable: ["Settings unavailable", "retry"],
  review_failed: ["Review failed", "retry"],
  conversation_admission: ["Reply admission failed", "retry"],
  provider: ["Provider error", "retry"],
  publication_failed: ["Publish failed", "retry"],
  publication_unresolved: ["Unverified", "retry"],
  action_unresolved: ["Action unresolved", "retry"],
  panic: ["Panic", "retry"],
  tossed: ["Tossed", "retry"],
  exit_failed: ["Write failed", "retry"],
};

const classLabels = {
  credentials: "Credentials",
  permission: "Permission",
  network: "Network",
  other: "Failed",
};
const clockPath = "M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18";
const MAX_ATTEMPTS = 3;
const MAX_CYCLES = 5;

const paths = {
  fail: "M12 3 2 20h20L12 3Zm0 6v5m0 3v.5",
  fix: "M14.5 5.5a4 4 0 0 0-5 5L3 17l4 4 6.5-6.5a4 4 0 0 0 5-5l-2.5 2.5-2-2 2.5-2.5Z",
  retry: "M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5",
  github:
    "M10 6H6a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4M14 4h6v6m0-6-9 9",
  ok: "M5 12.5 10 17.5 19 7",
};

function icon(path: string) {
  return `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
}

function humanize(code: string) {
  const text = code.replaceAll("_", " ").trim();
  return text ? text[0].toUpperCase() + text.slice(1) : "Failed";
}

export function failedReviews(item: QueueItem, snapshot: MonitoringSnapshot) {
  return (snapshot.reviews ?? []).filter(
    (candidate) =>
      item.review_keys.includes(candidate.key) &&
      candidate.run &&
      ["failed", "manual_retry", "interrupted"].includes(
        candidate.run.operation.state,
      ),
  );
}

// Parked publications this item can retry. Terminal ones (stale, discarded,
// a deleted remote batch) never take another batch.
export function failedPublications(
  item: QueueItem,
  snapshot: MonitoringSnapshot,
): PublicationCandidate[] {
  const runs = new Set(
    (snapshot.reviews ?? [])
      .filter((candidate) => item.review_keys.includes(candidate.key))
      .map((candidate) => candidate.run?.operation.id),
  );
  return (snapshot.publications ?? []).filter((candidate) => {
    const publication = candidate.publication;
    return (
      publication &&
      runs.has(candidate.review_operation_id) &&
      !["discarded", "stale"].includes(publication.phase) &&
      publication.receipts.at(-1)?.state !== "deleted" &&
      publication.operation.state !== "completed" &&
      (publication.uncertain ||
        ["failed", "manual_retry"].includes(publication.operation.state))
    );
  });
}

export function failureCause(item: QueueItem, snapshot: MonitoringSnapshot) {
  for (const warning of item.warnings) {
    const match = /^Monitoring failure: ([a-z0-9_]+)\./.exec(warning);
    if (match) return match[1];
  }
  if (item.job.waiting === "account_disconnected")
    return "account_disconnected";
  const health = (snapshot.health ?? []).find(
    (entry) =>
      entry.repository_id === item.job.configuration_id &&
      entry.provider_account_id === item.job.account_id &&
      entry.last_failure,
  );
  if (health?.last_failure) return health.last_failure;
  if (
    item.action_status?.effects.some((effect) => effect.state === "uncertain")
  )
    return "action_unresolved";
  if (item.failure_cause)
    return item.failure_class === "credentials"
      ? "signed_out"
      : item.failure_cause;
  if (failedReviews(item, snapshot).length) return "review_failed";
  return "";
}

export function failureLabel(cause: string, item?: QueueItem) {
  if (item && cause.startsWith("publication_")) return failureChip(item);
  return causes[cause]?.[0] ?? (cause ? humanize(cause) : "Failed");
}

// Packet exit causes recover through their own review, never a repository poll.
const exitCauses = ["panic", "tossed", "exit_failed"];

function fixTarget(cause: string): FixTarget | undefined {
  if (causes[cause]?.[1] !== "fix") return undefined;
  return /copilot|^ai_/.test(cause) ? "ai" : "repository-account";
}

// The account a credentials failure needs: a review run's comes from its AI
// account; repository checks and publications use the git account.
export function credentialsFixTarget(
  item: QueueItem,
  snapshot: MonitoringSnapshot,
): FixTarget {
  return failedReviews(item, snapshot).some(
    (candidate) => candidate.run?.operation.failure === "credentials",
  )
    ? "ai"
    : "repository-account";
}

export type RecoveryCommand = [string, Record<string, unknown>];

// Every recovery this failure needs: a repository poll retry and each of the
// item's own failed review runs. Poll first, so reviews restart on fresh data.
export function retryCommands(
  item: QueueItem,
  snapshot: MonitoringSnapshot,
): RecoveryCommand[] {
  const commands: RecoveryCommand[] = [];
  const effect = item.action_status?.effects.find(
    (entry) =>
      entry.state === "uncertain" ||
      (entry.state === "confirmed" && entry.error),
  );
  if (effect) {
    commands.push(["reconcile_provider_action", { id: effect.id }]);
    return commands;
  }
  const cause = failureCause(item, snapshot);
  // A parked toss or a failed exit write restarts the PR's toss budget.
  if (
    item.failure_class !== "credentials" &&
    ["tossed", "exit_failed"].includes(cause)
  )
    return [["retry_packet_state", { itemId: item.id }]];
  const operation = (snapshot.health ?? []).find(
    (entry) =>
      entry.repository_id === item.job.configuration_id &&
      entry.provider_account_id === item.job.account_id &&
      ["manual_retry", "failed"].includes(entry.operation?.state ?? ""),
  )?.operation;
  // Repository-level poll failures retry the poll.
  const monitoring = ![
    "review_failed",
    "publication_failed",
    "publication_unresolved",
    ...exitCauses,
    "",
  ].includes(cause);
  if (monitoring && operation)
    commands.push([
      "retry_monitoring_operation",
      { operationId: operation.id },
    ]);
  // A revision is reviewed once: only a run that itself failed, with nothing
  // published or pending for it, may be started again.
  for (const candidate of failedReviews(item, snapshot)) {
    if (
      candidate.run?.result ||
      (snapshot.publications ?? []).some(
        (publication) =>
          publication.publication &&
          publication.review_operation_id === candidate.run?.operation.id,
      )
    )
      continue;
    commands.push(["start_review", { candidateKey: candidate.key }]);
  }
  // Parked publications reconcile the original batch; the host never reposts.
  for (const candidate of failedPublications(item, snapshot))
    commands.push([
      "publish_review",
      { reviewOperationId: candidate.review_operation_id },
    ]);
  if (commands.length) return commands;
  if (operation)
    return [["retry_monitoring_operation", { operationId: operation.id }]];
  if (monitoring) return [["check_now", {}]];
  const status = item.action_status;
  if (
    status?.blockers.length &&
    status.provider_observed_at &&
    status.observation_retry_blocker === null
  )
    return [["retry_action_observation", { itemId: status.item_id }]];
  return [["check_now", {}]];
}

// Runs recoveries once each, in order, never repeating an identical command.
export async function runRecoveries(
  commands: RecoveryCommand[],
  onError: (error: unknown) => void,
) {
  const done = new Set<string>();
  for (const command of commands) {
    const key = JSON.stringify(command);
    if (done.has(key)) continue;
    done.add(key);
    try {
      await invoke(...command);
    } catch (error) {
      onError(error);
    }
  }
}

function button(label: string, path: string, name: string) {
  const element = document.createElement("button");
  element.type = "button";
  element.setAttribute("aria-label", name);
  element.innerHTML = `${icon(path)}<span>${label}</span>`;
  return element;
}

function githubLink(item: QueueItem, context: QueueDetailContext) {
  const link = document.createElement("button");
  link.type = "button";
  link.className = "job-provider-link";
  link.setAttribute("aria-label", "Open PR on GitHub");
  link.title = "Open PR on GitHub";
  link.innerHTML = icon(paths.github);
  link.onclick = async () => {
    link.disabled = true;
    await openDestination(item.id, null, context.showError);
    link.disabled = false;
  };
  return link;
}

export function renderQueueStatus(
  root: HTMLElement,
  item: QueueItem,
  context: QueueDetailContext,
) {
  const block = document.createElement("section");
  block.className = "queue-status";
  block.dataset.queueStatus = item.state;
  const title = document.createElement("h2");
  title.className = "queue-status-title";
  title.textContent = `${item.job.repository_name} #${item.job.number}: ${item.job.title}`;
  const failed = item.state === "failed";
  const cause = failed ? failureCause(item, context.snapshot) : "";
  const label = document.createElement("p");
  label.className = "queue-status-label";
  label.setAttribute("role", "status");
  label.dataset.failureCause = cause;
  const progress =
    !failed && item.progress ? progressPresentation[item.progress] : null;
  if (progress) label.dataset.progress = item.progress!;
  label.innerHTML = icon(failed ? paths.fail : (progress?.[1] ?? paths.ok));
  label.append(
    Object.assign(document.createElement("strong"), {
      textContent: failed
        ? failureLabel(cause, item)
        : (progress?.[0] ?? stateLabels[item.state]),
    }),
  );
  const actions = document.createElement("div");
  actions.className = "queue-status-actions";
  if (failed) {
    const target = context.fix ? fixTarget(cause) : undefined;
    const retry = async (control: HTMLButtonElement) => {
      control.disabled = true;
      try {
        await runRecoveries(retryCommands(item, context.snapshot), (error) =>
          context.showError(typeof error === "string" ? error : "Failed"),
        );
      } finally {
        control.disabled = false;
        await context.refresh();
      }
    };
    const retryButton = button("Retry", paths.retry, "Retry");
    retryButton.onclick = () => void retry(retryButton);
    const primary = target ? button("Fix", paths.fix, "Fix") : retryButton;
    primary.classList.add("queue-primary");
    primary.dataset.queuePrimary = target ? "fix" : "retry";
    if (target) {
      primary.onclick = () => context.fix?.(target);
      retryButton.classList.add("queue-secondary");
    }
    actions.append(primary);
    if (target) actions.append(retryButton);
    actions.append(githubLink(item, context));
  } else {
    const open = githubLink(item, context);
    open.classList.add("queue-primary");
    open.dataset.queuePrimary = "open";
    open.innerHTML = `${icon(paths.github)}<span>Open</span>`;
    actions.append(open);
  }
  block.append(title, label);
  if (failed) renderFailureFacts(block, item, cause, context);
  block.append(actions);
  root.append(block);
  renderActivity(root, item, context.snapshot, context.showError);
  if (!failed) {
    const synopsis = (context.snapshot.reviews ?? [])
      .filter((candidate) => item.review_keys.includes(candidate.key))
      .map((candidate) => {
        const output = candidate.run?.result?.output;
        // A `ready_for_human` exit keeps its summary local: shown, never posted.
        // A sign-off's comment is shown as well (posted only on its own threads).
        return (
          output?.synopsis ||
          (output?.exit?.exit === "ready_for_human"
            ? output.exit.summary
            : "") ||
          (output?.exit?.exit === "lgtm" ? output.exit.comment : "")
        );
      })
      .find(Boolean);
    if (synopsis) {
      const body = document.createElement("p");
      body.className = "queue-synopsis";
      body.dataset.queueSynopsis = "";
      body.textContent = synopsis;
      root.append(body);
    }
  }
}

function renderFailureFacts(
  block: HTMLElement,
  item: QueueItem,
  cause: string,
  context: QueueDetailContext,
) {
  const now = Date.now() / 1000;
  const review = failedReviews(item, context.snapshot)[0];
  const publication = failedPublications(item, context.snapshot)[0]
    ?.publication;
  const operation =
    review?.run?.operation ??
    publication?.operation ??
    (context.snapshot.health ?? []).find(
      (entry) =>
        entry.repository_id === item.job.configuration_id &&
        entry.provider_account_id === item.job.account_id &&
        entry.operation,
    )?.operation ??
    null;
  const health = (context.snapshot.health ?? []).find(
    (entry) =>
      entry.repository_id === item.job.configuration_id &&
      entry.provider_account_id === item.job.account_id,
  );
  const code = item.failed_teammate?.cause ?? operation?.failure ?? cause;
  const klass = classLabels[item.failure_class ?? "other"];
  const rows: [string, string | HTMLElement][] = [
    ["Cause", code ? `${klass} / ${code}` : klass],
  ];
  if (operation) {
    const cycle = operation.resume_cycle ?? 0;
    rows.push([
      "Attempts",
      `${operation.attempt_count} of ${MAX_ATTEMPTS}${
        cycle > 0 || operation.resume_at != null
          ? ` \u00b7 cycle ${cycle} of ${MAX_CYCLES}`
          : ""
      }`,
    ]);
  }
  const last =
    review || publication
      ? operation?.initial_attempt_at
      : (health?.last_attempt ?? operation?.initial_attempt_at);
  if (last) rows.push(["Last attempt", `${compactDuration(now - last)} ago`]);
  const next = document.createElement("span");
  const resume = item.resume_at ?? operation?.resume_at ?? null;
  if (resume != null) {
    next.innerHTML = `${icon(clockPath)}`;
    next.className = "queue-fact-time";
    next.append(compactDuration(resume - now));
  } else {
    next.textContent =
      item.failure_class === "credentials" ? "On reconnect" : "Manual";
  }
  rows.push(["Next retry", next]);
  if (item.failed_teammate)
    rows.push(["Teammate", item.failed_teammate.agent_name]);
  else if (review) rows.push(["Agent", review.run?.selection.agent.name ?? ""]);
  if (item.failed_duck) rows.push(["Rubber duck", item.failed_duck]);
  rows.push(["Revision", item.job.head_sha.slice(0, 7)]);
  rows.push(["Account", item.job.account_login]);
  const list = document.createElement("dl");
  list.className = "context-list queue-facts";
  for (const [label, value] of rows) {
    if (value === "") continue;
    const term = document.createElement("dt");
    term.textContent = label;
    const description = document.createElement("dd");
    if (typeof value === "string") description.textContent = value;
    else description.append(value);
    list.append(term, description);
  }
  block.append(list);
  const detail = review?.run?.error ?? publication?.error ?? health?.last_error;
  if (detail) {
    const line = document.createElement("code");
    line.className = "queue-error";
    line.dataset.queueError = "";
    line.title = detail;
    line.tabIndex = 0;
    line.textContent = detail;
    block.append(line);
  }
}

export const detailsIcon = icon("m6 9 6 6 6-6");
