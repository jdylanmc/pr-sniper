import type { MonitoringSnapshot } from "./monitoring";
import type { WorkKind } from "./panel";
import type { QueueItem } from "./queue";
import type { ReviewSelection } from "./resources";
import type { AutomationSnapshot } from "./automation";
import type { FollowUpCandidate } from "./follow-up";
import { actualIntelligence } from "./copilot";
import { effectiveWatchedAuthors, watchSummary } from "./policy";

export const purposes: Record<WorkKind, string> = {
  normal: "Normal pass",
  primary_final: "Primary final review",
  reply: "Targeted reply",
  mention: "Primary conversation",
};

export interface WorkPresentation {
  agent: string;
  subject: string;
  reference: string;
  ordinal: string;
  count: number | null;
  state: string;
  job?: QueueItem["job"];
  selection?: ReviewSelection | null;
  captured: boolean;
  attempt: number | null;
  trigger: string;
  reason?: string | null;
  conversation?: {
    analysis: string;
    publication: string;
    cancelled: boolean;
  };
}

function conversationState(candidate: FollowUpCandidate) {
  const run = candidate.run;
  // Analysis completion does not settle a remote mutation, even after cancel.
  if (run.uncertain || run.phase === "unresolved") return "outcome_unknown";
  if (run.phase === "stale_after_publication") return "stale_after_publication";
  if (run.publication) {
    if (run.publication.state === "completed")
      return run.receipt ? "published" : "outcome_unknown";
    if (run.cancelled) return "stopped";
    if (["failed", "manual_retry"].includes(run.publication.state))
      return "publication_failed";
    if (["queued", "interrupted"].includes(run.publication.state))
      return "publication_retry";
    return "publishing";
  }
  if (run.cancelled) return "stopped";
  if (run.analysis?.state === "completed") {
    if (run.result?.output.decision === "quiet") return "completed";
    if (run.result?.output.decision === "human_input_required")
      return candidate.superseded && !candidate.human_gate
        ? "answered_human_input"
        : "human_input_required";
    if (run.result?.output.decision === "reply")
      return candidate.automatic_publication
        ? "waiting_publication"
        : "completed_local";
    return "outcome_unknown";
  }
  if (run.analysis?.state === "queued" && run.analysis.attempt_count > 0)
    return "retry_queued";
  return run.analysis?.state ?? run.phase;
}

const conversationLabels: Record<string, string> = {
  outcome_unknown: "Outcome unknown",
  human_input_required: "Human input required",
  answered_human_input: "Human input answered",
  waiting_publication: "Awaiting publication",
  completed_local: "Completed locally",
  publication_failed: "Publication failed",
  publication_retry: "Publication retry queued",
  publishing: "Publishing",
  published: "Published",
  stale_after_publication: "Published; stale evidence",
  retry_queued: "Retry queued",
};

