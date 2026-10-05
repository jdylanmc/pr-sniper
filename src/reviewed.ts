import { invoke } from "@tauri-apps/api/core";
import type { PanelDetail } from "./panel";
import type { MonitoringSnapshot } from "./monitoring";
import type { QueueItem } from "./queue";

type ReviewedFilter = "all" | "approved" | "follow_up" | "ready";
type ResultDestination = Extract<PanelDetail, { type: "item" | "job" }>;

interface ResultCursor {
  version: number;
  sequence: number;
  item_id: string;
}

interface UnavailableCursor {
  version: number;
  work_id: string;
}

interface ResultRow {
  item_id: string;
  job: QueueItem["job"];
  activity_sequence: number;
  completed_passes: number;
  review_attempts: number;
  conversations: number;
}

interface UnavailableResult {
  destination: ResultDestination;
  message: string;
}

interface ResultPage {
  results: ResultRow[];
  next_cursor: ResultCursor | null;
  unavailable: UnavailableResult[];
  unavailable_count: number;
  next_unavailable_cursor: UnavailableCursor | null;
}

const pageSize = 12;
const filters: { id: ReviewedFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "approved", label: "Approved" },
  { id: "follow_up", label: "Follow-up" },
  { id: "ready", label: "Ready" },
];

function hasApprovalReceipt(item: QueueItem | undefined): boolean {
  return (
    item?.action_status?.effects.some(
      (effect) => effect.action === "approve" && effect.receipt !== null,
    ) ?? false
  );
}

function needsFollowUp(item: QueueItem | undefined): boolean {
  return (
    item?.state === "waiting_for_author" ||
    item?.state === "waiting_for_human" ||
    item?.feedback?.some((feedback) =>
      ["open", "human_input_required"].includes(feedback.state),
    ) === true
  );
}

function matchesFilter(
  filter: ReviewedFilter,
  item: QueueItem | undefined,
): boolean {
  switch (filter) {
    case "all":
      return true;
    case "approved":
      return hasApprovalReceipt(item);
    case "follow_up":
      return needsFollowUp(item);
    case "ready":
      return item?.state === "machine_signed_off";
  }
}

function hasReviewedEvidence(
  row: ResultRow,
  item: QueueItem | undefined,
): boolean {
  if (row.completed_passes > 0 || row.conversations > 0) return true;
  const onlyActiveWork =
    item !== undefined &&
    ["queued", "reviewing", "confirmation_required"].includes(item.state);
  return row.review_attempts > 0 && !onlyActiveWork;
}

function displayOutcome(item: QueueItem | undefined): {
  key: string;
  text: string;
  approval: boolean;
} {
  if (!item)
    return {
      key: "unavailable",
      text: "Outcome unavailable; no state is inferred.",
      approval: false,
    };
  const approval = hasApprovalReceipt(item);
  const key = approval
    ? "approved"
    : item.state === "machine_signed_off"
      ? "ready"
      : needsFollowUp(item)
        ? "follow-up"
        : ["failed", "blocked", "stale", "stale_after_publication"].includes(
              item.state,
            )
          ? "attention"
          : "other";
  return { key, text: item.summary, approval };
}

function countLabel(value: number, singular: string, plural: string): string {
  return `${value} ${value === 1 ? singular : plural}`;
}

export interface ReviewedView {
  setSnapshot(snapshot: MonitoringSnapshot | undefined): void;
  activate(): void;
  refresh(): void;
}

