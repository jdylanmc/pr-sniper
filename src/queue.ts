import { invoke } from "@tauri-apps/api/core";

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
  id: string;
  aliases?: string[];
  job: {
    work?: NormalWork;
    repository_name: string;
    number: number;
    title: string;
    head_sha: string;
    account_id: string;
    account_login: string;
    author_login: string | null;
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
    persist(id);
    const url = new URL(location.href);
    url.hash = id ? new URLSearchParams({ item: id }).toString() : "";
    history.replaceState(null, "", url);
    draw(focus);
  }

  function draw(focus: boolean) {
    if (!loaded) return;
    signature = JSON.stringify([items, selected]);
    const matches = (item: QueueItem) =>
      item.id === selected || !!item.aliases?.includes(selected ?? "");
    root.replaceChildren();
    const intro = document.createElement("p");
    intro.className = "hint";
    intro.textContent =
      "Ready PRs and work that needs your input come first. Published findings wait on the PR author. GitHub approval and merge status are not inferred.";
    root.append(intro);
    if (!items.length) {
      const empty = document.createElement("p");
      empty.textContent =
        "No detected pull requests yet. Configure monitoring in Settings, then Check Now.";
      root.append(empty);
    }
    for (const item of items) {
      const row = document.createElement("article");
      row.className = "queue-item";
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
      heading.textContent = `${item.job.repository_name} #${item.job.number}: ${item.job.title}`;
      const context = document.createElement("p");
      context.className = "hint";
      context.textContent = `PR author: ${item.job.author_login ?? "unavailable"}. Acting GitHub account: ${item.job.account_login} (${item.job.account_id}). Reviewed / detected head: ${item.job.head_sha}.`;
      if (item.job.work)
        context.append(
          ` Iteration ${item.job.work.iteration} (${item.job.work.iteration_id}).`,
        );
      const summary = document.createElement("p");
      summary.textContent = item.summary;
      row.append(state, heading, context, summary);
      for (const warning of item.warnings) {
        const text = document.createElement("p");
        text.className = "review-failure";
        text.textContent = warning;
        row.append(text);
      }
      const actions = document.createElement("div");
      actions.className = "actions";
      const evidence = document.createElement("button");
      evidence.type = "button";
      evidence.textContent = "Evidence and actions";
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
      actions.append(evidence, github);
      row.append(actions);
      root.append(row);
    }
    if (initialized && selected !== null) {
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
  }

  window.addEventListener("hashchange", () => {
    if (root.isConnected) choose(fromUrl(), true);
  });
  return (next: QueueItem[]) => {
    items = next;
    loaded = true;
    if (JSON.stringify([items, selected]) !== signature) draw(false);
  };
}
