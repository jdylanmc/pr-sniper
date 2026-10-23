import { restoreShared } from "./interned";
import { invoke } from "@tauri-apps/api/core";
import { whenReleased } from "./stable-dom";
import { actualIntelligence } from "./copilot";
import { mountAutomation, type AutomationSnapshot } from "./automation";
import type { PanelDetail } from "./panel";
import { renderActions } from "./actions";
import { detailsIcon, renderQueueStatus, type FixTarget } from "./queue-detail";
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
  humanQueue,
} from "./queue";
import {
  renderConfiguration,
  renderWorkContext,
  workPresentation,
} from "./work-presentation";

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
  last_error?: string | null;
  in_flight: boolean;
  scheduled_run?: number;
  scan_pending?: boolean;
  scan_requested?: boolean;
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
    initial_attempt_at?: number;
    retry_deadline: number;
    next_attempt_at: number | null;
    failure: string | null;
    resume_cycle?: number;
    resume_at?: number | null;
  } | null;
}

export type Job = QueueItem["job"] & {
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
};

export interface MonitoringSnapshot {
  pending_threads?: {
    work_id: string;
    binding: { repository_name: string; number: number };
    thread: { id: string };
    blocked: string | null;
  }[];
  mentions?: MentionRouting[];
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
  exit_writes?: ExitWrite[];
}

// Host execution of a `leave_comments` exit's replies and resolutions.
export interface ExitWrite {
  id: string;
  review_operation_id: string;
  phase: "planned" | "verified" | "completed" | "tossed" | "failed";
  steps: {
    kind: "reply" | "resolve";
    state: "planned" | "attempted" | "done";
    receipt: string | null;
  }[];
  published: boolean;
  consumed: boolean;
  error: string | null;
}

export interface ReportFinding {
  file: string;
  line: number;
  nano: string;
  full: string;
  fix: string;
}

export type ExitOutcome =
  | {
      exit: "comments";
      new_comments: unknown[];
      body?: string;
      reply_comments: unknown[];
      resolve_own_threads: string[];
    }
  | { exit: "ready_for_human"; summary: string }
  | { exit: "wait"; reason: "ci" | "draft" | "required_reviews" }
  | { exit: "approve"; comment: string }
  | { exit: "approve_and_merge"; comment: string }
  | { exit: "submit_report" }
  | {
      exit: "lgtm";
      comment: string;
      resolve_own_threads: string[];
      action: "ready" | "approve" | "merge";
    };

