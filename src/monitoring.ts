import { invoke } from "@tauri-apps/api/core";
import { mountAutomation, type AutomationSnapshot } from "./automation";
import type { PanelDetail } from "./panel";
import { renderActions } from "./actions";
import {
  renderFollowUps,
  type FollowUpCandidate,
  type MentionRouting,
} from "./follow-up";
import {
  openDestination,
  renderQueue,
  type QueueItem,
  type NormalWork,
  renderItemEvidence,
} from "./queue";
import { renderNotificationHistory } from "./notifications";

interface Health {
  repository_id: string;
  name: string;
  schedule_key: string;
  provider_account_id: string | null;
  account_login: string | null;
  assignment_id: string | null;
  agent_id: string | null;
  agent_name: string | null;
  enabled: boolean;
  last_attempt: number | null;
  last_success: number | null;
  next_run: number;
  schedule_available: boolean;
  last_failure: string | null;
  in_flight: boolean;
  conversation_admission_pending?: boolean;
  manual_pending: boolean;
  operation: {
    id: string;
    operation_type: string;
    state:
      | "queued"
      | "running"
      | "interrupted"
      | "completed"
      | "failed"
      | "manual_retry";
    attempt_count: number;
    retry_deadline: number;
    next_attempt_at: number | null;
    failure: string | null;
  } | null;
}

interface Job {
  work?: NormalWork;
  account_id: string;
  account_login: string;
  repository_name: string;
  number: number;
  title: string;
  head_sha: string;
  author_id: string | null;
  author_login: string | null;
  watched_author: boolean;
  all_authors: boolean;
  requested_reviewer: boolean;
  waiting: string;
}

export interface MonitoringSnapshot {
  mentions?: MentionRouting[];
  global_scan?: {
    schedule_key: string;
    next_run: number;
    pending: string[];
  } | null;
  tracked?: {
    pull_request_id: string;
    lifecycle: "open" | "closed" | "merged";
  }[];
  items?: QueueItem[];
  health: Health[];
  jobs: Job[];
  reviews?: ReviewCandidate[];
  publications?: PublicationCandidate[];
  follow_ups?: FollowUpCandidate[];
}

interface PublicationCandidate {
  local_only?: boolean;
  review_operation_id: string;
  automatic: boolean;
  blocked: string | null;
  publication: {
    id: string;
    phase:
      | "preparing"
      | "pending"
      | "published"
      | "stale"
      | "stale_after_publication"
      | "stopped"
      | "unresolved";
    operation: NonNullable<Health["operation"]>;
    error: string | null;
    uncertain: boolean;
    cancelled: boolean;
    history: unknown[];
    batch: { unmappable: number[] } | null;
    receipts: {
      review_id: string;
      state: "pending" | "commented" | "deleted";
      comment_ids: string[];
    }[];
  } | null;
}

interface ReviewCandidate {
  key: string;
  assignment_id: string;
  agent_name: string;
  job: Job;
  trust_required: boolean;
  blocked: string | null;
  planned_selection?: import("./resources").ReviewSelection | null;
  run: {
    feedback_context?:
      | {
          id: string;
          body: string;
          closed: boolean;
          owner_agent_id: string;
          original_head: string;
        }[]
      | null;
    trust_confirmed?: boolean;
    phase: string;
    error: string | null;
    selection: import("./resources").ReviewSelection;
    job?: Job;
    operation: NonNullable<Health["operation"]>;
    result: {
      reviewed_base_sha?: string | null;
      output: {
        feedback_conflict?: boolean;
        held_findings?: { path: string; title: string; explanation: string }[];
        synopsis: string;
        decision: "machine_sign_off" | "human_input_required";
        files: { path: string; explanation: string; order: number }[];
        findings: {
          path: string;
          side: string;
          line: number;
          severity: string;
          title: string;
          explanation: string;
          confidence: number;
        }[];
      };
      model: string;
      session_id: string;
      runtime_version: string;
      input_tokens: number;
      output_tokens: number;
      tool_calls: number;
    } | null;
  } | null;
}

const time = (value: number | null) =>
  value === null ? "Never" : new Date(value * 1000).toLocaleString();

const blockingFailures = new Set([
  "account_binding_required",
  "account_disconnected",
  "configuration",
  "invalid_schedule",
  "invalid_global_cron",
  "provider_unavailable",
  "scope_confirmation_required",
  "settings_unavailable",
]);

function healthLabel(item: Health) {
  if (!item.enabled) return "Disabled";
  if (item.operation?.state === "manual_retry") return "Manual retry required";
  if (item.operation?.state === "failed") return "Failed; correction required";
  if (item.operation?.state === "interrupted") return "Interrupted";
  if (item.operation?.state === "queued") return "Retry queued";
  if (item.in_flight) return "Checking";
  if (item.conversation_admission_pending)
    return "Conversation admission pending";
  if (item.last_failure && blockingFailures.has(item.last_failure))
    return "Blocked";
  if (!item.schedule_available) return "Unavailable";
  return item.manual_pending
    ? "Scheduled; immediate check pending"
    : "Scheduled";
}

