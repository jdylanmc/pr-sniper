import { invoke } from "@tauri-apps/api/core";
import { onCommitted, STATE_CHANGED } from "./changes";
import { setText } from "./stable-dom";

export interface AutomationSnapshot {
  paused: boolean;
  capacity: number;
  active: number;
  stopping: number;
  waiting: number;
  blocked: number;
  work: {
    key: { kind: "normal" | "primary_final" | "reply" | "mention"; id: string };
    enqueue_order: number;
    state: "active" | "stopping" | "waiting" | "blocked";
    reason: string | null;
    failure?: FailureClass | null;
    // Rate-limit backpressure: runnable work waiting until this Unix time.
    delayed_until?: number | null;
    offline?: { last_success: number | null; next_check_at: number } | null;
    // The primary waits for this teammate's report.
    blocked_by?: {
      teammate: { assignment_id: string; agent_name: string };
    } | null;
  }[];
}

export type FailureClass = "credentials" | "permission" | "network" | "other";

export function mountAutomation(
  root: HTMLElement,
  onSnapshot?: (state: AutomationSnapshot | undefined) => void,
  options: {
    compact?: boolean;
    onError?: (message: string) => void;
  } = {},
) {
  root.innerHTML = options.compact
    ? `<button type="button" class="monitoring-toggle" data-toggle-automation disabled aria-label="Monitoring unavailable"><span data-monitoring-label>Unavailable</span><span class="switch-track" aria-hidden="true"></span></button><span class="sr-only" role="status" data-automation-status>Loading</span><span class="sr-only" role="alert" data-automation-error hidden></span>`
    : `<h2>Shared AI capacity</h2><p role="status" data-automation-status>Loading</p>
    <button type="button" data-toggle-automation disabled>Pause automation</button>
    <p role="alert" data-automation-error hidden></p><ol data-ai-work></ol>`;
  const status = root.querySelector<HTMLElement>("[data-automation-status]")!;
  const button = root.querySelector<HTMLButtonElement>(
    "[data-toggle-automation]",
  )!;
  const error = root.querySelector<HTMLElement>("[data-automation-error]")!;
  const list = root.querySelector<HTMLOListElement>("[data-ai-work]");
  const view: { pending?: boolean; actionError?: string } = {};
  let state: AutomationSnapshot | undefined;
  let revision = 0;
  async function refresh() {
    if (view.pending || !root.isConnected) return;
    const request = ++revision;
    try {
      const next = await invoke<AutomationSnapshot>("automation_snapshot");
      if (!root.isConnected || view.pending || request !== revision) return;
      state = next;
      error.textContent = view.actionError ?? "";
      error.hidden = !error.textContent;
      onSnapshot?.(next);
      setText(
        status,
        `${next.paused ? "Paused" : "Running"}; ${next.active} occupied / ${next.capacity} AI slots (${next.stopping} stopping); ${next.waiting} waiting; ${next.blocked} blocked`,
      );
      const label = next.paused ? "Resume automation" : "Pause automation";
      if (options.compact) {
        button.setAttribute("aria-label", label);
        button.setAttribute("aria-pressed", String(!next.paused));
        setText(
          button.querySelector("[data-monitoring-label]")!,
          next.paused ? "Paused" : "Monitoring",
        );
        button.title = status.textContent ?? "";
      } else setText(button, label);
      button.disabled = false;
      list?.replaceChildren(
        ...next.work.map((work) => {
          const row = document.createElement("li");
          row.dataset.workId = work.key.id;
          row.textContent = `${work.key.kind.replaceAll("_", " ")} ${work.key.id}: ${work.state}; queue order ${work.enqueue_order}${work.reason ? `. ${work.reason}` : ""}`;
          return row;
        }),
      );
    } catch (cause) {
      if (!root.isConnected || request !== revision) return;
      state = undefined;
      onSnapshot?.(undefined);
      button.disabled = true;
      if (options.compact) {
        button.removeAttribute("aria-pressed");
        button.setAttribute("aria-label", "Monitoring unavailable");
        setText(
          button.querySelector("[data-monitoring-label]")!,
          "Unavailable",
        );
        button.title = "Unavailable";
      }
      status.textContent = "Unavailable";
      const message = typeof cause === "string" ? cause : "Unavailable";
      error.textContent = [view.actionError, message]
        .filter(Boolean)
        .join("\n");
      error.hidden = false;
      options.onError?.(message);
    }
  }
  button.onclick = async () => {
    if (!state || view.pending) return;
    view.pending = true;
    revision++;
    button.disabled = true;
    status.textContent = "Pending";
    error.hidden = true;
    view.actionError = undefined;
    try {
      await invoke("set_automation_paused", { paused: !state.paused });
    } catch (cause) {
      error.textContent = typeof cause === "string" ? cause : "Change failed";
      view.actionError = error.textContent;
      error.hidden = false;
      options.onError?.(error.textContent);
    } finally {
      view.pending = false;
      await refresh();
    }
  };
  void refresh();
  const timer = window.setInterval(() => {
    if (!root.isConnected) window.clearInterval(timer);
    else void refresh();
  }, 5000);
  onCommitted(root, [STATE_CHANGED], () => void refresh());
  return refresh;
}
