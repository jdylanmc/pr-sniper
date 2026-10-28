import { invoke } from "@tauri-apps/api/core";
import { whenReleased } from "./stable-dom";
import { renderActions, type ActionStatus } from "./actions";
import type { FailureClass } from "./automation";
import { renderFacts } from "./work-presentation";
import { progressChip, type ExitName, type Progress } from "./exits";

export interface NormalWork {
  id: string;
  item_id: string;
  iteration_id: string;
  iteration: number;
  agent_id: string;
  enqueue_order: number;
  pass_ordinal: number;
  trigger:
    | "admission"
    | "new_revision"
    | "reopened"
    | "assignment_added"
    | "legacy_admission";
  admission: {
    watched_author: boolean;
    all_authors: boolean;
    requested_reviewer: boolean;
  };
  legacy_item_id?: string;
}

export interface QueueItem {
  dismiss_token?: string | null;
  dismissed?: boolean;
  action_status?: ActionStatus | null;
  feedback?: {
    context: {
      id: string;
      root_id: string;
      owner_agent_id: string;
      original_head: string;
      title: string;
      body: string;
    };
    state: string;
    reason: string | null;
  }[];
  id: string;
  aliases?: string[];
  job: {
    work?: NormalWork;
    provider: string;
    configuration_id: string;
    repository_id: string;
    pull_request_id: string;
    trigger_policy: string;
    repository_name: string;
    number: number;
    title: string;
    head_sha: string;
    account_id: string;
    account_login: string;
    author_login: string | null;
    waiting?: string;
  };
  state:
    | "machine_signed_off"
    | "waiting_for_human"
    | "waiting_for_author"
    | "waiting_on_ci"
    | "waiting_on_draft"
    | "waiting_on_reviews"
    | "confirmation_required"
    | "awaiting_publication"
    | "queued"
    | "reviewing"
    | "failed"
    | "stale"
    | "stale_after_publication"
    | "approved"
    | "closed"
    | "merged";
  failure_class?: FailureClass | null;
  // A parked teammate holds the primary back; the cause is typed.
  failed_teammate?: {
    assignment_id: string;
    agent_name: string;
    cause: TeammateCause;
  } | null;
  // The primary's Rubber Duck could not be set up: its Agent name.
  failed_duck?: string | null;
  // A parked run's Agent model refused service.
  failed_model?: FailedModel | null;
  // What failed and its likeliest fix (typed; failed items only).
  failed_operation?: FailedOperation | null;
  remedy?: "sign_in" | "change_model" | "reconcile" | "retry" | null;
  // The primary waits for this teammate's report.
  blocked_by?: {
    teammate: { assignment_id: string; agent_name: string };
  } | null;
  failure_cause?:
    | "publication_failed"
    | "publication_unresolved"
    | "panic"
    | "tossed"
    | "exit_failed"
    | null;
  // The primary's exit and the host effect it still has in flight.
  exit?: ExitName | null;
  progress?: Progress | null;
  resume_at?: number | null;
  summary: string;
  warnings: string[];
  review_keys: string[];
  follow_up_ids: string[];
}

const labels: Record<QueueItem["state"], string> = {
  machine_signed_off: "Ready for your final review",
  waiting_for_human: "Needs your input",
  waiting_for_author: "Waiting for PR author",
  waiting_on_ci: "Waiting on CI",
  waiting_on_draft: "Waiting on draft",
  waiting_on_reviews: "Waiting on reviews",
  confirmation_required: "Needs your confirmation",
  awaiting_publication: "Awaiting comment publication",
  queued: "Queued",
  reviewing: "Reviewing",
  failed: "Failed",
  stale: "Stale review",
  stale_after_publication: "Stale after publication",
  approved: "Approved",
  closed: "Closed on GitHub",
  merged: "Merged on GitHub",
};

export type FailedOperation =
  | "repository_check"
  | "review"
  | "publication"
  | "exit_write"
  | "follow_up"
  | "provider_action";
export type ModelRefusal = "rejected" | "unavailable" | "retired";
export interface FailedModel {
  agent_id: string;
  agent_name: string;
  model: string;
  refusal: ModelRefusal;
  status?: number | null;
}
export const modelRefusals: Record<ModelRefusal, string> = {
  rejected: "Rejected",
  unavailable: "Unavailable",
  retired: "Retired",
};
const modelPath =
  "M8 8h8v8H8zM5 10H3m2 4H3m18-4h-2m2 4h-2M10 5V3m4 2V3m-4 18v-2m4 2v-2M6 6h12v12H6z";
