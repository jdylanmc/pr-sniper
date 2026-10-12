import { invoke } from "@tauri-apps/api/core";
import { renderConfiguration } from "./work-presentation";
import type { Job, ReviewCandidate } from "./monitoring";
import { openDestination } from "./queue";

export interface ActionStatus {
  item_id: string;
  final_valid: boolean;
  provider_observed_at: number | null;
  observation_retry_blocker: string | null;
  primary_assignment_id: string | null;
  machine_clear: boolean;
  personal_review: string;
  permissions: { approve: boolean; merge: boolean };
  provider_approval?: { id: string; actor_id: string; head: string } | null;
  final_review: {
    id: string;
    enqueue_order: number;
    cancelled: boolean;
    execution: NonNullable<ReviewCandidate["run"]> & {
      job: Job;
    };
  } | null;
  effects: {
    id: string;
    action: "approve" | "merge";
    state: string;
    cancelled: boolean;
    error: string | null;
    receipt: {
      id: string;
      actor_id: string;
      head: string;
      action: string;
      merge_commit: string | null;
    } | null;
  }[];
  blockers: string[];
}

export function renderActions(
  root: HTMLElement,
  status: ActionStatus,
  showError: (value: string) => void,
  refresh: () => Promise<void>,
  configuration = true,
) {
  const section = document.createElement("section");
  section.className = "review-run";
  const summary = document.createElement("p");
  summary.textContent = `Machine clearance: ${status.machine_clear ? "clear" : "not clear"}; primary ${status.primary_assignment_id ?? "none"}; approve ${status.permissions.approve ? "on" : "off"}; merge ${status.permissions.merge ? "on" : "off"}; personal review ${status.personal_review}`;
  section.append(summary);
  const observed = document.createElement("p");
  observed.textContent = `Final evidence: ${status.final_valid ? "valid" : "not validated"}; observed ${status.provider_observed_at ? new Date(status.provider_observed_at * 1000).toISOString() : "never"}`;
  section.append(observed);
  if (status.provider_approval) {
    const approval = document.createElement("p");
    approval.textContent = `Provider approval ${status.provider_approval.id} by ${status.provider_approval.actor_id} on ${status.provider_approval.head}`;
    section.append(approval);
  }
  for (const blocker of status.blockers) {
    const message = document.createElement("p");
    message.textContent = blocker;
    section.append(message);
  }
  const act = async (
    command: string,
    args: Record<string, unknown>,
    button: HTMLButtonElement,
  ) => {
    button.disabled = true;
    try {
      await invoke(command, args);
    } catch (cause) {
      showError(typeof cause === "string" ? cause : "Failed");
    } finally {
      button.disabled = false;
      await refresh();
    }
  };
  if (status.blockers.length && status.provider_observed_at) {
    const retry = document.createElement("button");
    retry.textContent = "Retry provider evidence";
    retry.disabled = status.observation_retry_blocker !== null;
    retry.onclick = () =>
      void act("retry_action_observation", { itemId: status.item_id }, retry);
    section.append(retry);
    if (status.observation_retry_blocker) {
      const reason = document.createElement("p");
      reason.textContent = `Refresh unavailable: ${status.observation_retry_blocker}`;
      section.append(reason);
    }
  }
  if (status.final_review) {
    const final = status.final_review;
    const state = document.createElement("p");
    state.textContent = `Primary final full review: ${final.execution.phase}. State: ${final.execution.operation.state}; attempt ${final.execution.operation.attempt_count}; queue order ${final.enqueue_order}`;
    section.append(state);
    if (configuration)
      renderConfiguration(
        section,
        final.execution.operation.attempt_count > 0
          ? "Captured final execution configuration"
          : "Planned final configuration",
        final.execution.selection,
      );
    if (final.execution.result) {
      const guide = document.createElement("details");
      const summary = document.createElement("summary");
      const files = final.execution.result.output.files;
      summary.textContent = `Complete final file guide (${files.length} files)`;
      const list = document.createElement("ol");
      for (const file of [...files].sort((a, b) => a.order - b.order)) {
        const entry = document.createElement("li");
        const link = document.createElement("a");
        link.href = "#";
        link.textContent = file.path;
        link.onclick = (event) => {
          event.preventDefault();
          void openDestination(status.item_id, file.path, showError);
        };
        entry.append(link, `: ${file.explanation}`);
        list.append(entry);
      }
      guide.append(summary, list);
      section.append(guide);
    }
    const details = document.createElement("details");
    const title = document.createElement("summary");
    title.textContent = "Final review context";
    const body = document.createElement("pre");
    body.textContent = JSON.stringify(final, null, 2);
    details.append(title, body);
    section.append(details);
    if (final.execution.operation.state !== "completed") {
      const running = final.execution.operation.state === "running";
      const queued = ["queued", "interrupted"].includes(
        final.execution.operation.state,
      );
      const button = document.createElement("button");
      button.textContent = running
        ? "Cancel final review"
        : queued && !final.cancelled
          ? "Cancel queued final review"
          : "Retry final full review";
      button.onclick = () =>
        void act(
          running || (queued && !final.cancelled)
            ? "cancel_final_review"
            : "start_final_review",
          {
            id: final.id,
          },
          button,
        );
      section.append(button);
    }
  } else if (status.permissions.approve || status.permissions.merge) {
    const message = document.createElement("p");
    message.textContent = "No valid final review";
    section.append(message);
  }
  for (const effect of status.effects) {
    const line = document.createElement("p");
    line.textContent = `${effect.action === "approve" ? "GitHub approval" : "GitHub merge"}: ${effect.state.replaceAll("_", " ")}; ${effect.receipt ? `receipt ${effect.receipt.id}; ${effect.receipt.actor_id}; ${effect.receipt.head}` : "no receipt"}${effect.error ? `; ${effect.error}` : ""}`;
    if (effect.cancelled) line.append("; cancelled");
    section.append(line);
    const evidence = document.createElement("details");
    const heading = document.createElement("summary");
    heading.textContent = `${effect.action} receipt`;
    const body = document.createElement("pre");
    body.textContent = JSON.stringify(effect, null, 2);
    evidence.append(heading, body);
    section.append(evidence);
    if (
      effect.state === "uncertain" ||
      (effect.state === "confirmed" && effect.error)
    ) {
      const reconcile = document.createElement("button");
      reconcile.textContent = "Reconcile original action";
      reconcile.onclick = () =>
        void act("reconcile_provider_action", { id: effect.id }, reconcile);
      section.append(reconcile);
    }
    if (!effect.cancelled && ["prepared", "uncertain"].includes(effect.state)) {
      const button = document.createElement("button");
      button.textContent = "Stop action";
      button.onclick = () =>
        void act("cancel_provider_action", { id: effect.id }, button);
      section.append(button);
    }
  }
  root.append(section);
}
