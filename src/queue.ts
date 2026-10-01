import { invoke } from "@tauri-apps/api/core";
import { renderActions, type ActionStatus } from "./actions";
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
    | "blocked"
    | "stale"
    | "stale_after_publication"
    | "closed"
    | "merged";
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
  failed: "Failed / recovery required",
  blocked: "Blocked",
  stale: "Stale review",
  stale_after_publication: "Stale after publication",
  closed: "Closed on GitHub",
  merged: "Merged on GitHub",
};

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
  return [...current.values()].filter(
    (item) =>
      ![
        "queued",
        "reviewing",
        "awaiting_publication",
        "waiting_for_author",
        "closed",
        "merged",
      ].includes(item.state),
  );
}

export async function openDestination(
  itemId: string,
  file: string | null,
  showError: (message: string) => void,
) {
  try {
    await invoke("open_queue_destination", { itemId, file });
  } catch (error) {
    showError(
      typeof error === "string"
        ? error
        : "Could not open this exact GitHub destination. No alternative PR was selected.",
    );
  }
}

export function renderQueue(
  root: HTMLElement,
  showError: (message: string) => void,
  select: (item: QueueItem | null | undefined, focus: boolean) => void,
  refresh: () => Promise<void>,
  options: { compact?: boolean; externalSelection?: boolean } = {},
) {
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
        showError(
          typeof error === "string"
            ? error
            : "Queue selection could not be saved; it may not survive restart.",
        );
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
        showError(
          "Could not restore the queue destination. Select an item explicitly; no substitute was selected.",
        );
        select(undefined, false);
        draw(false);
      });

  function choose(id: string | null, focus: boolean) {
    selectionRevision++;
    initialized = true;
    selected = id;
    if (!options.externalSelection) {
      persist(id);
      const url = new URL(location.href);
      url.hash = id ? new URLSearchParams({ item: id }).toString() : "";
      history.replaceState(null, "", url);
    }
    draw(focus);
  }

  function draw(focus: boolean) {
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
    const intro = document.createElement("p");
    intro.className = "hint";
    intro.textContent =
      "Ready PRs and work that needs your input come first. Published findings wait on the PR author. GitHub approval and merge status are not inferred.";
    if (!options.compact) root.append(intro);
    if (!items.length) {
      const empty = document.createElement("p");
      empty.textContent = options.compact
        ? "You're all caught up. Human handoffs and actionable problems appear here; Agent work is in Running. Configure monitoring in Settings, or use Status to Check Now."
        : "No detected pull requests yet. Configure monitoring in Settings, then Check Now.";
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
      state.textContent = labels[item.state];
      const heading = document.createElement("h3");
      heading.textContent = options.compact
        ? item.job.title
        : `${item.job.repository_name} #${item.job.number}: ${item.job.title}`;
      const context = document.createElement("p");
      context.className = "hint";
      context.textContent = `PR author: ${item.job.author_login ?? "unavailable"}. Acting GitHub account: ${item.job.account_login} (${item.job.account_id}). Reviewed / detected head: ${item.job.head_sha}.`;
      if (item.job.work)
        context.append(
          ` Iteration ${item.job.work.iteration} (${item.job.work.iteration_id}).`,
        );
      const summary = document.createElement("p");
      summary.textContent = item.summary;
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
        summary.className = "queue-summary-text";
        summary.textContent =
          item.state === "machine_signed_off"
            ? `${item.review_keys.length}/${item.review_keys.length} Agents clear`
            : item.state === "waiting_for_human"
              ? "Read the saved findings and conversation."
              : item.state === "confirmation_required"
                ? "Review the exact work and its permissions."
                : item.summary;
        row.append(top, heading, reference, summary);
        const receipts =
          item.action_status?.effects.filter((effect) => effect.receipt) ?? [];
        for (const effect of receipts) {
          const receipt = document.createElement("p");
          receipt.className = "queue-receipt";
          receipt.textContent = `${effect.action === "approve" ? "Approval" : "Merge"} recorded; not personal review`;
          row.append(receipt);
        }
        const footer = document.createElement("div");
        footer.className = "queue-card-footer";
        const avatar = document.createElement("span");
        avatar.className = "queue-avatar";
        avatar.setAttribute("aria-hidden", "true");
        avatar.textContent = (item.job.author_login ?? "?").slice(0, 2);
        const author = document.createElement("span");
        author.textContent = item.job.author_login ?? "Author unavailable";
        const trigger = document.createElement("span");
        trigger.className = "queue-trigger";
        trigger.textContent = item.job.work?.admission.requested_reviewer
          ? "Requested reviewer"
          : item.job.work?.admission.watched_author
            ? "Watched author"
            : "PR evidence";
        footer.append(avatar, author, trigger);
        row.append(footer);
      } else row.append(state, heading, context, summary);
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
      evidence.onclick = () => choose(item.id, true);
      const github = document.createElement("button");
      github.type = "button";
      github.textContent = "Open PR on GitHub";
      github.onclick = async () => {
        github.disabled = true;
        await openDestination(item.id, null, showError);
        github.disabled = false;
      };
      actions.append(evidence);
      if (!options.compact) actions.append(github);
      row.append(actions);
      root.append(row);
    }
    if (initialized && selected !== null && !options.externalSelection) {
      const navigation = document.createElement("p");
      navigation.setAttribute("role", "status");
      navigation.textContent = items.some(matches)
        ? "Showing evidence and actions for the selected account, repository and revision."
        : "The exact saved queue destination is no longer available. No different PR or revision was selected.";
      const clear = document.createElement("button");
      clear.textContent = "Show all evidence";
      clear.onclick = () => choose(null, false);
      navigation.append(" ", clear);
      root.append(navigation);
    }
    select(
      !initialized ? undefined : selected === null ? null : items.find(matches),
      focus,
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
    const title = document.createElement("h3");
    title.textContent = "This pull request";
    context.append(title);
    renderFacts(context, [
      ["Repository", item.job.repository_name],
      ["PR author", item.job.author_login ?? "Unavailable"],
      ["GitHub identity", `${item.job.account_login} (${item.job.account_id})`],
      ["Iteration", String(item.job.work?.iteration ?? "Not recorded")],
      ["Revision head", item.job.head_sha.slice(0, 7)],
    ]);
    const provenance = document.createElement("details");
    const label = document.createElement("summary");
    label.textContent = "PR identity and provenance";
    provenance.append(label);
    renderFacts(provenance, [
      ["Item ID", item.id],
      [
        "Iteration ID",
        item.job.work?.iteration_id ?? "Legacy iteration identity not recorded",
      ],
      ["Full revision head", item.job.head_sha],
    ]);
    const external = document.createElement("button");
    external.textContent = "Open PR on GitHub";
    external.className = "job-provider-link";
    external.onclick = () => void openDestination(item.id, null, showError);
    const guidance = document.createElement("p");
    guidance.textContent =
      "Personal review, comments and approval happen on GitHub. Machine clearance and recorded automation never mean you personally reviewed this PR. No acknowledgment is required to unlock a separately permitted merge.";
    (root.querySelector(".detail-hero") ?? context).append(external);
    context.append(guidance, provenance);
    root.append(context);
    for (const warning of item.warnings) {
      const line = document.createElement("p");
      line.className = "review-failure";
      line.textContent = warning;
      root.append(line);
    }
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
      entry.textContent = `${feedback.state.replaceAll("_", " ")}: ${feedback.context.title || feedback.context.root_id}. Owner ${feedback.context.owner_agent_id}; original head ${feedback.context.original_head}. ${feedback.reason ?? ""}${feedback.state === "cleared" ? " Agent reassessment, not provider thread closure." : ""}`;
      const body = document.createElement("pre");
      body.textContent = feedback.context.body;
      details.append(entry, body);
    }
    root.append(details);
  }
}
