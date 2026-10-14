import { invoke } from "@tauri-apps/api/core";
import crosshair from "./crosshair.svg";
import "./style.css";
import { renderMonitoring } from "./monitoring";
import { mountSettingsWithGenie } from "./genie";
import { mountPanel } from "./panel";

const app = document.querySelector<HTMLElement>("#app")!;
const titles: Record<string, string> = {
  queue: "Review Queue",
  settings: "Settings",
};
const requested = new URLSearchParams(location.search).get("view") ?? "panel";
const view = Object.keys(titles).includes(requested) ? requested : "panel";
app.innerHTML = `
  <header><img src="${crosshair}" alt="PR Sniper crosshair" /><div>
    <p class="eyebrow">PR SNIPER</p>
    <h1></h1>
  </div></header>
  <p id="error" role="alert" hidden></p>
  <section id="content"></section>`;
app.querySelector("h1")!.textContent = titles[view] ?? "PR Sniper";
const content = app.querySelector<HTMLElement>("#content")!;
const error = app.querySelector<HTMLElement>("#error")!;

function showError(message: string) {
  error.textContent = message;
  error.hidden = false;
}

async function loadQueue() {
  error.hidden = true;
  try {
    const state = await invoke<{ error: string | null }>("snapshot");
    if (state.error) showError(state.error);
    renderMonitoring(content, showError);
  } catch {
    showError("Failed");
  }
}

if (view === "panel") void mountPanel(app);
else if (view === "settings") void mountSettingsWithGenie(app);
else void loadQueue();