// Model id plus its typed refusal: the HTTP status when known.
export function modelChip(failed: FailedModel): HTMLElement {
  const chip = document.createElement("span");
  chip.className = "queue-teammate queue-model";
  chip.dataset.teammateState = "failed";
  chip.dataset.queueModel = failed.model;
  chip.dataset.modelRefusal = failed.refusal;
  chip.title = `${failed.agent_name}: ${failed.model}`;
  chip.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${modelPath}"/></svg>`;
  const name = document.createElement("span");
  name.textContent = failed.model;
  const code = document.createElement("strong");
  code.textContent = failed.status
    ? String(failed.status)
    : modelRefusals[failed.refusal];
  chip.append(name, code);
  return chip;
}

export type TeammateCause =
  | "timeout"
  | "rate_limited"
  | "network"
  | "provider"
  | "permanent"
  | "superseded"
  | "credentials"
  | "permission"
  | "model"
  | "panic";
export const teammateCauses: Record<TeammateCause, string> = {
  timeout: "Timeout",
  rate_limited: "Rate limit",
  network: "Network",
  provider: "Provider",
  permanent: "Permanent",
  superseded: "Superseded",
  credentials: "Credentials",
  permission: "Permission",
  model: "Model",
  panic: "Panic",
};
const teammatePath =
  "M9 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm7 1a2.5 2.5 0 1 0 0-5M3 19c0-3 2.7-5 6-5s6 2 6 5M17 14c2.3.2 4 1.7 4 4";
// Agent name plus a typed cause (parked) or a clock (waiting): icons, no prose.
export function teammateChip(
  agentName: string,
  cause: TeammateCause | null,
): HTMLElement {
  const chip = document.createElement("span");
  chip.className = "queue-teammate";
  chip.dataset.queueTeammate = agentName;
  chip.dataset.teammateState = cause ? "failed" : "waiting";
  if (cause) chip.dataset.teammateCause = cause;
  chip.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${teammatePath}"/></svg>`;
  const name = document.createElement("span");
  name.textContent = agentName;
  chip.append(name);
  if (cause) {
    const label = document.createElement("strong");
    label.textContent = teammateCauses[cause] ?? cause;
    chip.append(label);
  } else {
    chip.insertAdjacentHTML(
      "beforeend",
      `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18"/></svg>`,
    );
  }
  return chip;
}
function itemTeammateChip(item: QueueItem) {
  if (item.failed_teammate)
    return teammateChip(
      item.failed_teammate.agent_name,
      item.failed_teammate.cause,
    );
  if (item.blocked_by)
    return teammateChip(item.blocked_by.teammate.agent_name, null);
  return null;
}

const credentials = (item: QueueItem) =>
  item.state === "failed" && item.failure_class === "credentials";
const failureChips: Record<FailureClass, string> = {
  credentials: "Credentials",
  permission: "Permission",
  model: "Model",
  network: "Network",
  other: "Failed",
};
const causeChips: Record<string, string> = {
  publication_unresolved: "Unverified",
  publication_failed: "Publish failed",
  panic: "Panic",
  tossed: "Tossed",
  exit_failed: "Write failed",
};
// Access and network classes keep their chip; otherwise the typed cause.
export function failureChip(item: QueueItem) {
  const failureClass = item.failure_class ?? "other";
  if (failureClass === "other" && item.failure_cause)
    return causeChips[item.failure_cause] ?? failureChips.other;
  return failureChips[failureClass];
}
// Compact duration: 45s, 12m, 3h, 2d.
export function compactDuration(seconds: number) {
  const value = Math.max(0, Math.round(seconds));
  if (value < 60) return `${value}s`;
  if (value < 3600) return `${Math.round(value / 60)}m`;
  if (value < 86400) return `${Math.round(value / 3600)}h`;
  return `${Math.round(value / 86400)}d`;
}