function waitingLabel(waiting: string) {
  switch (waiting) {
    case "trust_confirmation":
      return "Waiting for explicit trust confirmation; no review has started.";
    case "human_start":
      return "Automatic start is disabled; start an assigned Agent below.";
    case "agent_unavailable":
      return "Automatic start is configured; an available assigned Agent can review this revision.";
    case "account_disconnected":
      return "Not actionable: the acting GitHub account is disconnected.";
    case "repository_disabled":
      return "Not actionable: repository monitoring is disabled.";
    case "repository_removed":
      return "Not actionable: the repository was removed from Settings.";
    case "binding_changed":
      return "Not actionable: the repository account binding changed.";
    case "policy_changed":
      return "Not actionable: the trigger policy changed.";
    case "superseded":
      return "Not actionable: a newer head revision superseded this detection.";
    case "ineligible":
      return "Not actionable: this revision is no longer eligible.";
    case "no_longer_current":
      return "Not seen in the latest open-PR scan; rechecked next scan. This is not a terminal closed or merged state.";
    case "scope_excluded":
      return "Not actionable: this unchanged existing revision was excluded by the confirmed monitoring scope.";
    case "closed":
      return "GitHub confirmed this PR closed. This iteration is terminal.";
    case "merged":
      return "GitHub confirmed this PR merged. This iteration is terminal.";
    case "assignment_removed":
      return "Not actionable: this Agent assignment was removed or replaced.";
    default:
      return `Not actionable: unrecognized queue state (${waiting}).`;
  }
}

function conversationItem(
  snapshot: MonitoringSnapshot,
  detail: Extract<PanelDetail, { type: "job" }>,
): QueueItem | undefined {
  const candidates = (snapshot.follow_ups ?? []).filter(
    ({ run }) =>
      run.id === detail.id &&
      (run.target?.kind === "mention" ? "mention" : "reply") === detail.kind,
  );
  if (candidates.length !== 1) return undefined;
  const run = candidates[0].run;
  const job = (run.context ?? run.review)?.job;
  if (!job) return undefined;
  const work = job.work;
  const matches = (snapshot.items ?? []).filter(
    (item) =>
      item.follow_up_ids.includes(run.id) &&
      item.job.provider === job.provider &&
      item.job.account_id === job.account_id &&
      item.job.configuration_id === job.configuration_id &&
      item.job.repository_id === job.repository_id &&
      item.job.pull_request_id === job.pull_request_id &&
      item.job.head_sha === job.head_sha &&
      item.job.number === job.number &&
      (work
        ? !!work.item_id &&
          !!work.iteration_id &&
          item.id === work.item_id &&
          item.job.work?.item_id === work.item_id &&
          item.job.work.iteration_id === work.iteration_id
        : item.job.trigger_policy === job.trigger_policy),
  );
  // A legacy head can span several reopened iterations; never pick by order.
  return matches.length === 1 ? matches[0] : undefined;
}

