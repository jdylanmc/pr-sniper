import { invoke } from "@tauri-apps/api/core";
import { keyValues, type KeyValue } from "./key-values";
import { renderConfiguration } from "./work-presentation";
import type { Job, ReviewCandidate } from "./monitoring";

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

const retryPath = "M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5";
const okPath = "M5 12.5 10 17.5 19 7";
const warnPath = "M12 3 2 20h20L12 3Zm0 6v5m0 3v.5";

const label = (value: string) => value.replaceAll("_", " ");

function chip(text: string, ok: boolean, path: string) {
  const element = document.createElement("span");
  element.className = "action-chip";
  element.dataset.ok = String(ok);
  element.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
  element.append(text);
  return element;
}

function group(section: HTMLElement, title: string, rows: KeyValue[]) {
  const visible = rows.filter(([, value]) => value !== "");
  if (!visible.length) return null;
  const element = document.createElement("div");
  element.className = "action-group";
  element.dataset.actionGroup = title;
  const heading = document.createElement("div");
  heading.className = "action-group-title";
  heading.textContent = title;
  element.append(heading);
  keyValues(element, visible);
  section.append(element);
  return heading;
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
  const chips = document.createElement("div");
  chips.className = "action-chips";
  chips.append(
    chip(
      status.machine_clear ? "Clear" : "Not clear",
      status.machine_clear,
      status.machine_clear ? okPath : warnPath,
    ),
    chip(
      status.final_valid ? "Valid" : "Unvalidated",
      status.final_valid,
      status.final_valid ? okPath : warnPath,
    ),
  );
  for (const [name, on] of [
    ["Approve", status.permissions.approve],
    ["Merge", status.permissions.merge],
  ] as const)
    if (on) chips.append(chip(name, true, okPath));
  section.append(chips);
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
  const final = status.final_review;
  const execution = final?.execution;
  const result = execution?.result ?? null;
  const agent = execution?.selection.agent;
  group(section, "Agent", [
    ["Agent", agent?.name ?? ""],
    ["Model", result?.model ?? agent?.model ?? ""],
    [
      "Effort",
      agent ? (agent.intelligence?.reasoning_effort ?? "Default") : "",
    ],
    [
      "Tokens",
      result ? `${result.input_tokens} / ${result.output_tokens}` : "",
    ],
    ["Tools", result ? String(result.tool_calls) : ""],
    [
      "Read",
      result?.coverage
        ? `${result.coverage.read} of ${result.coverage.changed}`
        : "",
    ],
    ["Session", result?.session_id ?? "", true],
    ["State", execution ? label(execution.operation.state) : ""],
    ["Attempts", execution ? String(execution.operation.attempt_count) : ""],
    ["Order", final ? String(final.enqueue_order) : ""],
  ]);
  group(section, "Result", [
    ["Exit", result?.output.exit ? label(result.output.exit.exit) : ""],
    [
      "Personal review",
      status.personal_review.startsWith("Required")
        ? "Required"
        : "Not inferred",
    ],
    ["Evidence", status.final_valid ? "Valid" : "Unvalidated"],
    [
      "Observed",
      status.provider_observed_at
        ? new Date(status.provider_observed_at * 1000).toISOString()
        : "Never",
    ],
    ["Primary", status.primary_assignment_id ?? ""],
  ]);
  const provider: KeyValue[] = [];
  const peer = status.provider_approval;
  if (peer)
    provider.push(
      ["Approval", peer.id, true],
      ["Actor", peer.actor_id],
      ["Head", peer.head, true],
    );
  for (const effect of status.effects) {
    const name = effect.action === "approve" ? "Approve" : "Merge";
    provider.push([
      name,
      label(effect.state) + (effect.cancelled ? " \u00b7 cancelled" : ""),
    ]);
    provider.push([
      `${name} receipt`,
      effect.receipt?.id ?? "None",
      !!effect.receipt,
    ]);
    if (effect.receipt) {
      provider.push([`${name} actor`, effect.receipt.actor_id]);
      provider.push([`${name} head`, effect.receipt.head, true]);
      if (effect.receipt.merge_commit)
        provider.push(["Merge commit", effect.receipt.merge_commit, true]);
    }
    if (effect.error) provider.push([`${name} error`, effect.error]);
  }
  for (const blocker of new Set(status.blockers))
    provider.push(["Blocked", blocker]);
  if (status.observation_retry_blocker)
    provider.push(["Refresh", status.observation_retry_blocker]);
  const heading = group(section, "Provider", provider);
  const refreshable = status.blockers.length && status.provider_observed_at;
  if (refreshable) {
    const retry = document.createElement("button");
    retry.type = "button";
    retry.className = "action-refresh";
    retry.title = "Retry provider evidence";
    retry.setAttribute("aria-label", "Retry provider evidence");
    retry.innerHTML = `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${retryPath}"/></svg>`;
    retry.disabled = status.observation_retry_blocker !== null;
    retry.onclick = () =>
      void act("retry_action_observation", { itemId: status.item_id }, retry);
    if (heading) heading.append(retry);
    else section.append(retry);
  }
  if (final && execution) {
    if (configuration)
      renderConfiguration(
        section,
        execution.operation.attempt_count > 0
          ? "Configuration"
          : "Planned configuration",
        execution.selection,
      );
    const details = document.createElement("details");
    const title = document.createElement("summary");
    title.textContent = "JSON";
    const body = document.createElement("pre");
    body.textContent = JSON.stringify(final, null, 2);
    details.append(title, body);
    section.append(details);
  }
  for (const effect of status.effects) {
    const evidence = document.createElement("details");
    const heading = document.createElement("summary");
    heading.textContent = `JSON ${effect.action}`;
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