export function workPresentation(
  snapshot: MonitoringSnapshot,
  kind: WorkKind,
  id: string,
): WorkPresentation | undefined {
  let job: QueueItem["job"] | undefined;
  let selection: ReviewSelection | null | undefined;
  let state = "waiting";
  let captured = false;
  let attempt: number | null = null;
  let trigger = "Not recorded";
  let ordinal = purposes[kind];
  let count: number | null = null;
  let reason: string | null = null;
  let conversation: WorkPresentation["conversation"];
  if (kind === "normal") {
    const candidate = snapshot.reviews?.find((r) => r.key === id);
    if (!candidate) return;
    job = candidate.run?.job ?? candidate.job;
    captured = !!candidate.run?.operation.attempt_count;
    selection = captured
      ? candidate.run?.selection
      : candidate.planned_selection;
    attempt = candidate.run?.operation.attempt_count ?? 0;
    state = candidate.run?.operation.state ?? "waiting";
    reason = candidate.blocked ?? candidate.run?.error ?? null;
    if (candidate.job.waiting === "superseded") state = "superseded";
    trigger = job.work
      ? {
          admission: job.work.admission.watched_author
            ? "New PR from watched author"
            : job.work.admission.requested_reviewer
              ? "Requested reviewer"
              : "New PR in monitored scope",
          new_revision: "New head revision",
          reopened: "PR reopened",
          assignment_added: "Agent assigned to tracked PR",
          legacy_admission: "Legacy admission",
        }[job.work.trigger]
      : "Legacy admission";
    ordinal = job.work ? `Pass ${job.work.pass_ordinal}` : "Normal pass";
    count = job.work?.pass_ordinal ?? null;
  } else if (kind === "primary_final") {
    const final = snapshot.items?.find(
      (i) => i.action_status?.final_review?.id === id,
    )?.action_status?.final_review;
    if (!final) return;
    job = final.execution.job;
    selection = final.execution.selection;
    attempt = final.execution.operation.attempt_count;
    captured = attempt > 0;
    state = final.execution.operation.state;
    reason = final.execution.error;
    if (final.cancelled) state = "stopped";
    if (job.waiting === "superseded") state = "superseded";
    trigger =
      "Current passes and concerns clear; separately permitted provider action";
  } else {
    const candidate = snapshot.follow_ups?.find(
      (f) =>
        f.run.id === id &&
        (f.run.target?.kind === "mention" ? "mention" : "reply") === kind,
    );
    if (!candidate) {
      const pending =
        kind === "reply" &&
        snapshot.pending_threads?.find((intent) => intent.work_id === id);
      if (pending)
        return {
          agent: "Primary unavailable",
          subject: `Discussion ${pending.thread.id}`,
          reference: `${pending.binding.repository_name} #${pending.binding.number}`,
          ordinal,
          count,
          state: "blocked",
          captured: false,
          attempt: null,
          trigger: pending.blocked ?? "Awaiting durable primary assessment",
        };
      const mention =
        kind === "mention" && snapshot.mentions?.find((m) => m.work_id === id);
      if (!mention) return;
      return {
        agent: "Primary unavailable",
        subject: `Comment ${mention.comment.id}`,
        reference: `${mention.binding.repository_name} #${mention.binding.number}`,
        ordinal,
        count,
        state: "blocked",
        captured: false,
        attempt: null,
        trigger: `External comment ${mention.comment.id}; ${mention.blocked ?? "Awaiting admission"}`,
      };
    }
    const run = candidate.run;
    const context = run.context ?? run.review;
    job = context?.job;
    captured = !!run.analysis?.attempt_count || !!run.result;
    selection = captured ? context?.selection : candidate.planned_selection;
    attempt = run.analysis?.attempt_count ?? 0;
    state = conversationState(candidate);
    reason = candidate.blocked ?? run.error;
    if (job?.waiting === "superseded" && !run.publication && !run.uncertain)
      state = "superseded";
    conversation = {
      analysis: run.analysis?.state.replaceAll("_", " ") ?? "Not started",
      publication: run.receipt
        ? `Confirmed GitHub reply ${run.receipt}${run.error ? "; verification or recovery needs attention" : ""}`
        : run.uncertain || run.phase === "unresolved"
          ? "Outcome unknown; no confirmed reply receipt. Reconcile the original intent."
          : run.publication
            ? `${run.publication.state.replaceAll("_", " ")}; no confirmed reply receipt`
            : candidate.captured_local_response
              ? "Completed local response; no Reply Comment grant was captured. Later permissions do not replay it."
              : run.result?.output.decision === "reply" &&
                  !candidate.automatic_publication
                ? "Local response; Reply Comment permission is off. No provider write authorized."
                : run.result && run.result.output.decision !== "reply"
                  ? "No automated reply"
                  : "Not started; no confirmed reply receipt",
      cancelled: run.cancelled,
    };
    trigger = `Other-user comment ${run.trigger_id}; ${kind === "mention" ? "primary conversation assessment" : "primary thread assessment"}`;
    ordinal =
      run.reply_ordinal == null
        ? purposes[kind]
        : `${kind === "mention" ? "Mention" : "Reply"} ${run.reply_ordinal}`;
    count = run.reply_ordinal ?? null;
  }
  return {
    agent: selection?.agent.name ?? "Agent configuration unavailable",
    subject: job?.title ?? "PR context unavailable",
    reference: job
      ? `${job.repository_name} #${job.number}`
      : "Repository unavailable",
    ordinal,
    count,
    state,
    job,
    selection,
    captured,
    attempt,
    trigger,
    reason,
    conversation,
  };
}

export function renderFacts(root: HTMLElement, entries: [string, string][]) {
  const list = document.createElement("dl");
  list.className = "context-list";
  for (const [label, value] of entries) {
    const term = document.createElement("dt");
    term.textContent = label;
    const description = document.createElement("dd");
    description.textContent = value;
    list.append(term, description);
  }
  root.append(list);
}