export function mountReviewed(
  root: HTMLElement,
  open: (destination: ResultDestination, opener: HTMLButtonElement) => void,
): ReviewedView {
  let filter: ReviewedFilter = "all";
  let rows: ResultRow[] = [];
  let unavailable: UnavailableResult[] = [];
  let nextCursor: ResultCursor | null = null;
  let nextUnavailableCursor: UnavailableCursor | null = null;
  let resultsExhausted = false;
  let unavailableExhausted = false;
  let unavailableCount = 0;
  let snapshot: MonitoringSnapshot | undefined;
  let loaded = false;
  let loading = false;
  let error: string | null = null;
  let requestRevision = 0;
  let retryReset = true;

  function itemFor(row: ResultRow): QueueItem | undefined {
    return snapshot?.items?.find(
      (item) => item.id === row.item_id || item.aliases?.includes(row.item_id),
    );
  }

  function restoreFocus(
    focusedFilter: string | undefined,
    focusedAction: "refresh" | "older" | "retry" | undefined,
    focusedItem: string | undefined,
    focusedItemAction: "evidence" | "final" | undefined,
  ) {
    if (focusedFilter) {
      root
        .querySelector<HTMLButtonElement>(
          `[data-reviewed-filter="${CSS.escape(focusedFilter)}"]`,
        )
        ?.focus({ preventScroll: true });
    } else if (focusedAction === "refresh") {
      root
        .querySelector<HTMLButtonElement>("[data-reviewed-refresh]")
        ?.focus({ preventScroll: true });
    } else if (focusedAction === "older") {
      const older = root.querySelector<HTMLButtonElement>(
        "[data-reviewed-older]",
      );
      (
        older ??
        root.querySelector<HTMLButtonElement>("[data-reviewed-refresh]")
      )?.focus({ preventScroll: true });
    } else if (focusedAction === "retry") {
      const retry = root.querySelector<HTMLButtonElement>(
        "[data-reviewed-retry]",
      );
      (
        retry ??
        root.querySelector<HTMLButtonElement>("[data-reviewed-refresh]")
      )?.focus({ preventScroll: true });
    } else if (focusedItem) {
      root
        .querySelector<HTMLButtonElement>(
          focusedItemAction === "final"
            ? "[data-reviewed-final-open]"
            : `[data-reviewed-open="${CSS.escape(focusedItem)}"]`,
        )
        ?.focus({ preventScroll: true });
    }
  }

  function render() {
    const active =
      document.activeElement instanceof HTMLElement &&
      root.contains(document.activeElement)
        ? document.activeElement
        : null;
    const focusedFilter = active?.dataset.reviewedFilter;
    const focusedAction = active?.hasAttribute("data-reviewed-refresh")
      ? "refresh"
      : active?.hasAttribute("data-reviewed-older")
        ? "older"
        : active?.hasAttribute("data-reviewed-retry")
          ? "retry"
          : undefined;
    const focusedItem = active?.closest<HTMLElement>("[data-reviewed-item]")
      ?.dataset.reviewedItem;
    const focusedItemAction = active?.hasAttribute("data-reviewed-final-open")
      ? "final"
      : active?.hasAttribute("data-reviewed-open")
        ? "evidence"
        : undefined;
    root.replaceChildren();

    const intro = document.createElement("p");
    intro.className = "work-caption reviewed-caption";
    intro.textContent =
      "Open PR results, newest meaningful activity first. Filters use current review state and provider receipts, not personal-review assumptions.";
    root.append(intro);

    const controls = document.createElement("div");
    controls.className = "reviewed-controls";
    const filterGroup = document.createElement("div");
    filterGroup.className = "reviewed-filters";
    filterGroup.setAttribute("role", "group");
    filterGroup.setAttribute("aria-label", "Filter Reviewed results");
    for (const option of filters) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "reviewed-filter";
      button.dataset.reviewedFilter = option.id;
      button.setAttribute("aria-pressed", String(filter === option.id));
      button.textContent = option.label;
      button.onclick = () => {
        button.focus({ preventScroll: true });
        filter = option.id;
        render();
      };
      filterGroup.append(button);
    }
    const refresh = document.createElement("button");
    refresh.type = "button";
    refresh.className = "reviewed-refresh";
    refresh.dataset.reviewedRefresh = "true";
    refresh.textContent = loading ? "Refreshing..." : "Refresh results";
    refresh.onclick = () => {
      refresh.focus({ preventScroll: true });
      void loadPage(true);
    };
    controls.append(filterGroup, refresh);
    root.append(controls);

    const scope = document.createElement("p");
    scope.className = "reviewed-scope";
    scope.textContent =
      "Filters apply to loaded pages. Load older results to continue through the history.";
    root.append(scope);

    if (loading && rows.length === 0 && unavailable.length === 0) {
      const status = document.createElement("p");
      status.className = "reviewed-status";
      status.setAttribute("role", "status");
      status.textContent = "Loading Reviewed results...";
      root.append(status);
    }
    if (error) {
      const alert = document.createElement("p");
      alert.className = "reviewed-error";
      alert.setAttribute("role", "alert");
      alert.textContent = error;
      const retry = document.createElement("button");
      retry.type = "button";
      retry.dataset.reviewedRetry = "true";
      retry.textContent = "Retry";
      retry.onclick = () => {
        retry.focus({ preventScroll: true });
        void loadPage(retryReset);
      };
      root.append(alert, retry);
    }

    const visibleRows = rows.filter((row) => {
      const item = itemFor(row);
      return hasReviewedEvidence(row, item) && matchesFilter(filter, item);
    });
    const list = document.createElement("ol");
    list.className = "reviewed-list";
    list.id = "reviewed-results-list";
    list.setAttribute(
      "aria-label",
      "Open PR results, newest meaningful activity first",
    );
    for (const row of visibleRows) {
      const item = itemFor(row);
      const outcome = displayOutcome(item);
      const entry = document.createElement("li");
      const article = document.createElement("article");
      article.className = "reviewed-entry";
      article.dataset.reviewedItem = row.item_id;
      article.dataset.reviewedOutcome = outcome.key;

      const heading = document.createElement("h2");
      heading.className = "reviewed-title";
      heading.textContent = `${row.job.repository_name} #${row.job.number}: ${row.job.title}`;
      const status = document.createElement("p");
      status.className = "reviewed-outcome";
      status.textContent = outcome.text;

      const facts: string[] = [];
      if (row.job.work) facts.push(`Iteration ${row.job.work.iteration}`);
      if (row.completed_passes > 0)
        facts.push(
          countLabel(
            row.completed_passes,
            "completed pass",
            "completed passes",
          ),
        );
      if (row.review_attempts > 0)
        facts.push(
          countLabel(row.review_attempts, "review attempt", "review attempts"),
        );
      if (row.conversations > 0)
        facts.push(
          countLabel(row.conversations, "conversation", "conversations"),
        );
      const meta = document.createElement("p");
      meta.className = "reviewed-meta";
      meta.textContent = facts.length
        ? facts.join(", ")
        : "Saved review activity is available; current outcome could not be matched.";
      article.append(heading, status, meta);
      if (outcome.approval) {
        const receipt = document.createElement("p");
        receipt.className = "reviewed-receipt";
        receipt.textContent =
          "A provider approval receipt is recorded. This is not personal review.";
        article.append(receipt);
      }
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.reviewedOpen = row.item_id;
      button.textContent = "Open evidence";
      button.setAttribute(
        "aria-label",
        `Open evidence for ${row.job.repository_name} #${row.job.number}`,
      );
      button.onclick = () =>
        open({ type: "item", item_id: row.item_id }, button);
      article.append(button);
      const final = item?.action_status?.final_review;
      if (final?.execution.operation.state === "completed") {
        const finalReview = document.createElement("div");
        finalReview.className = "reviewed-final";
        const label = document.createElement("p");
        label.textContent = `Primary final review: ${final.execution.phase}`;
        const finalButton = document.createElement("button");
        finalButton.type = "button";
        finalButton.dataset.reviewedFinalOpen = "true";
        finalButton.textContent = "Open final review";
        finalButton.setAttribute(
          "aria-label",
          `Open primary final review for ${row.job.repository_name} #${row.job.number}`,
        );
        finalButton.onclick = () =>
          open(
            { type: "job", kind: "primary_final", id: final.id },
            finalButton,
          );
        finalReview.append(label, finalButton);
        article.append(finalReview);
      }
      entry.append(article);
      list.append(entry);
    }
    root.append(list);

    if (visibleRows.length === 0 && !error && (!loading || loaded)) {
      const empty = document.createElement("p");
      empty.className = "reviewed-empty";
      if (!loaded) {
        empty.setAttribute("role", "status");
        empty.textContent = "Reviewed results have not been loaded.";
      } else if (filter !== "all" && hasOlder()) {
        empty.textContent = `No ${filterLabel(filter)} results in loaded pages. Load older results to keep searching.`;
      } else if (filter !== "all") {
        empty.textContent = `No ${filterLabel(filter)} results are available for still-open PRs.`;
      } else if (hasOlder()) {
        empty.textContent =
          "No reviewed results are present on this page. Load older results to continue.";
      } else {
        empty.textContent =
          "No completed review results for still-open PRs yet.";
      }
      root.append(empty);
    }

    if (unavailableCount > 0) {
      const section = document.createElement("section");
      section.className = "reviewed-unavailable";
      const heading = document.createElement("h2");
      heading.textContent =
        unavailableCount === 1
          ? "One destination is unavailable"
          : `${unavailableCount} destinations are unavailable`;
      const note = document.createElement("p");
      note.textContent =
        "These mention records could not be safely linked to an exact PR iteration. No substitute was selected.";
      section.append(heading, note);
      for (const result of unavailable) {
        const article = document.createElement("article");
        article.className = "reviewed-unavailable-entry";
        article.dataset.reviewedUnavailableDestination = JSON.stringify(
          result.destination,
        );
        const message = document.createElement("p");
        message.textContent = result.message;
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = "Open exact destination";
        button.onclick = () => open(result.destination, button);
        article.append(message, button);
        section.append(article);
      }
      root.append(section);
    }

    const more = hasOlder();
    if (more) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "reviewed-older";
      button.dataset.reviewedOlder = "true";
      button.textContent = loading
        ? "Loading older results..."
        : "Load older results";
      button.onclick = () => {
        button.focus({ preventScroll: true });
        void loadPage(false);
      };
      root.append(button);
    } else if (loaded && rows.length > 0) {
      const end = document.createElement("p");
      end.className = "reviewed-end";
      end.textContent = "All available open-PR results are loaded.";
      root.append(end);
    }

    restoreFocus(focusedFilter, focusedAction, focusedItem, focusedItemAction);
  }

  function hasOlder(): boolean {
    return loaded && (!resultsExhausted || !unavailableExhausted);
  }

  function filterLabel(value: ReviewedFilter): string {
    return filters.find((option) => option.id === value)?.label ?? "matching";
  }

  async function loadPage(reset: boolean): Promise<void> {
    if (loading) return;
    const revision = ++requestRevision;
    const previous = reset
      ? {
          rows,
          unavailable,
          nextCursor,
          nextUnavailableCursor,
          resultsExhausted,
          unavailableExhausted,
          unavailableCount,
          loaded,
        }
      : undefined;
    retryReset = reset;
    loading = true;
    error = null;
    render();
    try {
      const page = await invoke<ResultPage>("result_page", {
        request: {
          limit: pageSize,
          cursor: reset ? null : nextCursor,
          unavailable_cursor: reset ? null : nextUnavailableCursor,
        },
      });
      if (revision !== requestRevision) return;
      const nextRows = reset ? [] : [...rows];
      const seenRows = new Set(nextRows.map((row) => row.item_id));
      if (reset || !resultsExhausted) {
        for (const row of page.results)
          if (!seenRows.has(row.item_id)) {
            nextRows.push(row);
            seenRows.add(row.item_id);
          }
        nextCursor = page.next_cursor;
        resultsExhausted = page.next_cursor === null;
      }
      const nextUnavailable = reset ? [] : [...unavailable];
      const seenUnavailable = new Set(
        nextUnavailable.map((entry) => JSON.stringify(entry.destination)),
      );
      if (reset || !unavailableExhausted) {
        for (const entry of page.unavailable) {
          const key = JSON.stringify(entry.destination);
          if (!seenUnavailable.has(key)) {
            nextUnavailable.push(entry);
            seenUnavailable.add(key);
          }
        }
        nextUnavailableCursor = page.next_unavailable_cursor;
        unavailableExhausted = page.next_unavailable_cursor === null;
      }
      rows = nextRows;
      unavailable = nextUnavailable;
      unavailableCount = page.unavailable_count;
      loaded = true;
    } catch (cause) {
      if (revision !== requestRevision) return;
      if (previous) {
        rows = previous.rows;
        unavailable = previous.unavailable;
        nextCursor = previous.nextCursor;
        nextUnavailableCursor = previous.nextUnavailableCursor;
        resultsExhausted = previous.resultsExhausted;
        unavailableExhausted = previous.unavailableExhausted;
        unavailableCount = previous.unavailableCount;
        loaded = previous.loaded;
      }
      error =
        typeof cause === "string"
          ? cause
          : "Reviewed results could not be read. Check local storage or Diagnostics; no status is inferred.";
    } finally {
      if (revision === requestRevision) {
        loading = false;
        render();
      }
    }
  }

  return {
    setSnapshot(value) {
      if (snapshot === value) return;
      snapshot = value;
      render();
    },
    activate() {
      if (!loaded && !loading && !error) void loadPage(true);
      else render();
    },
    refresh() {
      void loadPage(true);
    },
  };
}
