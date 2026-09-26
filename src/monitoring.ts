import { invoke } from "@tauri-apps/api/core";

interface Health {
  repository_id: string;
  name: string;
  enabled: boolean;
  last_attempt: number | null;
  last_success: number | null;
  next_run: number;
  schedule_available: boolean;
  last_failure: string | null;
  in_flight: boolean;
}

interface Job {
  account_id: string;
  account_login: string;
  repository_name: string;
  number: number;
  title: string;
  head_sha: string;
  author_id: string | null;
  author_login: string | null;
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
  root.innerHTML = `<button id="check-now" type="button">Check Now</button>
    <p>Monitoring runs only while the menu-bar app is active. Checks read GitHub metadata and queue eligible revisions; they never start an agent or publish comments.</p>
    <h2>Schedule health</h2><section id="schedule-health"></section>
    <h2>Detected pull requests</h2><section id="review-jobs"></section>`;
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
      if (!snapshot.health.length) {
        health.textContent =
          "No configured repositories. Add and bind a GitHub repository in Settings.";
      }
      for (const item of snapshot.health) {
        const row = document.createElement("p");
        const state = !item.enabled
          ? "Disabled"
          : item.in_flight
            ? "Checking"
            : "Scheduled";
        const next = !item.enabled
          ? "Disabled"
          : item.schedule_available
            ? time(item.next_run)
            : "Unavailable";
        row.textContent = `${item.name}: ${state}. Last attempt: ${time(item.last_attempt)}. Last success: ${time(item.last_success)}. Next run: ${next}. Last failure: ${item.last_failure ?? "None"}.`;
        health.append(row);
      }
      if (!snapshot.jobs.length)
        jobs.textContent = "No eligible revisions detected.";
      for (const job of snapshot.jobs) {
        const row = document.createElement("article");
        const title = document.createElement("h3");
        title.textContent = `${job.repository_name} #${job.number}: ${job.title}`;
        const author =
          job.author_login && job.author_id
            ? `${job.author_login} (${job.author_id})`
            : "deleted or unavailable";
        const reasons = [
          job.watched_author ? "watched author" : "",
          job.requested_reviewer ? "requested reviewer" : "",
        ]
          .filter(Boolean)
          .join(" and ");
        const waiting =
          job.waiting === "trust_confirmation"
            ? "Waiting for explicit trust confirmation; no review has started."
            : job.waiting === "human_start"
              ? "Automatic start is disabled; manual review start is not implemented here."
              : "Automatic start is configured, but review-agent support is not implemented here.";
        const details = document.createElement("p");
        details.textContent = `Acting account: ${job.account_login} (${job.account_id}). Author: ${author}. Trigger: ${reasons}. Head ${job.head_sha}. ${waiting}`;
        row.append(title, details);
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
  }, 5000);
}