export function renderIntelligenceDiagnostics(
  root: HTMLElement,
  snapshot: MonitoringSnapshot,
) {
  const details = document.createElement("details");
  details.dataset.intelligenceDiagnostics = "true";
  const summary = document.createElement("summary");
  summary.textContent = "Recorded job Intelligence";
  details.append(summary);
  const note = document.createElement("p");
  note.textContent =
    "Captured requests and actual session reports, not today's Agent settings. These are execution records, separate from the redacted host log.";
  details.append(note);
  let count = 0;
  const seen = new Set<string>();
  const append = (
    id: string,
    selection: ReviewSelection,
    result: {
      model: string;
      session_id: string;
      intelligence?: import("./policy").AgentIntelligence | null;
    } | null,
  ) => {
    if (seen.has(id)) return;
    seen.add(id);
    count++;
    const heading = document.createElement("h4");
    heading.textContent = `Job ${id}`;
    details.append(heading);
    renderFacts(details, [
      ["Requested model", selection.agent.model],
      [
        "Requested reasoning effort",
        selection.agent.intelligence
          ? (selection.agent.intelligence.reasoning_effort ??
            "Provider default")
          : "Not recorded (legacy evidence)",
      ],
      [
        "Requested context window",
        selection.agent.intelligence
          ? (selection.agent.intelligence.context_tier ?? "Provider default")
          : "Not recorded (legacy evidence)",
      ],
      ["Actual model", result?.model ?? "Not recorded"],
      ["Session", result?.session_id ?? "Not recorded"],
      ["Actual Intelligence", actualIntelligence(result?.intelligence)],
    ]);
  };
  for (const candidate of snapshot.reviews ?? []) {
    if (candidate.run)
      append(
        candidate.run.operation.id,
        candidate.run.selection,
        candidate.run.result,
      );
  }
  for (const { run } of snapshot.follow_ups ?? []) {
    const selection =
      run.context?.selection ??
      run.review?.selection ??
      (run.target?.kind === "owned" ? run.target.review.selection : undefined);
    if (selection) append(run.id, selection, run.result);
  }
  for (const item of snapshot.items ?? []) {
    const execution = item.action_status?.final_review?.execution;
    if (execution)
      append(execution.operation.id, execution.selection, execution.result);
  }
  if (!count) details.append("No recorded job configuration available.");
  root.append(details);
}

