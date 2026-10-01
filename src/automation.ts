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

export function mountAutomation(root: HTMLElement) {
  root.innerHTML = `<h2>Shared AI capacity</h2><p role="status" data-automation-status>Reading automation state...</p>
    <button type="button" data-toggle-automation disabled>Pause automation</button>
    <p class="hint">Pause stops new polling, AI work and provider writes. Stopping workers keep their slots until teardown. Already-started remote mutations may have succeeded and still require reconciliation. Capacity is saved in Settings > Preferences.</p>
    <p role="alert" data-automation-error hidden></p><ol data-ai-work></ol>`;
  const status = root.querySelector<HTMLElement>("[data-automation-status]")!;
  const button = root.querySelector<HTMLButtonElement>(
    "[data-toggle-automation]",
  )!;
  const error = root.querySelector<HTMLElement>("[data-automation-error]")!;
  const list = root.querySelector<HTMLOListElement>("[data-ai-work]")!;
  let state: AutomationSnapshot | undefined;
  let busy = false;
  let revision = 0;
  async function refresh() {
    if (busy || !root.isConnected) return;
    const request = ++revision;
    try {
      const next = await invoke<AutomationSnapshot>("automation_snapshot");
      if (!root.isConnected || busy || request !== revision) return;
      state = next;
      status.textContent = `${next.paused ? "Paused" : "Running"}; ${next.active} occupied / ${next.capacity} AI slots (${next.stopping} stopping); ${next.waiting} waiting; ${next.blocked} blocked.`;
      button.textContent = next.paused
        ? "Resume automation"
        : "Pause automation";
      button.disabled = false;
      list.replaceChildren(
        ...next.work.map((work) => {
          const row = document.createElement("li");
          row.dataset.workId = work.key.id;
          row.textContent = `${work.key.kind.replaceAll("_", " ")} ${work.key.id}: ${work.state}; queue order ${work.enqueue_order}${work.reason ? `. ${work.reason}` : ""}`;
          return row;
        }),
      );
    } catch (cause) {
      if (!root.isConnected || request !== revision) return;
      button.disabled = true;
      status.textContent =
        "Automation state unavailable; occupancy is unknown.";
      error.textContent =
        typeof cause === "string" ? cause : "Cannot read automation state.";
      error.hidden = false;
    }
  }
  button.onclick = async () => {
    if (!state || busy) return;
    busy = true;
    revision++;
    button.disabled = true;
    error.hidden = true;
    try {
      await invoke("set_automation_paused", { paused: !state.paused });
    } catch (cause) {
      error.textContent =
        typeof cause === "string"
          ? cause
          : "Automation change failed; inspect the current saved state.";
      error.hidden = false;
    } finally {
      busy = false;
      await refresh();
    }
  };
  void refresh();
  const timer = window.setInterval(() => {
    if (!root.isConnected) window.clearInterval(timer);
    else void refresh();
  }, 5000);
  return refresh;
}