export function renderMonitoring(
  root: HTMLElement,
  showError: (message: string) => void,
  options: {
    panel?: boolean;
    navigate?: (detail: PanelDetail) => void;
    onSnapshot?: (snapshot: MonitoringSnapshot) => void;
    onAutomation?: (snapshot: AutomationSnapshot | undefined) => void;
  } = {},
) {
  root.innerHTML = `<div data-monitor-overview><div class="actions"><button id="check-now" type="button">Check Now</button><button id="queue-settings" type="button">Open Settings</button><button id="queue-diagnostics" type="button">Open Diagnostics</button></div>
    <section id="automation-controls"></section>
    <h2>Your review inbox</h2><section id="handoff-queue"></section></div>
    <div data-monitor-detail><section data-item-evidence></section>
    <h2 id="evidence-heading" tabindex="-1">Review evidence and actions</h2>
    <p>Monitoring and assigned reviews run while the ${trayAdjective} app is active. Copilot uses read-only tools. GitHub comments require a separate publication gate; machine sign-off is not approval.</p>
    <p class="hint">Saved review evidence is tied to the head shown. GitHub links open the live PR or current diff in your browser's signed-in account; check its current revision and requirements before deciding to merge.</p>
    <h2 data-normal-heading>Agent reviews</h2><section id="agent-reviews"></section>
    <h2 data-conversation-heading>Thread follow-ups</h2><section id="thread-follow-ups"></section></div>
    <div data-monitor-recovery>
    <h2>Notifications</h2><section id="notification-history"></section>
    <h2>Schedule health</h2><section id="schedule-health"></section>
    <h2>Detected pull requests</h2><section id="review-jobs"></section></div>`;
  const check = root.querySelector<HTMLButtonElement>("#check-now")!;
  const recoveryRoot = root.querySelector<HTMLElement>(
    "[data-monitor-recovery]",
  )!;
  const automationRoot = root.querySelector<HTMLElement>(
    "#automation-controls",
  )!;
  const refreshAutomation = mountAutomation(
    automationRoot,
    options.onAutomation,
    { compact: options.panel, onError: options.panel ? showError : undefined },
  );
  const health = root.querySelector<HTMLElement>("#schedule-health")!;
  const jobs = root.querySelector<HTMLElement>("#review-jobs")!;
  const reviews = root.querySelector<HTMLElement>("#agent-reviews")!;
  const trust = new Set<string>();
  const expanded = new Set<string>();
  const pending = new Set<string>();
  const publishConsent = new Set<string>();
  let reviewsSignature = "";
  let loading = false;
  let snapshot: MonitoringSnapshot | undefined;
  let panelDetail: PanelDetail | undefined;
  let missingDetail: string | null = null;
  let detailSignature = "";
  let detailRevision = 0;
  let selection: QueueItem | null | undefined;
  const refreshNotifications = renderNotificationHistory(
    root.querySelector<HTMLElement>("#notification-history")!,
    showError,
  );
  const followUps = renderFollowUps(
    root.querySelector<HTMLElement>("#thread-follow-ups")!,
    showError,
    refresh,
  );
  const queue = renderQueue(
    root.querySelector<HTMLElement>("#handoff-queue")!,
    showError,
    (item, focus) => {
      if (options.panel) {
        if (focus && item)
          options.navigate?.({ type: "item", item_id: item.id });
        return;
      }
      selection = item;
      renderEvidence();
      if (focus) {
        const heading = root.querySelector<HTMLElement>("#evidence-heading")!;
        heading.focus();
        heading.scrollIntoView({ block: "start" });
      }
    },
    refresh,
    { compact: options.panel, externalSelection: options.panel },
  );
  if (options.panel) {
    root.querySelector<HTMLElement>("[data-monitor-detail]")!.hidden = true;
    root.querySelector<HTMLElement>("[data-monitor-recovery]")!.hidden = true;
  }
  for (const [id, command] of [
    ["#queue-settings", "open_settings"],
    ["#queue-diagnostics", "open_diagnostics"],
  ]) {
    root.querySelector(id)!.addEventListener("click", async () => {
      try {
        await invoke(command);
      } catch {
        showError(
          `Could not open the requested application window. Try the ${trayAdjective} menu.`,
        );
      }
    });
  }

  function renderEvidence() {
    if (!snapshot) return;
    const current = snapshot;
    let jobDetail: Extract<PanelDetail, { type: "job" }> | undefined;
    if (options.panel) {
      const target = panelDetail;
      const detail = root.querySelector<HTMLElement>("[data-monitor-detail]")!;
      root.querySelector<HTMLElement>("[data-monitor-overview]")!.hidden =
        !!target;
      detail.hidden = !target;
      if (!target) return;
      const job = target.type === "job" ? target : undefined;
      jobDetail = job;
      const item =
        target.type === "item"
          ? current.items?.find(
              (i) =>
                i.id === target.item_id || i.aliases?.includes(target.item_id),
            )
          : job?.kind === "normal"
            ? current.items?.find((i) => i.review_keys.includes(job.id))
            : job?.kind === "primary_final"
              ? current.items?.find(
                  (i) => i.action_status?.final_review?.id === job.id,
                )
              : job
                ? conversationItem(current, job)
                : undefined;
      const found =
        target.type === "item"
          ? !!item
          : job?.kind === "normal"
            ? current.reviews?.some((r) => r.key === job.id)
            : job?.kind === "primary_final"
              ? !!item
              : current.follow_ups?.some(
                  (f) =>
                    f.run.id === job?.id &&
                    (f.run.target?.kind === "mention" ? "mention" : "reply") ===
                      job?.kind,
                ) ||
                (job?.kind === "mention" &&
                  current.mentions?.some((m) => m.work_id === job.id));
      const evidence = root.querySelector<HTMLElement>("[data-item-evidence]")!;
      if (!found || missingDetail) {
        evidence.textContent =
          missingDetail ??
          "This exact saved PR iteration or job is unavailable. No other item was selected.";
        evidence.setAttribute("role", "status");
        reviews.replaceChildren();
        followUps([]);
        root
          .querySelector<HTMLElement>("#thread-follow-ups")!
          .replaceChildren();
        root.querySelector<HTMLElement>("[data-normal-heading]")!.hidden = true;
        root.querySelector<HTMLElement>("[data-conversation-heading]")!.hidden =
          true;
        detailSignature = "";
        reviewsSignature = "";
        return;
      }
      selection = item;
      const next = JSON.stringify([target, item]);
      if (next !== detailSignature) {
        detailSignature = next;
        evidence.replaceChildren();
        evidence.removeAttribute("role");
        if (item) {
          const title = document.createElement("h2");
          title.textContent = `${item.job.repository_name} #${item.job.number}: ${item.job.title}`;
          const summary = document.createElement("p");
          summary.textContent = item.summary;
          evidence.append(title, summary);
          if (target.type === "item")
            renderItemEvidence(evidence, item, showError, refresh);
          else if (jobDetail?.kind === "primary_final" && item.action_status)
            renderActions(evidence, item.action_status, showError, refresh);
        } else if (
          jobDetail?.kind === "reply" ||
          jobDetail?.kind === "mention"
        ) {
          evidence.textContent =
            "Parent PR iteration context unavailable. No other iteration was selected; captured conversation follows.";
          evidence.setAttribute("role", "status");
        }
      }
    }
    if (selection === undefined && !options.panel) {
      reviews.textContent =
        "Select an available queue item to inspect its saved evidence.";
      reviewsSignature = "";
      followUps([]);
      root.querySelector<HTMLElement>("#thread-follow-ups")!.textContent =
        "Select an available queue item to inspect its conversation.";
      return;
    }
    const candidates = (snapshot.reviews ?? []).filter((r) =>
      jobDetail
        ? jobDetail.kind === "normal" && r.key === jobDetail.id
        : selection === null || !!selection?.review_keys.includes(r.key),
    );
    const replies = (snapshot.follow_ups ?? []).filter((f) =>
      jobDetail
        ? (jobDetail.kind === "reply" || jobDetail.kind === "mention") &&
          f.run.id === jobDetail.id
        : selection === null || !!selection?.follow_up_ids.includes(f.run.id),
    );
    const signature = JSON.stringify([
      candidates,
      snapshot.publications,
      snapshot.items,
      panelDetail,
    ]);
    if (signature !== reviewsSignature) {
      renderReviews(candidates, snapshot.publications ?? []);
      reviewsSignature = signature;
    }
    const mentions = (snapshot.mentions ?? []).filter((m) =>
      jobDetail
        ? jobDetail.kind === "mention" &&
          (m.work_id === jobDetail.id || m.follow_up_id === jobDetail.id)
        : selection === null ||
          (selection?.job.repository_name === m.binding.repository_name &&
            selection.job.account_id === m.binding.account_id &&
            selection.job.number === m.binding.number),
    );
    followUps(replies, snapshot.follow_ups ?? [], mentions);
    if (options.panel) {
      root.querySelector<HTMLElement>("[data-normal-heading]")!.hidden =
        candidates.length === 0;
      reviews.hidden = candidates.length === 0;
      root.querySelector<HTMLElement>("[data-conversation-heading]")!.hidden =
        replies.length === 0 && mentions.length === 0;
      root.querySelector<HTMLElement>("#thread-follow-ups")!.hidden =
        replies.length === 0 && mentions.length === 0;
    }
  }

  async function act(candidate: ReviewCandidate, cancel: boolean) {
    if (pending.has(candidate.key)) return;
    pending.add(candidate.key);
    try {
      if (cancel) {
        await invoke("cancel_review", {
          operationId: candidate.run!.operation.id,
        });
      } else {
        await invoke("start_review", {
          candidateKey: candidate.key,
          confirmTrust: trust.has(candidate.key),
        });
        trust.delete(candidate.key);
      }
    } catch (error) {
      showError(
        typeof error === "string"
          ? error
          : "Review action failed. Check local diagnostics.",
      );
    } finally {
      pending.delete(candidate.key);
      reviewsSignature = "";
      await refresh();
    }
  }

  async function publicationAction(
    candidate: ReviewCandidate,
    publication: PublicationCandidate,
    cancel: boolean,
  ) {
    if (pending.has(candidate.key)) return;
    pending.add(candidate.key);
    try {
      if (cancel) {
        await invoke("cancel_publication", {
          publicationId: publication.publication!.id,
        });
      } else {
        await invoke("publish_review", {
          reviewOperationId: candidate.run!.operation.id,
        });
      }
      publishConsent.delete(candidate.run!.operation.id);
    } catch (error) {
      showError(
        typeof error === "string"
          ? error
          : "Publication action failed. Check the original review before retrying.",
      );
    } finally {
      pending.delete(candidate.key);
      reviewsSignature = "";
      await refresh();
    }
  }

  function renderPublication(
    row: HTMLElement,
    candidate: ReviewCandidate,
    state: PublicationCandidate,
  ) {
    const publication = state.publication;
    if (state.local_only && !publication) {
      const info = document.createElement("p");
      info.textContent =
        "Local-only evidence: comment publication is off. The PR author has not been notified; no manual machine-publication task is pending.";
      row.append(info);
      return;
    }
    const info = document.createElement("p");
    info.className = "publication-state";
    info.textContent = publication
      ? `Publication: ${publication.phase.replaceAll("_", " ")}. Operation: ${publication.operation.state}; attempt ${publication.operation.attempt_count}; deadline ${time(publication.operation.retry_deadline)}; prior budgets ${publication.history.length}.`
      : `Publication: ${state.automatic ? "automatic when eligible" : "waiting for confirmation"}.`;
    row.append(info);
    if (state.blocked || publication?.error) {
      const error = document.createElement("p");
      error.className = "review-failure";
      error.textContent = [state.blocked, publication?.error]
        .filter(Boolean)
        .join(" ");
      row.append(error);
    }
    if (publication?.uncertain) {
      const warning = document.createElement("p");
      warning.textContent =
        "GitHub outcome is unresolved. Reconciliation checks the original batch; it does not authorize a replacement.";
      row.append(warning);
    }
    const receipt = publication?.receipts.at(-1);
    if (receipt) {
      const remote = document.createElement("p");
      remote.textContent = `Confirmed GitHub review ${receipt.review_id}: ${receipt.state}. ${receipt.comment_ids.length} inline comment receipt(s). This is not GitHub approval.`;
      row.append(remote);
    }
    if (publication?.batch?.unmappable.length) {
      const warning = document.createElement("p");
      warning.textContent = `${publication.batch.unmappable.length} finding(s) could not be mapped to the reviewed diff and were not posted. All findings remain visible above.`;
      row.append(warning);
    }
    const terminal =
      publication?.phase === "stale" ||
      (publication?.operation.state === "completed" &&
        ["published", "stale_after_publication"].includes(publication.phase)) ||
      receipt?.state === "deleted";
    const working =
      publication &&
      ["running", "queued", "interrupted"].includes(
        publication.operation.state,
      );
    if (working && !publication.cancelled && receipt?.state !== "commented") {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.textContent = "Withdraw publication confirmation";
      cancel.disabled = pending.has(candidate.key);
      cancel.onclick = () => {
        cancel.disabled = true;
        void publicationAction(candidate, state, true);
      };
      row.append(cancel);
    } else if (!terminal && (!state.blocked || publication)) {
      const label = document.createElement("label");
      const consent = document.createElement("input");
      consent.type = "checkbox";
      consent.checked = publishConsent.has(candidate.run!.operation.id);
      label.append(
        consent,
        `Publish or reconcile this exact revision as ${candidate.job.account_login} (${candidate.job.account_id}). This submits comments, not approval.`,
      );
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = publication
        ? "Reconcile / retry publication"
        : "Publish review";
      button.disabled = !consent.checked || pending.has(candidate.key);
      consent.onchange = () => {
        if (consent.checked) publishConsent.add(candidate.run!.operation.id);
        else publishConsent.delete(candidate.run!.operation.id);
        button.disabled = !consent.checked || pending.has(candidate.key);
      };
      button.onclick = () => {
        button.disabled = true;
        void publicationAction(candidate, state, false);
      };
      row.append(label, button);
    }
    if (
      (!candidate.run?.result?.reviewed_base_sha ||
        state.blocked ||
        publication?.phase === "stale") &&
      !candidate.blocked
    ) {
      const retry = document.createElement("button");
      retry.type = "button";
      retry.textContent = "Review again";
      retry.disabled =
        pending.has(candidate.key) ||
        (candidate.trust_required && !trust.has(candidate.key));
      if (candidate.trust_required) {
        const label = document.createElement("label");
        const consent = document.createElement("input");
        consent.type = "checkbox";
        consent.checked = trust.has(candidate.key);
        label.append(
          consent,
          "I trust this exact revision for another read-only AI review.",
        );
        consent.onchange = () => {
          if (consent.checked) trust.add(candidate.key);
          else trust.delete(candidate.key);
          retry.disabled = pending.has(candidate.key) || !consent.checked;
        };
        row.append(label);
      }
      retry.onclick = () => {
        retry.disabled = true;
        void act(candidate, false);
      };
      row.append(retry);
    }
  }

  function renderReviews(
    candidates: ReviewCandidate[],
    publications: PublicationCandidate[],
  ) {
    reviews.replaceChildren();
    if (!candidates.length) {
      reviews.textContent =
        "No assigned reviews. Configure an Agent with a Copilot account and model, assign it to a repository, then detect an eligible revision.";
      return;
    }
    for (const candidate of candidates) {
      const row = document.createElement("article");
      row.className = "review-run";
      const heading = document.createElement("h3");
      const run = candidate.run;
      const capturedJob = run?.job ?? candidate.job;
      heading.textContent = `${capturedJob.repository_name} #${capturedJob.number} / ${run?.selection.agent.name ?? candidate.agent_name}`;
      const identity = document.createElement("p");
      identity.className = "hint";
      identity.textContent = `GitHub: ${capturedJob.account_login} (${capturedJob.account_id}). Head ${capturedJob.head_sha}.`;
      row.append(heading, identity);
      if (candidate.job.work) {
        const work = candidate.job.work;
        const provenance = document.createElement("p");
        provenance.textContent = `Normal pass ${work.pass_ordinal}; iteration ${work.iteration} (${work.iteration_id}); queue order ${work.enqueue_order}; work ID ${work.id}; cause: ${work.trigger.replaceAll("_", " ")}. Retry attempts are separate.`;
        row.append(provenance);
      }
      const state = document.createElement("p");
      state.textContent = run
        ? `${run.phase}. State: ${run.operation.state}; attempt ${run.operation.attempt_count}; deadline ${run.operation.attempt_count === 0 ? "starts at first execution" : time(run.operation.retry_deadline)}. Copilot account: ${run.selection.agent.ai_account?.account_id ?? "not captured"}; model: ${run.selection.agent.model}.`
        : (candidate.blocked ??
          (candidate.trust_required
            ? "Trust confirmation required for this exact revision."
            : "Waiting for manual start or the automatic start gate."));
      row.append(state);
      const configuration = (
        label: string,
        selection: import("./resources").ReviewSelection | null | undefined,
      ) => {
        const details = document.createElement("details");
        const summary = document.createElement("summary");
        summary.textContent = label;
        details.append(summary);
        const text = (title: string, value: unknown) => {
          const heading = document.createElement("h4");
          heading.textContent = title;
          const body = document.createElement("pre");
          body.textContent =
            typeof value === "string"
              ? value
              : (JSON.stringify(value, null, 2) ?? "Not captured");
          details.append(heading, body);
        };
        if (!selection) {
          text(
            "Unavailable",
            candidate.blocked ?? "No saved planned configuration is available.",
          );
        } else {
          text("Agent", selection.agent);
          text("Agent prompt", selection.agent.prompt);
          text("Repository policy", selection.policy);
          text("Review preset", selection.preset);
          if (selection.configuration) {
            text(
              "Repository and assignments",
              selection.configuration.repository,
            );
            text(
              "Captured assignment authority (not a current provider grant)",
              selection.configuration.authority,
            );
            if (!selection.configuration.doctrines.length)
              text("Doctrines", "None selected");
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
        }
        row.append(details);
      };
      if (run) configuration("Captured execution configuration", run.selection);
      if (run?.feedback_context) {
        const details = document.createElement("details");
        const summary = document.createElement("summary");
        summary.textContent = `Captured prior feedback (${run.feedback_context.length})`;
        const body = document.createElement("pre");
        body.textContent = JSON.stringify(run.feedback_context, null, 2);
        details.append(summary, body);
        row.append(details);
      }
      if (!run || ["queued", "interrupted"].includes(run.operation.state))
        configuration(
          "Planned configuration (revalidated at start; not execution evidence)",
          candidate.planned_selection,
        );
      if (run?.error) {
        const error = document.createElement("p");
        error.className = "review-failure";
        error.textContent = run.error;
        row.append(error);
      }
      if (candidate.blocked && run) {
        const blocked = document.createElement("p");
        blocked.textContent = candidate.blocked;
        row.append(blocked);
      }
      const result = run?.result;
      if (result) {
        if (result.output.feedback_conflict) {
          const conflict = document.createElement("p");
          conflict.textContent =
            "New output overlaps a human-closed concern's file. It was not republished; human judgment is required.";
          row.append(conflict);
          for (const finding of result.output.held_findings ?? []) {
            const held = document.createElement("p");
            held.textContent = `Held locally: ${finding.path}: ${finding.title}. ${finding.explanation}`;
            row.append(held);
          }
        }
        const decision = document.createElement("p");
        decision.className = "review-decision";
        decision.textContent =
          result.output.decision === "machine_sign_off"
            ? "Local result: machine sign-off. Automated analysis completed; publication and handoff status are separate. This is not GitHub approval."
            : "Human input required by the review. Published findings wait on the PR author; unpublished findings still need your attention.";
        const synopsis = document.createElement("p");
        synopsis.textContent = result.output.synopsis;
        row.append(decision, synopsis);
        for (const finding of result.output.findings) {
          const item = document.createElement("details");
          const summary = document.createElement("summary");
          summary.textContent = `${finding.severity}: ${finding.title} (${finding.path}, ${finding.side} line ${finding.line}; confidence ${finding.confidence}%)`;
          const explanation = document.createElement("p");
          explanation.textContent = finding.explanation;
          item.append(summary, explanation);
          row.append(item);
        }
        const guide = document.createElement("details");
        guide.open = expanded.has(candidate.key);
        guide.addEventListener("toggle", () => {
          if (guide.open) expanded.add(candidate.key);
          else expanded.delete(candidate.key);
        });
        const summary = document.createElement("summary");
        summary.textContent = `Complete file guide (${result.output.files.length} files)`;
        const list = document.createElement("ol");
        for (const file of result.output.files) {
          const item = document.createElement("li");
          const path = document.createElement("strong");
          path.textContent = file.path;
          const queueItem = snapshot?.items?.find((item) =>
            item.review_keys.includes(candidate.key),
          );
          if (queueItem) {
            const link = document.createElement("a");
            link.href = "#";
            link.textContent = file.path;
            link.title = "Open this file in the current GitHub PR diff";
            link.onclick = (event) => {
              event.preventDefault();
              void openDestination(queueItem.id, file.path, showError);
            };
            path.replaceChildren(link);
          }
          item.append(path, `: ${file.explanation}`);
          list.append(item);
        }
        guide.append(summary, list);
        const usage = document.createElement("p");
        usage.className = "hint";
        usage.textContent = `Session ${result.session_id}; runtime ${result.runtime_version}; model ${result.model}. Tokens: ${result.input_tokens} input, ${result.output_tokens} output. Read-tool calls: ${result.tool_calls}.`;
        row.append(guide, usage);
        const publication = publications.find(
          (p) => p.review_operation_id === run.operation.id,
        );
        if (publication) renderPublication(row, candidate, publication);
      } else if (!candidate.blocked) {
        const isRunning = run?.operation.state === "running";
        const isQueued =
          !!run && ["queued", "interrupted"].includes(run.operation.state);
        let consent: HTMLInputElement | undefined;
        if (candidate.trust_required && !isRunning) {
          const label = document.createElement("label");
          consent = document.createElement("input");
          consent.type = "checkbox";
          consent.checked = trust.has(candidate.key);
          label.append(
            consent,
            "I trust this exact revision for read-only AI review. This does not allow code execution or publication.",
          );
          row.append(label);
        }
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = isRunning
          ? "Cancel review"
          : isQueued
            ? "Cancel queued review"
            : run
              ? "Retry review"
              : "Start review";
        const updateDisabled = () => {
          button.disabled =
            pending.has(candidate.key) ||
            (!isRunning &&
              !isQueued &&
              candidate.trust_required &&
              !run?.trust_confirmed &&
              !trust.has(candidate.key));
        };
        consent?.addEventListener("change", () => {
          if (consent.checked) trust.add(candidate.key);
          else trust.delete(candidate.key);
          updateDisabled();
        });
        updateDisabled();
        button.addEventListener("click", () => {
          button.disabled = true;
          void act(candidate, isRunning || isQueued);
        });
        row.append(button);
      }
      reviews.append(row);
    }
  }

  async function refresh() {
    if (loading || !root.isConnected) return;
    loading = true;
    const requestedDetail = detailRevision;
    try {
      snapshot = await invoke<MonitoringSnapshot>("monitoring_snapshot");
      if (requestedDetail === detailRevision) missingDetail = null;
      options.onSnapshot?.(snapshot);
      void refreshAutomation();
      if (snapshot.items)
        queue(
          options.panel
            ? snapshot.items.filter(
                (i) =>
                  ![
                    "closed",
                    "merged",
                    "stale",
                    "stale_after_publication",
                    "waiting_for_author",
                  ].includes(i.state),
              )
            : snapshot.items,
        );
      renderEvidence();
      health.replaceChildren();
      jobs.replaceChildren();
      if (snapshot.global_scan) {
        const scan = document.createElement("p");
        scan.textContent = `Global scan: ${snapshot.global_scan.schedule_key}. Next scan: ${snapshot.global_scan.next_run > 0 ? time(snapshot.global_scan.next_run) : "Unavailable; choose a global cron schedule in Settings"}. Pending repositories: ${snapshot.global_scan.pending.length}.`;
        health.append(scan);
      }
      if (!snapshot.health.length) {
        health.textContent =
          "No configured repositories. Add and bind a GitHub repository in Settings.";
      }
      for (const item of snapshot.health) {
        const row = document.createElement("p");
        const state = healthLabel(item);
        const next = !item.enabled
          ? "Disabled"
          : item.schedule_available
            ? time(item.next_run)
            : "Unavailable";
        const account = item.provider_account_id
          ? item.account_login
            ? `${item.account_login} (${item.provider_account_id})`
            : `stable ID ${item.provider_account_id} (login unavailable)`
          : "unbound";
        const assignment = item.assignment_id
          ? `${item.agent_name ?? "Missing agent"} (${item.agent_id ?? "unknown agent ID"}), assignment ${item.assignment_id}`
          : snapshot.global_scan
            ? "repository read; fans out to the scan's assignments"
            : "legacy repository schedule";
        row.textContent = `${item.name}: ${state}. Acting account: ${account}. Assignment: ${assignment}. Schedule: ${item.schedule_key || "Unavailable"}. Last attempt: ${time(item.last_attempt)}. Last success: ${time(item.last_success)}. Next run: ${next}. Last failure: ${item.last_failure ?? "None"}.`;
        if (item.operation) {
          row.append(
            ` Operation: ${item.operation.operation_type}; attempts ${item.operation.attempt_count}; retry deadline ${time(item.operation.retry_deadline)}.`,
          );
        }
        if (
          item.operation?.state === "manual_retry" ||
          item.operation?.state === "failed"
        ) {
          const retry = document.createElement("button");
          retry.type = "button";
          retry.textContent =
            item.operation.state === "failed"
              ? "Retry after correction"
              : "Retry";
          retry.addEventListener("click", async () => {
            retry.disabled = true;
            try {
              await invoke("retry_monitoring_operation", {
                operationId: item.operation!.id,
              });
              await refresh();
            } catch {
              showError(
                "Could not retry the monitoring operation. Check its current state and local diagnostics.",
              );
            } finally {
              retry.disabled = false;
            }
          });
          row.append(" ", retry);
        }
        health.append(row);
      }
      if (!snapshot.jobs.length)
        jobs.textContent = snapshot.tracked?.length
          ? `${snapshot.tracked.length} pull requests tracked; no Agent jobs yet. Assign an Agent; its first normal pass is created by the next global scan.`
          : "No eligible revisions detected.";
      for (const job of snapshot.jobs) {
        const row = document.createElement("article");
        const title = document.createElement("h3");
        title.textContent = `${job.repository_name} #${job.number}: ${job.title}`;
        const author =
          job.author_login && job.author_id
            ? `${job.author_login} (${job.author_id})`
            : "deleted or unavailable";
        const admission = job.work?.admission ?? job;
        const reasons = [
          admission.watched_author ? "watched author" : "",
          admission.all_authors ? "all-author monitoring scope" : "",
          admission.requested_reviewer ? "requested reviewer" : "",
        ]
          .filter(Boolean)
          .join(" and ");
        const details = document.createElement("p");
        details.textContent = `Acting account: ${job.account_login} (${job.account_id}). Author: ${author}. Trigger: ${reasons || "historical detection"}. Head ${job.head_sha}. ${waitingLabel(job.waiting)}`;
        if (job.work)
          details.append(
            ` Normal pass ${job.work.pass_ordinal}; iteration ${job.work.iteration}; queue order ${job.work.enqueue_order}; cause ${job.work.trigger.replaceAll("_", " ")}.`,
          );
        row.append(title, details);
        jobs.append(row);
      }
    } catch {
      showError(
        "Could not read monitoring state. Check local storage and diagnostics; this is not an empty successful check.",
      );
    } finally {
      loading = false;
      void refreshNotifications();
    }
  }

  check.addEventListener("click", async () => {
    check.disabled = true;
    try {
      await invoke("check_now");
      await refresh();
    } catch {
      showError(
        "Could not start an immediate check. Check saved schedules and local storage.",
      );
    } finally {
      check.disabled = false;
    }
  });
  void refresh();
  const timer = window.setInterval(() => {
    if (!check.isConnected) window.clearInterval(timer);
    else void refresh();
  }, 5000);
  return {
    refresh,
    detail: (
      target: PanelDetail | undefined,
      missing: string | null = null,
    ) => {
      detailRevision++;
      panelDetail = target;
      missingDetail = missing;
      root.querySelector<HTMLElement>("[data-monitor-overview]")!.hidden =
        !!target;
      root.querySelector<HTMLElement>("[data-monitor-detail]")!.hidden =
        !target;
      if (target && !snapshot)
        root.querySelector<HTMLElement>("[data-item-evidence]")!.textContent =
          missing ?? "Loading this exact saved destination...";
      renderEvidence();
      void refresh();
    },
    automation: (destination: HTMLElement) => {
      if (automationRoot.parentElement !== destination)
        destination.prepend(automationRoot);
    },
    tools: (destination: HTMLElement) => {
      recoveryRoot.hidden = false;
      destination.append(recoveryRoot);
    },
  };
}
import { trayAdjective } from "./platform";