export function renderConfiguration(
  root: HTMLElement,
  label: string,
  selection: ReviewSelection | null | undefined,
  unavailable = "No saved configuration is available.",
  options: { open?: boolean; note?: string; job?: QueueItem["job"] } = {},
) {
  const details = document.createElement("details");
  details.className = "work-configuration";
  details.dataset.disclosure = "configuration";
  details.open = options.open ?? false;
  const summary = document.createElement("summary");
  summary.textContent = label;
  details.append(summary);
  const text = (title: string, value: string | undefined) => {
    const heading = document.createElement("h4");
    heading.textContent = title;
    const body = document.createElement("p");
    body.className = "configuration-text";
    body.textContent = value === undefined ? "Not recorded" : value || "None";
    details.append(heading, body);
  };
  if (options.note) {
    const note = document.createElement("p");
    note.textContent = options.note;
    details.append(note);
  }
  if (!selection) text("Unavailable", unavailable);
  else {
    const { agent, policy, configuration } = selection;
    const authority = configuration?.authority;
    const on = (value: boolean) => (value ? "Yes" : "No");
    const title = document.createElement("h4");
    title.textContent = "Identities and model";
    details.append(title);
    renderFacts(details, [
      ["Agent", agent.name ?? "Not recorded"],
      [
        "AI provider",
        agent.ai_account?.provider === "copilot"
          ? "GitHub Copilot"
          : "Not recorded",
      ],
      ["AI account", agent.ai_account?.account_id ?? "Not recorded"],
      ["Model", agent.model ?? "Not recorded"],
      [
        "Requested reasoning effort",
        agent.intelligence
          ? (agent.intelligence.reasoning_effort ?? "Provider default")
          : "Not recorded (legacy snapshot; no chosen override evidence)",
      ],
      [
        "Requested context window",
        agent.intelligence
          ? (agent.intelligence.context_tier ?? "Provider default")
          : "Not recorded (legacy snapshot; no chosen override evidence)",
      ],
      [
        "GitHub identity",
        options.job
          ? `${options.job.account_login} (${options.job.account_id})`
          : (configuration?.repository.provider_account_id ?? "Not recorded"),
      ],
      [
        "Role",
        authority
          ? authority.primary
            ? "Primary Agent"
            : "Assigned Agent (not primary)"
          : "Not recorded",
      ],
    ]);
    const policyTitle = document.createElement("h4");
    policyTitle.textContent = "Policy";
    details.append(policyTitle);
    renderFacts(details, [
      ["May comment", authority ? on(authority.comment) : "Not recorded"],
      [
        "May reply",
        authority
          ? authority.reply === undefined
            ? "Not recorded (historical)"
            : on(authority.reply)
          : "Not recorded",
      ],
      ["May approve", authority ? on(authority.approve) : "Not recorded"],
      ["May merge", authority ? on(authority.merge) : "Not recorded"],
    ]);
    if (policy)
      renderFacts(details, [
        [
          "Retired publication preference (historical only)",
          policy.automatic_comment_publication
            ? "Recorded automatic; assignment grants govern current writes"
            : "Recorded off; assignment grants govern current writes",
        ],
        [
          "Watched authors",
          policy.watched_authors
            .map((author) => `${author.login} (${author.id})`)
            .join(", ") || "None",
        ],
        ["Reviewer assignment", on(policy.reviewer_assignment)],
        [
          "Watch choices",
          watchSummary(
            policy,
            effectiveWatchedAuthors(
              policy,
              selection.configuration?.repository.watched_authors,
            ),
          ),
        ],
      ]);
    else {
      const missing = document.createElement("p");
      missing.textContent = "Repository policy not recorded in this snapshot.";
      details.append(missing);
    }
    text(
      "Authority boundary",
      "Saved assignment permissions are not a current provider grant. Native gates are rechecked before execution and publication.",
    );
    text("Agent prompt", agent.prompt);
    text("Policy prompt", policy?.prompt);
    text("Review preset", selection.preset ?? "None selected");
    text("Signature", agent.signature);
    if (selection.configuration) {
      const doctrines = document.createElement("h4");
      doctrines.textContent = "Doctrines";
      details.append(doctrines);
      text(
        "Captured doctrine catalog",
        selection.configuration.doctrine_catalog
          ? JSON.stringify(selection.configuration.doctrine_catalog, null, 2)
          : "Catalog revision not captured. Today's library is not historical evidence.",
      );
      if (!selection.configuration.doctrines.length)
        details.append("None selected");
      for (const doctrine of selection.configuration.doctrines)
        text(`Doctrine: ${doctrine.title}`, doctrine.body);
    } else {
      text(
        "Legacy snapshot",
        "Full doctrine and assignment configuration was not captured. Today's settings are not historical evidence.",
      );
      if (selection.doctrine)
        text("Retained legacy doctrine text", selection.doctrine);
    }
    const raw = document.createElement("details");
    raw.dataset.disclosure = "raw-configuration";
    const rawTitle = document.createElement("summary");
    rawTitle.textContent = "Full saved configuration";
    const body = document.createElement("pre");
    body.textContent = JSON.stringify(selection, null, 2);
    raw.append(rawTitle, body);
    details.append(raw);
  }
  root.append(details);
}