export interface PublicationCandidate {
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
      | "discarded"
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

export interface ReviewCandidate {
  key: string;
  assignment_id: string;
  agent_name: string;
  job: Job;
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
    phase: string;
    error: string | null;
    selection: import("./resources").ReviewSelection;
    job?: Job;
    operation: NonNullable<Health["operation"]>;
    mode?: "legacy" | "packet";
    // Content-free record of the packet the run was given.
    packet?: {
      state_id: string;
      hash: string;
      schema: number;
      tool_set: string[];
      exits_offered: string[];
      exits_denied: unknown[];
    } | null;
    // The Rubber Duck Agent whose setup failed the last attempt.
    failed_duck?: string | null;
    // Duck calls of the last attempt that ended without a result.
    attempt_ducks?: DuckCall[];
    result: {
      reviewed_base_sha?: string | null;
      output: {
        exit?: ExitOutcome | null;
        report?: { findings: ReportFinding[] } | null;
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
      intelligence?: import("./policy").AgentIntelligence | null;
      input_tokens: number;
      output_tokens: number;
      tool_calls: number;
      coverage?: { read: number; changed: number } | null;
      ducks?: DuckCall[];
    } | null;
  } | null;
}

/** One Rubber Duck call, stored with the primary's result. */
export interface DuckCall {
  ordinal: number;
  input: {
    kind: "line" | "thread" | "pr";
    path?: string;
    line?: number;
    end_line?: number;
    thread_id?: string;
    claim: string;
  };
  outcome:
    | {
        outcome: "verdict";
        verdict: {
          verdict: "confirmed" | "revise" | "rejected" | "unverifiable";
          nano: string;
        };
      }
    | { outcome: "failure"; failure: string };
  /** Typed cause of a failed call, when known. */
  reason?: string;
  agent: string;
  model: string;
  input_tokens?: number;
  output_tokens?: number;
  duration_ms: number;
  /** Further refusals folded into this entry. */
  repeats?: number;
  /** Refusals of other kinds folded in past the entry cap, by failure id. */
  others?: Record<string, number>;
}

const time = (value: number | null) =>
  value === null ? "Never" : new Date(value * 1000).toLocaleString();

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
    navigate?: (detail: PanelDetail, opener?: HTMLElement) => void;
    onSnapshot?: (snapshot: MonitoringSnapshot | undefined) => void;
    onAutomation?: (snapshot: AutomationSnapshot | undefined) => void;
    fix?: (target: FixTarget) => void;
  } = {},
) {
  root.innerHTML = `<div data-monitor-overview><section id="automation-controls"></section>
    <h2>Inbox</h2><section id="handoff-queue"></section></div>
    <div data-monitor-detail><section data-item-evidence></section>
    <div data-evidence-guidance><h2 id="evidence-heading" class="sr-only" tabindex="-1">Review evidence and actions</h2></div>
    
    <details data-queue-details><summary aria-label="Details" title="Details">${detailsIcon}</summary><section data-item-more></section>
    <h2 data-normal-heading>Agent reviews</h2><section id="agent-reviews"></section>
    <h2 data-conversation-heading>Thread follow-ups</h2><section id="thread-follow-ups"></section></details></div>`;
  const automationRoot = root.querySelector<HTMLElement>(
    "#automation-controls",
  )!;
  let automation: AutomationSnapshot | undefined;
  const refreshAutomation = mountAutomation(
    automationRoot,
    (value) => {
      automation = value;
      options.onAutomation?.(value);
      whenReleased(renderEvidence);
    },
    { compact: options.panel, onError: options.panel ? showError : undefined },
  );
  const reviews = root.querySelector<HTMLElement>("#agent-reviews")!;
  const expanded = new Set<string>();
  const pending = new Set<string>();
  const publishConsent = new Set<string>();
  let reviewsSignature = "";
  let loading = false;
  let snapshot: MonitoringSnapshot | undefined;
  let panelDetail: PanelDetail | undefined;
  let missingDetail: string | null = null;
  let detailSignature = "";
  let evidenceTarget = "";
  let detailRevision = 0;
  let selection: QueueItem | null | undefined;
  const followUps = renderFollowUps(
    root.querySelector<HTMLElement>("#thread-follow-ups")!,
    showError,
    refresh,
  );
  const queue = renderQueue(
    root.querySelector<HTMLElement>("#handoff-queue")!,
    showError,
    (item, focus, opener) => {
      if (options.panel) {
        if (focus && item)
          options.navigate?.({ type: "item", item_id: item.id }, opener);
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
    const overview = root.querySelector<HTMLElement>(
      "[data-monitor-overview]",
    )!;
    overview.querySelector("h2")!.remove();
    root.querySelector<HTMLElement>("[data-monitor-detail]")!.hidden = true;
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
      root.querySelector<HTMLElement>("[data-evidence-guidance]")!.hidden =
        !!job;
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
      const details = root.querySelector<HTMLDetailsElement>(
        "[data-queue-details]",
      )!;
      const more = root.querySelector<HTMLElement>("[data-item-more]")!;
      details.hidden = !found || !!missingDetail;
      if (!found || missingDetail) {
        evidence.textContent = missingDetail ?? "Unavailable";
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
      const next = JSON.stringify([
        target,
        jobDetail
          ? [
              workPresentation(current, jobDetail.kind, jobDetail.id),
              item?.id,
              jobDetail.kind === "primary_final" && item?.action_status,
            ]
          : item,
        jobDetail &&
          automation?.work.find(
            (entry) => entry.key.kind === job?.kind && entry.key.id === job.id,
          ),
        !!automation,
      ]);
      if (next !== detailSignature) {
        const sameJob =
          !!jobDetail && evidenceTarget === JSON.stringify(target);
        const disclosures = sameJob
          ? [
              ...evidence.querySelectorAll<HTMLDetailsElement>(
                "[data-disclosure]",
              ),
            ].map(
              (element) => [element.dataset.disclosure!, element.open] as const,
            )
          : [];
        const focused = document.activeElement;
        const focusSelector =
          sameJob &&
          focused instanceof HTMLElement &&
          evidence.contains(focused)
            ? focused.matches(".job-provider-link")
              ? ".job-provider-link"
              : focused.matches("summary") &&
                  focused.parentElement?.dataset.disclosure
                ? `[data-disclosure="${CSS.escape(focused.parentElement.dataset.disclosure)}"] > summary`
                : undefined
            : undefined;
        const scroller = evidence.closest<HTMLElement>(".panel-content");
        const scroll = scroller?.scrollTop;
        const changedTarget = evidenceTarget !== JSON.stringify(target);
        detailSignature = next;
        evidenceTarget = JSON.stringify(target);
        evidence.replaceChildren();
        evidence.removeAttribute("role");
        if (jobDetail) {
          renderWorkContext(
            evidence,
            current,
            jobDetail.kind,
            jobDetail.id,
            automation,
            item
              ? () => void openDestination(item.id, null, showError)
              : undefined,
          );
          if (jobDetail.kind === "primary_final" && item?.action_status) {
            const context = document.createElement("h2");
            context.textContent = "Provider";
            evidence.append(context);
            renderActions(
              evidence,
              item.action_status,
              showError,
              refresh,
              false,
            );
          }
        } else if (item) {
          renderQueueStatus(evidence, item, {
            snapshot: current,
            showError,
            refresh,
            fix: options.fix,
          });
          more.replaceChildren();
          renderItemEvidence(more, item, showError, refresh, true);
        }
        if (jobDetail || !options.panel) details.open = true;
        else if (changedTarget) details.open = false;
        details.classList.toggle("bare", !!jobDetail || !options.panel);
        for (const [id, open] of disclosures) {
          const element = evidence.querySelector<HTMLDetailsElement>(
            `[data-disclosure="${CSS.escape(id)}"]`,
          );
          if (element) element.open = open;
        }
        if (focusSelector)
          evidence
            .querySelector<HTMLElement>(focusSelector)
            ?.focus({ preventScroll: true });
        if (sameJob && scroller && scroll !== undefined)
          scroller.scrollTop = scroll;
      }
    }
    if (selection === undefined && !options.panel) {
      reviews.textContent = "No selection";
      reviewsSignature = "";
      followUps([]);
      root.querySelector<HTMLElement>("#thread-follow-ups")!.textContent =
        "No selection";
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
    const publications = (snapshot.publications ?? []).filter((publication) =>
      candidates.some(
        (candidate) =>
          candidate.run?.operation.id === publication.review_operation_id,
      ),
    );
    const signature = JSON.stringify([
      candidates,
      publications,
      candidates.map(
        (candidate) =>
          current.items?.find((item) =>
            item.review_keys.includes(candidate.key),
          )?.id,
      ),
      panelDetail,
    ]);
    if (signature !== reviewsSignature) {
      renderReviews(candidates, publications);
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
    followUps(replies, snapshot.follow_ups ?? [], mentions, !jobDetail);
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
        });
      }
    } catch (error) {
      showError(typeof error === "string" ? error : "Failed");
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
      showError(typeof error === "string" ? error : "Failed");
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
      info.textContent = "Local only";
      row.append(info);
      return;
    }
    const info = document.createElement("p");
    info.className = "publication-state";
    info.textContent = publication
      ? `Publication: ${publication.phase.replaceAll("_", " ")}; ${publication.operation.state}; attempt ${publication.operation.attempt_count}; deadline ${time(publication.operation.retry_deadline)}; prior ${publication.history.length}`
      : `Publication: ${state.automatic ? "automatic" : "unconfirmed"}`;
    row.append(info);
    // A discarded batch is replaced by a fresh review; its refusal is history.
    const refusal = publication?.phase === "discarded" ? null : state.blocked;
    if (refusal || publication?.error) {
      const error = document.createElement("p");
      error.className = "review-failure";
      error.textContent = [refusal, publication?.error]
        .filter(Boolean)
        .join(" ");
      row.append(error);
    }
    if (publication?.uncertain) {
      const warning = document.createElement("p");
      warning.textContent = "Outcome unresolved";
      row.append(warning);
    }
    const receipt = publication?.receipts.at(-1);
    if (receipt) {
      const remote = document.createElement("p");
      remote.textContent = `Review ${receipt.review_id}: ${receipt.state}; ${receipt.comment_ids.length} comments`;
      row.append(remote);
    }
    if (publication?.batch?.unmappable.length) {
      const warning = document.createElement("p");
      warning.textContent = `${publication.batch.unmappable.length} unmapped`;
      row.append(warning);
    }
    const terminal =
      publication?.phase === "stale" ||
      publication?.phase === "discarded" ||
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
      cancel.textContent = "Withdraw publication";
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
        `Publish as ${candidate.job.account_login} (${candidate.job.account_id})`,
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
        publication?.phase === "stale") &&
      !candidate.blocked
    ) {
      const retry = document.createElement("button");
      retry.type = "button";
      retry.textContent = "Review again";
      retry.disabled = pending.has(candidate.key);
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
      reviews.textContent = "No reviews";
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
      identity.textContent = `${capturedJob.account_login} (${capturedJob.account_id}) \u00b7 ${capturedJob.head_sha}`;
      row.append(heading, identity);
      if (candidate.job.work) {
        const work = candidate.job.work;
        const provenance = document.createElement("p");
        provenance.textContent = `Normal pass ${work.pass_ordinal}; iteration ${work.iteration} (${work.iteration_id}); queue order ${work.enqueue_order}; work ID ${work.id}; ${work.trigger.replaceAll("_", " ")}`;
        row.append(provenance);
      }
      const state = document.createElement("p");
      state.textContent = run
        ? `${run.phase}; ${run.operation.state}; attempt ${run.operation.attempt_count}; deadline ${run.operation.attempt_count === 0 ? "none" : time(run.operation.retry_deadline)}; ${run.selection.agent.ai_account?.account_id ?? "?"}; ${run.selection.agent.model}`
        : (candidate.blocked ?? "Queued");
      row.append(state);
      const singleJob = options.panel && panelDetail?.type === "job";
      if (!singleJob && run && run.operation.attempt_count > 0)
        renderConfiguration(
          row,
          "Captured execution configuration",
          run.selection,
        );
      if (run?.feedback_context) {
        const details = document.createElement("details");
        const summary = document.createElement("summary");
        summary.textContent = `Captured prior feedback (${run.feedback_context.length})`;
        const body = document.createElement("pre");
        body.textContent = JSON.stringify(run.feedback_context, null, 2);
        details.append(summary, body);
        row.append(details);
      }
      if (
        (!singleJob && (!run || run.operation.attempt_count === 0)) ||
        (run &&
          run.operation.attempt_count > 0 &&
          ["queued", "interrupted"].includes(run.operation.state))
      )
        renderConfiguration(
          row,
          "Planned configuration",
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
          conflict.textContent = "Held: human input required";
          row.append(conflict);
          for (const finding of result.output.held_findings ?? []) {
            const held = document.createElement("p");
            held.textContent = `Held: ${finding.path}: ${finding.title}`;
            row.append(held);
          }
        }
        const exit = result.output.exit;
        const decision = document.createElement("p");
        decision.className = "review-decision";
        decision.textContent = exit
          ? exit.exit.replaceAll("_", " ")
          : result.output.decision === "machine_sign_off"
            ? "Machine sign-off"
            : "Human input required";
        row.append(decision);
        // Legacy results (history only) carry a synopsis, findings and a guide.
        if (!exit) {
          const synopsis = document.createElement("p");
          synopsis.textContent = result.output.synopsis;
          row.append(synopsis);
        }
        for (const finding of exit ? [] : result.output.findings) {
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
        for (const file of [...result.output.files].sort(
          (a, b) => a.order - b.order,
        )) {
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
            link.title = "Open file";
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
        usage.textContent = `Session ${result.session_id}; runtime ${result.runtime_version}; model ${result.model}; ${actualIntelligence(result.intelligence)}; tokens ${result.input_tokens} in / ${result.output_tokens} out; tool calls ${result.tool_calls}`;
        if (!exit) row.append(guide);
        row.append(usage);
        const publication = publications.find(
          (p) => p.review_operation_id === run.operation.id,
        );
        if (publication) renderPublication(row, candidate, publication);
      } else if (!candidate.blocked && run) {
        const isRunning = run?.operation.state === "running";
        const isQueued =
          !!run && ["queued", "interrupted"].includes(run.operation.state);
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = isRunning
          ? "Cancel review"
          : isQueued
            ? "Cancel queued review"
            : "Retry review";
        button.disabled = pending.has(candidate.key);
        button.addEventListener("click", () => {
          button.disabled = true;
          void act(candidate, isRunning || isQueued);
        });
        row.append(button);
      }
      reviews.append(row);
    }
  }

  // Deferred like every background redraw; queued after any earlier redraw,
  // so the failure is what remains once the press ends.
  function showUnavailable() {
    if (snapshot) return;
    root.querySelector<HTMLElement>("#handoff-queue")!.textContent =
      "Unavailable";
    root.querySelector<HTMLElement>("[data-item-evidence]")!.textContent =
      "Unavailable";
    reviews.replaceChildren();
    reviewsSignature = "";
    detailSignature = "";
    followUps([]);
  }

  async function refresh() {
    if (loading || !root.isConnected) return;
    loading = true;
    const requestedDetail = detailRevision;
    try {
      snapshot = restoreShared<MonitoringSnapshot>(
        await invoke("monitoring_snapshot"),
      );
      if (requestedDetail === detailRevision) missingDetail = null;
      options.onSnapshot?.(snapshot);
      void refreshAutomation();
      if (snapshot.items)
        queue(options.panel ? humanQueue(snapshot.items) : snapshot.items);
      // Evidence controls redraw after a press ends so its click survives.
      whenReleased(renderEvidence);
    } catch {
      snapshot = undefined;
      options.onSnapshot?.(undefined);
      if (options.panel) {
        queue.invalidate();
        whenReleased(showUnavailable);
      }
      showError("Failed");
    } finally {
      loading = false;
      void refreshAutomation();
    }
  }

  void refresh();
  const timer = window.setInterval(() => {
    if (!root.isConnected) window.clearInterval(timer);
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
          missing ?? "Loading";
      renderEvidence();
      void refresh();
    },
    automation: (destination: HTMLElement) => {
      if (automationRoot.parentElement !== destination)
        destination.prepend(automationRoot);
    },
  };
}
