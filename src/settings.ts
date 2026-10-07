import { invoke } from "@tauri-apps/api/core";
import {
  describeReadFailureDetails,
  readFailureMissingScope,
  renderConnection,
} from "./connections";
import {
  renderGithubAuth,
  githubAccountFailureMessage,
  type GithubAccount,
} from "./github-auth";
import {
  renderCopilotAuth,
  modelSelectable,
  reasoningEfforts,
  contextTiers,
  contextCapacity,
  runtimeContextTier,
  intelligenceError,
  type CopilotAccount,
  type CopilotAuth,
  type CopilotModel,
} from "./copilot";
import type { Agent, Doctrine, Assignment, WatchedIdentity } from "./policy";
import {
  type Repository,
  primaryAssignmentId,
  repositoryProviderContext,
} from "./repositories";
import { doctrineTitles } from "./policy";
import {
  type Settings,
  type DoctrineCatalog,
  doctrineCatalogLabel,
  type ResourceEdit,
  acceptResource,
  globalPreferences,
  savedResources,
  saveResource,
  sameResource,
} from "./resources";
import "./settings.css";
import { createDialogs } from "./dialogs";
import { mountNotificationSettings } from "./notifications";
import { mountAutomation, type AutomationSnapshot } from "./automation";

interface ConfiguredRepository extends Repository {
  watched_authors?: WatchedIdentity[];
  assignments?: Assignment[];
}
interface ResolvedRepository {
  identity: { id: string };
  repository: { id: string; name: string };
  account_generation: number;
  pull_request?: { id: string; number: number; title: string };
}
interface RepositoryBrowseWarning {
  boundary:
    | "repository_page"
    | "repository_metadata"
    | "pagination"
    | "organization_access";
  page: number;
  error: string | Record<string, number>;
}
interface Snapshot {
  settings: Settings | null;
  doctrine_catalog?: DoctrineCatalog | null;
  settings_persisted: boolean;
  isolated: boolean;
  login_registration: "absent" | "registered" | "invalid" | null;
  error: string | null;
}
type Section =
  | "home"
  | "accounts"
  | "ai-tooling"
  | "git-repository"
  | "copilot"
  | "github"
  | "repositories"
  | "capacity"
  | "doctrines"
  | "agents"
  | "preferences";

export type SetupTarget =
  | "ai"
  | "repository-account"
  | "agents"
  | "doctrines"
  | "repositories"
  | "preferences";
export interface GuidedReturn {
  back: () => void;
}

// Direct integrations, distinct from the models available through Copilot.
const modelProviders: { id: string; label: string; available: boolean }[] = [
  { id: "copilot", label: "GitHub Copilot", available: true },
  { id: "claude", label: "Claude", available: false },
  { id: "codex", label: "Codex", available: false },
  { id: "grok", label: "Grok", available: false },
];

