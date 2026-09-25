import { invoke } from "@tauri-apps/api/core";
import { renderConnection } from "./connections";
import { renderGithubAuth, type GithubAccount } from "./github-auth";
import {
  renderCopilotAuth,
  modelSelectable,
  type CopilotAccount,
  type CopilotAuth,
  type CopilotModel,
} from "./copilot";
import type {
  Agent,
  Doctrine,
  Policy,
  Assignment,
  Schedule,
  WatchedIdentity,
} from "./policy";
import type { Repository } from "./repositories";
import "./settings.css";
import { createDialogs } from "./dialogs";

interface ConfiguredRepository extends Repository {
  watched_authors?: WatchedIdentity[];
  assignments?: Assignment[];
}
interface Settings {
  launch_at_login: boolean;
  defaults: Policy;
  repositories?: ConfiguredRepository[];
  root_folder?: string;
  doctrines?: Doctrine[];
  agents?: Agent[];
}
interface Snapshot {
  settings: Settings | null;
  settings_persisted: boolean;
  isolated: boolean;
  login_registration: "absent" | "registered" | "invalid" | null;
  error: string | null;
}
interface Discovered {
  path: string;
  name: string | null;
  unavailable: string | null;
}
interface Discovery {
  root: string;
  repositories: Discovered[];
  warnings: string[];
}
type Section = "doctrines" | "agents" | "integrations" | "preferences";

// Direct integrations, distinct from the models available through Copilot.
const modelProviders: { id: string; label: string; available: boolean }[] = [
  { id: "copilot", label: "GitHub Copilot", available: true },
  { id: "claude", label: "Claude", available: false },
  { id: "codex", label: "Codex", available: false },
  { id: "grok", label: "Grok", available: false },
];

