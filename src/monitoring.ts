import { invoke } from "@tauri-apps/api/core";

interface Health {
  repository_id: string;
  name: string;
  last_attempt: number | null;
  last_success: number | null;
  next_run: number;
  last_failure: string | null;
  in_flight: boolean;
}

interface Job {
  repository_name: string;
  number: number;
  title: string;
  head_sha: string;
  watched_author: boolean;
  requested_reviewer: boolean;
  waiting: string;
}

interface MonitoringSnapshot {
  health: Health[];
  jobs: Job[];
}

const time = (value: number | null) =>
  value === null ? "Never" : new Date(value * 1000).toLocaleString();

export function renderMonitoring(
  root: HTMLElement,
  showError: (message: string) => void,
) {
  root.innerHTML = `<button id="check-now">Check Now</button>
    <p>Checks run in the menu-bar host, including while windows are closed. Agent execution and comment publication are not implemented; no review has been performed.</p>
    <h2>Schedule health</h2><section id="schedule-health"></section>
    <h2>Eligible revisions</h2><section id="review-jobs"></section>`;
  const check = root.querySelector<HTMLButtonElement>("#check-now")!;
  const health = root.querySelector<HTMLElement>("#schedule-health")!;
  const jobs = root.querySelector<HTMLElement>("#review-jobs")!;
  let loading = false;
  async function refresh() {
    if (loading || !root.isConnected) return;
    loading = true;
    try {
      const snapshot = await invoke<MonitoringSnapshot>("monitoring_snapshot");
      health.replaceChildren();
      jobs.replaceChildren();
      if (!snapshot.health.length)
        health.textContent =
          "No enabled repositories scheduled. Configure repositories in Settings.";
      for (const item of snapshot.health) {
        const row = document.createElement("p");
        row.textContent = `${item.name}: ${item.in_flight ? "Checking" : "Waiting"}. Last attempt: ${time(item.last_attempt)}. Last success: ${time(item.last_success)}. Next run: ${time(item.next_run)}. Last failure: ${item.last_failure ?? "None"}.`;
        health.append(row);
      }
      if (!snapshot.jobs.length)
        jobs.textContent = "No eligible revisions detected.";
      for (const job of snapshot.jobs) {
        const row = document.createElement("article");
        const title = document.createElement("h3");
        title.textContent = `${job.repository_name} #${job.number}: ${job.title}`;
        const detail = document.createElement("p");
        const reasons = [
          job.watched_author ? "watched author" : "",
          job.requested_reviewer ? "requested reviewer" : "",
        ]
          .filter(Boolean)
          .join(" and ");
        const waiting =
          job.waiting === "trust_confirmation"
            ? "Trust confirmation required; execution unavailable"
            : job.waiting === "human_start"
              ? "Waiting for human start; execution unavailable"
              : "Waiting for agent implementation";
        detail.textContent = `Head ${job.head_sha}. Trigger: ${reasons}. ${waiting}.`;
        row.append(title, detail);
        jobs.append(row);
      }
    } catch {
      showError(
        "Could not read monitoring state. Check local storage and diagnostics; this is not an empty successful check.",
      );
    } finally {
      loading = false;
    }
  }
  check.addEventListener("click", async () => {
    check.disabled = true;
    try {
      await invoke("check_now");
      await refresh();
    } catch {
      showError(
        "Could not start an immediate check. Check saved schedules and local storage.",
      );
    } finally {
      check.disabled = false;
    }
  });
  void refresh();
  const timer = window.setInterval(() => {
    if (!check.isConnected) window.clearInterval(timer);
    else void refresh();
  }, 1000);
}
