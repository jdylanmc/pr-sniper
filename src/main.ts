import { invoke } from "@tauri-apps/api/core";
import { renderRepositories, type Repository } from "./repositories";
import { renderPolicyForm, type Policy } from "./policy";
import crosshair from "./crosshair.svg";
import "./style.css";

interface Snapshot {
  settings: {
    launch_at_login: boolean;
    defaults: Policy;
    repositories?: Repository[];
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
  doctor: "Setup Doctor",
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
        <label><input id="login" type="checkbox" /> Request launch at login</label>
        <p>Off by default. Changed only by your explicit choice here, never on application startup.</p>
        <p id="login-note"></p>
        <button id="diagnostics">Open redacted diagnostics</button>
        <h2>Global defaults</h2>
        <p>These defaults apply unless a repository overrides a field. Agent start and comment publication are independent gates, both off by default. Future execution must recheck current settings; saved gates do not authorize action forever.</p>
        <section id="global-policy"></section>
        <section id="repository-settings"></section>
        <h2>Connections</h2>
        <p>GitHub and review-agent setup are not implemented in this foundation.</p>`;
      if (state.settings)
        renderPolicyForm(
          content.querySelector("#global-policy")!,
          state.settings.defaults,
          null,
          load,
          showError,
        );
      renderRepositories(
        content.querySelector("#repository-settings")!,
        state.settings === null ? null : (state.settings.repositories ?? []),
        state.settings?.defaults ?? null,
        load,
        showError,
      );
      const login = content.querySelector<HTMLInputElement>("#login")!;
      login.checked = state.settings?.launch_at_login === true;
      login.disabled =
        state.isolated ||
        state.settings === null ||
        state.login_registration === null;
      content.querySelector("#login-note")!.textContent = state.isolated
        ? "Isolated development run: changing macOS login items is disabled."
        : state.login_registration === null
          ? "Launch registration status is unavailable. No startup change was made. Check the error above; the checkbox shows only your saved request."
          : state.login_registration === "registered"
            ? "Registration targets this application. macOS may still prevent login launch; check Login Items. The checkbox shows your saved request, not effective macOS state."
            : state.login_registration === "absent"
              ? "No launch registration exists. The checkbox shows your saved request; startup never reapplies it."
              : "The launch registration is invalid or targets a different application. No startup change was made. The checkbox shows only your saved request.";
      login.addEventListener("change", async () => {
        login.disabled = true;
        try {
          await invoke("save_login", { enabled: login.checked });
          await load();
        } catch (cause) {
          // Refresh saved intent and registration after a partial or failed change.
          await load();
          showError(
            cause ===
              "Settings were not saved and the previous login registration could not be restored. Inspect macOS Login Items."
              ? "Settings were not saved and the previous registration could not be restored. Check macOS Login Items before relying on startup."
              : "Could not complete the startup preference change. Check the saved request, registration status, local permissions and macOS Login Items.",
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