const clockPath = "M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18";
const retryPath = "M20 12a8 8 0 1 1-2.3-5.7M20 4v4h-4";
const retryIcon = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${retryPath}"/></svg>`;
// Credentials parks retry too: each attempt re-verifies the account first.
const retryable = (item: QueueItem) => item.state === "failed";

export function humanQueue(items: QueueItem[]): QueueItem[] {
  const current = new Map<string, QueueItem>();
  for (const item of items) {
    const job = item.job;
    const key = JSON.stringify([
      job.provider,
      job.account_id,
      job.configuration_id,
      job.repository_id,
      job.pull_request_id,
    ]);
    const previous = current.get(key);
    const historical = (value: QueueItem) =>
      ["stale", "stale_after_publication"].includes(value.state);
    if (
      !previous ||
      (job.work && previous.job.work
        ? job.work.iteration > previous.job.work.iteration
        : historical(previous) && !historical(item))
    )
      current.set(key, item);
  }
  return [...current.values()]
    .filter(
      (item) =>
        !item.dismissed &&
        ![
          "queued",
          "reviewing",
          "awaiting_publication",
          "waiting_for_author",
          "waiting_on_ci",
          "waiting_on_draft",
          "waiting_on_reviews",
          "approved",
          "closed",
          "merged",
        ].includes(item.state),
    )
    .sort((a, b) => Number(credentials(b)) - Number(credentials(a)));
}

export async function openDestination(
  itemId: string,
  file: string | null,
  showError: (message: string) => void,
) {
  try {
    await invoke("open_queue_destination", { itemId, file });
  } catch (error) {
    showError(typeof error === "string" ? error : "Open failed");
  }
}

