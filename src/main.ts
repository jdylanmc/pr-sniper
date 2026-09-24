import { invoke } from "@tauri-apps/api/core";
import crosshair from "./crosshair.svg";
import "./style.css";
import { mountSettings } from "./settings";

interface Snapshot {
  settings: {
    launch_at_login: boolean;
  } | null;
  login_registration: "absent" | "registered" | "invalid" | null;
  isolated: boolean;
  error: string | null;
  version: string;
}

interface Diagnostic {
  timestamp_secs: number;
  event: string;
}

const app = document.querySelector<HTMLElement>("#app")!;
const view = new URLSearchParams(location.search).get("view") ?? "status";
const titles: Record<string, string> = {
  queue: "Review Queue",
  settings: "Settings",
  status: "Status",
  diagnostics: "Diagnostics",
};
app.innerHTML = `
  <header><img src="${crosshair}" alt="PR Sniper crosshair" /><div>
    <p class="eyebrow">PR SNIPER</p>
    <h1></h1>
  </div></header>
  <p id="error" role="alert" hidden></p>
  <section id="content"></section>
  <footer>Final review stays human. Closing this window keeps PR Sniper in the menu bar.</footer>`;
app.querySelector("h1")!.textContent = titles[view] ?? "Status";
const content = app.querySelector<HTMLElement>("#content")!;
const error = app.querySelector<HTMLElement>("#error")!;
let loadRevision = 0;

function showError(message: string) {
  error.textContent = message;
  error.hidden = false;
}

async function load() {
  const revision = ++loadRevision;
  error.hidden = true;
  try {
    const state = await invoke<Snapshot>("snapshot");
    if (revision !== loadRevision) return;
    const messages = [state.error].filter(Boolean);
    if (messages.length) showError(messages.join("\n"));
    if (view === "diagnostics") {
      content.innerHTML = `<p>Local host events only. Tokens, commands, paths and provider data are never recorded. Most recent log, up to 256 KiB.</p><button id="refresh">Refresh</button><pre id="log"></pre>`;
      content
        .querySelector("#refresh")!
        .addEventListener("click", () => void load());
      const entries = await invoke<Diagnostic[]>("diagnostics");
      content.querySelector("#log")!.textContent = entries.length
        ? entries
            .map(
              (entry) =>
                `${new Date(entry.timestamp_secs * 1000).toISOString()}  ${entry.event}`,
            )
            .join("\n")
        : "No host events recorded.";
    } else if (view === "queue") {
      content.innerHTML = `<h2>No review queue yet</h2><p>Settings can verify GitHub connections and read PR metadata. Polling, reviews and comment publication are not implemented. Queue behavior arrives in a later slice.</p>`;
    } else {
      content.innerHTML = `<h2>Menu-bar host is running</h2><p>Verify GitHub connections and read complete PR metadata explicitly in Settings. Monitoring is not implemented, no background reviews are running, and no comments will be published.</p><p id="version"></p>`;
      content.querySelector("#version")!.textContent =
        `PR Sniper ${state.version}`;
    }
  } catch {
    if (revision !== loadRevision) return;
    showError(
      [
        "Could not read application status or diagnostics. Check local storage permissions; no raw error details are exposed.",
      ]
        .filter(Boolean)
        .join("\n"),
    );
  }
}

if (view === "settings") void mountSettings(app);
else void load();
window.addEventListener("focus", () => {
  if (view === "settings") return;
  void load();
});
