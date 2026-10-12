import { invoke } from "@tauri-apps/api/core";
import crosshair from "./crosshair.svg";
import "./style.css";
import { renderMonitoring, type MonitoringSnapshot } from "./monitoring";
import { renderIntelligenceDiagnostics } from "./work-presentation";
import { mountSettingsWithGenie } from "./genie";
import { mountPanel } from "./panel";
import type { DoctrineCatalog, Settings } from "./resources";
import {
  type Diagnostic,
  diagnosticLog,
  doctrineDiagnostics,
} from "./diagnostics";

interface Snapshot {
  settings: Pick<Settings, "launch_at_login" | "doctrine_reset"> | null;
  login_registration: "absent" | "registered" | "invalid" | null;
  isolated: boolean;
  error: string | null;
  version: string;
  doctrine_catalog?: DoctrineCatalog | null;
}

const app = document.querySelector<HTMLElement>("#app")!;
const view = new URLSearchParams(location.search).get("view") ?? "panel";
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
  <section id="content"></section>`;
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
      content.innerHTML = `<button id="refresh">Refresh</button><pre id="log"></pre>`;
      content
        .querySelector("#refresh")!
        .addEventListener("click", () => void load());
      const [entries, jobs] = await Promise.all([
        invoke<Diagnostic[]>("diagnostics"),
        invoke<MonitoringSnapshot>("monitoring_snapshot"),
      ]);
      if (revision !== loadRevision) return;
      content.querySelector("#log")!.textContent =
        doctrineDiagnostics(
          state.doctrine_catalog,
          state.settings?.doctrine_reset,
        ) + diagnosticLog(entries);
      renderIntelligenceDiagnostics(content, jobs);
    } else if (view === "queue") {
      renderMonitoring(content, showError);
    } else {
      content.innerHTML = `<p id="version"></p>`;
      content.querySelector("#version")!.textContent =
        `PR Sniper ${state.version}`;
    }
  } catch {
    if (revision !== loadRevision) return;
    showError("Failed");
  }
}

if (view === "panel") void mountPanel(app);
else if (view === "settings") void mountSettingsWithGenie(app);
else if (view === "queue") void load();
else void load();
window.addEventListener("focus", () => {
  if (view === "panel" || view === "settings" || view === "queue") return;
  void load();
});