export function renderQueue(
  root: HTMLElement,
  showError: (message: string) => void,
  select: (
    item: QueueItem | null | undefined,
    focus: boolean,
    opener?: HTMLElement,
  ) => void,
  refresh: () => Promise<void>,
  options: { compact?: boolean; externalSelection?: boolean } = {},
) {
  if (options.compact) root.setAttribute("aria-label", "Human handoffs");
  const fromUrl = () => new URLSearchParams(location.hash.slice(1)).get("item");
  let selected: string | null | undefined = fromUrl();
  let items: QueueItem[] = [];
  let signature = "";
  let loaded = false;
  let initialized = false;
  let selectionRevision = 0;
  let saving = Promise.resolve();

  function persist(id: string | null) {
    saving = saving.then(async () => {
      try {
        await invoke("select_queue_item", { itemId: id });
      } catch (error) {
        showError(typeof error === "string" ? error : "Not saved");
      }
    });
  }

  if (options.externalSelection) {
    selected = null;
    initialized = true;
  } else
    void invoke<string | null>("queue_selection")
      .then((stored) => {
        if (selectionRevision) return;
        selected = fromUrl() ?? stored;
        initialized = true;
        select(selected === null ? null : undefined, false);
        if (fromUrl() !== null) persist(selected);
        draw(false);
      })
      .catch(() => {
        if (selectionRevision) return;
        initialized = true;
        selected = fromUrl() ?? undefined;
        showError("Restore failed");
        select(undefined, false);
        draw(false);
      });

  function choose(id: string | null, focus: boolean, opener?: HTMLElement) {
    selectionRevision++;
    initialized = true;
    selected = id;
    if (!options.externalSelection) {
      persist(id);
      const url = new URL(location.href);
      url.hash = id ? new URLSearchParams({ item: id }).toString() : "";
      history.replaceState(null, "", url);
    }
    draw(focus, opener);
  }

  function draw(focus: boolean, opener?: HTMLElement) {
    if (!loaded) return;
    const active =
      document.activeElement instanceof HTMLElement &&
      root.contains(document.activeElement)
        ? document.activeElement
        : null;
    const focusedItem =
      active?.closest<HTMLElement>("[data-item-id]")?.dataset.itemId;
    const focusedLabel =
      active instanceof HTMLButtonElement ? active.textContent : null;
    signature = JSON.stringify([items, selected]);
    const matches = (item: QueueItem) =>
      item.id === selected || !!item.aliases?.includes(selected ?? "");
    root.replaceChildren();
    if (!items.length) {
      const empty = document.createElement(options.compact ? "div" : "p");
      if (options.compact) {
        empty.className = "queue-empty";
        empty.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 4h16l2 14H2L4 4Zm-1 9h5l2 3h4l2-3h5M7 8h10"/></svg><span class="sr-only">Empty</span>`;
      } else empty.textContent = "No pull requests";
      root.append(empty);
    }
    for (const item of items) {
      const row = document.createElement("article");
      row.className = "queue-item";
      row.dataset.itemId = item.id;
      row.dataset.state = item.state;
      row.dataset.selected = String(matches(item));
      row.setAttribute(
        "aria-label",
        `${item.job.repository_name} #${item.job.number}`,
      );
      const state = document.createElement("p");
      state.className = "queue-state";
      state.textContent =
        options.compact && item.state === "machine_signed_off"
          ? "Ready for you"
          : item.state === "failed"
            ? failureChip(item)
            : labels[item.state];
      const heading = document.createElement("h3");
      heading.textContent = options.compact
        ? item.job.title
        : `${item.job.repository_name} #${item.job.number}: ${item.job.title}`;
      const context = document.createElement("p");
      context.className = "hint";
      context.textContent = `${item.job.author_login ?? "Unknown"} \u00b7 ${item.job.account_login} (${item.job.account_id}) \u00b7 ${item.job.head_sha}`;
      if (item.job.work)
        context.append(
          ` \u00b7 Iteration ${item.job.work.iteration} (${item.job.work.iteration_id})`,
        );
      if (options.compact) {
        const top = document.createElement("div");
        top.className = "queue-card-top";
        const iteration = document.createElement("span");
        iteration.textContent = item.job.work
          ? `Iteration ${item.job.work.iteration}`
          : "Legacy revision";
        if (
          item.state === "failed" &&
          item.failure_class !== "credentials" &&
          item.resume_at
        ) {
          const resume = document.createElement("span");
          resume.className = "queue-resume";
          resume.dataset.queueResume = "";
          resume.title = "Automatic retry";
          resume.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${clockPath}"/></svg>`;
          resume.append(compactDuration(item.resume_at - Date.now() / 1000));
          state.append(resume);
        }
        top.append(state, iteration);
        const reference = document.createElement("p");
        reference.className = "queue-reference";
        reference.textContent = `${item.job.repository_name} #${item.job.number}`;
        row.append(top, heading, reference);
        const teammate = itemTeammateChip(item);
        if (teammate) row.append(teammate);
        if (item.failed_model) row.append(modelChip(item.failed_model));
        if (item.progress) row.append(progressChip(item.progress));
        const receipts =
          item.action_status?.effects.filter((effect) => effect.receipt) ?? [];
        for (const effect of receipts) {
          const receipt = document.createElement("p");
          receipt.className = "queue-receipt";
          receipt.textContent = `${effect.action === "approve" ? "Approval" : "Merge"} recorded`;
          row.append(receipt);
        }
        const footer = document.createElement("div");
        footer.className = "queue-card-footer";
        const avatar = document.createElement("span");
        avatar.className = "queue-avatar";
        avatar.setAttribute("aria-hidden", "true");
        avatar.textContent = (item.job.author_login ?? "?").slice(0, 2);
        const author = document.createElement("span");
        author.textContent = item.job.author_login ?? "Unknown";
        const trigger = document.createElement("span");
        trigger.className = "queue-trigger";
        trigger.textContent = item.job.work?.admission.requested_reviewer
          ? "Requested reviewer"
          : item.job.work?.admission.watched_author
            ? "Watched author"
            : "PR evidence";
        footer.append(avatar, author, trigger);
        row.append(footer);
      } else {
        row.append(state, heading, context);
        const teammate = itemTeammateChip(item);
        if (teammate) row.append(teammate);
        if (item.failed_model) row.append(modelChip(item.failed_model));
        if (item.progress) row.append(progressChip(item.progress));
      }
      if (!options.compact) renderItemEvidence(row, item, showError, refresh);
      for (const warning of options.compact ? [] : item.warnings) {
        const text = document.createElement("p");
        text.className = "review-failure";
        text.textContent = warning;
        row.append(text);
      }
      const actions = document.createElement("div");
      actions.className = "actions";
      actions.setAttribute("role", "group");
      actions.setAttribute(
        "aria-label",
        `Actions for ${item.job.repository_name} #${item.job.number}; account ${item.job.account_id}; item ${item.id}`,
      );
      const evidence = document.createElement("button");
      evidence.type = "button";
      evidence.textContent = "Evidence and actions";
      if (options.compact) {
        evidence.className = "queue-card-open";
        const label = document.createElement("span");
        label.className = "sr-only";
        label.textContent = "Evidence and actions";
        evidence.replaceChildren(label);
      }
      evidence.setAttribute("aria-pressed", String(matches(item)));
      if (options.compact)
        evidence.setAttribute(
          "aria-description",
          `${item.job.repository_name} #${item.job.number}: ${item.job.title}`,
        );
      evidence.onclick = () => choose(item.id, true, evidence);
      const github = document.createElement("button");
      github.type = "button";
      github.textContent = "Open PR on GitHub";
      github.onclick = async () => {
        github.disabled = true;
        await openDestination(item.id, null, showError);
        github.disabled = false;
      };
      actions.append(evidence);
      if (credentials(item) || (item.state === "failed" && item.failed_model)) {
        const fix = document.createElement("button");
        fix.type = "button";
        fix.className = "queue-fix";
        fix.dataset.fixAccount = "";
        fix.textContent = "Fix";
        if (!credentials(item) && item.failed_model) {
          fix.dataset.changeModel = item.failed_model.agent_id;
          fix.setAttribute("aria-label", "Fix: change model");
          fix.title = "Change model";
        }
        fix.onclick = () =>
          fix.dispatchEvent(
            new CustomEvent("pr-sniper:fix-account", {
              bubbles: true,
              detail: { id: item.id },
            }),
          );
        actions.append(fix);
      }
      if (retryable(item)) {
        const retry = document.createElement("button");
        retry.type = "button";
        retry.className = "queue-retry";
        retry.dataset.retryItem = item.id;
        retry.setAttribute("aria-label", "Retry");
        retry.title = "Retry";
        retry.innerHTML = retryIcon;
        retry.onclick = async () => {
          retry.disabled = true;
          retry.dispatchEvent(
            new CustomEvent("pr-sniper:retry-items", {
              bubbles: true,
              detail: { ids: [item.id] },
            }),
          );
        };
        actions.append(retry);
      }
      if (item.dismiss_token && !item.dismissed) {
        const dismiss = document.createElement("button");
        dismiss.type = "button";
        dismiss.className = "queue-dismiss";
        dismiss.dataset.dismissItem = item.id;
        dismiss.setAttribute("aria-label", "Dismiss stale review");
        dismiss.title = "Dismiss stale review";
        dismiss.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="m6 6 12 12M6 18 18 6"/></svg>`;
        dismiss.onclick = async () => {
          const hadFocus = document.activeElement === dismiss;
          dismiss.disabled = true;
          try {
            await invoke("dismiss_stale_review", {
              itemId: item.id,
              expectedToken: item.dismiss_token,
            });
            await refresh();
            if (
              hadFocus &&
              document.activeElement === document.body &&
              !root.closest("[hidden]")
            ) {
              const heading =
                root
                  .closest(".panel-shell")
                  ?.querySelector<HTMLElement>("[data-panel-heading]") ?? root;
              heading.tabIndex = -1;
              heading.focus({ preventScroll: true });
            }
          } catch (error) {
            showError(typeof error === "string" ? error : "Dismiss failed");
          } finally {
            dismiss.disabled = false;
          }
        };
        actions.append(dismiss);
      }
      if (!options.compact) actions.append(github);
      row.append(actions);
      if (options.compact) {
        const kind =
          item.state === "machine_signed_off" ? "ready" : "attention";
        let group = root.querySelector<HTMLElement>(
          `[data-queue-group="${kind}"]`,
        );
        if (!group) {
          group = document.createElement("div");
          group.className = "queue-group";
          group.dataset.queueGroup = kind;
          root.append(group);
        }
        group.append(row);
      } else root.append(row);
    }
    const retryIds = items.filter(retryable).map((item) => item.id);
    const attention = root.querySelector<HTMLElement>(
      '[data-queue-group="attention"]',
    );
    if (options.compact && attention && retryIds.length > 1) {
      const all = document.createElement("button");
      all.type = "button";
      all.className = "work-retry-all";
      all.setAttribute("aria-label", "Retry all failed");
      all.title = "Retry all failed";
      all.innerHTML = `${retryIcon}<span>${retryIds.length}</span>`;
      all.onclick = () => {
        all.disabled = true;
        all.dispatchEvent(
          new CustomEvent("pr-sniper:retry-items", {
            bubbles: true,
            detail: { ids: retryIds },
          }),
        );
      };
      attention.prepend(all);
    }
    if (options.compact && items.length) {
      for (const kind of ["ready", "attention"]) {
        if (root.querySelector(`[data-queue-group="${kind}"]`)) continue;
        const empty = document.createElement("div");
        empty.className = "queue-group";
        empty.dataset.queueGroup = kind;
        empty.dataset.empty = "true";
        empty.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 4h16l2 14H2L4 4Zm-1 9h5l2 3h4l2-3h5M7 8h10"/></svg><span class="sr-only">Nothing here</span>`;
        root.append(empty);
      }
    }
    if (initialized && selected !== null && !options.externalSelection) {
      const navigation = document.createElement("p");
      navigation.setAttribute("role", "status");
      navigation.textContent = items.some(matches) ? "" : "Unavailable";
      const clear = document.createElement("button");
      clear.textContent = "Show all evidence";
      clear.onclick = () => choose(null, false);
      navigation.append(" ", clear);
      root.append(navigation);
    }
    select(
      !initialized ? undefined : selected === null ? null : items.find(matches),
      focus,
      opener,
    );
    if (!focus && focusedItem && focusedLabel && !root.closest("[hidden]")) {
      const row = [
        ...root.querySelectorAll<HTMLElement>("[data-item-id]"),
      ].find((row) => row.dataset.itemId === focusedItem);
      [...(row?.querySelectorAll<HTMLButtonElement>("button") ?? [])]
        .find((button) => button.textContent === focusedLabel)
        ?.focus({ preventScroll: true });
    }
  }

  window.addEventListener("hashchange", () => {
    if (root.isConnected && !options.externalSelection) choose(fromUrl(), true);
  });
  // Background updates redraw after a press ends so its click survives.
  const redraw = () => {
    if (JSON.stringify([items, selected]) !== signature) draw(false);
  };
  const update = (next: QueueItem[]) => {
    items = next;
    loaded = true;
    whenReleased(redraw);
  };
  return Object.assign(update, {
    invalidate: () => {
      signature = "";
    },
    select: (id: string | null) => {
      selected = id;
      initialized = true;
      draw(false);
    },
  });
}

