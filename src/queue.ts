import { invoke } from "@tauri-apps/api/core";
import { renderActions, type ActionStatus } from "./actions";
import type { FailureClass } from "./automation";
import { renderFacts } from "./work-presentation";

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
    | "confirmation_required"
    | "awaiting_publication"
    | "queued"
    | "reviewing"
    | "failed"
    | "stale"
    | "stale_after_publication"
    | "closed"
    | "merged";
  failure_class?: FailureClass | null;
  summary: string;
  warnings: string[];
  review_keys: string[];
  follow_up_ids: string[];
}

const labels: Record<QueueItem["state"], string> = {
  machine_signed_off: "Ready for your final review",
  waiting_for_human: "Needs your input",
  waiting_for_author: "Waiting for PR author",
  confirmation_required: "Needs your confirmation",
  awaiting_publication: "Awaiting comment publication",
  queued: "Queued",
  reviewing: "Reviewing",
  failed: "Failed",
  stale: "Stale review",
  stale_after_publication: "Stale after publication",
  closed: "Closed on GitHub",
  merged: "Merged on GitHub",
};

const credentials = (item: QueueItem) =>
  item.state === "failed" && item.failure_class === "credentials";
const failureChips: Record<FailureClass, string> = {
  credentials: "Credentials",
  permission: "Permission",
  network: "Network",
  other: "Failed",
};
const retryPath = "M20 12a8 8 0 1 1-2.3-5.7M20 4v4h-4";
const retryIcon = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${retryPath}"/></svg>`;
const retryable = (item: QueueItem) =>
  item.state === "failed" && !credentials(item);

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
        ![
          "queued",
          "reviewing",
          "awaiting_publication",
          "waiting_for_author",
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
            ? failureChips[item.failure_class ?? "other"]
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
        top.append(state, iteration);
        const reference = document.createElement("p");
        reference.className = "queue-reference";
        reference.textContent = `${item.job.repository_name} #${item.job.number}`;
        row.append(top, heading, reference);
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
      } else row.append(state, heading, context);
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
      if (credentials(item)) {
        const fix = document.createElement("button");
        fix.type = "button";
        fix.className = "queue-fix";
        fix.dataset.fixAccount = "";
        fix.textContent = "Fix";
        fix.onclick = () =>
          fix.dispatchEvent(
            new CustomEvent("pr-sniper:fix-account", { bubbles: true }),
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
  const update = (next: QueueItem[]) => {
    items = next;
    loaded = true;
    if (JSON.stringify([items, selected]) !== signature) draw(false);
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