const sections: Record<Section, [string, string]> = {
  home: ["Settings", ""],
  accounts: ["Accounts", "GitHub and Copilot, kept separate."],
  "ai-tooling": ["AI Tooling", "Subscriptions used by your review agents."],
  "git-repository": [
    "Git Repository",
    "Repository access and publication identities.",
  ],
  copilot: ["GitHub Copilot", "AI access, separate from repository identity."],
  github: ["GitHub", "Repository access and publication identity."],
  repositories: ["Repositories", "Acting accounts and review assignments."],
  capacity: [
    "Concurrent reviews",
    "A limit on running work, not saved agents.",
  ],
  doctrines: [
    "Doctrines",
    "Principles your agents review by. Plain text, yours to edit.",
  ],
  agents: [
    "Agents",
    "A model, shared doctrines, a prompt and a signature, ready to assign.",
  ],
  preferences: ["Preferences", "Schedule, AI capacity and native controls."],
};
// Bespoke reticle/scope marks, not a generic icon-kit -- each nods at the
// tab's job rather than a stock glyph.
const iconPaths = {
  back: '<path d="m14 6-6 6 6 6M8 12h12"/>',
  home: '<path d="M4 6h16M4 12h16M4 18h16"/><circle cx="8" cy="6" r="2"/><circle cx="16" cy="12" r="2"/><circle cx="10" cy="18" r="2"/>',
  accounts:
    '<circle cx="9" cy="7" r="3"/><path d="M3 21v-3a6 6 0 0 1 12 0v3M16 4a3 3 0 0 1 0 6m2 4a6 6 0 0 1 3 4v3"/>',
  repositories: '<path d="M3 6h7l2 2h9v12H3zM3 6V4h7l2 2"/>',
  capacity: '<path d="M2 12h5l3-9 4 18 3-9h5"/>',
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
const option = (value: string, label: string, selected: string) =>
  `<option value="${escape(value)}" ${value === selected ? "selected" : ""}>${escape(label)}</option>`;
const words = (text: string) =>
  text.trim() ? text.trim().split(/\s+/).length : 0;
function repositorySessionFailure(cause: unknown) {
  if (
    cause &&
    typeof cause === "object" &&
    "stage" in cause &&
    cause.stage === "session" &&
    "account_id" in cause &&
    typeof cause.account_id === "string" &&
    "error" in cause
  ) {
    return { accountId: cause.account_id, error: cause.error };
  }
}

const reason = (error: unknown): string => {
  const session = repositorySessionFailure(error);
  if (session) {
    return session.error === "configuration" || session.error === "broken_cli"
      ? "The selected GitHub session is unavailable. Check secure credential access or reconnect this account, then retry."
      : reason(session.error);
  }
  const details = describeReadFailureDetails(error);
  if (details) return details;
  const errors: Record<string, string> = {
    invalid_repository:
      "Enter owner/repository, an HTTPS github.com repository URL, or a pull request URL with a positive PR number. Other providers and embedded credentials are not supported.",
    revision_changed:
      "The pull request revision changed during this lookup. Retry to verify its current revision.",
    signed_out:
      "GitHub is disconnected. Connect the PR Sniper GitHub OAuth App, then try again.",
    missing_read_permission:
      "GitHub denied read access for this account. Verify repository access and this app's organization authorization with your administrator, then retry.",
    repository_unavailable:
      "GitHub could not find this repository or hides it from the selected account. Check the URL and this account's repository access; for a private organization, check the PR Sniper OAuth App's organization approval and single sign-on authorization with its administrator, then retry. A 404 does not prove the repository is absent.",
    rate_limited: "GitHub rate limited this lookup. Wait before trying again.",
    network: "Cannot reach GitHub. Check your network and try again.",
    timeout: "GitHub lookup timed out. Try again.",
    invalid_response:
      "GitHub returned malformed or unsupported data. Retry; if it persists, report this lookup failure.",
    incomplete_read:
      "GitHub did not return a complete repository list. More repositories may be unavailable; retry.",
    missing_scope:
      "The selected GitHub authorization does not grant the required repo scope. Reconnect this account in Accounts and review the PR Sniper OAuth App's public/private repository access request, then retry.",
    scope_unverified:
      "GitHub did not provide the scope evidence needed to verify this read. No missing scope or grant is established. Retry this selected account; if it persists, check this app's authorization in GitHub. No repository was accepted from this unverified read.",
    organization_policy_denied:
      "GitHub reported an organization authorization restriction. Check the PR Sniper OAuth App's organization approval and single sign-on authorization in GitHub; if restricted, ask the organization administrator for access, then retry. Reconnecting alone cannot bypass organization policy.",
    organization_policy_denied_with_missing_scope:
      "GitHub reported an organization authorization restriction and an authorization missing repo scope. Ask the organization administrator to approve the PR Sniper OAuth App; also reconnect this selected account in Accounts with repository access. Reconnecting alone cannot bypass organization policy.",
    repository_changed:
      "The repository identity changed. Refresh the owner or check the URL, then retry.",
    authentication_changed:
      "The GitHub connection changed during this request. Retry with the current connected account.",
    provider_failure:
      "GitHub lookup failed. Check provider health and try again.",
    provider_rejected:
      "GitHub rejected this lookup. Check the current repository URL/input and provider policy before retrying; this does not establish a missing scope.",
    wrong_identity:
      "GitHub returned a different account. Reconnect the selected account; no other account will be used.",
    configuration:
      "Repository lookup configuration is unavailable. Check local/provider setup and retry.",
  };
  if (error && typeof error === "object") {
    if (
      "rate_limited_after" in error &&
      typeof error.rate_limited_after === "number" &&
      error.rate_limited_after >= 0
    )
      return `${errors.rate_limited} Retry after ${error.rate_limited_after} seconds.`;
    if (
      "provider_failure_after" in error &&
      typeof error.provider_failure_after === "number" &&
      error.provider_failure_after >= 0
    )
      return `${errors.provider_failure} Retry after ${error.provider_failure_after} seconds.`;
  }
  return typeof error === "string"
    ? (errors[error] ?? error)
    : "This action failed. Check local access and try again.";
};

export async function mountSettings(
  app: HTMLElement,
  options: {
    embedded?: boolean;
    openGenie?: (opener: HTMLElement) => void;
  } = {},
) {
  const accountSection = "Accounts";
  if (options.embedded) app.className = "settings-window settings-page";
  else {
    document.body.classList.add("settings-page");
    app.className = "settings-window";
  }
  const legacySections = Object.entries(sections).filter(([key]) =>
    ["accounts", "repositories", "agents", "doctrines", "preferences"].includes(
      key,
    ),
  );
  app.innerHTML = `${
    options.embedded
      ? ""
      : `<aside class="settings-sidebar"><div class="settings-brand"><svg viewBox="0 0 32 32" fill="none" aria-hidden="true"><circle cx="16" cy="16" r="9" stroke="currentColor" stroke-width="1.7"/><path d="M16 2v8m0 12v8M2 16h8m12 0h8" stroke="currentColor" stroke-width="1.7"/><circle cx="16" cy="16" r="2.5" fill="currentColor"/></svg>PR Sniper</div><p class="settings-caption">Preferences</p>
    <nav aria-label="Settings sections">${legacySections
      .map(
        ([key, [title]]) =>
          `<button type="button" data-section="${key}">${icon(key as keyof typeof iconPaths)}${title}</button>`,
      )
      .join("")}</nav>
    <label class="mobile-section">Section<select aria-label="Settings section">${legacySections
      .map(([key, [title]]) => option(key, title, "accounts"))
      .join("")}</select></label></aside>`
  }
    <div class="settings-main"><div class="settings-genie-entry"><button type="button" data-open-genie>Set up with Genie</button><button type="button" data-return-genie hidden>Back to Genie</button><p data-genie-save-note hidden>Each save is applied immediately. Close hides this editor; Back and Cancel follow the unsaved-field guidance below. Repository Save is authorization; no further scope confirmation is needed.</p></div><header class="settings-heading"><h1 tabindex="-1">Settings</h1><p>Sign in to an AI subscription, then connect the repositories it should watch.</p></header>
    <p id="error" role="alert" hidden></p><div data-catalog-conflict hidden><p role="alert">Resource saved. Agents changed in another window. The previous doctrine catalog and Agent selections are retained until you reload. Close open editors first; unrelated preference and repository drafts stay intact.</p><button type="button" data-reload-catalog>Reload Agents and doctrines</button></div><section id="content"></section>
    <footer class="settings-savebar"><span role="status" id="save-status">Loading settings...</span><button id="reload-settings" hidden>Discard draft and reload</button><button id="reset-settings" disabled>Reset changes</button><button class="primary" id="save-settings" disabled>Save preferences</button></footer></div>`;
  const content = app.querySelector<HTMLElement>("#content")!;
  const error = app.querySelector<HTMLElement>("#error")!;
  const status = app.querySelector<HTMLElement>("#save-status")!;
  const save = app.querySelector<HTMLButtonElement>("#save-settings")!;
  const reset = app.querySelector<HTMLButtonElement>("#reset-settings")!;
  const reload = app.querySelector<HTMLButtonElement>("#reload-settings")!;
  const catalogNotice = app.querySelector<HTMLElement>(
    "[data-catalog-conflict]",
  )!;
  const reloadCatalog = app.querySelector<HTMLButtonElement>(
    "[data-reload-catalog]",
  )!;
  const accountParking = document.createElement("div");
  accountParking.hidden = true;
  // Keep native sign-in widgets mounted off-page without their layout selectors.
  app.after(accountParking);
  let accountContent: HTMLElement | undefined;
  let refreshRepositoryRows: (() => void) | undefined;
  const settingsBack = document.createElement("button");
  settingsBack.type = "button";
  settingsBack.className = "settings-home-back";
  settingsBack.setAttribute("aria-label", "Back to Settings");
  settingsBack.title = "Back to Settings; retain unsaved changes";
  settingsBack.innerHTML = icon("back");
  settingsBack.hidden = true;
  app.querySelector(".settings-heading")!.prepend(settingsBack);
  let snapshot: Snapshot;
  let saved: Settings;
  let draft: Settings;
  let section: Section = options.embedded ? "home" : "accounts";
  let homeScroll = 0;
  let homeOpener: Section | undefined;
  const accountScroll = new Map<Section, number>();
  const accountParents: Partial<Record<Section, Section>> = {
    accounts: "home",
    "ai-tooling": "accounts",
    "git-repository": "accounts",
    copilot: "ai-tooling",
    github: "git-repository",
  };
  let accountRead = 0;
  let busy = false;
  let startupPending = false;
  let startupError = "";
  let conflict = false;
  let catalogConflict = false;
  let revision = 0;
  let routeGeneration = 0;
  app
    .closest("[data-panel-view]")
    ?.addEventListener("pr-sniper:section-active", (event) => {
      if (!(event as CustomEvent<boolean>).detail) routeGeneration++;
    });
  let githubAccounts: GithubAccount[] = [];
  let repositoryAccountsState: "loading" | "ready" | "unavailable" = "loading";
  const pendingSetup = new Set<string>();
  const setupMonitoringOff = new Set<string>();
  let repositoryAutomation: AutomationSnapshot | undefined;
  let repositoryAutomationRead = 0;
  const pendingPullRequests = new Map<
    string,
    {
      number: number;
      accountId: string;
      repositoryId: string;
      generation: number;
    }
  >();
  let copilotAccounts: CopilotAccount[] = [];
  let updateAgentAccounts: (() => void) | undefined;
  let updateRepositoryAccounts: (() => void) | undefined;
  let refreshAgentAccounts: (() => void) | undefined;
  let guidance: GuidedReturn | undefined;
  const returnGenie = app.querySelector<HTMLButtonElement>(
    "[data-return-genie]",
  )!;
  const openGenie = app.querySelector<HTMLButtonElement>("[data-open-genie]")!;
  const genieNote = app.querySelector<HTMLElement>("[data-genie-save-note]")!;
  genieNote.className = "settings-hint";
  const settingsHeading = app.querySelector<HTMLElement>(".settings-heading")!;
  if (options.embedded) {
    settingsHeading.hidden = true;
    app.querySelector<HTMLElement>(".settings-savebar")!.hidden = true;
  }
  settingsHeading.classList.add("settings-heading-with-genie");
  settingsHeading
    .querySelector("h1")!
    .after(app.querySelector(".settings-genie-entry")!);
  openGenie.hidden = !options.openGenie;
  openGenie.onclick = () => options.openGenie?.(openGenie);
  returnGenie.onclick = () => guidance?.back();
  const leaveGuidance = () => {
    guidance = undefined;
    returnGenie.hidden = true;
    app.querySelector<HTMLElement>("[data-genie-save-note]")!.hidden = true;
    openGenie.hidden = !options.openGenie;
    settingsBack.hidden = !options.embedded || section === "home";
  };
  const dialogs = createDialogs(
    content,
    () => revision++,
    (opener, parent) => restoreControl(opener, parent ?? content, parent),
  );
  const notificationView = { target: "" };
  const automationView = {};
  const preferenceDisclosures = new Set<string>();
  const dirty = () => !!draft && !sameResource(draft, saved);
  const preferencesDirty = () =>
    !!draft &&
    !sameResource(globalPreferences(draft), globalPreferences(saved));
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
  function restoreControl(
    opener: HTMLElement,
    scope = content,
    fallback: HTMLElement | undefined = app.querySelector("h1")!,
  ) {
    if (!scope.isConnected || scope.closest('[hidden],[aria-hidden="true"]'))
      return;
    const key = opener.dataset.focusKey;
    const replacement = opener.isConnected
      ? opener
      : key
        ? scope.querySelector<HTMLElement>(
            `[data-focus-key="${CSS.escape(key)}"]`,
          )
        : opener.id
          ? scope.querySelector<HTMLElement>(`#${CSS.escape(opener.id)}`)
          : null;
    const target =
      replacement &&
      !replacement.matches(":disabled") &&
      replacement.getClientRects().length
        ? replacement
        : fallback;
    target?.focus({ preventScroll: true });
  }
  function rememberControl(
    scope = content,
    fallback?: HTMLElement,
    opener = document.activeElement,
  ) {
    // Retain logical identity across this redraw, not a node it will detach.
    return opener instanceof HTMLElement && scope.contains(opener)
      ? () => restoreControl(opener, scope, fallback)
      : () => {};
  }
  function changed() {
    revision++;
    status.textContent = busy
      ? "Working..."
      : catalogConflict
        ? "Resource saved; reload Agents and doctrines. Drafts retained."
        : dirty()
          ? "Unsaved changes"
          : "All changes saved";
    save.disabled = !preferencesDirty() || busy;
    reset.disabled = !dirty() || busy;
    reload.hidden = !conflict;
    reload.disabled = busy;
    catalogNotice.hidden = !catalogConflict;
    reloadCatalog.disabled = busy;
  }

  async function commitResource(
    edit: ResourceEdit,
    modal?: HTMLDialogElement,
    accountGeneration?: number,
    retainRepositoryDraft = false,
  ) {
    if (busy) throw "Another resource save is in progress. Try again.";
    if (catalogConflict && (edit.kind === "agent" || edit.kind === "doctrine"))
      throw "Resource changed in another window. Your draft has not been written. Close this editor and reload Agents and doctrines before saving.";
    busy = true;
    const currentDraft =
      retainRepositoryDraft && edit.kind === "repository"
        ? repositories().find((r) => r.id === edit.id)
        : undefined;
    const retained = currentDraft ? clone(currentDraft) : undefined;
    changed();
    if (modal) modal.dataset.closeLocked = "true";
    const focused = document.activeElement;
    const controls = [
      ...app.querySelectorAll<
        | HTMLInputElement
        | HTMLButtonElement
        | HTMLSelectElement
        | HTMLTextAreaElement
      >("input,button,select,textarea"),
    ].map((control) => ({ control, disabled: control.disabled }));
    controls.forEach(({ control }) => (control.disabled = true));
    try {
      const result = await saveResource(edit, accountGeneration);
      const accepted = clone(saved);
      acceptResource(accepted, clone(result.settings), edit);
      // The submitted edit proves only its own changes, not external Agent
      // edits. Keep the old catalog/reference snapshot until explicit reload.
      catalogConflict ||= !sameResource(
        accepted.agents ?? [],
        result.settings.agents ?? [],
      );
      for (const settings of [saved, draft]) {
        if (catalogConflict && edit.kind === "doctrine") continue;
        acceptResource(settings, clone(result.settings), edit);
        if (!catalogConflict)
          settings.doctrines = clone(result.settings.doctrines ?? []);
      }
      if (retained && edit.kind === "repository") {
        const committed = draft.repositories?.find((r) => r.id === edit.id);
        if (committed) {
          retained.enabled = committed.enabled;
          acceptResource(draft, { ...draft, repositories: [retained] }, edit);
        }
      }
      if (!catalogConflict) snapshot.doctrine_catalog = result.doctrine_catalog;
      if (result.warning) showError(result.warning);
    } catch (cause) {
      if (reason(cause).startsWith("Resource changed")) conflict = true;
      throw cause;
    } finally {
      controls.forEach(
        ({ control, disabled }) => (control.disabled = disabled),
      );
      // Disabling a focused control can move focus to body while IPC is pending.
      if (
        focused instanceof HTMLElement &&
        app.contains(focused) &&
        document.activeElement === document.body
      )
        restoreControl(focused, app);
      if (modal) delete modal.dataset.closeLocked;
      busy = false;
      changed();
    }
  }

  const repositoryEdit = (
    repository: Repository,
    value: Repository | null = repository,
  ): ResourceEdit => ({
    kind: "repository",
    id: repository.id,
    expected: clone(
      saved.repositories?.find((r) => r.id === repository.id) ?? null,
    ),
    value: clone(value),
  });

  function repositoryMonitoringDetail(repository: Repository): string {
    if (
      repositoryAccountsState !== "ready" ||
      !githubAccounts.some(
        (a) =>
          a.account_id === repository.provider_account_id &&
          a.state === "connected",
      )
    )
      return repository.enabled
        ? "Reconnect account"
        : "No new scans or reviews / Reconnect account";
    if (!repository.enabled) return "No new scans or reviews";
    if (!repository.assignments?.length) return "Needs setup";
    if (!repositoryAutomation)
      return "Global Monitoring state unavailable; check Status and reopen Repositories to retry";
    if (repositoryAutomation.paused) return "Global Monitoring paused";
    if (repositoryAutomation.active >= repositoryAutomation.capacity)
      return "Waiting for AI capacity";
    return "Global Monitoring, access and execution checks still apply";
  }

  async function refreshRepositoryAutomation(update: () => void) {
    const read = ++repositoryAutomationRead;
    try {
      const state = await invoke<AutomationSnapshot>("automation_snapshot");
      if (read !== repositoryAutomationRead) return;
      repositoryAutomation = state;
    } catch {
      if (read !== repositoryAutomationRead) return;
      repositoryAutomation = undefined;
    }
    update();
  }

  async function toggleRepositoryMonitoring(
    id: string,
    enabled: boolean,
    modal?: HTMLDialogElement,
  ) {
    const current = saved.repositories?.find((r) => r.id === id);
    if (!current) throw "Save this repository configuration first.";
    await commitResource(
      repositoryEdit(current, { ...clone(current), enabled }),
      modal,
      undefined,
      true,
    );
    pendingSetup.delete(id);
    setupMonitoringOff.delete(id);
  }

  function dialog(title: string, body: string, opener: HTMLElement) {
    const modal = document.createElement("dialog");
    modal.className = "settings-dialog";
    modal.setAttribute("aria-label", title);
    modal.innerHTML = `<div class="dialog-head"><h2>${escape(title)}</h2><button type="button" aria-label="Close dialog">Close</button></div><div class="dialog-body">${body}</div>`;
    modal.querySelector("button")!.onclick = () => modal.close();
    dialogs.show(modal, opener);
    return modal;
  }
  function confirmDialog(
    title: string,
    body: string,
    action: string,
    opener: HTMLElement,
  ) {
    const modal = dialog(
      title,
      `<p>${body}</p><button class="primary" id="confirm">${escape(action)}</button>`,
      opener,
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
    const restoreFocus = rememberControl();
    const scroll = content.scrollTop;
    const awaitingAgentAccess =
      document.activeElement === content.querySelector("#new-agent");
    let focusAfterRender: Element | null;
    updateAgentAccounts = undefined;
    updateRepositoryAccounts = undefined;
    refreshAgentAccounts = undefined;
    refreshRepositoryRows = undefined;
    if (accountContent) accountParking.append(accountContent);
    app.querySelector("h1")!.textContent = sections[section][0];
    app.dataset.settingsSection = section;
    app.toggleAttribute("data-settings-home", section === "home");
    const accountOverview = [
      "accounts",
      "ai-tooling",
      "git-repository",
    ].includes(section);
    app.toggleAttribute("data-account-overview", accountOverview);
    settingsBack.hidden = !options.embedded || section === "home" || !!guidance;
    const parent = accountParents[section] ?? "home";
    settingsBack.setAttribute("aria-label", `Back to ${sections[parent][0]}`);
    settingsBack.title = `Back to ${sections[parent][0]}; retain unsaved changes`;
    app.querySelector<HTMLElement>(".settings-savebar")!.hidden =
      section === "repositories" ||
      (!!options.embedded && (section === "home" || accountOverview));
    settingsHeading.hidden = section === "home";
    app.dataset.resourceLibrary =
      section === "repositories"
        ? "repositories"
        : section === "agents" || section === "doctrines"
          ? section
          : "";
    app.toggleAttribute(
      "data-preferences",
      section === "preferences" || section === "capacity",
    );
    app.querySelector(".settings-heading p")!.textContent =
      sections[section][1];
    app
      .querySelectorAll<HTMLButtonElement>("[data-section]")
      .forEach((button) => {
        if (button.dataset.section === section)
          button.setAttribute("aria-current", "page");
        else button.removeAttribute("aria-current");
      });
    const select = app.querySelector<HTMLSelectElement>(
      ".mobile-section select",
    );
    if (select) select.value = section;
    content.replaceChildren();
    if (section === "home") renderHome();
    if (accountOverview) {
      if (!options.embedded && section === "accounts") renderAccounts();
      else renderAccountOverview();
    }
    if (section === "copilot" || section === "github") renderAccounts();
    if (section === "repositories") renderRepositories();
    if (section === "doctrines") renderDoctrines();
    if (section === "agents")
      renderAgents(() => {
        if (awaitingAgentAccess && document.activeElement === focusAfterRender)
          restoreFocus();
      });
    if (section === "preferences" || section === "capacity")
      renderPreferences();
    content.prepend(genieNote);
    if (saved.doctrine_reset) {
      const notice = document.createElement("p");
      notice.className = "settings-hint";
      notice.dataset.doctrineReset = "true";
      notice.setAttribute("role", "status");
      notice.textContent = `Pre-alpha doctrine library reset: replaced ${saved.doctrine_reset.previous_count} doctrines with the 10 shipped defaults; removed ${saved.doctrine_reset.removed_references} obsolete Agent references. Later library edits are preserved.`;
      content.prepend(notice);
    }
    changed();
    content.scrollTop = scroll;
    restoreFocus();
    focusAfterRender = document.activeElement;
  }
  function navigate(next: Section) {
    if (busy) return;
    section = next;
    render();
  }
  function openFromHome(next: Section) {
    homeScroll = content.scrollTop;
    homeOpener = next;
    navigate(next);
    content.scrollTop = 0;
    (next === "capacity"
      ? content.querySelector<HTMLElement>("#global-capacity")
      : settingsHeading.querySelector<HTMLElement>("h1")
    )?.focus();
  }
  settingsBack.onclick = () => {
    if (busy) return;
    const previous = section;
    const parent = accountParents[section] ?? "home";
    leaveGuidance();
    navigate(parent);
    content.scrollTop =
      parent === "home" ? homeScroll : (accountScroll.get(parent) ?? 0);
    const opener = content.querySelector<HTMLElement>(
      `[data-settings-destination="${parent === "home" ? homeOpener : previous}"]`,
    );
    opener?.focus({ preventScroll: true });
    opener?.scrollIntoView({ block: "nearest" });
  };
  app
    .querySelectorAll<HTMLButtonElement>("[data-section]")
    .forEach((button) => {
      button.onclick = () => navigate(button.dataset.section as Section);
    });
  const mobileSection = app.querySelector<HTMLSelectElement>(
    ".mobile-section select",
  );
  if (mobileSection)
    mobileSection.onchange = (event) => {
      navigate((event.target as HTMLSelectElement).value as Section);
    };

  function overviewRow(
    key: Section,
    description: string,
    value = "",
    symbol: keyof typeof iconPaths = "integrations",
  ) {
    return `<button type="button" class="settings-home-row" data-settings-destination="${key}" data-focus-key="settings:${key}" aria-label="${sections[key][0]}" aria-describedby="settings-${key}-description settings-${key}-count"><span class="settings-row-symbol">${icon(symbol)}</span><span class="settings-row-copy"><strong>${sections[key][0]}</strong><small id="settings-${key}-description">${description}</small></span><span class="settings-row-value" id="settings-${key}-count" data-settings-count="${key}">${value}</span><svg class="settings-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg></button>`;
  }
  function renderHome() {
    const row = (
      key:
        | "accounts"
        | "agents"
        | "repositories"
        | "doctrines"
        | "capacity"
        | "preferences",
      description: string,
      value = "",
    ) =>
      overviewRow(
        key,
        description,
        value,
        key === "preferences" ? "home" : key,
      );
    content.innerHTML = `<div class="settings-overview">
      <div class="settings-intro"><span class="settings-summary-icon">${icon("home")}</span><div><strong>Your review setup</strong><p>Shared agents. Your rules.</p></div></div>
      <nav aria-label="Review setup" class="settings-home-group">
        ${row("accounts", "GitHub and Copilot, kept separate", "Reading...")}
        ${row("agents", "Reusable review configurations", String(saved.agents?.length ?? 0))}
        ${row("repositories", "Assignments, people, and monitoring", String(saved.repositories?.length ?? 0))}
        ${row("doctrines", "The principles behind each review", String(saved.doctrines?.length ?? 0))}
      </nav>
      <nav aria-label="Application preferences" class="settings-home-group">
        ${row("capacity", "A limit on running work, not saved agents", String(saved.capacity))}
        ${row("preferences", "Automation, notifications, startup")}
      </nav>
      <button type="button" data-home-genie data-focus-key="settings:genie">Set up with Genie</button>
    </div>`;
    content
      .querySelectorAll<HTMLButtonElement>("[data-settings-destination]")
      .forEach((button) => {
        button.onclick = () =>
          openFromHome(button.dataset.settingsDestination as Section);
      });
    const genie =
      content.querySelector<HTMLButtonElement>("[data-home-genie]")!;
    genie.hidden = !options.openGenie;
    genie.onclick = () => options.openGenie?.(genie);
    refreshAccountSummaries();
  }

  function renderAccountOverview() {
    const comingSoon = (label: string, symbol: keyof typeof iconPaths) =>
      `<div class="settings-home-row settings-provider-planned" aria-disabled="true"><span class="settings-row-symbol">${icon(symbol)}</span><span class="settings-row-copy"><strong>${escape(label)}</strong></span><span class="settings-row-value">Coming soon</span></div>`;
    const rows =
      section === "accounts"
        ? overviewRow(
            "ai-tooling",
            sections["ai-tooling"][1],
            "Reading...",
            "agents",
          ) +
          overviewRow(
            "git-repository",
            sections["git-repository"][1],
            "Reading...",
            "repositories",
          )
        : section === "ai-tooling"
          ? overviewRow(
              "copilot",
              "Connect your Copilot subscription",
              "Reading...",
              "agents",
            ) +
            modelProviders
              .filter((provider) => !provider.available)
              .map((provider) => comingSoon(provider.label, "agents"))
              .join("")
          : overviewRow(
              "github",
              "Repository access and publication",
              "Reading...",
              "repositories",
            ) +
            ["Azure DevOps", "Bitbucket"]
              .map((label) => comingSoon(label, "repositories"))
              .join("");
    content.innerHTML = `<div class="settings-overview"><nav class="settings-home-group" aria-label="${sections[section][0]}">${rows}</nav></div>`;
    content
      .querySelectorAll<HTMLButtonElement>("[data-settings-destination]")
      .forEach((button) => {
        button.onclick = () => {
          if (busy) return;
          accountScroll.set(section, content.scrollTop);
          navigate(button.dataset.settingsDestination as Section);
          content.scrollTop = 0;
          settingsHeading.querySelector<HTMLElement>("h1")!.focus();
        };
      });
    refreshAccountSummaries();
  }

  function refreshAccountSummaries() {
    const summaries = [
      ...content.querySelectorAll<HTMLElement>(
        '[data-settings-count="accounts"], [data-settings-count="ai-tooling"], [data-settings-count="git-repository"], [data-settings-count="copilot"], [data-settings-count="github"]',
      ),
    ];
    const read = ++accountRead;
    void Promise.allSettled([
      invoke<{ accounts: GithubAccount[] }>("github_auth_state"),
      invoke<CopilotAuth>("copilot_auth_state"),
    ]).then(([github, copilot]) => {
      if (read !== accountRead || !summaries.some((node) => node.isConnected))
        return;
      if (github.status === "fulfilled") githubAccounts = github.value.accounts;
      if (copilot.status === "fulfilled")
        copilotAccounts = copilot.value.accounts;
      for (const summary of summaries) {
        const key = summary.dataset.settingsCount;
        const states =
          key === "accounts"
            ? [github, copilot]
            : key === "ai-tooling" || key === "copilot"
              ? [copilot]
              : [github];
        const failure = states.find((state) => state.status === "rejected");
        if (failure?.status === "rejected") {
          summary.textContent = "Unavailable";
          showError(
            `Account summary unavailable: ${reason(failure.reason)} Open the provider to retry.`,
          );
          continue;
        }
        const accounts = states.flatMap<GithubAccount | CopilotAccount>(
          (state) => (state.status === "fulfilled" ? state.value.accounts : []),
        );
        const connected = accounts.filter(
          (account) => account.state === "connected",
        ).length;
        summary.textContent = `${connected} connected${accounts.length > connected ? ` / ${accounts.length - connected} need attention` : ""}`;
      }
    });
  }

  // ---------------------------------------------------------------- Doctrines

  const doctrineUsers = (doctrine: Doctrine) =>
    agents().filter((agent) =>
      doctrineTitles(agent).some(
        (title) =>
          title.trim().toLowerCase() === doctrine.title.trim().toLowerCase(),
      ),
    );
  const agentRepositories = (agent: Agent) =>
    repositories().filter((repository) =>
      (repository.assignments ?? []).some(
        (assignment) => assignment.agent_id === agent.id,
      ),
    );

  function compactEditor(modal: HTMLDialogElement, backTitle: string) {
    modal.classList.add("shared-resource-editor");
    if (modal.parentElement?.classList.contains("dialog-fallback"))
      modal.parentElement.classList.add("shared-resource-host");
    const back = modal.querySelector<HTMLButtonElement>(".dialog-head button")!;
    back.classList.add("resource-back");
    back.title = backTitle;
    back.innerHTML = icon("back");
  }

  function resourceEditor(
    modal: HTMLDialogElement,
    deletion?: { edit: ResourceEdit; blocked: string },
  ) {
    compactEditor(modal, "Back to library; discard unsaved fields");
    modal.querySelector<HTMLButtonElement>("[data-cancel-resource]")!.onclick =
      () => modal.close();
    if (!deletion) return;
    const remove = modal.querySelector<HTMLButtonElement>(
      "[data-delete-resource]",
    )!;
    const confirmation = modal.querySelector<HTMLElement>(
      "[data-delete-confirmation]",
    )!;
    const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
    remove.onclick = () => {
      if (deletion.blocked) {
        alert.textContent = deletion.blocked;
        alert.hidden = false;
        alert.scrollIntoView({ block: "nearest" });
        return;
      }
      confirmation.hidden = false;
      confirmation.querySelector("button")!.focus();
    };
    confirmation.querySelector<HTMLButtonElement>(
      "[data-keep-resource]",
    )!.onclick = () => {
      confirmation.hidden = true;
      remove.focus();
    };
    confirmation.querySelector<HTMLButtonElement>(
      "[data-confirm-delete]",
    )!.onclick = async () => {
      try {
        await commitResource(deletion.edit, modal);
        modal.close();
        render();
      } catch (cause) {
        alert.textContent = reason(cause);
        alert.hidden = false;
        alert.scrollIntoView({ block: "nearest" });
      }
    };
  }
  const resourceActions = (kind: "agent" | "doctrine", existing: boolean) =>
    `<p class="settings-hint">Save applies this shared ${kind} immediately. Back or Cancel discards only this editor's unsaved fields; earlier saves stay applied.</p>
    <p role="alert" hidden></p><div class="resource-actions"><button class="primary">Save ${kind}</button><button type="button" data-cancel-resource>Cancel</button></div>
    ${existing ? `<button type="button" class="resource-delete" data-delete-resource>Delete ${kind}</button><div data-delete-confirmation hidden><p>Delete this saved ${kind} and discard these unsaved fields? Completed review evidence is retained.</p><div class="resource-actions"><button type="button" data-confirm-delete>Confirm deletion</button><button type="button" data-keep-resource>Keep ${kind}</button></div></div>` : ""}`;

  function renderDoctrines() {
    content.innerHTML = `<p class="settings-hint" data-doctrine-catalog>${escape(doctrineCatalogLabel(snapshot.doctrine_catalog))}</p><div class="section-actions resource-toolbar"><p>Reusable principles, shared across Agents. Plain text, never commands.</p><button class="primary" id="new-doctrine">New doctrine</button></div><div class="doctrine-list resource-library"></div>`;
    const list = content.querySelector(".doctrine-list")!;
    if (!doctrines().length)
      list.innerHTML =
        '<div class="settings-empty"><strong>No doctrines yet</strong><p>Create a doctrine to add your own review principles.</p></div>';
    for (const doctrine of doctrines()) {
      const row = document.createElement("article");
      row.className = "doctrine-card resource-card";
      row.innerHTML = `<span class="resource-symbol">${icon("doctrines")}</span><div class="resource-copy"><h3>${escape(doctrine.title)}</h3><p class="resource-preview">${escape(doctrine.body)}</p><p class="word-count">${words(doctrine.body)} words / ${doctrineUsers(doctrine).length} Agents</p></div><div class="card-actions"><button data-edit>Edit</button><button data-remove>Delete</button></div>`;
      for (const action of ["edit", "remove"])
        row.querySelector<HTMLElement>(`[data-${action}]`)!.dataset.focusKey =
          `doctrine:${doctrine.title}:${action}`;
      row.querySelector<HTMLButtonElement>("[data-edit]")!.onclick = (event) =>
        editDoctrine(event.currentTarget as HTMLButtonElement, doctrine);
      row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick = async (
        event,
      ) => {
        const opener = event.currentTarget as HTMLButtonElement;
        const usedBy = doctrineUsers(doctrine);
        if (usedBy.length) {
          showError(
            "This doctrine is used by an Agent. Remove or replace its references and save the Agent before deleting it.",
          );
          return;
        }
        if (
          !(await confirmDialog(
            "Delete this doctrine?",
            `Delete "${escape(doctrine.title)}" from the saved library? Completed review evidence is retained.`,
            "Delete doctrine",
            opener,
          ))
        )
          return;
        try {
          await commitResource({
            kind: "doctrine",
            title: doctrine.title,
            expected:
              saved.doctrines?.find((d) => d.title === doctrine.title) ?? null,
            value: null,
          });
          render();
        } catch (cause) {
          showError(reason(cause));
        }
      };
      list.append(row);
    }
    content.querySelector<HTMLButtonElement>("#new-doctrine")!.onclick = (
      event,
    ) => editDoctrine(event.currentTarget as HTMLButtonElement);
  }

  function editDoctrine(opener: HTMLElement, existing?: Doctrine) {
    const users = existing ? doctrineUsers(existing) : [];
    const modal = dialog(
      existing ? "Edit doctrine" : "New doctrine",
      `<form><label>Title<input name="title" required maxlength="100" value="${escape(existing?.title ?? "")}" placeholder="e.g. bounded-context" /></label><label>Principles<textarea name="body" rows="10" required>${escape(existing?.body ?? "")}</textarea></label><p class="word-count" data-count>${words(existing?.body ?? "")} words</p><p class="settings-hint">Title doubles as this doctrine's slug -- keep it short and unique. Plain text only, never commands or credentials. Around 500 words is a friendly length, not a limit.</p>
      <p class="resource-impact">${users.length ? `Shared by ${users.length} Agents: ${escape(users.map((agent) => agent.name).join(", "))}. Saving updates their shared principles; renaming preserves references. Remove these references and save the Agents before deleting.` : "Not used by any Agent. Select this doctrine from an Agent editor to share it."} Completed review evidence keeps its captured text.</p>${resourceActions("doctrine", !!existing)}</form>`,
      opener,
    );
    resourceEditor(
      modal,
      existing
        ? {
            edit: {
              kind: "doctrine",
              title: existing.title,
              expected:
                saved.doctrines?.find((d) => d.title === existing.title) ??
                null,
              value: null,
            },
            blocked: users.length
              ? "This doctrine is used by an Agent. Remove or replace its references and save the Agent before deleting it."
              : "",
          }
        : undefined,
    );
    const body = modal.querySelector<HTMLTextAreaElement>("[name=body]")!;
    const count = modal.querySelector<HTMLElement>("[data-count]")!;
    body.oninput = () => {
      count.textContent = `${words(body.value)} words`;
    };
    modal.querySelector("form")!.onsubmit = async (event) => {
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
        await commitResource(
          {
            kind: "doctrine",
            title: existing?.title ?? title,
            expected:
              saved.doctrines?.find((d) => d.title === existing?.title) ?? null,
            value: { title, body: body.value },
          },
          modal,
        );
        if (existing) opener.dataset.focusKey = `doctrine:${title}:edit`;
        modal.close();
        render();
      } catch (cause) {
        alert.textContent = typeof cause === "string" ? cause : reason(cause);
        alert.hidden = false;
      }
    };
  }

  // -------------------------------------------------------------------- Agents

  function renderAgents(onReady: () => void) {
    content.innerHTML = `<div class="section-actions resource-toolbar"><p>Unlimited saved configurations. AI capacity is set separately.</p><button class="primary" id="new-agent" disabled>New agent</button></div><div class="resource-account-notice"><p class="settings-hint" data-copilot-status>Reading Copilot accounts...</p><button id="manage-copilot">Manage Copilot accounts</button></div><div class="agent-list resource-library"></div>`;
    const list = content.querySelector(".agent-list")!;
    if (!agents().length)
      list.innerHTML = `<div class="settings-empty"><strong>No agents yet</strong><p>Create one to start assigning it to repositories in Repositories.</p></div>`;
    for (const agent of agents()) {
      const account = copilotAccounts.find(
        (a) => a.account_id === agent.ai_account?.account_id,
      );
      const row = document.createElement("article");
      row.className = "agent-card resource-card";
      row.innerHTML = `<span class="resource-symbol">${icon("agents")}</span><div class="resource-copy"><h3>${escape(agent.name)}</h3><p class="resource-meta">${escape(agent.model)} / ${agentRepositories(agent).length} repositories</p><p class="resource-preview">${escape(agent.prompt)}</p><p data-account-state>${escape(agent.ai_account ? `Copilot: ${account?.login ?? agent.ai_account.account_id}. ${account?.state === "connected" ? "Sign-in verified; model access checked in Edit." : "Reconnect required; Agent blocked."}` : "Unconfigured. Choose an AI account and an actual model; the legacy selection is retained.")}</p></div><div class="card-actions"><button data-edit>Edit</button><button data-remove>Delete</button></div>`;
      row.dataset.agentId = agent.id;
      for (const action of ["edit", "remove"])
        row.querySelector<HTMLElement>(`[data-${action}]`)!.dataset.focusKey =
          `agent:${agent.id}:${action}`;
      row.querySelector<HTMLButtonElement>("[data-edit]")!.onclick = (event) =>
        editAgent(event.currentTarget as HTMLButtonElement, agent);
      row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick = async (
        event,
      ) => {
        const opener = event.currentTarget as HTMLButtonElement;
        const assigned = agentRepositories(agent);
        if (assigned.length) {
          showError(
            "This Agent is assigned to a repository. Remove or replace its assignments and save the repository before deleting it.",
          );
          return;
        }
        if (
          !(await confirmDialog(
            "Delete this agent?",
            `Delete "${escape(agent.name)}" from saved Agents? Completed review evidence is retained.`,
            "Delete agent",
            opener,
          ))
        )
          return;
        try {
          await commitResource({
            kind: "agent",
            id: agent.id,
            expected: saved.agents?.find((a) => a.id === agent.id) ?? null,
            value: null,
          });
          render();
        } catch (cause) {
          showError(reason(cause));
        }
      };
      list.append(row);
    }
    content.querySelector<HTMLButtonElement>("#new-agent")!.onclick = (event) =>
      editAgent(event.currentTarget as HTMLButtonElement);
    content.querySelector<HTMLButtonElement>("#manage-copilot")!.onclick =
      () => {
        navigate(options.embedded ? "copilot" : "accounts");
        if (options.embedded) {
          homeOpener = "accounts";
          settingsHeading.querySelector<HTMLElement>("h1")!.focus();
        }
      };
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
            : `No verified Copilot connection. Connect an account in ${accountSection}; existing Agents and assignments are retained.`;
          content.querySelector<HTMLButtonElement>("#new-agent")!.disabled =
            !connected;
          onReady();
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

  function editAgent(opener: HTMLElement, existing?: Agent) {
    const assigned = existing ? agentRepositories(existing) : [];
    const modal = dialog(
      existing ? "Edit agent" : "New agent",
      `<form><label>Name<input name="name" required maxlength="80" value="${escape(existing?.name ?? "")}" placeholder="e.g. The Nitpicker" /></label>
        <div class="resource-intelligence"><h3>Intelligence</h3><p class="settings-hint">GitHub Copilot. The AI account is separate from the GitHub account used for repository actions.</p>
        <label>AI account<select name="ai-account" aria-label="AI account"><option value="">Choose a Copilot account</option>${copilotAccounts.map((a) => `<option value="${escape(a.account_id)}" ${a.account_id === existing?.ai_account?.account_id ? "selected" : ""} ${a.state === "connected" ? "" : "disabled"}>${escape(a.login)} (${escape(a.account_id)})${a.state === "connected" ? "" : " - reconnect required"}</option>`).join("")}${existing?.ai_account && !copilotAccounts.some((a) => a.account_id === existing.ai_account?.account_id) ? `<option selected disabled value="${escape(existing.ai_account.account_id)}">Copilot ${escape(existing.ai_account.account_id)} - reconnect required</option>` : ""}</select></label>
        <label>Model<select name="model" aria-label="Model" disabled><option value="${escape(existing?.model ?? "")}">${escape(existing?.model ?? "Choose an account first")}</option></select></label>
        <label>Reasoning effort<select name="reasoning-effort" aria-label="Reasoning effort" disabled></select></label>
        <label>Context window<select name="context-tier" aria-label="Context window" disabled></select></label>
        <p class="settings-hint" data-intelligence-status aria-live="polite"></p>
        <p class="settings-hint" data-model-status role="status"></p><button type="button" data-retry-models>Retry model list</button><button type="button" data-cancel-models hidden>Cancel model lookup</button></div>
        <label>Prompt<textarea name="prompt" rows="4" required>${escape(existing?.prompt ?? "Review this pull request for correctness, risk, and readability.")}</textarea></label>
        <fieldset class="resource-doctrines"><legend>Doctrines</legend><p class="settings-hint" data-doctrine-catalog>${escape(doctrineCatalogLabel(snapshot.doctrine_catalog))}</p><p class="settings-hint">Select zero, one or many. Existing order is retained; new selections append in library order. Filtering does not change selections.</p><label>Filter doctrines<input type="search" data-doctrine-filter placeholder="Find principles..." /></label><p class="settings-hint" data-selection-count aria-live="polite"></p><div class="doctrine-choices" tabindex="0" role="group" aria-label="Available doctrines">${doctrines()
          .map(
            (d) =>
              `<label data-doctrine-choice><input type="checkbox" name="doctrine" value="${escape(d.title)}" ${existing && doctrineTitles(existing).some((t) => t.trim().toLowerCase() === d.title.trim().toLowerCase()) ? "checked" : ""} /><span>${escape(d.title)}</span></label>`,
          )
          .join(
            "",
          )}</div><p class="settings-hint" data-no-doctrines hidden></p></fieldset>
        <label>Signature<input name="signature" required maxlength="80" value="${escape(existing?.signature ?? "PR Sniper \u{1F3AF}")}" /></label>
        <p class="settings-hint">Custom signature is saved for the signature-customization follow-up. Current publication uses the canonical PR Sniper signature.</p>
        <p class="settings-hint">No review or test prompt runs here. Saving an existing unconfigured Agent preserves its selection until you explicitly replace it.</p>
        <p class="resource-impact">${assigned.length ? `Shared by ${assigned.length} repositories: ${escape(assigned.map((repository) => repository.name).join(", "))}. Remove or replace those assignments and save the repositories before deleting.` : "Not assigned to any repository."} Comment, Approve, Merge and primary designation belong to repository assignments, never this shared Agent. Completed evidence keeps its captured configuration.</p>${resourceActions("agent", !!existing)}</form>`,
      opener,
    );
    resourceEditor(
      modal,
      existing
        ? {
            edit: {
              kind: "agent",
              id: existing.id,
              expected: saved.agents?.find((a) => a.id === existing.id) ?? null,
              value: null,
            },
            blocked: assigned.length
              ? "This Agent is assigned to a repository. Remove or replace its assignments and save the repository before deleting it."
              : "",
          }
        : undefined,
    );
    const filter = modal.querySelector<HTMLInputElement>(
      "[data-doctrine-filter]",
    )!;
    const choices = [
      ...modal.querySelectorAll<HTMLElement>("[data-doctrine-choice]"),
    ];
    const updateChoices = () => {
      const query = filter.value.trim().toLowerCase();
      let visible = 0;
      let selected = 0;
      let hiddenSelected = 0;
      for (const choice of choices) {
        const input = choice.querySelector<HTMLInputElement>("input")!;
        choice.hidden = !input.value.toLowerCase().includes(query);
        if (!choice.hidden) visible++;
        if (input.checked) {
          selected++;
          if (choice.hidden) hiddenSelected++;
        }
      }
      modal.querySelector("[data-selection-count]")!.textContent =
        `${selected} selected${hiddenSelected ? ` (${hiddenSelected} hidden by filter)` : ""} / ${visible} shown`;
      const empty = modal.querySelector<HTMLElement>("[data-no-doctrines]")!;
      empty.hidden = visible > 0;
      empty.textContent = choices.length
        ? "No matching doctrines. Clear the filter to see the library."
        : "No doctrines yet. Save or cancel this Agent, then add principles in Doctrines.";
    };
    filter.oninput = updateChoices;
    filter.onkeydown = (event) => {
      if (event.key === "Enter") event.preventDefault();
    };
    modal
      .querySelector(".doctrine-choices")!
      .addEventListener("change", updateChoices);
    updateChoices();
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
    let catalogLoaded = false;
    const effortSelect = modal.querySelector<HTMLSelectElement>(
      "[name=reasoning-effort]",
    )!;
    const contextSelect = modal.querySelector<HTMLSelectElement>(
      "[name=context-tier]",
    )!;
    const intelligenceStatus = modal.querySelector<HTMLElement>(
      "[data-intelligence-status]",
    )!;
    let effort = existing?.intelligence?.reasoning_effort ?? "";
    let context = existing?.intelligence?.context_tier ?? "";
    let intelligenceChanged = false;
    const intelligence = () => ({
      reasoning_effort: effort || null,
      context_tier: context || null,
    });
    function describeIntelligence() {
      const model = catalog.find((m) => m.id === modelSelect.value);
      const ready = catalogLoaded && !!model && modelSelectable(model);
      const render = (
        select: HTMLSelectElement,
        selected: string,
        values: string[],
        defaultLabel: string,
        label: (value: string) => string,
        supported: (value: string) => boolean = () => true,
      ) => {
        select.innerHTML =
          option("", defaultLabel, selected) +
          (selected && !values.includes(selected)
            ? `<option selected disabled value="${escape(selected)}">${escape(selected)} - ${catalogLoaded ? "unsupported; retained" : "retained; discovery unavailable"}</option>`
            : "") +
          values
            .map(
              (value) =>
                `<option value="${escape(value)}" ${selected === value ? "selected" : ""} ${supported(value) ? "" : "disabled"}>${escape(label(value))}${supported(value) ? "" : " - unsupported by pinned runtime"}</option>`,
            )
            .join("");
        select.disabled = !ready || (!values.some(supported) && !selected);
      };
      render(
        effortSelect,
        effort,
        reasoningEfforts(model),
        `Provider default${model?.defaultReasoningEffort ? ` (${model.defaultReasoningEffort})` : ""}`,
        (value) => value,
      );
      const capacity = contextCapacity(model);
      render(
        contextSelect,
        context,
        contextTiers(model),
        `Provider default${capacity ? ` (${capacity})` : ""}`,
        (value) =>
          `${value === "default" ? "Standard" : value === "long_context" ? "Long context" : value}${contextCapacity(model, value) ? ` (${contextCapacity(model, value)})` : ""}`,
        runtimeContextTier,
      );
      intelligenceStatus.textContent = !catalogLoaded
        ? "Capabilities are unavailable until model discovery succeeds. Deliberate choices are retained, not declared invalid."
        : !ready
          ? "Choose an available model to check Intelligence. Deliberate choices are retained."
          : intelligenceError(model, intelligence())
            ? `${intelligenceError(model, intelligence())} Your choice is retained. Select a supported value or explicitly choose Provider default before saving.`
            : [
                reasoningEfforts(model).length
                  ? ""
                  : "No reasoning efforts advertised; Provider default only.",
                contextTiers(model).some(runtimeContextTier)
                  ? ""
                  : "No supported context tiers advertised; Provider default only.",
                model?.capabilities?.limits?.max_context_window_tokens !==
                undefined
                  ? `Advertised model maximum: ${model.capabilities.limits.max_context_window_tokens.toLocaleString("en-US")} tokens.`
                  : "",
                "Provider defaults are resolved by the runtime. Actual settings are checked before inference and captured with each result.",
              ]
                .filter(Boolean)
                .join(" ");
    }
    effortSelect.onchange = () => {
      effort = effortSelect.value;
      intelligenceChanged = true;
      describeIntelligence();
    };
    contextSelect.onchange = () => {
      context = contextSelect.value;
      intelligenceChanged = true;
      describeIntelligence();
    };
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
      catalogLoaded = false;
      describeIntelligence();
      const accountId = accountSelect.value;
      const selected = modelSelect.value;
      const current = ++request;
      modelSelect.disabled = true;
      if (
        !copilotAccounts.some(
          (a) => a.account_id === accountId && a.state === "connected",
        )
      ) {
        modelStatus.textContent = existing
          ? `Connect or reconnect this account in ${accountSection}. The saved model is retained, but this Agent is unconfigured.`
          : "Choose a verified Copilot account, then load its models.";
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
        catalogLoaded = true;
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
      describeIntelligence();
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
        catalogLoaded = false;
        describeIntelligence();
        modelSelect.disabled = true;
        modelStatus.textContent = selected
          ? `Reconnect this account in ${accountSection}. Your model and draft are retained.`
          : "Choose a verified Copilot account, then load its models.";
      } else if (!selectedWasConnected) {
        void loadModels();
      }
      selectedWasConnected = connected;
    };
    void loadModels();
    modal.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
      const name = modal
        .querySelector<HTMLInputElement>("[name=name]")!
        .value.trim();
      const model =
        modal.querySelector<HTMLSelectElement>("[name=model]")!.value;
      const selectedDoctrines = [
        ...modal.querySelectorAll<HTMLInputElement>("[name=doctrine]:checked"),
      ].map((input) => input.value);
      const previousTitles = existing ? doctrineTitles(existing) : [];
      const sameTitle = (a: string, b: string) =>
        a.trim().toLowerCase() === b.trim().toLowerCase();
      const selectedTitles = [
        ...previousTitles.filter((title) =>
          selectedDoctrines.some((d) => sameTitle(d, title)),
        ),
        ...selectedDoctrines.filter(
          (title) => !previousTitles.some((d) => sameTitle(d, title)),
        ),
      ];
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
        const sameIntelligence =
          effort === (existing?.intelligence?.reasoning_effort ?? "") &&
          context === (existing?.intelligence?.context_tier ?? "");
        const selectedModel = catalog.find((m) => m.id === model);
        const modelAvailable =
          catalogLoaded && selectedModel && modelSelectable(selectedModel);
        if (!unchanged && (!accountId || !modelAvailable))
          throw "Choose a verified AI account and a model returned for that account. Retry the model list if unavailable.";
        if (!modelAvailable && !sameIntelligence)
          throw "Model capabilities are unavailable. Retry discovery before changing Intelligence; saved choices are retained.";
        if (modelAvailable) {
          const invalid = intelligenceError(selectedModel, intelligence());
          if (invalid)
            throw `${invalid} Choose a supported value or Provider default; no setting was discarded.`;
        }
        const values: Agent = {
          id: existing?.id ?? newIdentity(),
          name,
          model,
          ...(existing?.intelligence ||
          !existing ||
          !unchanged ||
          intelligenceChanged
            ? { intelligence: intelligence() }
            : {}),
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
          ...(existing &&
          existing.doctrines === undefined &&
          JSON.stringify(selectedTitles) === JSON.stringify(previousTitles)
            ? existing.doctrine
              ? { doctrine: existing.doctrine }
              : {}
            : { doctrines: selectedTitles }),
        };
        await commitResource(
          {
            kind: "agent",
            id: values.id,
            expected: saved.agents?.find((a) => a.id === values.id) ?? null,
            value: values,
          },
          modal,
        );
        modal.close();
        render();
      } catch (cause) {
        alert.textContent = typeof cause === "string" ? cause : reason(cause);
        alert.hidden = false;
      }
    };
  }

  // --------------------------------------------------- Accounts / repositories

  function renderAccounts() {
    if (accountContent) {
      showAccountProvider();
      content.append(accountContent);
      window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts"));
      return;
    }
    accountContent = document.createElement("div");
    accountContent.innerHTML = `<div class="integration-group" data-account-provider="github"><h2>Git repositories</h2><div class="github-auth"></div></div>
      <div class="integration-group" data-account-provider="copilot"><h2>AI integration</h2><div class="copilot-auth"></div><div class="integration-grid" ${options.embedded ? "hidden" : ""}>${modelProviders
        .filter((m) => !m.available)
        .map(
          (m) =>
            `<button type="button" class="integration-card" data-disabled="true" disabled aria-disabled="true"><strong>Direct ${escape(m.label)}</strong><span>Coming soon</span></button>`,
        )
        .join("")}</div></div>`;
    showAccountProvider();
    content.append(accountContent);
    renderCopilotAuth(
      accountContent.querySelector(".copilot-auth")!,
      (accounts) => {
        copilotAccounts = accounts;
      },
    );
    renderGithubAuth(
      accountContent.querySelector(".github-auth")!,
      (accounts) => {
        repositoryAccountsState = accounts ? "ready" : "unavailable";
        githubAccounts = (accounts ?? []).filter(
          (a) =>
            a.provider === "github" &&
            (a.state === "connected" || a.state === "reconnect_required"),
        );
        updateRepositoryAccounts?.();
        refreshRepositoryRows?.();
      },
    );
  }

  function showAccountProvider() {
    accountContent
      ?.querySelectorAll<HTMLElement>("[data-account-provider]")
      .forEach((group) => {
        group.hidden =
          !!options.embedded && group.dataset.accountProvider !== section;
      });
  }

  function renderRepositories() {
    content.innerHTML = `<section class="repository-library" aria-label="Repositories">
      <div class="section-actions"><h2>Your repositories</h2><button class="primary" id="add-repository" aria-label="Add repository by URL">+ URL</button></div>
      <div class="repository-list native-repository-list"></div>
      ${conflict ? '<p role="alert">Saved settings changed in another window. Your unsaved draft is retained.</p><button data-repository-reload>Discard draft and reload</button>' : ""}
      <h2>Browse from an account</h2><div data-repository-accounts class="native-repository-list"></div>
      <p class="settings-hint">Choose a personal or organization owner. Saving repository configuration authorizes current and future matching pull requests.</p>
      <p class="settings-hint">Azure DevOps organization browsing is coming soon.</p></section>`;
    const rows = () => {
      const list = content.querySelector<HTMLElement>(".repository-list");
      const accounts = content.querySelector<HTMLElement>(
        "[data-repository-accounts]",
      );
      if (!list || !accounts) return;
      const restore = rememberControl(content);
      list.innerHTML =
        repositories()
          .map((repository) => {
            const account = githubAccounts.find(
              (a) => a.account_id === repository.provider_account_id,
            );
            const committed =
              saved.repositories?.find((r) => r.id === repository.id) ??
              repository;
            const state = committed.enabled ? "Enabled" : "Disabled";
            const label =
              repositories().filter((r) => r.name === repository.name).length >
              1
                ? `${repository.name} as ${account?.login ?? "Account unavailable"}`
                : repository.name;
            return `<div class="native-repository-row repository-monitoring-row"><button type="button" class="repository-open" aria-label="${escape(label)}" data-dirty="${!sameResource(
              repository,
              saved.repositories?.find((r) => r.id === repository.id),
            )}" data-repository="${escape(repository.id)}" data-focus-key="repository:${escape(repository.id)}:settings">
          <span class="settings-row-copy"><strong>${escape(repository.name)}</strong><small>${repository.provider === "github" ? "GitHub" : "Azure DevOps"} / ${escape(account?.login ?? "Account unavailable")}</small><small data-repository-monitoring-detail="${escape(repository.id)}">${escape(repositoryMonitoringDetail(committed))}</small></span>
          <span class="settings-row-value" data-monitoring-state="${state.toLowerCase()}">${state}${
            sameResource(
              repository,
              saved.repositories?.find((r) => r.id === repository.id),
            )
              ? ""
              : " / Draft"
          }</span><span aria-hidden="true">›</span></button><button type="button" class="repository-monitoring-toggle" role="switch" aria-checked="${committed.enabled}" aria-label="Monitor ${escape(label)}" data-toggle-repository="${escape(repository.id)}" data-focus-key="repository:${escape(repository.id)}:monitoring" ${busy ? "disabled" : ""}>${committed.enabled ? "Disable" : "Enable"}</button></div>`;
          })
          .join("") ||
        '<p class="settings-empty">No repositories yet. Browse an account below or add a GitHub URL.</p>';
      accounts.innerHTML =
        repositoryAccountsState === "unavailable"
          ? '<p role="alert">GitHub accounts are unavailable. No connection is assumed.</p><button data-retry-accounts>Retry reading accounts</button>'
          : repositoryAccountsState === "loading"
            ? '<p role="status">Reading GitHub accounts...</p>'
            : githubAccounts
                .map(
                  (account) =>
                    `<div class="native-repository-row"><span class="settings-row-copy"><strong>${escape(account.login)}</strong><small>GitHub repository access${account.state === "connected" ? "" : " / Reconnect in Accounts"}</small></span><button type="button" data-browse-account="${escape(account.account_id)}" aria-label="Browse repositories as ${escape(account.login)}" ${account.state === "connected" ? "" : "disabled"}><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="10" cy="10" r="6"/><path d="m15 15 6 6"/></svg></button></div>`,
                )
                .join("") ||
              '<p class="settings-empty">Connect a GitHub account in Accounts to browse repositories.</p>';
      accounts
        .querySelector<HTMLButtonElement>("[data-retry-accounts]")
        ?.addEventListener("click", () =>
          window.dispatchEvent(
            new Event("pr-sniper:refresh-provider-accounts"),
          ),
        );
      list
        .querySelectorAll<HTMLButtonElement>("[data-toggle-repository]")
        .forEach((button) => {
          button.onclick = async () => {
            clearError();
            const restore = rememberControl(content);
            try {
              await toggleRepositoryMonitoring(
                button.dataset.toggleRepository!,
                button.getAttribute("aria-checked") !== "true",
              );
              rows();
              restore();
              void refreshRepositoryAutomation(updateMonitoringDetails);
            } catch (cause) {
              showError(reason(cause));
              rows();
              restore();
            }
          };
        });
      list
        .querySelectorAll<HTMLButtonElement>("[data-repository]")
        .forEach((button) => {
          button.onclick = () =>
            repositoryDialog(
              repositories().find((r) => r.id === button.dataset.repository)!,
              button,
            );
        });
      accounts
        .querySelectorAll<HTMLButtonElement>("[data-browse-account]")
        .forEach((button) => {
          button.onclick = () =>
            browseRepositories(
              githubAccounts.find(
                (a) => a.account_id === button.dataset.browseAccount,
              )!,
              button,
            );
        });
      restore();
    };
    const updateMonitoringDetails = () => {
      content
        .querySelectorAll<HTMLElement>("[data-repository-monitoring-detail]")
        .forEach((detail) => {
          const committed = saved.repositories?.find(
            (r) => r.id === detail.dataset.repositoryMonitoringDetail,
          );
          if (committed)
            detail.textContent = repositoryMonitoringDetail(committed);
        });
    };
    refreshRepositoryRows = rows;
    renderAccounts();
    if (accountContent) accountParking.append(accountContent);
    content.querySelector<HTMLButtonElement>("#add-repository")!.onclick = (
      event,
    ) => editRepository(event.currentTarget as HTMLElement);
    content
      .querySelector<HTMLButtonElement>("[data-repository-reload]")
      ?.addEventListener("click", () => reload.click());
    rows();
    void refreshRepositoryAutomation(updateMonitoringDetails);
  }

  async function addAndConfigure(
    accountId: string,
    resolved: ResolvedRepository,
    modal: HTMLDialogElement,
    opener: HTMLElement,
    previous?: ConfiguredRepository,
  ) {
    const requestedRoute = routeGeneration;
    if (
      resolved.identity.id !== accountId ||
      !githubAccounts.some(
        (a) =>
          a.provider === "github" &&
          a.account_id === accountId &&
          a.state === "connected",
      )
    )
      throw "The acting account changed or disconnected. Reconnect and try again.";
    let repository = repositories().find(
      (r) =>
        r.provider === "github" &&
        r.provider_account_id === accountId &&
        r.provider_repository_id === resolved.repository.id,
    );
    if (previous && repository && previous.id !== repository.id)
      throw `${resolved.repository.name} is already configured on another row under this account. Choose a different repository or cancel this edit.`;
    if (
      !repository ||
      (previous?.id === repository.id &&
        previous.name !== resolved.repository.name)
    ) {
      const value: ConfiguredRepository = {
        ...(previous ?? {}),
        id: previous?.id ?? newIdentity(),
        name: resolved.repository.name,
        enabled: previous?.enabled ?? false,
        provider: "github",
        provider_account_id: accountId,
        provider_repository_id: resolved.repository.id,
      };
      await commitResource(
        repositoryEdit(value),
        modal,
        resolved.account_generation,
      );
      if (!previous) pendingSetup.add(value.id);
      repository = repositories().find((r) => r.id === value.id)!;
    }
    if (resolved.pull_request) {
      pendingPullRequests.set(repository.id, {
        number: resolved.pull_request.number,
        accountId,
        repositoryId: resolved.repository.id,
        generation: resolved.account_generation,
      });
    } else {
      pendingPullRequests.delete(repository.id);
    }
    modal.close();
    render();
    if (app.closest("[hidden]") || requestedRoute !== routeGeneration) return;
    repositoryDialog(
      repository,
      content.querySelector<HTMLElement>(
        `[data-repository="${CSS.escape(repository.id)}"]`,
      ) ?? opener,
    );
  }

  function editRepository(
    opener: HTMLElement,
    repository?: ConfiguredRepository,
  ) {
    const modal = dialog(
      repository ? "Edit repository" : "Add repository by URL",
      `<form><label>Repository URL<input name="repository" required value="${escape(repository?.name ?? "")}" placeholder="https://github.com/owner/repository" autocomplete="off" /></label>
      <div class="repository-actor"><p class="settings-hint" role="status" id="repository-actor-status"></p><button type="button" data-change-account hidden>Change</button></div>
      <label data-account-choice hidden>Acting GitHub account<select name="account" aria-describedby="repository-actor-status"></select></label>
      <div class="settings-actions"><button type="button" data-recover-account hidden>Connect GitHub account</button><button type="button" data-retry-account hidden>Retry reading accounts</button></div>
      <p class="settings-hint">GitHub repository or pull request URL, or owner/repository. Uses Git Repository access, not your Copilot AI connection. Adding opens configuration; Save queues a requested PR regardless of watch filters, without granting action permissions.</p>
      <p role="alert" hidden></p><div class="resource-actions"><button type="button" data-cancel-resource>Cancel</button><button type="submit" class="primary">Add &amp; configure</button></div></form>`,
      opener,
    );
    resourceEditor(modal);
    modal.classList.add("repository-editor");
    const account = modal.querySelector<HTMLSelectElement>("[name=account]")!;
    account.setAttribute("aria-label", "Acting GitHub account");
    const input = modal.querySelector<HTMLInputElement>("[name=repository]")!;
    const status = modal.querySelector<HTMLElement>("[role=status]")!;
    const choice = modal.querySelector<HTMLElement>("[data-account-choice]")!;
    const change = modal.querySelector<HTMLButtonElement>(
      "[data-change-account]",
    )!;
    const recover = modal.querySelector<HTMLButtonElement>(
      "[data-recover-account]",
    )!;
    const retry = modal.querySelector<HTMLButtonElement>(
      "[data-retry-account]",
    )!;
    const submit = modal.querySelector<HTMLButtonElement>(
      'button[type="submit"]',
    )!;
    const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
    // Once selected, the actor is pinned even when discovery fails or another
    // account appears. Only the operator's Change choice can replace it.
    let selectedId = repository?.provider_account_id ?? "";
    let selectedLogin = githubAccounts.find(
      (a) => a.account_id === selectedId,
    )?.login;
    let choosing = false;
    let choiceRequired = !!repository;
    let generation = 0;
    let submitting = false;
    let selectedSnapshot = "";
    let renderedChoices = "";
    const compatible = () =>
      githubAccounts.filter(
        (a) => a.provider === "github" && a.state === "connected",
      );
    const selected = () =>
      compatible().find((a) => a.account_id === selectedId);
    const invalidate = () => {
      generation++;
      if (submitting) {
        submitting = false;
        alert.textContent =
          "The URL or account connection changed. Your input is retained; add and configure again to verify current access.";
        alert.hidden = false;
      }
    };
    const update = () => {
      if (!modal.isConnected) return;
      const provider = repositoryProviderContext(input.value);
      const available = compatible();
      if (
        !selectedId &&
        provider === "github" &&
        repositoryAccountsState === "ready"
      ) {
        if (available.length > 1) choiceRequired = true;
        if (!choiceRequired && available.length === 1)
          selectedId = available[0].account_id;
      }
      const actor = selected();
      const unavailable = githubAccounts.find(
        (a) => a.provider === "github" && a.account_id === selectedId,
      );
      selectedLogin = actor?.login ?? selectedLogin;
      const snapshot = JSON.stringify(actor ?? null);
      if (snapshot !== selectedSnapshot) invalidate();
      selectedSnapshot = snapshot;
      const choices =
        option("", "Choose a GitHub account", "") +
        available.map((a) => option(a.account_id, a.login, "")).join("") +
        (selectedId && !actor
          ? `<option selected disabled value="${escape(selectedId)}">${escape(selectedLogin ?? "Saved account")} - reconnect required</option>`
          : "");
      if (renderedChoices !== choices) {
        account.innerHTML = choices;
        renderedChoices = choices;
      }
      if (account.value !== selectedId) account.value = selectedId;
      const github = provider === "github";
      const ready = repositoryAccountsState === "ready";
      const reconnect =
        !!selectedId ||
        githubAccounts.some(
          (a) => a.provider === "github" && a.state === "reconnect_required",
        );
      choice.hidden =
        !github || (!choosing && (!ready || !!selectedId || !available.length));
      account.disabled = !github || busy;
      change.hidden = !github || !selectedId || choosing;
      recover.hidden = !github || !ready || !!actor;
      recover.textContent = reconnect
        ? "Reconnect GitHub account"
        : "Connect GitHub account";
      retry.hidden = !github || repositoryAccountsState !== "unavailable";
      submit.disabled = !github || !ready || !actor || submitting || busy;
      status.textContent = !github
        ? provider === "azure_devops"
          ? "Azure DevOps repository connections are coming soon. This URL cannot use a GitHub account."
          : provider === "invalid"
            ? "Enter a valid HTTPS repository URL or owner/repository."
            : "This repository provider is not supported. Use a GitHub repository URL."
        : repositoryAccountsState === "unavailable"
          ? "GitHub repository accounts are unavailable. Retry reading accounts; your URL and selected identity are retained."
          : !ready
            ? "Reading GitHub repository accounts..."
            : actor
              ? `GitHub / ${actor.login}${actor.warning ? " / Verification needs retry; current access will be checked before saving." : ""}`
              : selectedId
                ? `Reconnect ${selectedLogin ?? "the saved GitHub account"} for repository access, or explicitly Change the account. ${unavailable?.reason ? githubAccountFailureMessage(unavailable.reason) : "No other identity was selected."}`
                : available.length
                  ? "Choose a GitHub repository account before validating this URL. No account is selected."
                  : reconnect
                    ? "Reconnect a GitHub repository account in Accounts. Its authorization is unavailable; Copilot AI access is separate."
                    : "Connect a GitHub repository account to continue. Copilot AI access is separate; confirm the returned identity before using it.";
    };
    input.oninput = () => {
      invalidate();
      update();
    };
    change.onclick = () => {
      choosing = true;
      update();
      if (!choice.hidden) account.focus();
    };
    account.onchange = () => {
      invalidate();
      selectedId = account.value;
      selectedLogin = selected()?.login;
      choiceRequired = true;
      choosing = true;
      update();
    };
    account.onfocus = () => {
      choosing = true;
    };
    retry.onclick = () =>
      window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts"));
    recover.onclick = () => {
      // Reuse the mounted Git Repository connection, including its existing
      // confirmation flow. Closing recovery returns to this untouched URL draft.
      const auth = accountContent?.querySelector<HTMLElement>(".github-auth");
      const parent = auth?.parentElement;
      if (!auth || !parent) {
        alert.textContent =
          "GitHub account controls are unavailable. Close this editor and retry Accounts.";
        alert.hidden = false;
        return;
      }
      const recovery = dialog(
        "GitHub repository accounts",
        "<div data-repository-auth></div>",
        recover,
      );
      compactEditor(
        recovery,
        "Back to repository URL; retain input and account",
      );
      recovery.querySelector("[data-repository-auth]")!.append(auth);
      recovery.addEventListener(
        "pr-sniper:dialog-closed",
        () => parent.append(auth),
        { once: true },
      );
      window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts"));
    };
    updateRepositoryAccounts = update;
    update();
    modal.addEventListener(
      "pr-sniper:dialog-closed",
      () => {
        invalidate();
        if (updateRepositoryAccounts === update)
          updateRepositoryAccounts = undefined;
      },
      { once: true },
    );
    modal.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      if (submitting) return;
      const accountId = selectedId;
      const actor = selected();
      const requestedRoute = routeGeneration;
      const read = ++generation;
      const repositoryInput = input.value;
      submitting = true;
      alert.hidden = true;
      update();
      try {
        if (repositoryProviderContext(repositoryInput) !== "github")
          throw "This repository provider is not supported. Use a GitHub repository URL.";
        if (repositoryAccountsState !== "ready" || !actor)
          throw "Choose a connected GitHub account in Accounts, then retry.";
        const resolved = await invoke<ResolvedRepository>(
          "resolve_provider_repository",
          {
            provider: "github",
            accountId,
            repository: repositoryInput,
          },
        );
        if (
          !modal.open ||
          !modal.isConnected ||
          app.closest("[hidden]") ||
          requestedRoute !== routeGeneration ||
          read !== generation
        )
          return;
        if (
          resolved.identity.id !== accountId ||
          (actor.connection_generation !== undefined &&
            actor.connection_generation !== resolved.account_generation)
        )
          throw "The selected GitHub connection changed. Retry to verify its current access; no repository was saved.";
        await addAndConfigure(accountId, resolved, modal, opener, repository);
      } catch (cause) {
        if (modal.open && read === generation) {
          submitting = false;
          alert.textContent = `Cannot resolve this repository or pull request using the selected GitHub account. ${reason(cause)}`;
          alert.hidden = false;
          if (repositorySessionFailure(cause))
            window.dispatchEvent(
              new Event("pr-sniper:refresh-provider-accounts"),
            );
        }
      } finally {
        if (read === generation) {
          submitting = false;
          update();
        }
      }
    };
  }

  function browseRepositories(account: GithubAccount, opener: HTMLElement) {
    const modal = dialog(
      `Browse repositories as ${account.login}`,
      `<p class="settings-hint">Acting GitHub account: ${escape(account.login)}.</p>
      <label>Repository owner<select data-owner aria-label="Repository owner" disabled><option value="">Choose an owner</option></select></label>
      <label>Find a repository<input type="search" data-owner-search placeholder="Search this owner..." disabled /></label>
      <p role="status">Loading available owners...</p><p role="alert" hidden></p><button data-retry hidden>Retry</button>
      <div data-owner-results class="native-repository-list"></div>
      <p class="settings-hint">Only repositories accessible to this account are listed. Selecting adds a disabled row and opens configuration.</p>`,
      opener,
    );
    compactEditor(modal, "Back to repositories");
    modal.classList.add("repository-editor");
    const owner = modal.querySelector<HTMLSelectElement>("[data-owner]")!;
    const search = modal.querySelector<HTMLInputElement>(
      "[data-owner-search]",
    )!;
    const status = modal.querySelector<HTMLElement>("[role=status]")!;
    const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
    const retry = modal.querySelector<HTMLButtonElement>("[data-retry]")!;
    const list = modal.querySelector<HTMLElement>("[data-owner-results]")!;
    let generation = 0;
    let results: { id: string; name: string }[] = [];
    let loaded = false;
    let resultsOwner = "";
    let incomplete = false;
    let selecting = false;
    const browserRoute = routeGeneration;
    const current = (read: number) =>
      modal.open &&
      modal.isConnected &&
      read === generation &&
      browserRoute === routeGeneration &&
      !app.closest("[hidden]");
    const connected = () =>
      githubAccounts.some(
        (a) => a.account_id === account.account_id && a.state === "connected",
      );
    const renderResults = () => {
      const filtered = results.filter((r) =>
        r.name.toLowerCase().includes(search.value.trim().toLowerCase()),
      );
      list.innerHTML = filtered
        .map(
          (r) =>
            `<button type="button" class="native-repository-row" data-pick="${escape(r.id)}"><span class="settings-row-copy"><strong>${escape(r.name.split("/")[1])}</strong><small>${escape(r.name)}</small></span><span>${repositories().some((saved) => saved.provider === "github" && saved.provider_account_id === account.account_id && saved.provider_repository_id === r.id) ? "Added" : "Add"}</span><span aria-hidden="true">›</span></button>`,
        )
        .join("");
      if (loaded && owner.value)
        status.textContent = incomplete
          ? `${results.length} accessible repositories shown. Repository discovery is incomplete; more may be unavailable.`
          : !results.length
            ? "No accessible repositories for this owner."
            : !filtered.length
              ? "No matching repositories. Try another name."
              : `${results.length} accessible repositories loaded.`;
      list
        .querySelectorAll<HTMLButtonElement>("[data-pick]")
        .forEach((button) => {
          button.onclick = async () => {
            if (selecting) return;
            selecting = true;
            const read = generation;
            const requestedRoute = routeGeneration;
            list
              .querySelectorAll<HTMLButtonElement>("button")
              .forEach((b) => (b.disabled = true));
            const selected = results.find((r) => r.id === button.dataset.pick)!;
            try {
              if (!connected())
                throw "Account disconnected. Reconnect in Accounts.";
              const resolved = await invoke<ResolvedRepository>(
                "resolve_provider_repository",
                {
                  provider: "github",
                  accountId: account.account_id,
                  repository: selected.name,
                },
              );
              if (
                !current(read) ||
                app.closest("[hidden]") ||
                requestedRoute !== routeGeneration
              )
                return;
              if (resolved.identity.id !== account.account_id)
                throw "wrong_identity";
              if (resolved.repository.id !== selected.id)
                throw "Repository identity changed. Refresh this owner and try again.";
              await addAndConfigure(
                account.account_id,
                resolved,
                modal,
                opener,
              );
            } catch (cause) {
              if (current(read)) showReadFailure(cause);
            } finally {
              selecting = false;
              if (current(read)) renderResults();
            }
          };
        });
    };
    const showWarnings = (warnings: RepositoryBrowseWarning[] = []) => {
      incomplete = warnings.length > 0;
      alert.hidden = !incomplete;
      retry.hidden = !incomplete;
      alert.textContent = [
        ...new Set(
          warnings.map((warning) => {
            const boundary = {
              repository_page: "Repository page",
              repository_metadata: "Repository metadata",
              pagination: "Repository pagination",
              organization_access: "Organization access",
            }[warning.boundary];
            return `${boundary} (page ${warning.page}): ${reason(warning.error)}`;
          }),
        ),
      ].join(" ");
    };
    const reconcileOwners = (
      owners: { login: string; kind: string }[],
      selected: string,
    ) => {
      owner.innerHTML =
        option("", "Choose an owner", selected) +
        owners
          .map((o) => option(o.login, `${o.login} (${o.kind})`, selected))
          .join("") +
        (selected && !owners.some((o) => o.login === selected)
          ? option(
              selected,
              `${selected} (${incomplete ? "not in loaded catalog" : "unavailable"})`,
              selected,
            )
          : "");
      owner.disabled = false;
    };
    const showReadFailure = (cause: unknown) => {
      const session = repositorySessionFailure(cause);
      const error = session ? session.error : cause;
      const sameBinding = !session || session.accountId === account.account_id;
      if (
        !connected() ||
        (sameBinding &&
          (error === "wrong_identity" ||
            error === "authentication_changed" ||
            error === "signed_out" ||
            readFailureMissingScope(error) ||
            (session && (error === "configuration" || error === "broken_cli"))))
      ) {
        loaded = false;
        results = [];
        resultsOwner = "";
        owner.innerHTML = option("", "Choose an owner", "");
        owner.disabled = true;
        search.disabled = true;
        list.replaceChildren();
      }
      incomplete = true;
      status.textContent = loaded
        ? "Lookup failed. Previously loaded results remain visible; discovery is incomplete."
        : "Repository browsing unavailable.";
      alert.textContent = sameBinding
        ? reason(cause)
        : "The lookup returned a failure for another account. Retry this selected account.";
      alert.hidden = false;
      retry.hidden = false;
    };
    const load = async () => {
      const read = ++generation;
      const ownerLogin = owner.value;
      const retaining = loaded && resultsOwner === ownerLogin;
      if (!retaining) {
        loaded = false;
        results = [];
        list.replaceChildren();
      }
      search.disabled = !loaded;
      alert.hidden = true;
      retry.hidden = true;
      status.textContent = ownerLogin
        ? retaining
          ? "Refreshing repositories; previously loaded results remain visible..."
          : "Loading all accessible repositories for this owner..."
        : "Loading available owners...";
      try {
        if (!connected()) throw "Account disconnected. Reconnect in Accounts.";
        if (!ownerLogin) {
          const result = await invoke<{
            identity: { id: string };
            owners: { login: string; kind: string }[];
            warnings?: RepositoryBrowseWarning[];
          }>("list_provider_repository_owners", {
            provider: "github",
            accountId: account.account_id,
          });
          if (!current(read)) return;
          if (result.identity.id !== account.account_id) throw "wrong_identity";
          if (!connected()) throw "signed_out";
          showWarnings(result.warnings);
          reconcileOwners(result.owners, "");
          status.textContent = incomplete
            ? "Some owners may be unavailable. Choose a loaded owner or retry discovery."
            : "Choose the personal account or an available organization.";
        } else {
          const result = await invoke<{
            identity: { id: string };
            owners: { login: string; kind: string }[];
            repositories: { id: string; name: string }[];
            warnings?: RepositoryBrowseWarning[];
          }>("list_provider_repositories", {
            provider: "github",
            accountId: account.account_id,
            owner: ownerLogin,
          });
          if (!current(read)) return;
          if (result.identity.id !== account.account_id) throw "wrong_identity";
          if (!connected()) throw "signed_out";
          if (
            result.repositories.some(
              (r) =>
                r.name.split("/")[0].toLowerCase() !== ownerLogin.toLowerCase(),
            )
          )
            throw "GitHub returned an unexpected account or owner. Retry.";
          loaded = true;
          resultsOwner = ownerLogin;
          results = result.repositories;
          search.disabled = false;
          showWarnings(result.warnings);
          reconcileOwners(result.owners, ownerLogin);
          renderResults();
        }
      } catch (cause) {
        if (current(read)) showReadFailure(cause);
      }
    };
    owner.onchange = () => {
      search.value = "";
      void load();
    };
    search.oninput = renderResults;
    retry.onclick = () => void load();
    void load();
  }

  function repositoryDialog(
    repository: ConfiguredRepository,
    opener: HTMLElement,
  ) {
    const schedule = saved.defaults.schedule;
    const pendingPull = pendingPullRequests.get(repository.id);
    const modal = dialog(
      `Settings for ${repository.name}`,
      `<section class="repository-identity"><h3>${escape(repository.name)}</h3><p class="settings-hint">${escape(repository.provider === "github" ? `GitHub acting account: ${githubAccounts.find((account) => account.account_id === repository.provider_account_id)?.login ?? (repository.provider_account_id ? "Saved account unavailable" : "No account selected")}.` : "Azure DevOps account binding is not available in this build.")}</p></section>
        <section class="repository-group"><label class="repository-check"><input type="checkbox" role="switch" aria-label="Monitor ${escape(repository.name)}" data-repository-enabled ${repository.enabled || (pendingSetup.has(repository.id) && !setupMonitoringOff.has(repository.id)) ? "checked" : ""} /><span>Monitor this repository</span></label><p data-repository-monitoring-state role="status"></p><p class="settings-hint" data-repository-monitoring-detail></p></section>
        <section class="repository-group"><h2>Pull requests to watch</h2>
        <p class="settings-hint">Save authorizes all currently open and future matching pull requests. Eligible reviews start automatically; global monitoring off, pause and repository disablement still apply. Adding this row alone does not start monitoring.</p>
        <label for="repository-reviewer-trigger">Reviewer requests</label><select id="repository-reviewer-trigger" data-reviewer-trigger><option value="inherit">Use default (${saved.defaults.reviewer_assignment ? "on" : "off"})</option><option value="on">Include PRs explicitly requesting the acting account</option><option value="off">Do not admit through reviewer requests</option></select>
        <p class="settings-hint">Reviewer requests independently admit older or unwatched PRs. Once admitted, work stays tracked until verified closure or merge. Disablement and execution permissions still apply.</p></section>
        <section class="repository-group"><h2>Automation overrides</h2>
        <p class="settings-hint">Save repository authorizes automatic read-only reviews, including forks and later revisions. Review execution does not enable publication, approval or merge.</p>
        <label for="repository-publication">Comment publication</label><select id="repository-publication" data-publication><option value="inherit">Use default (${saved.defaults.automatic_comment_publication ? "automatic" : "local-only"})</option><option value="automatic">Publish automatically after revalidation</option><option value="manual">Off: retain normal findings locally</option></select>
        <p class="settings-hint">The assignment must also allow Comment. Uses the repository's GitHub account, never the Copilot account. This cannot approve or merge a pull request.</p>
        </section><section class="repository-group"><div class="section-actions"><h2>Agents on this repository</h2><button class="primary" data-assign-agent ${agents().length ? "" : "disabled"}>Assign agent</button></div>
        <div class="assignment-list"></div>
        ${agents().length ? "" : '<p class="settings-hint">Create an agent first, on the Agents tab.</p>'}
        <p class="settings-hint">Each assignment receives its own normal pass. Newly assigned Agents get missing work at the next global scan; adding one does not start a scan. Primary routes top-level mentions of the acting account, without enabling Approve or Merge.</p>
        </section><section class="repository-group"><div class="section-actions"><h2>People you watch</h2><button data-add-people>Add people</button></div>
        <div class="watchlist"></div>
        <p class="settings-hint">Optional. A nonempty effective watched-author filter qualifies those authors. An empty effective author filter means all authors. Pull requests requesting the signed-in account also qualify when the reviewer-request trigger is enabled. Exact GitHub login, no wildcards.</p>
        </section><section class="repository-group" data-global-schedule><h2>Saved global schedule</h2><p>${schedule.kind === "cron" ? `<code>${escape(schedule.expression)}</code>` : `Every ${schedule.minutes} minutes (saved legacy schedule)`} / ${escape(schedule.timezone)}</p><p class="settings-hint">${schedule.kind === "cron" ? "One schedule scans enabled repositories. Change it in Preferences;" : "Polling is blocked until you choose a global five-field cron schedule in Preferences. The saved legacy interval is retained;"} repository and Agent assignments have no separate polling controls.</p></section>
        <details class="repository-group"><summary>Repository and connection</summary><dl class="repository-binding-details"><dt>Account ID</dt><dd>${escape(repository.provider_account_id ?? "Unbound")}</dd><dt>Repository ID</dt><dd>${escape(repository.provider_repository_id ?? "Not verified")}</dd></dl><div class="settings-actions"><button id="rename-repository">Edit repository</button><button data-unbind-repository ${repository.provider_account_id ? "" : "disabled"}>Unbind account</button><button id="remove-repository">Remove repository</button></div><div class="connection"></div></details>
        <p class="settings-hint">Save applies only this repository. Back retains its draft for this session; Cancel discards it. Earlier assignment saves stay applied.</p>
        <p class="settings-hint" role="status" data-explicit-pr-status ${pendingPull ? "" : "hidden"}>${pendingPull ? `PR #${pendingPull.number} will be queued after valid Save, even outside watch filters. Pause, repository disablement, account/Agent availability and capacity still apply.` : ""}</p>
        <p role="alert" data-resource-error hidden></p><div class="resource-actions"><button class="primary" data-save-repository>Save repository</button><button data-cancel-repository aria-label="Cancel repository changes">Cancel</button></div>`,
      opener,
    );
    compactEditor(
      modal,
      "Back to repositories; retain unsaved repository changes",
    );
    modal.classList.add("repository-editor");
    const monitoring = modal.querySelector<HTMLInputElement>(
      "[data-repository-enabled]",
    )!;
    const monitoringState = modal.querySelector<HTMLElement>(
      "[data-repository-monitoring-state]",
    )!;
    const monitoringDetail = modal.querySelector<HTMLElement>(
      "[data-repository-monitoring-detail]",
    )!;
    const updateMonitoring = () => {
      if (!modal.isConnected) return;
      const committed =
        saved.repositories?.find((r) => r.id === repository.id) ?? repository;
      if (committed.enabled) {
        pendingSetup.delete(repository.id);
        setupMonitoringOff.delete(repository.id);
      }
      if (!pendingSetup.has(repository.id))
        monitoring.checked = committed.enabled;
      monitoringState.textContent = committed.enabled ? "Enabled" : "Disabled";
      monitoringState.dataset.monitoringState = committed.enabled
        ? "enabled"
        : "disabled";
      monitoringDetail.textContent = pendingSetup.has(repository.id)
        ? monitoring.checked
          ? "Starts after you save valid configuration. Turn this off to keep the repository disabled."
          : "Stays disabled when you save configuration."
        : `${repositoryMonitoringDetail(committed)}. This control saves monitoring immediately using saved configuration; other fields stay in your draft. Global Monitoring is separate.`;
    };
    const refreshSavedMonitoring = () => {
      updateMonitoring();
      refreshRepositoryRows?.();
    };
    updateMonitoring();
    void refreshRepositoryAutomation(updateMonitoring);
    monitoring.onchange = async () => {
      if (pendingSetup.has(repository.id)) {
        repository.enabled = false;
        if (monitoring.checked) setupMonitoringOff.delete(repository.id);
        else setupMonitoringOff.add(repository.id);
        updateMonitoring();
        changed();
        return;
      }
      const enabled = monitoring.checked;
      monitoring.checked =
        saved.repositories?.find((r) => r.id === repository.id)?.enabled ??
        false;
      const alert = modal.querySelector<HTMLElement>("[data-resource-error]")!;
      alert.hidden = true;
      try {
        await toggleRepositoryMonitoring(repository.id, enabled, modal);
        monitoring.checked = repository.enabled;
        updateMonitoring();
        refreshRepositoryRows?.();
        void refreshRepositoryAutomation(updateMonitoring);
      } catch (cause) {
        alert.textContent = reason(cause);
        alert.hidden = false;
        updateMonitoring();
      }
    };
    const reviewerTrigger = modal.querySelector<HTMLSelectElement>(
      "[data-reviewer-trigger]",
    )!;
    reviewerTrigger.value =
      repository.overrides?.reviewer_assignment === undefined
        ? "inherit"
        : repository.overrides.reviewer_assignment
          ? "on"
          : "off";
    reviewerTrigger.onchange = () => {
      repository.overrides ??= {};
      if (reviewerTrigger.value === "inherit")
        delete repository.overrides.reviewer_assignment;
      else
        repository.overrides.reviewer_assignment =
          reviewerTrigger.value === "on";
      changed();
    };
    modal.querySelector<HTMLButtonElement>("[data-save-repository]")!.onclick =
      async () => {
        try {
          const proposed = clone(repository);
          proposed.enabled = monitoring.checked;
          await commitResource(repositoryEdit(repository, proposed), modal);
          pendingSetup.delete(repository.id);
          setupMonitoringOff.delete(repository.id);
          const requestedPull = pendingPullRequests.get(repository.id);
          if (requestedPull) {
            const committed = saved.repositories?.find(
              (r) => r.id === repository.id,
            );
            if (
              !committed ||
              committed.provider_account_id !== requestedPull.accountId ||
              committed.provider_repository_id !== requestedPull.repositoryId
            )
              throw "Configuration saved, but the requested PR's repository account changed. Enter its URL again under the saved account.";
            const saveButton = modal.querySelector<HTMLButtonElement>(
              "[data-save-repository]",
            )!;
            saveButton.disabled = true;
            modal.dataset.closeLocked = "true";
            try {
              const admission = await invoke<{
                queued: boolean;
                message: string;
              }>("admit_explicit_pull_request", {
                expected: committed,
                number: requestedPull.number,
                accountGeneration: requestedPull.generation,
              });
              const status = modal.querySelector<HTMLElement>(
                "[data-explicit-pr-status]",
              )!;
              status.textContent = admission.message;
              status.hidden = false;
              modal.querySelector<HTMLElement>(
                "[data-resource-error]",
              )!.hidden = true;
              if (admission.queued) pendingPullRequests.delete(repository.id);
            } catch (cause) {
              throw `Configuration saved. PR #${requestedPull.number} intake needs attention; retry Save to reuse any existing work. ${reason(cause)}`;
            } finally {
              saveButton.disabled = false;
              delete modal.dataset.closeLocked;
            }
            return;
          }
          modal.close();
          render();
        } catch (cause) {
          const alert = modal.querySelector<HTMLElement>(
            "[data-resource-error]",
          )!;
          alert.textContent = reason(cause);
          alert.hidden = false;
        }
      };
    modal.querySelector<HTMLButtonElement>(
      "[data-cancel-repository]",
    )!.onclick = () => {
      acceptResource(draft, clone(saved), repositoryEdit(repository));
      if (!repositories().some((item) => item.id === repository.id))
        opener.dataset.focusKey = `repository:${repository.name}:settings`;
      modal.close();
      render();
    };
    renderAssignments();
    renderWatchlist();
    const publication =
      modal.querySelector<HTMLSelectElement>("[data-publication]")!;
    publication.value =
      repository.overrides?.automatic_comment_publication === undefined
        ? "inherit"
        : repository.overrides.automatic_comment_publication
          ? "automatic"
          : "manual";
    publication.onchange = () => {
      repository.overrides ??= {};
      if (publication.value === "inherit")
        delete repository.overrides.automatic_comment_publication;
      else
        repository.overrides.automatic_comment_publication =
          publication.value === "automatic";
      changed();
    };
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
    modal.querySelector<HTMLButtonElement>("[data-assign-agent]")!.onclick = (
      event,
    ) =>
      assignAgentDialog(
        event.currentTarget as HTMLButtonElement,
        repository,
        () => {
          renderAssignments();
          refreshSavedMonitoring();
        },
      );
    modal.querySelector<HTMLButtonElement>("[data-add-people]")!.onclick = (
      event,
    ) =>
      addPersonDialog(
        event.currentTarget as HTMLButtonElement,
        repository,
        () => {
          renderWatchlist();
          refreshSavedMonitoring();
        },
      );
    modal.querySelector<HTMLButtonElement>("#rename-repository")!.onclick =
      () => {
        modal.close();
        editRepository(opener, repository);
      };
    modal.querySelector<HTMLButtonElement>("#remove-repository")!.onclick = (
      event,
    ) => {
      const confirm = dialog(
        "Remove repository?",
        `<p>Remove ${escape(repository.name)} and its assignments from saved settings? Completed evidence is retained.</p><p role="alert" hidden></p><button class="primary" id="confirm-remove">Remove from settings</button>`,
        event.currentTarget as HTMLButtonElement,
      );
      compactEditor(confirm, "Back to repository; keep saved configuration");
      confirm.querySelector<HTMLButtonElement>("#confirm-remove")!.onclick =
        async () => {
          try {
            await commitResource(repositoryEdit(repository, null), confirm);
            confirm.close();
            modal.close();
            render();
          } catch (cause) {
            const alert = confirm.querySelector<HTMLElement>("[role=alert]")!;
            alert.textContent = reason(cause);
            alert.hidden = false;
          }
        };
    };
    modal.querySelector<HTMLButtonElement>(
      "[data-unbind-repository]",
    )!.onclick = (event) => {
      const persisted = saved.repositories?.find((r) => r.id === repository.id);
      if (!persisted || !sameResource(repository, persisted)) {
        const alert = modal.querySelector<HTMLElement>(
          "[data-resource-error]",
        )!;
        alert.textContent =
          "Save or Cancel repository changes before unbinding. Your draft and saved repository have not been changed.";
        alert.hidden = false;
        return;
      }
      const confirm = dialog(
        "Unbind repository account?",
        `<p>Unbind ${escape(repository.name)} from its acting account? Provider reads and actions require an explicit binding again. Assignments, permissions and completed evidence are retained.</p><p role="alert" hidden></p><button class="primary" data-confirm-unbind>Unbind account</button>`,
        event.currentTarget as HTMLButtonElement,
      );
      compactEditor(confirm, "Back to repository; keep the acting account");
      confirm.querySelector<HTMLButtonElement>(
        "[data-confirm-unbind]",
      )!.onclick = async () => {
        const next = clone(persisted);
        delete next.provider_account_id;
        delete next.provider_repository_id;
        try {
          await commitResource(repositoryEdit(repository, next), confirm);
          confirm.close();
          modal.close();
          render();
        } catch (cause) {
          const alert = confirm.querySelector<HTMLElement>("[role=alert]")!;
          alert.textContent = reason(cause);
          alert.hidden = false;
        }
      };
    };

    function renderAssignments(opener = document.activeElement) {
      const list = modal.querySelector<HTMLElement>(".assignment-list")!;
      const restoreFocus = rememberControl(
        list,
        modal.querySelector<HTMLElement>("[data-assign-agent]")!,
        opener,
      );
      const assignments = repository.assignments ?? [];
      if (!assignments.length) {
        list.innerHTML =
          '<p class="settings-empty">No agents assigned. This repository is watched but nothing reviews it yet.</p>';
        restoreFocus();
        return;
      }
      list.innerHTML = "";
      for (const assignment of assignments) {
        const agent = agents().find((a) => a.id === assignment.agent_id);
        const row = document.createElement("div");
        row.className = "assignment-row";
        row.innerHTML = `<div><strong>${escape(agent?.name ?? "Deleted agent")}</strong><p>${primaryAssignmentId(repository) === assignment.id ? `Primary${assignments.length === 1 ? " (automatic)" : ""} \u00b7 ` : ""}${assignment.comment ? "Comments" : "Silent"}${assignment.actions?.approve ? " \u00b7 Approve opted in" : ""}${assignment.actions?.merge ? " \u00b7 Merge opted in (primary only)" : ""}</p><p>${escape(agent?.model ?? "Repair the shared Agent reference")} / one normal pass</p></div><button data-edit>Edit</button><button data-remove>Remove</button>`;
        for (const action of ["edit", "remove"])
          row.querySelector<HTMLElement>(`[data-${action}]`)!.dataset.focusKey =
            `assignment:${assignment.id}:${action}`;
        row.querySelector<HTMLButtonElement>("[data-edit]")!.onclick = (
          event,
        ) =>
          assignAgentDialog(
            event.currentTarget as HTMLButtonElement,
            repository,
            () => {
              renderAssignments();
              refreshSavedMonitoring();
            },
            assignment,
          );
        row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick = (
          event,
        ) => {
          repository.assignments = assignments.filter((a) => a !== assignment);
          if (repository.primary_assignment_id === assignment.id)
            delete repository.primary_assignment_id;
          changed();
          renderAssignments(event.currentTarget as HTMLButtonElement);
        };
        list.append(row);
      }
      restoreFocus();
    }
    function renderWatchlist(opener = document.activeElement) {
      const list = modal.querySelector<HTMLElement>(".watchlist")!;
      const restoreFocus = rememberControl(
        list,
        modal.querySelector<HTMLElement>("[data-add-people]")!,
        opener,
      );
      const people = repository.watched_authors ?? [];
      if (!people.length) {
        list.innerHTML =
          '<p class="settings-empty">No people added for this repository. Inherited watched authors still apply; if the effective author filter is empty, all authors qualify under the saved configuration. Reviewer requests qualify when that trigger is enabled.</p>';
        restoreFocus();
        return;
      }
      list.innerHTML = "";
      for (const person of people) {
        const row = document.createElement("div");
        row.className = "watchlist-row";
        row.innerHTML = `<span><span>@${escape(person.login)}</span><small>GitHub ID ${escape(person.id)}</small></span><button data-remove aria-label="Remove ${escape(person.login)}">Remove</button>`;
        row.querySelector<HTMLElement>("[data-remove]")!.dataset.focusKey =
          `person:${person.id}:remove`;
        row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick = (
          event,
        ) => {
          repository.watched_authors = people.filter((p) => p !== person);
          changed();
          renderWatchlist(event.currentTarget as HTMLButtonElement);
        };
        list.append(row);
      }
      restoreFocus();
    }
  }

  function assignAgentDialog(
    opener: HTMLElement,
    repository: ConfiguredRepository,
    onSaved: () => void,
    existing?: Assignment,
  ) {
    const assignmentId = existing?.id ?? newIdentity();
    const sole =
      (repository.assignments?.length ?? 0) + (existing ? 0 : 1) === 1;
    const isPrimary = sole || primaryAssignmentId(repository) === assignmentId;
    const modal = dialog(
      existing ? "Edit assignment" : "Assign agent",
      `<form><label>Agent<select name="agent" aria-label="Agent" required>${option("", "Choose an Agent", existing?.agent_id ?? "")}${agents()
        .map((a) => option(a.id, a.name, existing?.agent_id ?? ""))
        .join("")}</select></label>
        <label class="repository-check"><input type="checkbox" name="primary" ${isPrimary ? "checked" : ""} ${sole ? "disabled" : ""} /><span>Primary<small>${sole ? "The sole assignment is primary automatically." : "At most one explicit primary per repository. Uncheck to leave none."}</small></span></label>
        <div class="permission-row"><label><input type="checkbox" name="comment" ${existing?.comment ? "checked" : ""} /><span>Comment<small>Allow comment publication, independently of approval and merge.</small></span></label><label><input type="checkbox" name="approve" ${existing?.actions?.approve ? "checked" : ""} /><span>Approve<small>Opt in to the acting GitHub account's approval after current Agent clearance and a primary final full review. Never self-approval or policy bypass.</small></span></label><label><input type="checkbox" name="merge" ${existing?.actions?.merge ? "checked" : ""} ${isPrimary ? "" : "disabled"} /><span>Merge<small>Independent opt-in; primary only, after final review, green CI and verified provider policies. Does not require Approve or personal acknowledgment.</small></span></label></div>
        <p class="settings-hint">Saving this assignment authorizes the selected Agent to review this repository's saved current and future matching pull requests and later revisions under its review-start setting. Primary selection never enables publication permissions. Polling is configured globally in Preferences. Saving commits this repository, not unrelated drafts.</p><p role="alert" hidden></p><div class="resource-actions"><button class="primary">${existing ? "Save assignment" : "Assign agent"}</button><button type="button" data-cancel-resource>Cancel</button></div></form>`,
      opener,
    );
    resourceEditor(modal);
    modal.classList.add("repository-editor", "assignment-editor");
    const primary = modal.querySelector<HTMLInputElement>("[name=primary]")!;
    const merge = modal.querySelector<HTMLInputElement>("[name=merge]")!;
    primary.onchange = () => {
      merge.disabled = !primary.checked;
    };
    modal.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
      try {
        const agentId =
          modal.querySelector<HTMLSelectElement>("[name=agent]")!.value;
        if (!agentId) throw "Choose an agent.";
        const value: Assignment = {
          id: assignmentId,
          agent_id: agentId,
          schedule: existing?.schedule ?? clone(saved.defaults.schedule),
          comment:
            modal.querySelector<HTMLInputElement>("[name=comment]")!.checked,
          approve: existing?.approve ?? false,
          actions: {
            approve:
              modal.querySelector<HTMLInputElement>("[name=approve]")!.checked,
            merge: merge.checked,
          },
        };
        const next = clone(repository);
        if (existing)
          next.assignments = (next.assignments ?? []).map((a) =>
            a.id === value.id ? value : a,
          );
        else (next.assignments ??= []).push(value);
        if (primary.checked && !sole) next.primary_assignment_id = value.id;
        else if (!primary.checked && next.primary_assignment_id === value.id)
          delete next.primary_assignment_id;
        await commitResource(repositoryEdit(repository, next), modal);
        Object.assign(repository, next);
        modal.close();
        onSaved();
      } catch (cause) {
        alert.textContent = typeof cause === "string" ? cause : reason(cause);
        alert.hidden = false;
      }
    };
  }

  function addPersonDialog(
    opener: HTMLElement,
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
      `<form class="person-lookup"><label>Acting GitHub account<select name="account" required>${option("", "Choose a GitHub account", repository.provider_account_id ?? "")}${availableAccounts.map((account) => option(account.account_id, `${account.login} (${account.account_id})`, repository.provider_account_id ?? "")).join("")}</select></label><label>GitHub login<input name="login" placeholder="octocat" autocomplete="off" required /></label><p class="settings-hint">${availableAccounts.length ? "Looks up the exact login through the selected GitHub account and stores its stable identity. No wildcards." : `No connected GitHub account is available. Connect one in ${accountSection}.`}</p><p role="alert" hidden></p><div class="resource-actions"><button type="submit" class="primary" ${availableAccounts.length ? "" : "disabled"}>Add person</button><button type="button" data-cancel-resource>Cancel</button></div></form>`,
      opener,
    );
    resourceEditor(picker);
    picker.classList.add("repository-editor");
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
            "rate_limited",
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

  function updateStartup() {
    const checkbox = content.querySelector<HTMLInputElement>("#login");
    const status = content.querySelector<HTMLElement>("#login-status");
    const error = content.querySelector<HTMLElement>("#login-error");
    if (!checkbox || !status || !error) return;
    error.textContent = startupError;
    error.hidden = !startupError;
    checkbox.checked = saved.launch_at_login;
    checkbox.disabled =
      startupPending ||
      snapshot.isolated ||
      snapshot.login_registration === null;
    status.textContent = startupPending
      ? "Updating startup request and reading native registration..."
      : `Saved request: ${saved.launch_at_login ? "On" : "Off"}. Registration: ${snapshot.login_registration ?? "unavailable"}. ${
          snapshot.isolated
            ? `Isolated development run: changing ${isWindows ? "Windows startup apps" : "macOS login items"} is disabled.`
            : `Registration is not effective ${platformName} launch state; ${startupSettingsName} can disable a registered entry.`
        }`;
  }

  function renderPreferences() {
    const schedule = draft.defaults.schedule;
    content.innerHTML = `<div class="preferences-view">
      <div class="preferences-boundary"><h2>Saved preferences</h2><span>Save to apply</span></div>
      <div class="settings-group preferences-saved"><fieldset aria-label="Global polling and capacity"><legend>Global polling and capacity</legend>
      <label class="preferences-capacity">AI capacity<input id="global-capacity" type="number" min="1" max="4294967295" step="1" value="${draft.capacity}" aria-describedby="capacity-guidance" /></label>
      <p id="capacity-guidance" class="settings-hint">Simultaneous jobs on this computer: normal passes, final primary reviews, replies and mentions. Default 4; any whole number from 1 to 4294967295. Saved Agents and queued PRs are not limited.</p>
      <p class="preferences-warning">Lowering capacity stops surplus AI work and queues fresh attempts. Completed evidence and provider receipts remain.</p>
      <div class="preferences-schedule">
      <label>Cron expression<input id="global-cron" value="${escape(schedule.kind === "cron" ? schedule.expression : "")}" placeholder="*/15 * * * *" /></label>
      <label>Schedule helper<select id="cron-helper"><option value="">Custom five-field expression</option><option value="*/15 * * * *">Every 15 minutes</option><option value="0 * * * *">Every hour</option><option value="0 9 * * MON-FRI">Weekdays at 09:00</option></select></label>
      <label>Time zone<input id="global-timezone" value="${escape(schedule.timezone)}" /></label>
      </div><p class="settings-hint">Five fields: minute, hour, day, month, weekday. Evaluated in this IANA time zone, including its daylight-saving rules. One global scan covers enabled, configured repositories. Shared AI capacity drains admitted work independently of polling.${schedule.kind === "interval" ? ` Saved legacy interval: ${schedule.minutes} minutes. Polling is blocked until you explicitly choose a cron expression; no automatic conversion.` : ""}</p></fieldset>
      <fieldset aria-label="Review execution"><legend>Review execution</legend><p class="settings-hint">Eligible reviews run automatically through shared AI capacity. Pause automation or disable a repository to stop new work. Publication, approval and merge have separate permissions.</p></fieldset>
      <fieldset aria-label="Comment publication"><legend>Comment publication</legend><label class="setting-row"><span>Publish review comments automatically<small>Default for assigned repositories that allow Comment. Revalidates revision, permissions and eligibility before publication. Never approves or merges.</small></span><input id="automatic-publication" type="checkbox" role="switch" ${draft.defaults.automatic_comment_publication ? "checked" : ""} /></label></fieldset>
      <p class="settings-hint preferences-permissions">Approve and Merge remain separate repository-assignment permissions, never global grants.</p></div>
      <div class="preferences-boundary"><h2>Immediate controls</h2><span>Applied separately</span></div>
      <p class="settings-hint">Pause, notification opt-in and startup commit immediately. Save preferences and Reset changes do not apply or undo them.</p>
      <div class="settings-group" id="automation-settings"></div>
      <div class="settings-group" id="notification-settings"></div>
      <div class="settings-group"><fieldset aria-label="Startup"><legend>Startup</legend><label class="setting-row"><span>Open PR Sniper at login<small>Changes the saved request and native registration immediately, not through Save preferences.</small></span><input id="login" type="checkbox" role="switch" aria-describedby="login-status" disabled /></label><p id="login-status" class="settings-hint" role="status"></p><p id="login-error" role="alert" hidden></p></fieldset></div>
      <div class="settings-group"><fieldset aria-label="Status and recovery"><legend>Status and recovery</legend><p class="settings-hint">Status keeps notification history, schedule health and pending-operation recovery reachable. Uncertain provider writes still need reconciliation.</p><button id="preferences-status" type="button">Open status and recovery</button>
      <p class="settings-hint">Settings and logs live in your ${isWindows ? "Windows local application-data" : "macOS app-support"} folder. Diagnostics shows redacted host events, not tokens.</p><button id="diagnostics" type="button">Open redacted diagnostics</button>
      <details id="preferences-readiness" class="preferences-readiness"><summary>Saved setup readiness</summary><p class="settings-hint" data-readiness role="status">Reading saved-resource readiness...</p></details></fieldset></div></div>`;
    updateStartup();
    const cron = content.querySelector<HTMLInputElement>("#global-cron")!;
    const helper = content.querySelector<HTMLSelectElement>("#cron-helper")!;
    const syncHelper = () => {
      helper.value = [...helper.options].some(
        (option) => option.value === cron.value,
      )
        ? cron.value
        : "";
    };
    syncHelper();
    const timezone =
      content.querySelector<HTMLInputElement>("#global-timezone")!;
    const updateSchedule = () => {
      draft.defaults.schedule = {
        kind: "cron",
        expression: cron.value,
        timezone: timezone.value,
      };
      syncHelper();
      changed();
    };
    cron.oninput = updateSchedule;
    timezone.oninput = () => {
      draft.defaults.schedule.timezone = timezone.value;
      changed();
    };
    content.querySelector<HTMLSelectElement>("#cron-helper")!.onchange = (
      event,
    ) => {
      const expression = (event.target as HTMLSelectElement).value;
      if (expression) {
        cron.value = expression;
        updateSchedule();
      }
    };
    content.querySelector<HTMLInputElement>("#global-capacity")!.oninput = (
      event,
    ) => {
      draft.capacity = Number((event.target as HTMLInputElement).value);
      changed();
    };
    const readiness = content.querySelector<HTMLElement>("[data-readiness]")!;
    void savedResources().then(
      ({ readiness: state }) => {
        if (!readiness.isConnected) return;
        const issues = [
          ...state.issues,
          ...state.repositories.flatMap((r) => r.issues),
        ];
        readiness.textContent = `${state.configuration_ready ? "Saved configuration is complete." : `Saved setup incomplete: ${issues.join(" ")}`} Account verification and model access remain separate requirements. Repository Save authorizes monitoring. Unsaved fields do not affect readiness.`;
      },
      (cause) => {
        if (readiness.isConnected)
          readiness.textContent = `Saved-resource readiness unavailable: ${reason(cause)}`;
      },
    );
    mountNotificationSettings(
      content.querySelector<HTMLElement>("#notification-settings")!,
      notificationView,
    );
    mountAutomation(
      content.querySelector<HTMLElement>("#automation-settings")!,
      undefined,
      { preferences: true, view: automationView },
    );
    content
      .querySelectorAll<HTMLDetailsElement>("details[id]")
      .forEach((details) => {
        details.open = preferenceDisclosures.has(details.id);
        details.ontoggle = () => {
          if (!details.isConnected) return;
          if (details.open) preferenceDisclosures.add(details.id);
          else preferenceDisclosures.delete(details.id);
        };
      });
    content.querySelector<HTMLInputElement>(
      "#automatic-publication",
    )!.onchange = (event) => {
      draft.defaults.automatic_comment_publication = (
        event.target as HTMLInputElement
      ).checked;
      changed();
    };
    content.querySelector<HTMLInputElement>("#login")!.onchange = async (
      event,
    ) => {
      if (startupPending) return;
      const checkbox = event.target as HTMLInputElement;
      const enabled = checkbox.checked;
      const errors: string[] = [];
      startupPending = true;
      startupError = "";
      updateStartup();
      try {
        await invoke("save_login", { enabled });
      } catch (cause) {
        errors.push(
          typeof cause === "string"
            ? cause
            : `Startup preference change could not complete. Check ${startupSettingsName} and the saved request.`,
        );
      } finally {
        try {
          const fresh = await invoke<Snapshot>("snapshot");
          if (fresh.settings) {
            draft.launch_at_login = fresh.settings.launch_at_login;
            saved.launch_at_login = fresh.settings.launch_at_login;
          }
          snapshot = { ...fresh, doctrine_catalog: snapshot.doctrine_catalog };
          if (!fresh.settings) snapshot.login_registration = null;
          if (fresh.error) errors.push(fresh.error);
          else if (!fresh.settings)
            errors.push(
              "Startup settings unavailable; the saved request is unconfirmed.",
            );
        } catch {
          snapshot.login_registration = null;
          errors.push(
            `Could not reload startup registration. Check ${startupSettingsName}.`,
          );
        }
        startupPending = false;
        startupError = errors.join("\n");
        updateStartup();
        changed();
        if (errors.length) showError(errors.join("\n"));
      }
    };
    content.querySelector<HTMLButtonElement>("#preferences-status")!.onclick =
      async (event) => {
        (event.currentTarget as HTMLButtonElement).focus({
          preventScroll: true,
        });
        try {
          await invoke("panel_navigate", {
            route: { tab: "settings", detail: { type: "status" } },
          });
        } catch (cause) {
          showError(
            typeof cause === "string"
              ? cause
              : "Could not open status and recovery.",
          );
        }
      };
    content.querySelector<HTMLButtonElement>("#diagnostics")!.onclick = async (
      event,
    ) => {
      (event.currentTarget as HTMLButtonElement).focus({ preventScroll: true });
      try {
        await invoke("open_diagnostics");
      } catch {
        showError("Could not open diagnostics.");
      }
    };
  }

  save.onclick = async () => {
    if (busy || !preferencesDirty()) return;
    dialogs.closeAll();
    clearError();
    try {
      await commitResource({
        kind: "preferences",
        expected: clone(globalPreferences(saved)),
        value: clone(globalPreferences(draft)),
      });
      conflict = false;
    } catch (cause) {
      const message = reason(cause);
      if (message.startsWith("Resource changed in another window")) {
        conflict = true;
        showError(
          `${message} Your unsaved changes are still here. Discard the draft and reload only when you are ready to replace them with the latest saved settings.`,
        );
      } else {
        showError(message);
      }
    } finally {
      render();
    }
  };
  reload.onclick = () => reloadSettings(false);
  reloadCatalog.onclick = () => reloadSettings(true);
  async function reloadSettings(catalogOnly: boolean) {
    if (busy || !(catalogOnly ? catalogConflict : conflict)) return;
    if (catalogOnly && dialogs.hasOpen()) {
      showError(
        "Close open editors before reloading Agents and doctrines. Your drafts are still available.",
      );
      return;
    }
    if (!catalogOnly) dialogs.closeAll();
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
      if (catalogOnly) {
        for (const settings of [saved, draft]) {
          settings.agents = clone(state.settings.agents ?? []);
          settings.doctrines = clone(state.settings.doctrines ?? []);
          settings.doctrine_catalog_version =
            state.settings.doctrine_catalog_version;
          settings.doctrine_reset = state.settings.doctrine_reset
            ? clone(state.settings.doctrine_reset)
            : undefined;
        }
      } else {
        saved = clone(state.settings);
        draft = clone(saved);
        conflict = false;
      }
      catalogConflict = false;
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
  }
  reset.onclick = () => {
    if (!busy) {
      draft = clone(saved);
      clearError();
      render();
    }
  };
  async function load() {
    const requestRevision = revision;
    try {
      const state = await invoke<Snapshot>("snapshot");
      if (
        requestRevision !== revision ||
        catalogConflict ||
        dirty() ||
        busy ||
        startupPending ||
        dialogs.hasOpen()
      )
        return;
      snapshot = state;
      if (!state.settings) {
        showError(
          state.error ??
            "Settings unavailable. Repair local configuration before saving.",
        );
        return;
      }
      const retainAccounts =
        (section === "accounts" ||
          section === "copilot" ||
          section === "github") &&
        content.querySelector(".account-connection") !== null &&
        sameResource(saved, state.settings);
      saved = clone(state.settings);
      draft = clone(saved);
      conflict = false;
      if (state.error) showError(state.error);
      if (retainAccounts) {
        changed();
        window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts"));
        return;
      }
      render();
    } catch {
      if (
        requestRevision !== revision ||
        catalogConflict ||
        dirty() ||
        busy ||
        startupPending ||
        dialogs.hasOpen()
      )
        return;
      showError(
        "Could not read Settings. Open the native application and check local storage access.",
      );
    }
  }
  window.addEventListener("focus", () => {
    if (app.closest("[hidden]")) return;
    refreshAgentAccounts?.();
    if (!dirty() && !busy && !startupPending && !dialogs.hasOpen()) void load();
  });
  await load();
  return {
    leaveGuidance,
    open(target: SetupTarget, origin: GuidedReturn) {
      if (!draft || busy) {
        showError(
          "Settings is not ready to navigate. Finish the current save or retry loading Settings.",
        );
        return false;
      }
      if (dialogs.hasOpen()) {
        showError(
          "An editor is already open. Its unsaved fields are intact; finish or cancel it before choosing another setup step.",
        );
        return false;
      }
      guidance = origin;
      returnGenie.hidden = false;
      openGenie.hidden = true;
      app.querySelector<HTMLElement>("[data-genie-save-note]")!.hidden = false;
      const next: Section =
        target === "ai" || target === "repository-account"
          ? target === "ai"
            ? "copilot"
            : "github"
          : target === "repositories"
            ? "repositories"
            : target;
      if (options.embedded)
        homeOpener = accountParents[next] ? "accounts" : next;
      if (section !== next) navigate(next);
      settingsBack.hidden = true;
      const selector =
        target === "ai"
          ? ".copilot-auth"
          : target === "repository-account"
            ? ".github-auth"
            : target === "repositories"
              ? ".repository-library"
              : target === "agents"
                ? ".resource-toolbar"
                : target === "doctrines"
                  ? ".resource-toolbar"
                  : "#content";
      const destination =
        content.querySelector<HTMLElement>(selector) ?? content;
      destination.scrollIntoView({ block: "start" });
      (
        destination.querySelector<HTMLElement>(
          "button:not(:disabled),input,select,[role=status][tabindex]",
        ) ?? returnGenie
      ).focus({ preventScroll: true });
      return true;
    },
  };
}
import {
  isWindows,
  platformName,
  startupSettingsName,
  trayAdjective,
} from "./platform";
