import { invoke } from "@tauri-apps/api/core";
import crosshair from "./crosshair.svg";
import "./style.css";

interface Snapshot {
  settings: { launch_at_login: boolean } | null;
  login_enabled: boolean | null;
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
  doctor: "Setup Doctor",
  diagnostics: "Diagnostics",
};
app.innerHTML = `
  <header><img src="${crosshair}" alt="PR Sniper crosshair" /><div>
    <p class="eyebrow">PR SNIPER / FOUNDATION</p>
    <h1></h1>
  </div></header>
  <p id="error" role="alert" hidden></p>
  <section id="content"></section>
  <footer>Final review stays human. Closing this window keeps PR Sniper in the menu bar.</footer>`;
app.querySelector("h1")!.textContent = titles[view] ?? "Status";
const content = app.querySelector<HTMLElement>("#content")!;
const error = app.querySelector<HTMLElement>("#error")!;

function showError(message: string) {
  error.textContent = message;
  error.hidden = false;
}

async function load() {
  error.hidden = true;
  try {
    const state = await invoke<Snapshot>("snapshot");
    if (state.error) showError(state.error);
    if (view === "settings") {
      content.innerHTML = `
        <h2>Startup</h2>
        <label><input id="login" type="checkbox" /> Launch PR Sniper at login</label>
        <p>Off by default. Changed only by your explicit choice here, never on application startup.</p>
        <p id="login-note"></p>
        <button id="diagnostics">Open redacted diagnostics</button>
        <h2>Connections</h2>
        <p>GitHub and review-agent setup are not implemented in this foundation.</p>`;
      const login = content.querySelector<HTMLInputElement>("#login")!;
      login.checked = state.login_enabled === true;
      login.disabled =
        state.isolated ||
        state.settings === null ||
        state.login_enabled === null;
      content.querySelector("#login-note")!.textContent = state.isolated
        ? "Isolated development run: changing macOS login items is disabled."
        : state.settings?.launch_at_login !== state.login_enabled
          ? "macOS login state differs from your saved preference. macOS is authoritative; no startup change was made."
          : "Current macOS login state is shown above.";
      login.addEventListener("change", async () => {
        login.disabled = true;
        try {
          await invoke("save_login", { enabled: login.checked });
          await load();
        } catch {
          // Reload OS truth even if persistence or diagnostics failed after a change.
          await load();
          showError(
            "Could not complete the startup preference change. Verify the displayed macOS state and local storage permissions.",
          );
        }
      });
      content
        .querySelector("#diagnostics")!
        .addEventListener("click", async () => {
          try {
            await invoke("open_diagnostics");
          } catch {
            showError("Could not open diagnostics.");
          }
        });
    } else if (view === "diagnostics") {
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
      content.innerHTML = `<h2>No review queue yet</h2><p>This foundation does not connect to GitHub, poll repositories, run reviews or publish comments. Queue behavior arrives in a later slice.</p>`;
    } else if (view === "doctor") {
      content.innerHTML = `<h2>Setup Doctor is not implemented yet</h2><p>No executables, accounts or permissions have been checked. This foundation runs no install or sign-in commands.</p><p>Use Settings to inspect redacted host diagnostics. Provider and agent health checks arrive in a later slice.</p>`;
    } else {
      content.innerHTML = `<h2>Menu-bar host is running</h2><p>Monitoring is not implemented. No repositories are connected, no background reviews are running, and no comments will be published.</p><p id="version"></p>`;
      content.querySelector("#version")!.textContent =
        `PR Sniper ${state.version}`;
    }
  } catch {
    showError(
      "Could not read application status or diagnostics. Check local storage permissions; no raw error details are exposed.",
    );
  }
}

void load();
window.addEventListener("focus", () => void load());
