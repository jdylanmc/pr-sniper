import { invoke } from "@tauri-apps/api/core";

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
  }[];
}

export function mountAutomation(
  root: HTMLElement,
  onSnapshot?: (state: AutomationSnapshot | undefined) => void,
  options: {
    compact?: boolean;
    preferences?: boolean;
    onError?: (message: string) => void;
    view?: {
      pending?: boolean;
      error?: string;
      refresh?: () => Promise<void>;
    };
  } = {},
) {
  root.innerHTML = options.compact
    ? `<button type="button" class="monitoring-toggle" data-toggle-automation disabled aria-label="Monitoring unavailable"><span data-monitoring-label>Unavailable</span><span class="switch-track" aria-hidden="true"></span></button><span class="sr-only" role="status" data-automation-status>Reading automation state...</span><span class="sr-only" role="alert" data-automation-error hidden></span>`
    : `<h2>Shared AI capacity</h2><p role="status" data-automation-status>Reading automation state...</p>
    <button type="button" data-toggle-automation disabled>Pause automation</button>
    <p class="hint">Pause stops new polling, AI work and provider writes. Stopping workers keep their slots until teardown. Already-started remote mutations may have succeeded and still require reconciliation. Capacity is saved in Settings > Preferences.</p>
    <p role="alert" data-automation-error hidden></p><ol data-ai-work></ol>`;
  if (options.preferences)
    root.innerHTML = `<fieldset aria-label="Global pause"><legend>Global pause</legend>
      <p role="status" data-automation-status>Reading automation state...</p>
      <button type="button" data-toggle-automation disabled>Pause automation</button>
      <p class="settings-hint">Pause stops new polling, AI work and provider writes immediately. Stopping workers keep their slots until teardown. Already-started remote mutations may have succeeded and still require reconciliation.</p>
      <p role="alert" data-automation-error hidden></p>
      <details id="preferences-capacity-work"><summary>Capacity work details</summary><ol data-ai-work></ol></details></fieldset>`;
  const status = root.querySelector<HTMLElement>("[data-automation-status]")!;
  const button = root.querySelector<HTMLButtonElement>(
    "[data-toggle-automation]",
  )!;
  const error = root.querySelector<HTMLElement>("[data-automation-error]")!;
  const list = root.querySelector<HTMLOListElement>("[data-ai-work]");
  const view = options.view ?? {};
  let state: AutomationSnapshot | undefined;
  let revision = 0;
  async function refresh() {
    if (view.pending || !root.isConnected) return;
    error.textContent = view.error ?? "";
    error.hidden = !view.error;
    const request = ++revision;
    try {
      const next = await invoke<AutomationSnapshot>("automation_snapshot");
      if (!root.isConnected || view.pending || request !== revision) return;
      state = next;
      onSnapshot?.(next);
      status.textContent = `${next.paused ? "Paused" : "Running"}; ${next.active} occupied / ${next.capacity} AI slots (${next.stopping} stopping); ${next.waiting} waiting; ${next.blocked} blocked.`;
      const label = next.paused ? "Resume automation" : "Pause automation";
      if (options.compact) {
        button.setAttribute("aria-label", label);
        button.setAttribute("aria-pressed", String(!next.paused));
        button.querySelector("[data-monitoring-label]")!.textContent =
          next.paused ? "Paused" : "Monitoring";
        button.title = `${status.textContent} Pause is separate from repository monitoring settings.`;
      } else button.textContent = label;
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
        button.querySelector("[data-monitoring-label]")!.textContent =
          "Unavailable";
        button.title = "Automation state unavailable; occupancy is unknown.";
      }
      status.textContent =
        "Automation state unavailable; occupancy is unknown.";
      error.textContent =
        typeof cause === "string" ? cause : "Cannot read automation state.";
      view.error = error.textContent;
      error.hidden = false;
      options.onError?.(error.textContent);
    }
  }
  button.onclick = async () => {
    if (!state || view.pending) return;
    view.pending = true;
    revision++;
    button.disabled = true;
    error.hidden = true;
    view.error = undefined;
    try {
      await invoke("set_automation_paused", { paused: !state.paused });
    } catch (cause) {
      error.textContent =
        typeof cause === "string"
          ? cause
          : "Automation change failed; inspect the current saved state.";
      view.error = error.textContent;
      error.hidden = false;
      options.onError?.(error.textContent);
    } finally {
      view.pending = false;
      await view.refresh?.();
    }
  };
  view.refresh = refresh;
  error.textContent = view.error ?? "";
  error.hidden = !view.error;
  if (view.pending)
    status.textContent =
      "Automation change pending; the saved pause state is not yet confirmed.";
  void refresh();
  const timer = window.setInterval(() => {
    if (!root.isConnected) window.clearInterval(timer);
    else void refresh();
  }, 5000);
  return refresh;
}