export function renderWorkContext(
  root: HTMLElement,
  snapshot: MonitoringSnapshot,
  kind: WorkKind,
  id: string,
  automation: AutomationSnapshot | undefined,
  open: (() => void) | undefined,
) {
  const work = workPresentation(snapshot, kind, id);
  if (!work) return;
  const section = document.createElement("section");
  section.dataset.workContext = id;
  const active = automation?.work.find(
    (entry) => entry.key.kind === kind && entry.key.id === id,
  );
  const state =
    active?.state === "active"
      ? "Running"
      : active?.state === "stopping"
        ? "Stopping"
        : (conversationLabels[work.state] ??
          (work.state === "completed"
            ? "Done"
            : work.state === "superseded"
              ? "Superseded"
              : ["failed", "manual_retry", "unresolved"].includes(work.state)
                ? "Failed"
                : work.state === "stopped"
                  ? "Stopped"
                  : active?.state === "blocked" ||
                      work.state === "blocked" ||
                      work.reason
                    ? "Blocked"
                    : work.state === "running" && !automation
                      ? "Activity unavailable"
                      : "Waiting"));
  const hero = document.createElement("section");
  hero.className = "detail-hero job-hero";
  hero.dataset.jobState = state.toLowerCase();
  const status = document.createElement("div");
  status.className = "job-status";
  const marker = document.createElement("span");
  marker.className = state === "Running" ? "work-spin" : "job-marker";
  marker.setAttribute("aria-hidden", "true");
  const statusText = document.createElement("strong");
  statusText.textContent = state;
  const purpose = document.createElement("span");
  purpose.textContent = purposes[kind];
  status.append(marker, statusText, purpose);
  const title = document.createElement("h2");
  title.textContent = work.agent;
  const subject = document.createElement("p");
  subject.textContent = work.subject;
  hero.append(status, title, subject);
  if (open) {
    const link = document.createElement("button");
    link.type = "button";
    link.className = "job-provider-link";
    link.textContent = "View pull request on GitHub";
    link.onclick = open;
    hero.append(link);
  } else {
    const missing = document.createElement("p");
    missing.textContent =
      "Parent PR iteration context unavailable. No other iteration was selected; captured conversation follows.";
    hero.append(missing);
  }
  const card = document.createElement("section");
  card.className = "detail-section job-facts";
  const heading = document.createElement("h3");
  heading.textContent = "This job";
  card.append(heading);
  const authority = work.selection?.configuration?.authority;
  const identity = work.job?.work;
  renderFacts(card, [
    ["Repository", work.job?.repository_name ?? "Unavailable"],
    [
      "Pull request",
      work.job
        ? `#${work.job.number} / ${identity ? `iteration ${identity.iteration}` : "iteration not recorded"}`
        : "Unavailable",
    ],
    ["Assigned Agent", work.agent],
    [
      `${work.captured ? "Captured" : "Planned"} role`,
      authority
        ? authority.primary
          ? "Primary Agent"
          : "Assigned Agent (not primary)"
        : "Not recorded; not inferred from current Settings",
    ],
    ["Triggered by", work.trigger],
    [
      kind === "normal"
        ? "Review count"
        : kind === "primary_final"
          ? "Purpose"
          : "Reply / mention count",
      kind === "primary_final"
        ? "Primary final review (separate from normal passes)"
        : work.count == null
          ? "Not recorded"
          : `${work.count} for this Agent on this PR`,
    ],
    [
      "Retry attempt",
      work.attempt === 0
        ? "Not started (0)"
        : String(work.attempt ?? "Not recorded"),
    ],
    ["Revision head", work.job?.head_sha.slice(0, 7) ?? "Unavailable"],
  ]);
  if (work.conversation)
    renderFacts(card, [
      ["Analysis", work.conversation.analysis],
      ["Reply publication", work.conversation.publication],
      ["Cancellation requested", work.conversation.cancelled ? "Yes" : "No"],
    ]);
  const note = document.createElement("p");
  note.textContent =
    kind === "normal"
      ? "One pass per Agent per iteration. Retries do not increase the review count."
      : kind === "primary_final"
        ? "The primary's final review is separate from normal passes, provider actions and personal review."
        : "Replies and mentions have their own count; they do not repeat the normal pass.";
  card.append(note);
  section.append(hero, card);
  renderConfiguration(
    section,
    "Assigned Agent configuration",
    work.selection,
    "No saved configuration is available. Today's settings are not execution evidence.",
    {
      open: true,
      job: work.job,
      note: work.captured
        ? "Captured for this execution."
        : "Planned configuration; revalidated at start. No execution is claimed.",
    },
  );
  const provenance = document.createElement("details");
  provenance.dataset.disclosure = "provenance";
  const provenanceTitle = document.createElement("summary");
  provenanceTitle.textContent = "Job identity and provenance";
  provenance.append(provenanceTitle);
  renderFacts(provenance, [
    ["Work ID", id],
    [
      "Iteration identity",
      identity
        ? `Iteration ${identity.iteration} (${identity.iteration_id})`
        : "Canonical iteration not recorded. A commit is not an iteration identity.",
    ],
    ["Item ID", identity?.item_id ?? "Not recorded"],
    ["Full revision head", work.job?.head_sha ?? "Unavailable"],
    ["Admission cause", identity?.trigger ?? "Not recorded"],
    ["Job purpose", purposes[kind]],
    ["Ordinal", work.ordinal],
  ]);
  section.append(provenance);
  if (active?.reason || work.reason) {
    const reason = document.createElement("p");
    reason.className = "review-failure";
    reason.textContent = active?.reason ?? work.reason ?? "";
    section.append(reason);
  }
  root.append(section);
}