export function renderItemEvidence(
  root: HTMLElement,
  item: QueueItem,
  showError: (value: string) => void,
  refresh: () => Promise<void>,
  handoff = false,
) {
  if (handoff) {
    const context = document.createElement("section");
    context.className = "detail-section";
    renderFacts(context, [
      ["Repository", item.job.repository_name],
      ["PR author", item.job.author_login ?? "Unavailable"],
      ["GitHub identity", `${item.job.account_login} (${item.job.account_id})`],
      ["Iteration", String(item.job.work?.iteration ?? "Not recorded")],
      ["Revision head", item.job.head_sha.slice(0, 7)],
    ]);
    const provenance = document.createElement("details");
    const label = document.createElement("summary");
    label.textContent = "IDs";
    provenance.append(label);
    renderFacts(provenance, [
      ["Item ID", item.id],
      [
        "Iteration ID",
        item.job.work?.iteration_id ?? "Legacy iteration identity not recorded",
      ],
      ["Full revision head", item.job.head_sha],
    ]);
    context.append(provenance);
    root.append(context);
  }
  if (item.action_status)
    renderActions(root, item.action_status, showError, refresh);
  if (item.feedback?.length) {
    const details = document.createElement("details");
    const title = document.createElement("summary");
    title.textContent = `Current owned feedback (${item.feedback.length})`;
    details.append(title);
    for (const feedback of item.feedback) {
      const entry = document.createElement("p");
      entry.textContent = `${feedback.state.replaceAll("_", " ")}: ${feedback.context.title || feedback.context.root_id}. Owner ${feedback.context.owner_agent_id}; original head ${feedback.context.original_head}. ${feedback.reason ?? ""}`;
      const body = document.createElement("pre");
      body.textContent = feedback.context.body;
      details.append(entry, body);
    }
    root.append(details);
  }
}