const sections: Record<Section, [string, string]> = {
  integrations: [
    "Integrations",
    "Sign in to an AI subscription, then connect the repositories it should watch.",
  ],
  doctrines: [
    "Doctrines",
    "Principles your agents review by. Plain text, yours to edit.",
  ],
  agents: [
    "Agents",
    "A model, a doctrine, a prompt and a signature, bundled up and ready to work.",
  ],
  preferences: [
    "Preferences",
    "The small stuff: login items, diagnostics, notifications.",
  ],
};
// Bespoke reticle/scope marks, not a generic icon-kit -- each nods at the
// tab's job rather than a stock glyph.
const iconPaths = {
  integrations:
    '<circle cx="8" cy="8" r="3.4"/><circle cx="17" cy="17" r="3.4"/><path d="M10.4 10.4 14.6 14.6"/>',
  doctrines:
    '<path d="M12 5c-2-1.3-5-1.6-7-1v14c2-.6 5-.3 7 1 2-1.3 5-1.6 7-1V4c-2-.6-5-.3-7 1Z"/><path d="M12 5v14"/>',
  agents:
    '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3"/>',
  preferences: '<circle cx="12" cy="12" r="7.5"/><path d="M12 8v4l2.6 2.6"/>',
  folder: '<path d="M3 6h7l2 2h9v12H3zM3 6V4h7l2 2"/>',
};
const icon = (key: keyof typeof iconPaths) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${iconPaths[key]}</svg>`;
const escape = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (c) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[c]!,
  );
// Settings are JSON data; older macOS webviews do not expose structuredClone.
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
function newIdentity() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
const localZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone;
const option = (value: string, label: string, selected: string) =>
  `<option value="${escape(value)}" ${value === selected ? "selected" : ""}>${escape(label)}</option>`;
const words = (text: string) =>
  text.trim() ? text.trim().split(/\s+/).length : 0;
const scheduleSummary = (schedule: Schedule) =>
  schedule.kind === "interval"
    ? `Every ${schedule.minutes} minutes`
    : `Cron ${schedule.expression}`;
const reason = (error: unknown) => {
  const errors: Record<string, string> = {
    signed_out:
      "GitHub is disconnected. Connect the PR Sniper GitHub OAuth App, then try again.",
    missing_read_permission:
      "This person is unavailable. Check the GitHub login and your account access.",
    rate_limited: "GitHub rate limited this lookup. Wait before trying again.",
    network: "Cannot reach GitHub. Check your network and try again.",
    timeout: "GitHub lookup timed out. Try again.",
    invalid_response:
      "Enter a valid GitHub login and try again. No identity was added.",
    provider_failure:
      "GitHub lookup failed. Check provider health and try again.",
  };
  return typeof error === "string"
    ? (errors[error] ?? error)
    : "This action failed. Check local access and try again.";
};

export async function mountSettings(app: HTMLElement) {
  document.body.classList.add("settings-page");
  app.className = "settings-window";
  app.innerHTML = `<aside class="settings-sidebar"><div class="settings-brand"><svg viewBox="0 0 32 32" fill="none" aria-hidden="true"><circle cx="16" cy="16" r="9" stroke="currentColor" stroke-width="1.7"/><path d="M16 2v8m0 12v8M2 16h8m12 0h8" stroke="currentColor" stroke-width="1.7"/><circle cx="16" cy="16" r="2.5" fill="currentColor"/></svg>PR Sniper</div><p class="settings-caption">Preferences</p>
    <nav aria-label="Settings sections">${Object.entries(sections)
      .map(
        ([key, [title]]) =>
          `<button type="button" data-section="${key}">${icon(key as Section)}${title}</button>`,
      )
      .join("")}</nav>
    <label class="mobile-section">Section<select aria-label="Settings section">${Object.entries(
      sections,
    )
      .map(([key, [title]]) => option(key, title, "integrations"))
      .join("")}</select></label></aside>
    <div class="settings-main"><header class="settings-heading"><h1 tabindex="-1">Integrations</h1><p>Sign in to an AI subscription, then connect the repositories it should watch.</p></header>
    <p id="error" role="alert" hidden></p><section id="content"></section>
    <footer class="settings-savebar"><span role="status" id="save-status">Loading settings...</span><button id="reload-settings" hidden>Discard draft and reload</button><button id="reset-settings" disabled>Reset changes</button><button class="primary" id="save-settings" disabled>Save changes</button></footer></div>`;
  const content = app.querySelector<HTMLElement>("#content")!;
  const error = app.querySelector<HTMLElement>("#error")!;
  const status = app.querySelector<HTMLElement>("#save-status")!;
  const save = app.querySelector<HTMLButtonElement>("#save-settings")!;
  const reset = app.querySelector<HTMLButtonElement>("#reset-settings")!;
  const reload = app.querySelector<HTMLButtonElement>("#reload-settings")!;
  let snapshot: Snapshot;
  let saved: Settings;
  let persisted: Settings;
  let draft: Settings;
  let section: Section = "integrations";
  let discovery: Discovery | null = null;
  let query = "";
  let busy = false;
  let conflict = false;
  let revision = 0;
  let githubAccounts: GithubAccount[] = [];
  let copilotAccounts: CopilotAccount[] = [];
  let disposeCopilot: (() => void) | undefined;
  let updateAgentAccounts: (() => void) | undefined;
  let refreshAgentAccounts: (() => void) | undefined;
  const dialogs = createDialogs(content, () => revision++);
  const dirty = () =>
    !!draft && JSON.stringify(draft) !== JSON.stringify(saved);
  const showError = (message: string) => {
    error.textContent = message;
    error.hidden = false;
  };
  const clearError = () => {
    error.hidden = true;
    error.textContent = "";
  };
  const repositories = () => draft.repositories ?? [];
  const doctrines = () => draft.doctrines ?? [];
  const agents = () => draft.agents ?? [];
  function changed() {
    revision++;
    status.textContent = dirty() ? "Unsaved changes" : "All changes saved";
    save.disabled = !dirty() || busy;
    reset.disabled = !dirty() || busy;
    reload.hidden = !conflict;
    reload.disabled = busy;
  }

  function dialog(title: string, body: string) {
    const modal = document.createElement("dialog");
    modal.className = "settings-dialog";
    modal.setAttribute("aria-label", title);
    modal.innerHTML = `<div class="dialog-head"><h2>${escape(title)}</h2><button type="button" aria-label="Close dialog">Close</button></div><div class="dialog-body">${body}</div>`;
    modal.querySelector("button")!.onclick = () => modal.close();
    dialogs.show(modal);
    return modal;
  }
  function confirmDialog(title: string, body: string, action: string) {
    const modal = dialog(
      title,
      `<p>${body}</p><button class="primary" id="confirm">${escape(action)}</button>`,
    );
    return new Promise<boolean>((resolve) => {
      let resolved = false;
      modal.querySelector<HTMLButtonElement>("#confirm")!.onclick = () => {
        resolved = true;
        modal.close();
        resolve(true);
      };
      modal.addEventListener("close", () => {
        if (!resolved) resolve(false);
      });
    });
  }

  function render() {
    if (!draft) return;
    dialogs.closeAll();
    updateAgentAccounts = undefined;
    refreshAgentAccounts = undefined;
    disposeCopilot?.();
    disposeCopilot = undefined;
    app.querySelector("h1")!.textContent = sections[section][0];
    app.querySelector(".settings-heading p")!.textContent =
      sections[section][1];
    app
      .querySelectorAll<HTMLButtonElement>("[data-section]")
      .forEach((button) => {
        if (button.dataset.section === section)
          button.setAttribute("aria-current", "page");
        else button.removeAttribute("aria-current");
      });
    app.querySelector<HTMLSelectElement>(".mobile-section select")!.value =
      section;
    content.replaceChildren();
    if (section === "integrations") renderIntegrations();
    if (section === "doctrines") renderDoctrines();
    if (section === "agents") renderAgents();
    if (section === "preferences") renderPreferences();
    changed();
  }
  function navigate(next: Section) {
    if (busy) return;
    section = next;
    render();
  }
  app
    .querySelectorAll<HTMLButtonElement>("[data-section]")
    .forEach((button) => {
      button.onclick = () => navigate(button.dataset.section as Section);
    });
  app.querySelector<HTMLSelectElement>(".mobile-section select")!.onchange = (
    event,
  ) => {
    navigate((event.target as HTMLSelectElement).value as Section);
  };

  // ---------------------------------------------------------------- Doctrines

  function renderDoctrines() {
    content.innerHTML = `<div class="section-actions"><h2>Your doctrines</h2><button class="primary" id="new-doctrine">New doctrine</button></div><p class="settings-hint">Doctrines are plain-text principles -- not commands. Attach one to an agent and it colors every review that agent does. Around 500 words is a friendly length, never a limit.</p><div class="doctrine-list"></div>`;
    const list = content.querySelector(".doctrine-list")!;
    if (!doctrines().length)
      list.innerHTML =
        '<div class="settings-empty"><strong>No doctrines yet</strong><p>Create a doctrine to add your own review principles.</p></div>';
    for (const doctrine of doctrines()) {
      const row = document.createElement("article");
      row.className = "doctrine-card";
      row.innerHTML = `<div><h3>${escape(doctrine.title)}</h3><p>${escape(doctrine.body)}</p><p class="word-count">${words(doctrine.body)} words</p></div><div class="card-actions"><button data-edit>Edit</button><button data-remove>Delete</button></div>`;
      row.querySelector<HTMLButtonElement>("[data-edit]")!.onclick = () =>
        editDoctrine(doctrine);
      row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick =
        async () => {
          const usedBy = agents().filter((a) => a.doctrine === doctrine.title);
          if (
            !(await confirmDialog(
              "Delete this doctrine?",
              `Delete "${escape(doctrine.title)}"? ${usedBy.length ? `${usedBy.length} agent${usedBy.length === 1 ? "" : "s"} using it will fall back to no doctrine.` : "Nothing is removed until you save changes."}`,
              "Delete doctrine",
            ))
          )
            return;
          draft.doctrines = doctrines().filter((d) => d !== doctrine);
          for (const a of agents())
            if (a.doctrine === doctrine.title) delete a.doctrine;
          render();
        };
      list.append(row);
    }
    content.querySelector<HTMLButtonElement>("#new-doctrine")!.onclick = () =>
      editDoctrine();
  }

  function editDoctrine(existing?: Doctrine) {
    const modal = dialog(
      existing ? "Edit doctrine" : "New doctrine",
      `<form><label>Title<input name="title" required maxlength="100" value="${escape(existing?.title ?? "")}" placeholder="e.g. boundaries" /></label><label>Principles<textarea name="body" rows="12" required>${escape(existing?.body ?? "")}</textarea></label><p class="word-count" data-count>${words(existing?.body ?? "")} words</p><p class="settings-hint">Title doubles as this doctrine's slug -- keep it short and unique. Plain text only, never credentials.</p><p role="alert" hidden></p><button class="primary">Save doctrine</button></form>`,
    );
    const body = modal.querySelector<HTMLTextAreaElement>("[name=body]")!;
    const count = modal.querySelector<HTMLElement>("[data-count]")!;
    body.oninput = () => {
      count.textContent = `${words(body.value)} words`;
    };
    modal.querySelector("form")!.onsubmit = (event) => {
      event.preventDefault();
      const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
      const title = modal
        .querySelector<HTMLInputElement>("[name=title]")!
        .value.trim();
      try {
        if (!title || !body.value.trim())
          throw "Give this doctrine a title and some principles.";
        if (
          doctrines().some(
            (d) =>
              d !== existing && d.title.toLowerCase() === title.toLowerCase(),
          )
        )
          throw "Choose a unique doctrine title.";
        if (existing) {
          for (const a of agents())
            if (a.doctrine === existing.title) a.doctrine = title;
          existing.title = title;
          existing.body = body.value;
        } else (draft.doctrines ??= []).push({ title, body: body.value });
        modal.close();
        render();
      } catch (cause) {
        alert.textContent = typeof cause === "string" ? cause : reason(cause);
        alert.hidden = false;
      }
    };
  }

  // -------------------------------------------------------------------- Agents

  function renderAgents() {
    content.innerHTML = `<div class="section-actions"><h2>Your agents</h2><button class="primary" id="new-agent" disabled>New agent</button></div><p class="settings-hint">Each Agent chooses an AI account and a model returned by that account. Doctrine, prompt and signature stay reusable across repository assignments. Repository credentials, not the AI account, determine the acting identity for repository access and future comments.</p><p class="settings-notice" data-copilot-status>Reading Copilot accounts...</p><button id="manage-copilot">Manage Copilot accounts</button><div class="agent-list"></div>`;
    const list = content.querySelector(".agent-list")!;
    if (!agents().length)
      list.innerHTML =
        '<div class="settings-empty"><strong>No agents yet</strong><p>Create one to start assigning it to repositories in Integrations.</p></div>';
    for (const agent of agents()) {
      const account = copilotAccounts.find(
        (a) => a.account_id === agent.ai_account?.account_id,
      );
      const row = document.createElement("article");
      row.className = "agent-card";
      row.innerHTML = `<div><h3>${escape(agent.name)}</h3><p>${escape(agent.prompt)}</p><p data-account-state>${escape(agent.ai_account ? `Copilot: ${account?.login ?? agent.ai_account.account_id}. ${account?.state === "connected" ? "Sign-in verified; model access checked in Edit." : "Reconnect required; Agent blocked."}` : "Unconfigured. Choose an AI account and an actual model; the legacy selection is retained.")}</p><div class="chip-row"><span class="chip">${escape(agent.model)}</span>${agent.doctrine ? `<span class="chip">${escape(agent.doctrine)}</span>` : ""}<span class="chip">${escape(agent.signature)}</span></div></div><div class="card-actions"><button data-edit>Edit</button><button data-remove>Delete</button></div>`;
      row.dataset.agentId = agent.id;
      row.querySelector<HTMLButtonElement>("[data-edit]")!.onclick = () =>
        editAgent(agent);
      row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick =
        async () => {
          const assigned = repositories().filter((r) =>
            (r.assignments ?? []).some((a) => a.agent_id === agent.id),
          );
          if (
            !(await confirmDialog(
              "Delete this agent?",
              `Delete "${escape(agent.name)}"? ${assigned.length ? `It is assigned to ${assigned.length} repositor${assigned.length === 1 ? "y" : "ies"}; those assignments will be removed too.` : "Nothing is removed until you save changes."}`,
              "Delete agent",
            ))
          )
            return;
          draft.agents = agents().filter((a) => a !== agent);
          for (const repository of repositories())
            repository.assignments = (repository.assignments ?? []).filter(
              (a) => a.agent_id !== agent.id,
            );
          render();
        };
      list.append(row);
    }
    content.querySelector<HTMLButtonElement>("#new-agent")!.onclick = () =>
      editAgent();
    content.querySelector<HTMLButtonElement>("#manage-copilot")!.onclick = () =>
      navigate("integrations");
    const notice = content.querySelector<HTMLElement>("[data-copilot-status]")!;
    let accountRequest = 0;
    const refreshAccounts = () => {
      if (!notice.isConnected) return;
      const request = ++accountRequest;
      void invoke<CopilotAuth>("copilot_auth_state").then(
        (view) => {
          if (!notice.isConnected || request !== accountRequest) return;
          copilotAccounts = view.accounts;
          updateAgentAccounts?.();
          const connected = copilotAccounts.some(
            (a) => a.state === "connected",
          );
          notice.textContent = connected
            ? "Copilot sign-in is separate from model access. Edit an Agent to load this account's current models."
            : "No verified Copilot connection. Connect an account in Integrations; existing Agents and assignments are retained.";
          content.querySelector<HTMLButtonElement>("#new-agent")!.disabled =
            !connected;
          for (const agent of agents()) {
            const state = content.querySelector<HTMLElement>(
              `[data-agent-id="${agent.id}"] [data-account-state]`,
            );
            const account = copilotAccounts.find(
              (a) => a.account_id === agent.ai_account?.account_id,
            );
            if (state && agent.ai_account)
              state.textContent = `Copilot: ${account?.login ?? agent.ai_account.account_id}. ${account?.state === "connected" ? "Sign-in verified; model access checked in Edit." : "Reconnect required; Agent blocked."}`;
          }
          if (view.accounts.some((a) => a.reason === "verification_pending"))
            setTimeout(refreshAccounts, 350);
        },
        () => {
          if (notice.isConnected && request === accountRequest)
            notice.textContent =
              "Copilot account state is unavailable. Retry from Manage Copilot accounts. Existing Agents are retained.";
        },
      );
    };
    refreshAgentAccounts = refreshAccounts;
    refreshAccounts();
  }

  function editAgent(existing?: Agent) {
    const modal = dialog(
      existing ? "Edit agent" : "New agent",
      `<form><label>Name<input name="name" required maxlength="80" value="${escape(existing?.name ?? "")}" placeholder="e.g. The Nitpicker" /></label>
        <p class="settings-hint">Provider: GitHub Copilot. Claude models offered through Copilot are allowed; the direct Claude integration remains unavailable.</p>
        <label>AI account<select name="ai-account" aria-label="AI account"><option value="">Choose a Copilot account</option>${copilotAccounts.map((a) => `<option value="${escape(a.account_id)}" ${a.account_id === existing?.ai_account?.account_id ? "selected" : ""} ${a.state === "connected" ? "" : "disabled"}>${escape(a.login)} (${escape(a.account_id)})${a.state === "connected" ? "" : " - reconnect required"}</option>`).join("")}${existing?.ai_account && !copilotAccounts.some((a) => a.account_id === existing.ai_account?.account_id) ? `<option selected disabled value="${escape(existing.ai_account.account_id)}">Copilot ${escape(existing.ai_account.account_id)} - reconnect required</option>` : ""}</select></label>
        <label>Model<select name="model" aria-label="Model" disabled><option value="${escape(existing?.model ?? "")}">${escape(existing?.model ?? "Choose an account first")}</option></select></label>
        <p class="settings-hint" data-model-status role="status"></p><button type="button" data-retry-models>Retry model list</button><button type="button" data-cancel-models hidden>Cancel model lookup</button>
        <label>Doctrine<select name="doctrine">${option("", "None", existing?.doctrine ?? "")}${doctrines()
          .map((d) => option(d.title, d.title, existing?.doctrine ?? ""))
          .join("")}</select></label>
        <label>Prompt<textarea name="prompt" rows="4" required>${escape(existing?.prompt ?? "Review this pull request for correctness, risk, and readability.")}</textarea></label>
        <label>Signature<input name="signature" required maxlength="80" value="${escape(existing?.signature ?? "PR Sniper \u{1F3AF}")}" /></label>
        <p class="settings-hint">No review or test prompt runs here. Saving an existing unconfigured Agent preserves its selection until you explicitly replace it.</p><p role="alert" hidden></p><button class="primary">Save agent</button></form>`,
    );
    const accountSelect =
      modal.querySelector<HTMLSelectElement>("[name=ai-account]")!;
    const modelSelect = modal.querySelector<HTMLSelectElement>("[name=model]")!;
    const modelStatus = modal.querySelector<HTMLElement>(
      "[data-model-status]",
    )!;
    const retryModels = modal.querySelector<HTMLButtonElement>(
      "[data-retry-models]",
    )!;
    const cancelModels = modal.querySelector<HTMLButtonElement>(
      "[data-cancel-models]",
    )!;
    let catalog: CopilotModel[] = [];
    let request = 0;
    let pendingLookup: { accountId: string; requestId: string } | undefined;
    const cancelLookup = () => {
      request++;
      if (pendingLookup) {
        const lookup = pendingLookup;
        pendingLookup = undefined;
        void invoke("cancel_copilot_models", lookup).catch(() => {
          if (modal.isConnected)
            modelStatus.textContent =
              "Cancellation could not be confirmed. The native lookup remains time-bounded.";
        });
      }
      cancelModels.hidden = true;
    };
    const observer = new MutationObserver(() => {
      if (!modal.isConnected) {
        cancelLookup();
        observer.disconnect();
      }
    });
    observer.observe(content, { childList: true, subtree: true });
    async function loadModels() {
      cancelLookup();
      catalog = [];
      const accountId = accountSelect.value;
      const selected = modelSelect.value;
      const current = ++request;
      modelSelect.disabled = true;
      if (
        !copilotAccounts.some(
          (a) => a.account_id === accountId && a.state === "connected",
        )
      ) {
        modelStatus.textContent =
          "Connect or reconnect this account in Integrations. The saved model is retained, but this Agent is unconfigured.";
        return;
      }
      const lookup = { accountId, requestId: newIdentity() };
      pendingLookup = lookup;
      cancelModels.hidden = false;
      modelStatus.textContent = "Loading models from this Copilot account...";
      try {
        const models = await invoke<CopilotModel[]>(
          "list_copilot_models",
          lookup,
        );
        if (
          !modal.isConnected ||
          current !== request ||
          accountSelect.value !== accountId
        )
          return;
        catalog = models;
        modelSelect.innerHTML =
          option("", "Choose a model", selected) +
          (selected && !models.some((m) => m.id === selected)
            ? `<option selected disabled value="${escape(selected)}">${escape(selected)} - unavailable; retained</option>`
            : "") +
          models
            .map(
              (m) =>
                `<option value="${escape(m.id)}" ${m.id === selected ? "selected" : ""} ${modelSelectable(m) ? "" : "disabled"}>${escape(m.name)} (${escape(m.id)})${modelSelectable(m) ? "" : " - unavailable by policy"}</option>`,
            )
            .join("");
        modelSelect.disabled = !models.some(modelSelectable);
        describeModel();
      } catch (cause) {
        if (modal.isConnected && current === request)
          modelStatus.textContent = `${reason(cause)} No replacement model selected. Retry when ready.`;
      } finally {
        if (current === request) {
          pendingLookup = undefined;
          cancelModels.hidden = true;
        }
      }
    }
    function describeModel() {
      const model = catalog.find((m) => m.id === modelSelect.value);
      const notices = [
        model?.policy?.terms,
        model?.warningText?.dataRetention,
        ...(model?.infoMessages?.map((m) => m.message) ?? []),
      ].filter(Boolean);
      modelStatus.textContent = !catalog.length
        ? "Copilot returned no models for this account. Sign-in remains verified; check access or policy and retry."
        : modelSelect.value && (!model || !modelSelectable(model))
          ? "The saved model is unavailable. It is retained, not replaced. Choose an available model or retry."
          : `${catalog.length} models returned by Copilot. This is not an inference test. ${notices.join(" ")}`;
    }
    accountSelect.onchange = () => {
      selectedWasConnected = copilotAccounts.some(
        (a) => a.account_id === accountSelect.value && a.state === "connected",
      );
      modelSelect.innerHTML = '<option value="">Choose a model</option>';
      void loadModels();
    };
    modelSelect.onchange = describeModel;
    retryModels.onclick = () => void loadModels();
    cancelModels.onclick = () => {
      cancelLookup();
      modelStatus.textContent = "Model lookup cancelled. Retry when ready.";
    };
    let selectedWasConnected = copilotAccounts.some(
      (a) => a.account_id === accountSelect.value && a.state === "connected",
    );
    updateAgentAccounts = () => {
      if (!modal.isConnected) return;
      const selected = accountSelect.value;
      accountSelect.innerHTML =
        option("", "Choose a Copilot account", selected) +
        copilotAccounts
          .map(
            (a) =>
              `<option value="${escape(a.account_id)}" ${a.account_id === selected ? "selected" : ""} ${a.state === "connected" ? "" : "disabled"}>${escape(a.login)} (${escape(a.account_id)})${a.state === "connected" ? "" : " - reconnect required"}</option>`,
          )
          .join("") +
        (selected && !copilotAccounts.some((a) => a.account_id === selected)
          ? `<option selected disabled value="${escape(selected)}">Copilot ${escape(selected)} - reconnect required</option>`
          : "");
      const connected = copilotAccounts.some(
        (a) => a.account_id === selected && a.state === "connected",
      );
      if (!connected) {
        cancelLookup();
        catalog = [];
        modelSelect.disabled = true;
        modelStatus.textContent = selected
          ? "Reconnect this account in Integrations. Your model and draft are retained."
          : "Choose a verified Copilot account, then load its models.";
      } else if (!selectedWasConnected) {
        void loadModels();
      }
      selectedWasConnected = connected;
    };
    void loadModels();
    modal.querySelector("form")!.onsubmit = (event) => {
      event.preventDefault();
      const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
      const name = modal
        .querySelector<HTMLInputElement>("[name=name]")!
        .value.trim();
      const model =
        modal.querySelector<HTMLSelectElement>("[name=model]")!.value;
      const doctrine =
        modal.querySelector<HTMLSelectElement>("[name=doctrine]")!.value;
      const prompt =
        modal.querySelector<HTMLTextAreaElement>("[name=prompt]")!.value;
      const signature = modal
        .querySelector<HTMLInputElement>("[name=signature]")!
        .value.trim();
      try {
        if (!name || !prompt.trim() || !signature)
          throw "Give this agent a name, a prompt, and a signature.";
        if (
          agents().some(
            (a) =>
              a !== existing && a.name.toLowerCase() === name.toLowerCase(),
          )
        )
          throw "Choose a unique agent name.";
        const accountId = accountSelect.value;
        const unchanged =
          !!existing &&
          accountId === (existing.ai_account?.account_id ?? "") &&
          model === existing.model;
        if (
          !unchanged &&
          (!accountId ||
            !catalog.some((m) => m.id === model && modelSelectable(m)))
        )
          throw "Choose a verified AI account and a model returned for that account. Retry the model list if unavailable.";
        const values: Agent = {
          id: existing?.id ?? newIdentity(),
          name,
          model,
          ...(accountId
            ? {
                ai_account: {
                  provider: "copilot" as const,
                  account_id: accountId,
                },
              }
            : {}),
          prompt,
          signature,
          ...(doctrine ? { doctrine } : {}),
        };
        if (existing) {
          Object.assign(existing, values);
          if (!doctrine) delete existing.doctrine;
        } else (draft.agents ??= []).push(values);
        modal.close();
        render();
      } catch (cause) {
        alert.textContent = typeof cause === "string" ? cause : reason(cause);
        alert.hidden = false;
      }
    };
  }

  // ------------------------------------------------------------- Integrations

  async function scan(choose: boolean) {
    clearError();
    busy = true;
    changed();
    try {
      const result = choose
        ? await invoke<Discovery | null>("choose_repository_folder")
        : await invoke<Discovery>("discover_repositories", {
            root: draft.root_folder,
          });
      if (result) {
        discovery = result;
        draft.root_folder = result.root;
      }
    } catch (cause) {
      showError(reason(cause));
    } finally {
      busy = false;
      render();
    }
  }

  function renderIntegrations() {
    content.innerHTML = `<div class="integration-group"><h2>AI integration</h2><div class="copilot-auth"></div><div class="integration-grid">${modelProviders
      .filter((m) => !m.available)
      .map(
        (m) =>
          `<div class="integration-card" data-disabled="true"><strong>Direct ${escape(m.label)}</strong><p>Coming soon.</p></div>`,
      )
      .join("")}</div></div>
      <div class="integration-group"><h2>Git repositories</h2><div class="github-auth"></div><div class="folder-card"><div class="folder-symbol">${icon("folder")}</div><div><strong>${escape(draft.root_folder ?? "Choose your repository folder")}</strong><p>${discovery ? `${discovery.repositories.length} local repositories discovered` : "Only a folder you choose is scanned."}</p></div><button id="choose-folder">Choose folder...</button></div>
      <div class="repository-toolbar"><input id="repo-search" type="search" aria-label="Find a repository" placeholder="Find a repository..." value="${escape(query)}" /><button id="select-visible">Select visible</button></div>
      <div class="list-label"><span>Repository</span><span id="selected-count"></span></div><div class="repository-list"></div>
      <p class="settings-hint">Monitoring configuration only. Reviews and comments stay separate. No polling runs in this build.</p>
      <div class="settings-actions"><button id="add-repository">Add repository manually...</button>${draft.root_folder ? '<button id="rescan">Scan chosen folder</button>' : ""}</div>
      ${discovery?.warnings.map((warning) => `<p class="settings-notice">${escape(warning)}</p>`).join("") ?? ""}</div>`;
    disposeCopilot = renderCopilotAuth(
      content.querySelector(".copilot-auth")!,
      (accounts) => {
        copilotAccounts = accounts;
      },
    );
    renderGithubAuth(
      content.querySelector(".github-auth")!,
      (account, accessible) => {
        let repository = repositories().find(
          (candidate) =>
            candidate.provider === "github" &&
            candidate.provider_account_id === account.account_id &&
            candidate.provider_repository_id === accessible.id,
        );
        repository ??= repositories().find(
          (candidate) =>
            candidate.provider === "github" &&
            candidate.name === accessible.name &&
            !candidate.provider_account_id &&
            !candidate.provider_repository_id,
        );
        if (repository) {
          repository.name = accessible.name;
          repository.enabled = true;
          repository.provider_account_id = account.account_id;
          repository.provider_repository_id = accessible.id;
        } else {
          repository = {
            id: newIdentity(),
            name: accessible.name,
            enabled: true,
            provider: "github",
            provider_account_id: account.account_id,
            provider_repository_id: accessible.id,
          };
          (draft.repositories ??= []).push(repository);
        }
        changed();
        rows();
      },
      (accounts) => {
        githubAccounts = accounts;
        if (content.querySelector(".repository-list")) rows();
      },
    );
    content.querySelector<HTMLButtonElement>("#choose-folder")!.onclick =
      () => {
        if (!busy) void scan(true);
      };
    content
      .querySelector<HTMLButtonElement>("#rescan")
      ?.addEventListener("click", () => {
        if (!busy) void scan(false);
      });
    content.querySelector<HTMLInputElement>("#repo-search")!.oninput = (
      event,
    ) => {
      query = (event.target as HTMLInputElement).value;
      rows();
    };
    content.querySelector<HTMLButtonElement>("#add-repository")!.onclick = () =>
      editRepository();
    content.querySelector<HTMLButtonElement>("#select-visible")!.onclick =
      () => {
        for (const item of visible()) if (item.name) select(item, true);
        rows();
      };
    function all() {
      type RepositoryRow = Discovered & {
        paths: string[];
        repositoryId?: string;
      };
      const list: RepositoryRow[] = [];
      const identities = new Map<string, Discovered & { paths: string[] }>();
      for (const item of discovery?.repositories ?? []) {
        const existing = item.name ? identities.get(item.name) : undefined;
        if (existing) existing.paths.push(item.path);
        else {
          const row = { ...item, paths: [item.path] };
          list.push(row);
          if (item.name) identities.set(item.name, row);
        }
      }
      const configuredNames = new Set(repositories().map((repo) => repo.name));
      const unconfigured = list.filter(
        (item) => !item.name || !configuredNames.has(item.name),
      );
      for (const repo of repositories()) {
        const discovered = identities.get(repo.name);
        unconfigured.push({
          name: repo.name,
          path: discovered?.path ?? "",
          paths: discovered?.paths ?? [],
          unavailable: discovered?.unavailable ?? null,
          repositoryId: repo.id,
        });
      }
      return unconfigured;
    }
    function visible() {
      return all().filter((item) =>
        `${item.name ?? ""} ${item.paths.join(" ")}`
          .toLowerCase()
          .includes(query.toLowerCase()),
      );
    }
    function select(
      item: Discovered & { repositoryId?: string },
      enabled: boolean,
    ) {
      const existing = item.repositoryId
        ? repositories().find((r) => r.id === item.repositoryId)
        : repositories().find(
            (r) => r.name === item.name && !r.provider_account_id,
          );
      if (existing) existing.enabled = enabled;
      else if (enabled && item.name)
        (draft.repositories ??= []).push({
          id: newIdentity(),
          name: item.name,
          enabled,
          provider: "github",
        });
      changed();
    }
    function rows() {
      const list = content.querySelector<HTMLElement>(".repository-list")!;
      list.replaceChildren();
      content.querySelector("#selected-count")!.textContent =
        `${repositories().filter((r) => r.enabled).length} selected`;
      if (!visible().length) {
        list.innerHTML = `<div class="settings-empty"><strong>${query ? "No matching repositories" : "No repositories yet"}</strong><p>${query ? "Try a repository or organization name." : "Choose a local folder or add a GitHub repository manually."}</p></div>`;
      }
      for (const item of visible()) {
        const repository = item.repositoryId
          ? repositories().find((r) => r.id === item.repositoryId)
          : repositories().find(
              (r) => r.name === item.name && !r.provider_account_id,
            );
        const actingAccount = githubAccounts.find(
          (account) => account.account_id === repository?.provider_account_id,
        );
        const providerLabel =
          repository?.provider === "azure_devops"
            ? "Azure DevOps"
            : repository?.provider_account_id
              ? actingAccount?.state === "connected"
                ? `GitHub as ${actingAccount.login}`
                : `GitHub as ${actingAccount?.login ?? repository.provider_account_id} - Needs attention`
              : "GitHub - account required";
        const row = document.createElement("article");
        row.className = "repository-row";
        const duplicateBinding =
          !!repository &&
          repositories().filter((candidate) => candidate.name === item.name)
            .length > 1;
        row.setAttribute(
          "aria-label",
          duplicateBinding
            ? `${item.name} as ${actingAccount?.login ?? repository.provider_account_id ?? repository.id}`
            : (item.name ??
                item.path.split("/").pop() ??
                "Unavailable repository"),
        );
        const assignmentCount = repository?.assignments?.length ?? 0;
        row.innerHTML = `<input type="checkbox" aria-label="Monitor ${escape(item.name ?? item.path.split("/").pop() ?? "repository")}" ${repository?.enabled ? "checked" : ""} ${!item.name ? "disabled" : ""} />
          <span class="repo-symbol">${icon("integrations")}</span><div class="repository-info"><strong>${escape(item.name?.split("/")[1] ?? item.path.split("/").pop() ?? "")}</strong><p>${escape(item.name ?? item.unavailable ?? "Unavailable")}</p><p>${escape(providerLabel)}</p>${item.paths.length ? `<details class="clone-paths"><summary>${item.paths.length} local ${item.paths.length === 1 ? "clone" : "clones"}</summary><ul>${item.paths.map((path) => `<li>${escape(path)}</li>`).join("")}</ul></details>` : ""}</div>
          ${item.name ? `<span class="repository-note">${assignmentCount ? `${assignmentCount} agent${assignmentCount === 1 ? "" : "s"} assigned` : "No agents assigned"}</span><button class="configure">Settings</button>` : ""}`;
        row.querySelector<HTMLInputElement>("input")!.onchange = (event) => {
          select(item, (event.target as HTMLInputElement).checked);
          content.querySelector("#selected-count")!.textContent =
            `${repositories().filter((r) => r.enabled).length} selected`;
        };
        row
          .querySelector<HTMLButtonElement>(".configure")
          ?.addEventListener("click", () => {
            let repo = item.repositoryId
              ? repositories().find((r) => r.id === item.repositoryId)
              : repositories().find(
                  (r) => r.name === item.name && !r.provider_account_id,
                );
            if (!repo) {
              repo = {
                id: newIdentity(),
                name: item.name!,
                enabled: false,
                provider: "github",
              };
              (draft.repositories ??= []).push(repo);
              changed();
            }
            repositoryDialog(repo);
          });
        list.append(row);
      }
    }
    rows();
  }

  function editRepository(repository?: ConfiguredRepository) {
    const selectedAccount =
      repository?.provider_account_id ?? githubAccounts[0]?.account_id ?? "";
    const modal = dialog(
      repository ? "Edit repository" : "Add repository",
      `<form><label>GitHub repository<input name="repository" required value="${escape(repository?.name ?? "")}" placeholder="owner/repository or https://github.com/owner/repository" /></label>${githubAccounts.length ? `<label>Acting GitHub account<select name="account" required>${githubAccounts.map((account) => option(account.account_id, `${account.login} (${account.account_id})`, selectedAccount)).join("")}</select></label>` : ""}<p class="settings-hint">${githubAccounts.length ? "PR Sniper validates this repository with the selected account before binding its stable identity." : "Connect a GitHub account to validate and bind this repository. Until then it remains explicitly unbound."} Adding is a draft until you save changes. It does not start reviews.</p><p role="alert" hidden></p><button class="primary">Use repository</button></form>`,
    );
    let submitting = false;
    const originDraft = draft;
    modal.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      if (submitting) return;
      submitting = true;
      const requestRevision = revision;
      const active = () =>
        modal.open && modal.isConnected && draft === originDraft;
      const current = () =>
        active() &&
        revision === requestRevision &&
        (!repository || repositories().includes(repository));
      const input = modal.querySelector<HTMLInputElement>("input")!;
      const button = modal.querySelector<HTMLButtonElement>("form button")!;
      button.disabled = true;
      try {
        const name = await invoke<string>("canonical_repository_name", {
          repository: input.value,
        });
        const accountId =
          modal.querySelector<HTMLSelectElement>("[name=account]")?.value;
        const resolved = accountId
          ? await invoke<{
              identity: { id: string; login: string };
              repository: { id: string; name: string };
            }>("resolve_provider_repository", {
              provider: "github",
              accountId,
              repository: name,
            })
          : undefined;
        if (!current()) return;
        if (resolved?.identity.id !== accountId)
          throw "GitHub returned an unexpected account identity.";
        const canonical = resolved?.repository.name ?? name;
        if (
          repositories().some(
            (r) =>
              r.name === canonical &&
              r.id !== repository?.id &&
              r.provider_account_id === accountId,
          )
        )
          throw "This GitHub repository is already configured.";
        if (repository) {
          repository.name = canonical;
          repository.provider_account_id = resolved?.identity.id;
          repository.provider_repository_id = resolved?.repository.id;
        } else
          (draft.repositories ??= []).push({
            id: newIdentity(),
            name: canonical,
            provider: "github",
            enabled: true,
            provider_account_id: resolved?.identity.id,
            provider_repository_id: resolved?.repository.id,
          });
        modal.close();
        render();
      } catch (cause) {
        if (!current()) return;
        const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
        alert.textContent = reason(cause);
        alert.hidden = false;
      } finally {
        submitting = false;
        if (active()) button.disabled = false;
      }
    };
  }

  function repositoryDialog(repository: ConfiguredRepository) {
    const modal = dialog(
      `Settings for ${repository.name}`,
      `<p class="settings-hint">${escape(repository.provider === "github" ? `GitHub acting account: ${githubAccounts.find((account) => account.account_id === repository.provider_account_id)?.login ?? repository.provider_account_id ?? "not selected"}.` : "Azure DevOps account binding is not available in this build.")}</p>
        <div class="section-actions"><h2>Agents on this repository</h2><button class="primary" data-assign-agent ${agents().length ? "" : "disabled"}>Assign agent</button></div>
        <div class="assignment-list"></div>
        ${agents().length ? "" : '<p class="settings-hint">Create an agent first, on the Agents tab.</p>'}
        <div class="section-actions"><h2>People you watch</h2><button data-add-people>Add people</button></div>
        <div class="watchlist"></div>
        <p class="settings-hint">Optional. Pull requests from anyone are eligible unless this list is non-empty, in which case only these people qualify. Exact GitHub login, no wildcards.</p>
        <details><summary>Repository and connection</summary><div class="settings-actions"><button id="rename-repository">Edit repository</button><button id="remove-repository">Remove repository</button></div><div class="connection"></div></details>`,
    );
    renderAssignments();
    renderWatchlist();
    if (
      saved.repositories?.some(
        (r) => r.id === repository.id && r.name === repository.name,
      )
    )
      renderConnection(modal.querySelector(".connection")!, repository, {
        available:
          githubAccounts.find(
            (account) => account.account_id === repository.provider_account_id,
          )?.state === "connected",
        label:
          githubAccounts.find(
            (account) => account.account_id === repository.provider_account_id,
          )?.login ?? repository.provider_account_id,
      });
    else
      modal.querySelector(".connection")!.textContent =
        "Save this repository before verifying its GitHub connection.";
    modal.querySelector<HTMLButtonElement>("[data-assign-agent]")!.onclick =
      () =>
        assignAgentDialog(repository, () => {
          renderAssignments();
        });
    modal.querySelector<HTMLButtonElement>("[data-add-people]")!.onclick = () =>
      addPersonDialog(repository, () => renderWatchlist());
    modal.querySelector<HTMLButtonElement>("#rename-repository")!.onclick =
      () => {
        modal.close();
        editRepository(repository);
      };
    modal.querySelector<HTMLButtonElement>("#remove-repository")!.onclick =
      () => {
        const confirm = dialog(
          "Remove repository?",
          `<p>Remove ${escape(repository.name)} and its assignments from your draft? Nothing is removed until Save changes.</p><button class="primary" id="confirm-remove">Remove from settings</button>`,
        );
        confirm.querySelector<HTMLButtonElement>("#confirm-remove")!.onclick =
          () => {
            draft.repositories = repositories().filter(
              (r) => r.id !== repository.id,
            );
            confirm.close();
            modal.close();
            render();
          };
      };

    function renderAssignments() {
      const list = modal.querySelector<HTMLElement>(".assignment-list")!;
      const assignments = repository.assignments ?? [];
      if (!assignments.length) {
        list.innerHTML =
          '<p class="settings-empty">No agents assigned. This repository is watched but nothing reviews it yet.</p>';
        return;
      }
      list.innerHTML = "";
      for (const assignment of assignments) {
        const agent = agents().find((a) => a.id === assignment.agent_id);
        const row = document.createElement("div");
        row.className = "assignment-row";
        row.innerHTML = `<div><strong>${escape(agent?.name ?? "Deleted agent")}</strong><p>${escape(scheduleSummary(assignment.schedule))} \u00b7 ${assignment.comment ? "Comments" : "Silent"}${assignment.approve ? " \u00b7 Approve (coming soon)" : ""}</p></div><button data-edit>Edit</button><button data-remove>Remove</button>`;
        row.querySelector<HTMLButtonElement>("[data-edit]")!.onclick = () =>
          assignAgentDialog(repository, () => renderAssignments(), assignment);
        row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick = () => {
          repository.assignments = assignments.filter((a) => a !== assignment);
          changed();
          renderAssignments();
        };
        list.append(row);
      }
    }
    function renderWatchlist() {
      const list = modal.querySelector<HTMLElement>(".watchlist")!;
      const people = repository.watched_authors ?? [];
      if (!people.length) {
        list.innerHTML =
          '<p class="settings-empty">No one added. Every pull request is eligible.</p>';
        return;
      }
      list.innerHTML = "";
      for (const person of people) {
        const row = document.createElement("div");
        row.className = "watchlist-row";
        row.innerHTML = `<span>@${escape(person.login)}</span><button data-remove aria-label="Remove ${escape(person.login)}">Remove</button>`;
        row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick = () => {
          repository.watched_authors = people.filter((p) => p !== person);
          changed();
          renderWatchlist();
        };
        list.append(row);
      }
    }
  }

  function assignAgentDialog(
    repository: ConfiguredRepository,
    onSaved: () => void,
    existing?: Assignment,
  ) {
    const schedule = existing?.schedule ?? {
      kind: "interval" as const,
      minutes: 15,
      timezone: localZone(),
    };
    const modal = dialog(
      existing ? "Edit assignment" : "Assign agent",
      `<form><label>Agent<select name="agent" required>${agents()
        .map((a) =>
          option(a.id, a.name, existing?.agent_id ?? agents()[0]?.id ?? ""),
        )
        .join("")}</select></label>
        <label>Check for pull requests<select name="frequency">${[
          5,
          15,
          30,
          60,
          ...(schedule.kind === "interval" ? [schedule.minutes] : []),
        ]
          .filter((n, i, a) => a.indexOf(n) === i)
          .map((n) =>
            option(
              String(n),
              `Every ${n} minutes`,
              schedule.kind === "interval" ? String(schedule.minutes) : "cron",
            ),
          )
          .join(
            "",
          )}${option("cron", "Custom schedule (cron)", schedule.kind === "cron" ? "cron" : "")}</select></label>
        <details ${schedule.kind === "cron" ? "open" : ""}><summary>Advanced scheduling</summary><label>Interval minutes<input name="minutes" type="number" min="1" step="1" value="${schedule.kind === "interval" ? schedule.minutes : 15}" /></label><label>Cron expression<input name="cron" value="${escape(schedule.kind === "cron" ? schedule.expression : "0 9 * * MON-FRI")}" /></label><label>Time zone<input name="timezone" value="${escape(schedule.timezone)}" /></label></details>
        <div class="permission-row"><label><input type="checkbox" name="comment" ${(existing?.comment ?? true) ? "checked" : ""} />Comment<small>Post findings on the pull request.</small></label><label><input type="checkbox" name="approve" disabled ${existing?.approve ? "checked" : ""} />Approve<small>Coming soon -- once we trust the aim.</small></label></div>
        <p class="settings-hint">Each assignment runs on its own timer, independent of any other agent on this repository.</p><p role="alert" hidden></p><button class="primary">${existing ? "Save assignment" : "Assign agent"}</button></form>`,
    );
    const frequency =
      modal.querySelector<HTMLSelectElement>("[name=frequency]")!;
    const minutes = modal.querySelector<HTMLInputElement>("[name=minutes]")!;
    const cron = modal.querySelector<HTMLInputElement>("[name=cron]")!;
    minutes.disabled = schedule.kind === "cron";
    cron.disabled = schedule.kind !== "cron";
    frequency.onchange = () => {
      if (frequency.value !== "cron") minutes.value = frequency.value;
      else modal.querySelector("details")!.open = true;
      minutes.disabled = frequency.value === "cron";
      cron.disabled = frequency.value !== "cron";
    };
    modal.querySelector("form")!.onsubmit = (event) => {
      event.preventDefault();
      const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
      try {
        const agentId =
          modal.querySelector<HTMLSelectElement>("[name=agent]")!.value;
        if (!agentId) throw "Choose an agent.";
        const value: Assignment = {
          id: existing?.id ?? newIdentity(),
          agent_id: agentId,
          schedule:
            frequency.value === "cron"
              ? {
                  kind: "cron",
                  expression: cron.value,
                  timezone:
                    modal.querySelector<HTMLInputElement>("[name=timezone]")!
                      .value,
                }
              : {
                  kind: "interval",
                  minutes: Number(minutes.value),
                  timezone:
                    modal.querySelector<HTMLInputElement>("[name=timezone]")!
                      .value,
                },
          comment:
            modal.querySelector<HTMLInputElement>("[name=comment]")!.checked,
          approve: false,
        };
        if (existing) Object.assign(existing, value);
        else (repository.assignments ??= []).push(value);
        changed();
        modal.close();
        onSaved();
      } catch (cause) {
        alert.textContent = typeof cause === "string" ? cause : reason(cause);
        alert.hidden = false;
      }
    };
  }

  function addPersonDialog(
    repository: ConfiguredRepository,
    onSaved: () => void,
  ) {
    const availableAccounts = githubAccounts.filter(
      (account) =>
        account.state === "connected" &&
        (!repository.provider_account_id ||
          account.account_id === repository.provider_account_id),
    );
    const people = repository.watched_authors ?? [];
    const picker = dialog(
      "Add people",
      `<form class="person-lookup"><label>Acting GitHub account<select name="account" required>${availableAccounts.map((account) => option(account.account_id, `${account.login} (${account.account_id})`, repository.provider_account_id ?? availableAccounts[0]?.account_id ?? "")).join("")}</select></label><label>GitHub login<input name="login" placeholder="octocat" autocomplete="off" required /></label><p class="settings-hint">${availableAccounts.length ? "Looks up the exact login through the selected GitHub account and stores its stable identity. No wildcards." : "No connected GitHub account is available. Connect one in Integrations."}</p><p role="alert" hidden></p><button type="submit" class="primary" ${availableAccounts.length ? "" : "disabled"}>Add person</button></form>`,
    );
    picker.querySelector<HTMLInputElement>("[name=login]")!.focus();
    picker.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      const input = picker.querySelector<HTMLInputElement>("[name=login]")!;
      const button = picker.querySelector<HTMLButtonElement>("form button")!;
      const alert = picker.querySelector<HTMLElement>("[role=alert]")!;
      button.disabled = true;
      alert.hidden = true;
      const lookupRevision = revision;
      try {
        const identity = await invoke<WatchedIdentity>(
          "resolve_provider_person",
          {
            provider: "github",
            accountId:
              picker.querySelector<HTMLSelectElement>("[name=account]")!.value,
            login: input.value.trim().replace(/^@/, ""),
          },
        );
        if (!picker.open || lookupRevision !== revision)
          throw "Settings changed during lookup. Add this person again.";
        if (people.some((p) => p.id === identity.id))
          throw "This person is already in this watchlist.";
        repository.watched_authors = [...people, identity];
        changed();
        picker.close();
        onSaved();
      } catch (cause) {
        alert.textContent = reason(cause);
        alert.hidden = false;
        if (
          typeof cause === "string" &&
          [
            "signed_out",
            "wrong_identity",
            "network",
            "timeout",
            "provider_failure",
            "invalid_response",
            "configuration",
          ].includes(cause)
        )
          window.dispatchEvent(
            new Event("pr-sniper:refresh-provider-accounts"),
          );
      } finally {
        button.disabled = false;
      }
    };
  }

  // ------------------------------------------------------------- Preferences

  function renderPreferences() {
    content.innerHTML = `<div class="settings-group"><fieldset aria-label="Startup"><legend>Startup</legend><label class="setting-row"><span>Open PR Sniper at login<small>${snapshot.isolated ? "Isolated development run: changing macOS login items is disabled." : `Saved request, not effective macOS state. Registration: ${snapshot.login_registration ?? "unavailable"}.`}</small></span><input id="login" type="checkbox" role="switch" ${draft.launch_at_login ? "checked" : ""} ${snapshot.isolated || snapshot.login_registration === null ? "disabled" : ""} /></label></fieldset></div>
      <div class="settings-group"><fieldset aria-label="Notifications"><legend>Notifications</legend><label class="setting-row"><span>Notify me when a review finishes<small>Coming soon.</small></span><input type="checkbox" role="switch" disabled /></label></fieldset></div>
      <div class="settings-group"><fieldset aria-label="Diagnostics"><legend>Diagnostics</legend><p class="settings-hint">Settings and logs live in your macOS app-support folder. Open a redacted diagnostics view to check in on them without exposing tokens.</p><button id="diagnostics">Open redacted diagnostics</button></fieldset></div>`;
    content.querySelector<HTMLInputElement>("#login")!.onchange = async (
      event,
    ) => {
      const checkbox = event.target as HTMLInputElement;
      checkbox.disabled = true;
      try {
        await invoke("save_login", { enabled: checkbox.checked });
      } catch {
        showError(
          "Startup preference change could not complete. Check macOS Login Items and the saved request.",
        );
      } finally {
        try {
          const fresh = await invoke<Snapshot>("snapshot");
          if (fresh.settings) {
            draft.launch_at_login = fresh.settings.launch_at_login;
            saved.launch_at_login = fresh.settings.launch_at_login;
            persisted.launch_at_login = fresh.settings.launch_at_login;
          }
          snapshot = fresh;
        } catch {
          showError(
            "Could not reload startup registration. Check macOS Login Items.",
          );
        }
        render();
      }
    };
    content.querySelector<HTMLButtonElement>("#diagnostics")!.onclick =
      async () => {
        try {
          await invoke("open_diagnostics");
        } catch {
          showError("Could not open diagnostics.");
        }
      };
  }

  save.onclick = async () => {
    if (busy || !dirty()) return;
    dialogs.closeAll();
    clearError();
    busy = true;
    changed();
    const controls = [
      ...app.querySelectorAll<
        | HTMLInputElement
        | HTMLButtonElement
        | HTMLSelectElement
        | HTMLTextAreaElement
      >("input,button,select,textarea"),
    ].map((control) => ({ control, disabled: control.disabled }));
    controls.forEach(({ control }) => {
      control.disabled = true;
    });
    try {
      const result = await invoke<{
        settings: Settings;
        warning: string | null;
      }>("save_preferences", { settings: draft, expected: persisted });
      persisted = clone(result.settings);
      saved = clone(result.settings);
      draft = clone(result.settings);
      conflict = false;
      if (result.warning) showError(result.warning);
    } catch (cause) {
      const message = reason(cause);
      if (message.startsWith("Settings changed in another window")) {
        conflict = true;
        showError(
          `${message} Your unsaved changes are still here. Discard the draft and reload only when you are ready to replace them with the latest saved settings.`,
        );
      } else {
        showError(message);
      }
    } finally {
      controls.forEach(({ control, disabled }) => {
        control.disabled = disabled;
      });
      busy = false;
      render();
    }
  };
  reload.onclick = async () => {
    if (busy || !conflict) return;
    dialogs.closeAll();
    clearError();
    busy = true;
    changed();
    const controls = [
      ...app.querySelectorAll<
        | HTMLInputElement
        | HTMLButtonElement
        | HTMLSelectElement
        | HTMLTextAreaElement
      >("input,button,select,textarea"),
    ].map((control) => ({ control, disabled: control.disabled }));
    controls.forEach(({ control }) => {
      control.disabled = true;
    });
    try {
      const state = await invoke<Snapshot>("snapshot");
      if (!state.settings) {
        showError(
          state.error ??
            "Settings unavailable. Repair local configuration before reloading.",
        );
        return;
      }
      snapshot = state;
      persisted = clone(state.settings);
      saved = clone(state.settings);
      draft = clone(saved);
      discovery = null;
      conflict = false;
      if (state.error) showError(state.error);
    } catch {
      showError(
        "Could not reload Settings. Your draft is still available; check local storage access and try again.",
      );
    } finally {
      controls.forEach(({ control, disabled }) => {
        control.disabled = disabled;
      });
      busy = false;
      render();
    }
  };
  reset.onclick = () => {
    if (!busy) {
      draft = clone(saved);
      discovery = null;
      clearError();
      render();
    }
  };
  async function load() {
    const requestRevision = revision;
    try {
      const state = await invoke<Snapshot>("snapshot");
      if (requestRevision !== revision || dirty() || busy || dialogs.hasOpen())
        return;
      snapshot = state;
      if (!state.settings) {
        showError(
          state.error ??
            "Settings unavailable. Repair local configuration before saving.",
        );
        return;
      }
      persisted = clone(state.settings);
      saved = clone(state.settings);
      draft = clone(saved);
      conflict = false;
      if (state.error) showError(state.error);
      render();
    } catch {
      if (requestRevision !== revision || dirty() || busy || dialogs.hasOpen())
        return;
      showError(
        "Could not read Settings. Open the native application and check local storage access.",
      );
    }
  }
  window.addEventListener("focus", () => {
    refreshAgentAccounts?.();
    if (!dirty() && !busy && !dialogs.hasOpen()) void load();
  });
  await load();
}
