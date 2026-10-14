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
import type {
  Agent,
  Doctrine,
  Assignment,
  PacketTool,
  WatchedIdentity,
} from "./policy";
import {
  type Admission,
  type AdmissionKind,
  type AdmissionRule,
  type Repository,
  primaryAssignmentId,
  repositoryProviderContext,
} from "./repositories";
import { defaultTools, doctrineTitles } from "./policy";
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
interface ExplicitPullRequestIntent {
  number: number;
  accountId: string;
  repositoryId: string;
  generation: number;
  failure?: string;
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
  | "accounts"
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

const sections: Record<Section, string> = {
  home: "Settings",
  accounts: "Accounts",
  "ai-tooling": "AI Tooling",
  "git-repository": "Git Repository",
  copilot: "GitHub Copilot",
  github: "GitHub",
  repositories: "Repositories",
  capacity: "Concurrent reviews",
  doctrines: "Doctrines",
  agents: "Agents",
  preferences: "Preferences",
};
// Bespoke reticle/scope marks, not a generic icon-kit -- each nods at the
// tab's job rather than a stock glyph.
const promptTokens: [string, string][] = [
  ["pr_title", "Pull request title"],
  ["pr_number", "Pull request number"],
  ["pr_author", "Author login"],
  ["head_sha", "Head commit"],
  ["base_sha", "Base commit"],
  ["changed_files", "Changed paths, one per line"],
  ["changed_file_count", "Number of changed files"],
  ["agent_name", "This agent's name"],
];
const iconPaths = {
  back: '<path d="m14 6-6 6 6 6M8 12h12"/>',
  brain:
    '<path d="M9 4a3 3 0 0 0-3 3 3 3 0 0 0-2 5 3 3 0 0 0 2 5 3 3 0 0 0 6 1V4a3 3 0 0 0-3 0Zm6 0a3 3 0 0 1 3 3 3 3 0 0 1 2 5 3 3 0 0 1-2 5 3 3 0 0 1-6 1"/>',
  refresh:
    '<path d="M20 11a8 8 0 0 0-14-4M4 4v4h4M4 13a8 8 0 0 0 14 4m2 3v-4h-4"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>',
  pencil: '<path d="M4 20h4L19 9l-4-4L4 16zM14 6l4 4"/>',
  reset: '<path d="M3 12a9 9 0 1 0 3-6.7M3 4v5h5"/>',
  up: '<path d="m6 14 6-6 6 6"/>',
  down: '<path d="m6 10 6 6 6-6"/>',
  trash: '<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v5M14 11v5"/>',
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
  genie:
    '<path d="M10.2 6.4V4.2a1.8 1.8 0 0 1 3.6 0v2.2M9.6 7.4c1.4 1 3.4 1 4.8 0M12 8.2C9 7.4 3 8.4 3 11.4c0 2.6 4.5 3.4 7.4 1.6M12 8.2c3-.8 9 .2 9 3.2 0 2.6-4.5 3.4-7.4 1.6M10.4 13c1.2.5 2.8.5 3.2 0M7.6 14.2c.6 3.4 2.8 5.7 5.8 6.6 2.2.4 4-.3 5-1.6-1.5.5-2.9.1-3.7-1 1.2-1.5 1.7-3.1 1.6-5"/>',
};
/** Icon, short name and one-line description per packet tool; `scope` limits it
 * to one assignment role. */
const toolRows: Record<
  PacketTool,
  {
    name: string;
    detail: string;
    path: string;
    scope?: "primary" | "secondary";
  }
> = {
  read_changes: {
    name: "Changes",
    detail: "Read the PR diff, paged",
    path: '<path d="M12 5v10M7 10h10M7 19h10"/>',
  },
  read_source: {
    name: "Source",
    detail: "Read any file at head or base",
    path: '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M13 5l-2 14"/>',
  },
  get_thread: {
    name: "Thread",
    detail: "Read a full review thread",
    path: '<path d="M4 5h16v11H9l-5 4z"/>',
  },
  get_checks: {
    name: "Checks",
    detail: "CI check names and status",
    path: '<circle cx="12" cy="12" r="8"/><path d="m8.5 12 2.5 2.5 4.5-5"/>',
  },
  read_check_log: {
    name: "Logs",
    detail: "Read a CI job log, paged",
    path: '<rect x="3.5" y="5" width="17" height="14" rx="2"/><path d="m7 10 3 2.5L7 15M12 15h5"/>',
  },
  list_tree: {
    name: "Tree",
    detail: "List repository files",
    path: '<path d="M5 4v16M5 7h6M5 13h6M5 19h6M11 4h8v6h-8zM11 16h8v4h-8z"/>',
  },
  search_code: {
    name: "Search",
    detail: "Search repository code",
    path: '<circle cx="11" cy="11" r="6"/><path d="m16 16 4 4"/>',
  },
  leave_comments: {
    name: "Comment",
    detail: "Post, reply, resolve own threads",
    scope: "primary",
    path: '<path d="M4 5h16v11H9l-5 4zM8 9h8M8 12h5"/>',
  },
  ready_for_human: {
    name: "Ready",
    detail: "Hand to you with a local summary",
    scope: "primary",
    path: '<circle cx="12" cy="8" r="3.2"/><path d="M5 20a7 7 0 0 1 14 0"/>',
  },
  approve_pull_request: {
    name: "Approve",
    detail: "Approve with a comment",
    scope: "primary",
    path: '<path d="m5 12 5 5 9-10"/>',
  },
  approve_and_merge: {
    name: "Merge",
    detail: "Approve, then merge",
    scope: "primary",
    path: '<circle cx="6" cy="6" r="2.4"/><circle cx="6" cy="18" r="2.4"/><circle cx="18" cy="12" r="2.4"/><path d="M6 8.4v7.2M8.4 6c5 0 7 2 7.2 4"/>',
  },
  panic_exit: {
    name: "Panic",
    detail: "Stop; Needs attention, auto-retry",
    path: '<path d="M12 4 3 20h18zM12 10v4M12 17h.01"/>',
  },
  submit_report: {
    name: "Report",
    detail: "Send findings to the primary",
    scope: "secondary",
    path: '<path d="M7 4h10v16H7zM10 9h4M10 13h4"/>',
  },
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
      ? "Session unavailable"
      : reason(session.error);
  }
  const details = describeReadFailureDetails(error);
  if (details) return details;
  const errors: Record<string, string> = {
    invalid_repository: "Invalid repository",
    revision_changed: "Revision changed",
    signed_out: "Reconnect required",
    missing_read_permission: "Access denied",
    repository_unavailable: "Repository unavailable",
    rate_limited: "Rate limited",
    network: "Offline",
    timeout: "Timed out",
    invalid_response: "Invalid response",
    incomplete_read: "Incomplete",
    missing_scope: "Reconnect required",
    scope_unverified: "Scope unverified",
    organization_policy_denied: "Blocked by organization",
    organization_policy_denied_with_missing_scope: "Blocked by organization",
    repository_changed: "Repository changed",
    authentication_changed: "Reconnect required",
    provider_failure: "Provider failed",
    provider_rejected: "Rejected",
    wrong_identity: "Wrong account",
    configuration: "Setup unavailable",
  };
  if (error && typeof error === "object") {
    if (
      "rate_limited_after" in error &&
      typeof error.rate_limited_after === "number" &&
      error.rate_limited_after >= 0
    )
      return `${errors.rate_limited} \u00b7 ${error.rate_limited_after}s`;
    if (
      "provider_failure_after" in error &&
      typeof error.provider_failure_after === "number" &&
      error.provider_failure_after >= 0
    )
      return `${errors.provider_failure} \u00b7 ${error.provider_failure_after}s`;
  }
  return typeof error === "string" ? (errors[error] ?? error) : "Failed";
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
        ([key, title]) =>
          `<button type="button" data-section="${key}">${icon(key as keyof typeof iconPaths)}${title}</button>`,
      )
      .join("")}</nav>
    <label class="mobile-section">Section<select aria-label="Settings section">${legacySections
      .map(([key, title]) => option(key, title, "accounts"))
      .join("")}</select></label></aside>`
  }
    <div class="settings-main"><div class="settings-genie-entry"><button type="button" data-open-genie hidden>Set up with Genie</button><button type="button" data-return-genie hidden>Back to Genie</button></div><header class="settings-heading"><h1 tabindex="-1">Settings</h1></header>
    <p id="error" role="alert" hidden></p><div data-catalog-conflict hidden><p role="alert">Catalog changed</p><button type="button" data-reload-catalog>Reload</button></div><section id="content"></section>
    <footer class="settings-savebar"><span role="status" id="save-status">Loading</span><button id="reload-settings" hidden>Discard draft and reload</button><button id="reset-settings" disabled>Reset changes</button><button class="primary" id="save-settings" disabled>Save preferences</button></footer></div>`;
  const content = app.querySelector<HTMLElement>("#content")!;
  const legacyGenie =
    app.querySelector<HTMLButtonElement>("[data-open-genie]")!;
  legacyGenie.hidden = !!options.embedded || !options.openGenie;
  legacyGenie.onclick = () => options.openGenie?.(legacyGenie);
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
  settingsBack.title = "Back";
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
  const pendingPullRequests = new Map<string, ExplicitPullRequestIntent>();
  let updateRepositoryIntake:
    | ((
        id: string,
        intent: ExplicitPullRequestIntent,
        message: string,
        failed: boolean,
      ) => boolean)
    | undefined;
  let copilotAccounts: CopilotAccount[] = [];
  let updateAgentAccounts: (() => void) | undefined;
  let updateRepositoryAccounts: (() => void) | undefined;
  let refreshAgentAccounts: (() => void) | undefined;
  let guidance: GuidedReturn | undefined;
  const returnGenie = app.querySelector<HTMLButtonElement>(
    "[data-return-genie]",
  )!;
  const settingsHeading = app.querySelector<HTMLElement>(".settings-heading")!;
  if (options.embedded) {
    settingsHeading.hidden = true;
    app.querySelector<HTMLElement>(".settings-savebar")!.hidden = true;
  }
  settingsHeading.classList.add("settings-heading-with-genie");
  settingsHeading
    .querySelector("h1")!
    .after(app.querySelector(".settings-genie-entry")!);
  returnGenie.onclick = () => guidance?.back();
  const leaveGuidance = () => {
    guidance = undefined;
    returnGenie.hidden = true;
    settingsBack.hidden = !options.embedded || section === "home";
  };
  const dialogs = createDialogs(
    content,
    () => revision++,
    (opener, parent) => restoreControl(opener, parent ?? content, parent),
  );
  const notificationView: Parameters<typeof mountNotificationSettings>[1] = {};
  const automationView = {};
  const preferenceDisclosures = new Set<string>();
  const dirty = () => !!draft && !sameResource(draft, saved);
  const preferencesDirty = () =>
    !!draft &&
    !sameResource(globalPreferences(draft), globalPreferences(saved));
  let errorTimer: ReturnType<typeof setTimeout> | undefined;
  const showError = (message: string) => {
    const label = terseError(message, true);
    if (label === null) return;
    error.textContent = label;
    error.title = message;
    error.setAttribute("aria-description", message);
    error.hidden = false;
    clearTimeout(errorTimer);
    errorTimer = setTimeout(() => (error.hidden = true), 8000);
  };
  error.addEventListener("click", () => (error.hidden = true));
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
      ? "Working"
      : catalogConflict
        ? "Reload required"
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
    if (busy) throw "Busy";
    if (catalogConflict && (edit.kind === "agent" || edit.kind === "doctrine"))
      throw "Resource changed in another window";
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
        : "Disabled / Reconnect account";
    if (!repository.enabled) return "Disabled";
    if (!repository.assignments?.length) return "Needs setup";
    if (!repositoryAutomation) return "Unavailable";
    if (repositoryAutomation.paused) return "Paused";
    if (repositoryAutomation.active >= repositoryAutomation.capacity)
      return "Waiting for capacity";
    return "Active";
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
    if (!current) throw "Unsaved";
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
    app.querySelector("h1")!.textContent = sections[section];
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
    settingsBack.setAttribute("aria-label", `Back to ${sections[parent]}`);
    settingsBack.title = "Back";
    app.querySelector<HTMLElement>(".settings-savebar")!.hidden =
      section === "repositories" ||
      (!!options.embedded &&
        (section === "home" ||
          accountOverview ||
          ["copilot", "github", "agents", "doctrines"].includes(section)));
    settingsHeading.hidden = section === "home";
    if (options.embedded)
      app.dispatchEvent(
        new CustomEvent("pr-sniper:settings-section", {
          detail: {
            title: section === "home" ? "" : sections[section],
            back: !settingsBack.hidden,
          },
        }),
      );
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
    value = "",
    symbol: keyof typeof iconPaths = "integrations",
  ) {
    return `<button type="button" class="settings-home-row" data-settings-destination="${key}" data-focus-key="settings:${key}" aria-label="${sections[key]}" aria-describedby="settings-${key}-count"><span class="settings-row-symbol">${icon(symbol)}</span><span class="settings-row-copy"><strong>${sections[key]}</strong></span><span class="settings-row-value" id="settings-${key}-count" data-settings-count="${key}">${value}</span><svg class="settings-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg></button>`;
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
      value = "",
    ) => overviewRow(key, value, key === "preferences" ? "home" : key);
    content.innerHTML = `<div class="settings-overview">
      <nav aria-label="Review setup" class="settings-home-group">
        ${row("accounts", "Reading...")}
        ${row("agents", String(saved.agents?.length ?? 0))}
        ${row("repositories", String(saved.repositories?.length ?? 0))}
        ${row("doctrines", String(saved.doctrines?.length ?? 0))}
      </nav>
      <nav aria-label="Application preferences" class="settings-home-group">
        ${row("capacity", String(saved.capacity))}
        ${row("preferences")}
      </nav>
      ${options.openGenie ? `<button type="button" data-home-genie data-focus-key="settings:genie" aria-label="Set up with Genie" title="Set up with Genie">${icon("genie")}</button>` : ""}
    </div>`;
    content
      .querySelectorAll<HTMLButtonElement>("[data-settings-destination]")
      .forEach((button) => {
        button.onclick = () =>
          openFromHome(button.dataset.settingsDestination as Section);
      });
    const genie = content.querySelector<HTMLButtonElement>("[data-home-genie]");
    if (genie) genie.onclick = () => options.openGenie?.(genie);
    refreshAccountSummaries();
  }

  function renderAccountOverview() {
    const comingSoon = (label: string, symbol: keyof typeof iconPaths) =>
      `<div class="settings-home-row settings-provider-planned" aria-disabled="true"><span class="settings-row-symbol">${icon(symbol)}</span><span class="settings-row-copy"><strong>${escape(label)}</strong></span><span class="settings-row-value">Coming soon</span></div>`;
    const rows =
      section === "accounts"
        ? overviewRow("ai-tooling", "Reading...", "agents") +
          overviewRow("git-repository", "Reading...", "repositories")
        : section === "ai-tooling"
          ? overviewRow("copilot", "Reading...", "agents") +
            modelProviders
              .filter((provider) => !provider.available)
              .map((provider) => comingSoon(provider.label, "agents"))
              .join("")
          : overviewRow("github", "Reading...", "repositories") +
            ["Azure DevOps", "Bitbucket"]
              .map((label) => comingSoon(label, "repositories"))
              .join("");
    content.innerHTML = `<div class="settings-overview"><nav class="settings-home-group" aria-label="${sections[section]}">${rows}</nav></div>`;
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
          showError(reason(failure.reason));
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

  async function unassignAgent(agent: Agent) {
    for (const repository of agentRepositories(agent)) {
      const current =
        saved.repositories?.find((r) => r.id === repository.id) ?? repository;
      const remaining = (current.assignments ?? []).filter(
        (assignment) => assignment.agent_id !== agent.id,
      );
      const next = { ...clone(current), assignments: remaining };
      if (!remaining.some((a) => a.id === next.primary_assignment_id))
        delete next.primary_assignment_id;
      // Monitoring needs at least one assigned Agent.
      if (!remaining.length) next.enabled = false;
      await commitResource(repositoryEdit(current, next));
    }
  }

  const assignedNote = (agent: Agent) => {
    const names = agentRepositories(agent).map((r) => escape(r.name));
    return names.length
      ? ` Assigned to ${names.length}: ${names.join(", ")}.`
      : "";
  };

  const editorStack: {
    modal: HTMLDialogElement;
    title: string;
    back: () => void;
    admission?: boolean;
  }[] = [];
  function announceEditor() {
    const top = editorStack[editorStack.length - 1];
    app.dispatchEvent(
      new CustomEvent("pr-sniper:settings-editor", {
        detail: top ? { title: top.title, back: top.back } : null,
      }),
    );
  }
  function dropEditor(match: (entry: (typeof editorStack)[number]) => boolean) {
    for (let i = editorStack.length - 1; i >= 0; i--)
      if (match(editorStack[i])) editorStack.splice(i, 1);
    announceEditor();
  }

  function compactEditor(modal: HTMLDialogElement, backTitle: string) {
    modal.classList.add("shared-resource-editor");
    if (modal.parentElement?.classList.contains("dialog-fallback"))
      modal.parentElement.classList.add("shared-resource-host");
    const back = modal.querySelector<HTMLButtonElement>(".dialog-head button")!;
    back.classList.add("resource-back");
    if (options.embedded) {
      const entry = {
        modal,
        title: modal.getAttribute("aria-label") ?? "",
        back: () => back.click(),
      };
      editorStack.push(entry);
      announceEditor();
      // The modal's own header button hides once the panel title takes over.
      requestAnimationFrame(() => {
        const active = document.activeElement;
        if (
          modal.isConnected &&
          (!active ||
            active === document.body ||
            (active instanceof HTMLElement && !active.getClientRects().length))
        )
          modal
            .querySelector<HTMLElement>(
              ".dialog-body :is(input, select, textarea, button):not(:disabled)",
            )
            ?.focus({ preventScroll: true });
      });
      const release = () => dropEditor((e) => e.modal === modal);
      modal.addEventListener("close", release);
      modal.addEventListener("pr-sniper:dialog-closed", release);
    }
    back.title = backTitle;
    back.innerHTML = icon("back");
    errorsFirst(modal);
  }

  // Save/validation errors render at the top of the editor, never beside or under
  // its Save/Cancel actions. A newly shown error scrolls into view and takes focus.
  function errorsFirst(modal: HTMLDialogElement) {
    const body = modal.querySelector<HTMLElement>(".dialog-body");
    if (!body) return;
    const saveErrors = [
      ...body.querySelectorAll<HTMLElement>("[role=alert]"),
    ].filter((alert) => {
      let next = alert.nextElementSibling;
      while (next?.matches("[role=alert]")) next = next.nextElementSibling;
      return !!next?.classList.contains("resource-actions");
    });
    for (const alert of saveErrors.reverse()) {
      alert.classList.add("editor-error");
      alert.tabIndex = -1;
      alert.parentElement!.prepend(alert);
    }
    // The latest activation in this editor was its Save.
    let saving = false;
    let savedAt = 0;
    modal.addEventListener(
      "click",
      (event) => {
        saving =
          event.target instanceof Element &&
          !!event.target.closest(".resource-actions .primary");
        savedAt = performance.now();
      },
      true,
    );
    modal.addEventListener(
      "submit",
      () => {
        saving = true;
        savedAt = performance.now();
      },
      true,
    );
    new MutationObserver((records) => {
      const shown = records
        .map((record) => record.target)
        .find(
          (target): target is HTMLElement =>
            target instanceof HTMLElement &&
            saveErrors.includes(target) &&
            !target.hidden &&
            !!target.textContent?.trim(),
        );
      if (!shown || !modal.isConnected) return;
      // Only a failed Save moves the reader; other errors leave focus where it is.
      if (!saving || performance.now() - savedAt > 15_000) return;
      saving = false;
      body.scrollTop = 0;
      shown.focus({ preventScroll: true });
    }).observe(body, {
      attributes: true,
      attributeFilter: ["hidden"],
      subtree: true,
    });
  }

  function warnBeforeDiscard(modal: HTMLDialogElement, resource: string) {
    const fields = () =>
      JSON.stringify(
        [
          ...modal.querySelectorAll<
            HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
          >("[name]"),
        ]
          .filter(
            (field) =>
              !(field instanceof HTMLInputElement) ||
              !["checkbox", "radio"].includes(field.type) ||
              field.checked,
          )
          .map((field) => [field.name, field.value]),
      );
    const initial = fields();
    const guard = (event: Event) => {
      if (fields() === initial) return;
      event.preventDefault();
      const focused = document.activeElement;
      const confirm = dialog(
        `Discard ${resource} changes?`,
        `<div class="resource-actions"><button type="button" data-keep>Keep editing</button><button type="button" data-discard>Discard changes</button></div>`,
        focused instanceof HTMLElement ? focused : modal,
      );
      compactEditor(confirm, "Back");
      confirm.querySelector<HTMLButtonElement>("[data-keep]")!.onclick = () =>
        confirm.close();
      confirm.querySelector<HTMLButtonElement>("[data-discard]")!.onclick =
        () => {
          confirm.close();
          release();
          modal.close();
        };
    };
    const release = () =>
      modal.removeEventListener("pr-sniper:dialog-closing", guard);
    modal.addEventListener("pr-sniper:dialog-closing", guard);
    return release;
  }

  function resourceEditor(
    modal: HTMLDialogElement,
    deletion?: {
      edit: ResourceEdit;
      blocked: string;
      before?: () => Promise<void>;
    },
    onDeleted?: () => void,
  ) {
    compactEditor(modal, "Back");
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
        await deletion.before?.();
        await commitResource(deletion.edit, modal);
        if (onDeleted) onDeleted();
        else {
          modal.close();
          render();
        }
      } catch (cause) {
        alert.textContent = reason(cause);
        alert.hidden = false;
        alert.scrollIntoView({ block: "nearest" });
      }
    };
  }
  const resourceActions = (kind: "agent" | "doctrine", existing: boolean) =>
    `<p role="alert" hidden></p><div class="resource-actions"><button class="primary">Save ${kind}</button><button type="button" data-cancel-resource>Cancel</button></div>
    ${existing ? `<button type="button" class="resource-delete" data-delete-resource>Delete ${kind}</button><div data-delete-confirmation hidden><div class="resource-actions"><button type="button" data-confirm-delete>Confirm deletion</button><button type="button" data-keep-resource>Keep ${kind}</button></div></div>` : ""}`;

  function renderDoctrines() {
    content.innerHTML = `<p class="doctrine-catalog-note" data-doctrine-catalog>${escape(doctrineCatalogLabel(snapshot.doctrine_catalog))}</p><div class="section-actions resource-toolbar doctrine-toolbar"><button class="primary" id="new-doctrine">New doctrine</button></div><div class="doctrine-list resource-library"></div>`;
    const list = content.querySelector(".doctrine-list")!;
    for (const doctrine of doctrines()) {
      const row = document.createElement("article");
      row.className = "doctrine-card resource-card";
      row.innerHTML = `<span class="resource-symbol">${icon("doctrines")}</span><div class="resource-copy"><h3>${escape(doctrine.title)}</h3><p class="resource-preview">${escape(doctrine.body)}</p><p class="word-count">${words(doctrine.body)} words / ${doctrineUsers(doctrine).length} Agents</p></div><div class="card-actions"><button data-edit>Edit</button><button data-remove aria-label="Delete" title="Delete">${icon("trash")}</button></div>`;
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
          showError("In use by an Agent");
          return;
        }
        if (
          !(await confirmDialog(
            "Delete this doctrine?",
            `Delete "${escape(doctrine.title)}"?`,
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

  function editDoctrine(
    opener: HTMLElement,
    existing?: Doctrine,
    onDone?: (saved: Doctrine) => void,
  ) {
    const users = existing ? doctrineUsers(existing) : [];
    const modal = dialog(
      existing ? `Editing ${existing.title}` : "New doctrine",
      `<form><label>Title<input name="title" required maxlength="100" value="${escape(existing?.title ?? "")}" placeholder="e.g. bounded-context" /></label><label>Principles<textarea name="body" rows="10" required>${escape(existing?.body ?? "")}</textarea></label><p class="word-count" data-count>${words(existing?.body ?? "")} words</p>
      <p class="resource-impact">${users.length ? `Shared by ${users.length} Agents: ${escape(users.map((agent) => agent.name).join(", "))}` : "Unused"}</p>${resourceActions("doctrine", !!existing)}</form>`,
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
            blocked: users.length ? "In use by an Agent" : "",
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
        if (!title || !body.value.trim()) throw "Title and principles required";
        if (
          doctrines().some(
            (d) =>
              d !== existing && d.title.toLowerCase() === title.toLowerCase(),
          )
        )
          throw "Title already used";
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
        if (onDone) onDone({ title, body: body.value });
        else render();
      } catch (cause) {
        alert.textContent = typeof cause === "string" ? cause : reason(cause);
        alert.hidden = false;
      }
    };
  }

  // -------------------------------------------------------------------- Agents

  function renderAgents(onReady: () => void) {
    content.innerHTML = `<div class="agent-list resource-library"></div><div class="section-actions resource-toolbar agent-footer"><button class="primary" id="new-agent" disabled>New agent</button><p class="sr-only" data-copilot-status>Loading</p></div>`;
    const list = content.querySelector(".agent-list")!;
    for (const agent of agents()) {
      const account = copilotAccounts.find(
        (a) => a.account_id === agent.ai_account?.account_id,
      );
      const row = document.createElement("article");
      row.className = "agent-card resource-card";
      row.innerHTML = `<span class="resource-symbol">${icon("agents")}</span><div class="resource-copy"><h3>${escape(agent.name)}</h3><p class="resource-meta">${escape(agent.model)} / ${agentRepositories(agent).length} repositories</p><p class="resource-preview">${escape(agent.prompt)}</p><p data-account-state>${agent.ai_account ? (account?.state === "connected" ? "" : "Reconnect required") : "Unconfigured"}</p></div><div class="card-actions"><button data-edit>Edit</button><button data-remove aria-label="Delete" title="Delete">${icon("trash")}</button></div>`;
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
        if (
          !(await confirmDialog(
            "Delete this agent?",
            `Delete "${escape(agent.name)}"?${assignedNote(agent)}`,
            assigned.length
              ? "Remove from repositories and delete"
              : "Delete agent",
            opener,
          ))
        )
          return;
        try {
          await unassignAgent(agent);
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
          notice.textContent = connected ? "Connected" : "Not connected";
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
              state.textContent =
                account?.state === "connected" ? "" : "Reconnect required";
          }
          if (view.accounts.some((a) => a.reason === "verification_pending"))
            setTimeout(refreshAccounts, 350);
        },
        () => {
          if (notice.isConnected && request === accountRequest)
            notice.textContent = "Unavailable";
        },
      );
    };
    refreshAgentAccounts = refreshAccounts;
    refreshAccounts();
  }

  const slug = (value: string, fallback: string) =>
    value
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || fallback;
  const defaultSignature = (name: string, model: string) =>
    `\u{1F52B} pr-sniper(${slug(name, "agent")}:${slug(model, "model")})`;
  const doctrineChoice = (title: string, checked: boolean) =>
    `<label data-doctrine-choice><input type="checkbox" name="doctrine" value="${escape(title)}" ${checked ? "checked" : ""} /><span>${escape(title)}</span><button type="button" data-edit-doctrine="${escape(title)}" aria-label="Edit doctrine ${escape(title)}" title="Edit doctrine">${icon("pencil")}</button></label>`;

  function editAgent(
    opener: HTMLElement,
    existing?: Agent,
    onSaved?: (agent?: Agent) => void,
  ) {
    const assigned = existing ? agentRepositories(existing) : [];
    const expected = clone(
      saved.agents?.find((a) => a.id === existing?.id) ?? null,
    );
    const modal = dialog(
      existing ? `Editing ${existing.name}` : "New agent",
      `<form><section class="agent-section"><label>Name<input name="name" required maxlength="80" value="${escape(existing?.name ?? "")}" placeholder="e.g. The Nitpicker" /></label>
        <label>AI account<select name="ai-account" aria-label="AI account"><option value="">Choose a Copilot account</option>${copilotAccounts.map((a) => `<option value="${escape(a.account_id)}" ${a.account_id === existing?.ai_account?.account_id ? "selected" : ""} ${a.state === "connected" ? "" : "disabled"}>${escape(a.login)} (${escape(a.account_id)})${a.state === "connected" ? "" : " - reconnect required"}</option>`).join("")}${existing?.ai_account && !copilotAccounts.some((a) => a.account_id === existing.ai_account?.account_id) ? `<option selected disabled value="${escape(existing.ai_account.account_id)}">Copilot ${escape(existing.ai_account.account_id)} - reconnect required</option>` : ""}</select></label>
        <div class="status-row"><p data-model-status role="status"></p><button type="button" class="fix-cta" data-fix-account hidden>Fix</button></div>${onSaved ? '<p data-editor-accounts role="status">Loading</p><button type="button" data-retry-accounts>Retry Copilot accounts</button>' : ""}</section>
        <section class="agent-section resource-intelligence">
        <label><span class="model-label">Model<button type="button" data-retry-models aria-label="Refresh model list" title="Refresh model list">${icon("refresh")}</button><button type="button" data-cancel-models aria-label="Cancel model lookup" title="Cancel model lookup" hidden><span class="spinner" aria-hidden="true"></span></button></span><select name="model" aria-label="Model" disabled><option value="${escape(existing?.model ?? "")}">${escape(existing?.model ?? "Choose an account first")}</option></select></label>
        <label>Context window<select name="context-tier" aria-label="Context window" disabled></select></label>
        <label>Reasoning effort<select name="reasoning-effort" aria-label="Reasoning effort" disabled></select></label>
        <p data-intelligence-status aria-live="polite"></p></section>
        <section class="agent-section"><button type="button" class="doctrine-row" data-open-doctrines aria-label="Choose doctrines"><span>Doctrines</span><span class="doctrine-summary" data-doctrine-summary></span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg></button><fieldset class="resource-doctrines"><legend class="sr-only">Doctrines</legend><label>Filter doctrines<input type="search" data-doctrine-filter placeholder="Find principles..." /></label><div class="doctrine-choices" role="group" aria-label="Available doctrines">${doctrines()
          .map((d) =>
            doctrineChoice(
              d.title,
              !!existing &&
                doctrineTitles(existing).some(
                  (t) =>
                    t.trim().toLowerCase() === d.title.trim().toLowerCase(),
                ),
            ),
          )
          .join("")}</div><p data-no-doctrines hidden></p></fieldset>
        <label><span class="prompt-label">Prompt<button type="button" class="info-tip" aria-label="Prompt tips" aria-expanded="false" aria-controls="prompt-tips">${icon("info")}</button><span class="tip-body" id="prompt-tips" role="group" aria-label="Prompt tokens and tips"><strong>Tokens</strong><span class="token-list">${promptTokens.map(([token, hint]) => `<button type="button" data-token="${token}"><code>{{${token}}}</code><span>${escape(hint)}</span></button>`).join("")}</span></span></span><textarea name="prompt" aria-label="Prompt" rows="4" required>${escape(existing?.prompt ?? "Review this pull request for correctness, risk, and readability.")}</textarea></label>
        <label><span class="model-label">Signature<button type="button" data-reset-signature aria-label="Reset signature to default" title="Reset to default">${icon("reset")}</button></span><input name="signature" aria-label="Signature" required maxlength="200" value="${escape(existing?.signature ?? defaultSignature(existing?.name ?? "", existing?.model ?? ""))}" /></label></section>
        <p class="resource-impact">${assigned.length ? `Shared by ${assigned.length} repositories: ${escape(assigned.map((repository) => repository.name).join(", "))}` : "Unassigned"}</p>${resourceActions("agent", !!existing)}</form>`,
      opener,
    );
    const promptField = modal.querySelector<HTMLTextAreaElement>(
      "textarea[name=prompt]",
    )!;
    const grow = () => {
      promptField.style.height = "auto";
      promptField.style.height = `${promptField.scrollHeight}px`;
    };
    modal.querySelectorAll<HTMLButtonElement>("[data-token]").forEach((b) => {
      b.onclick = () => {
        const text = `{{${b.dataset.token}}}`;
        const { selectionStart: from, selectionEnd: to } = promptField;
        promptField.setRangeText(text, from, to, "end");
        promptField.dispatchEvent(new Event("input", { bubbles: true }));
        promptField.focus();
      };
    });
    const tipButton = modal.querySelector<HTMLButtonElement>(".info-tip")!;
    const setTip = (open: boolean) =>
      tipButton.setAttribute("aria-expanded", String(open));
    tipButton.onclick = () =>
      setTip(tipButton.getAttribute("aria-expanded") !== "true");
    modal.addEventListener("click", (event) => {
      if (!(event.target as Element).closest(".prompt-label")) setTip(false);
    });
    modal.addEventListener("keydown", (event) => {
      if (event.key === "Escape") setTip(false);
    });
    promptField.addEventListener("input", grow);
    requestAnimationFrame(grow);
    let releaseDiscardGuard: (() => void) | undefined;
    const finish = (agent?: Agent) => {
      releaseDiscardGuard?.();
      modal.close();
      if (onSaved) onSaved(agent);
      else render();
    };
    resourceEditor(
      modal,
      existing
        ? {
            edit: {
              kind: "agent",
              id: existing.id,
              expected,
              value: null,
            },
            blocked: "",
            before: existing ? () => unassignAgent(existing) : undefined,
          }
        : undefined,
      () => finish(),
    );
    if (onSaved) compactEditor(modal, "Back");
    const filter = modal.querySelector<HTMLInputElement>(
      "[data-doctrine-filter]",
    )!;
    const list = modal.querySelector<HTMLElement>(".doctrine-choices")!;
    let choices = [
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
      void hiddenSelected;
      modal.querySelector("[data-doctrine-summary]")!.textContent =
        `${selected} selected`;
      const empty = modal.querySelector<HTMLElement>("[data-no-doctrines]")!;
      empty.hidden = visible > 0;
      empty.textContent = choices.length ? "No matches" : "None";
    };
    filter.oninput = updateChoices;
    filter.onkeydown = (event) => {
      if (event.key === "Enter") event.preventDefault();
    };
    list.addEventListener("change", updateChoices);
    updateChoices();
    const agentForm = modal.querySelector<HTMLFormElement>("form")!;
    const openRow = modal.querySelector<HTMLButtonElement>(
      "[data-open-doctrines]",
    )!;
    const closePicker = () => {
      delete agentForm.dataset.view;
      dropEditor((e) => e.modal === modal && e.title === "Doctrines");
      openRow.focus({ preventScroll: true });
    };
    openRow.onclick = () => {
      agentForm.dataset.view = "doctrines";
      if (options.embedded) {
        editorStack.push({ modal, title: "Doctrines", back: closePicker });
        announceEditor();
      }
      filter.focus({ preventScroll: true });
    };
    list.addEventListener("click", (event) => {
      const pencil = (event.target as Element).closest<HTMLButtonElement>(
        "[data-edit-doctrine]",
      );
      if (!pencil) return;
      const title = pencil.dataset.editDoctrine!;
      const doctrine = doctrines().find((d) => d.title === title);
      if (!doctrine) return;
      editDoctrine(pencil, doctrine, (updated) => {
        const checked = new Set(
          [...list.querySelectorAll<HTMLInputElement>("input:checked")].map(
            (i) => (i.value === title ? updated.title : i.value),
          ),
        );
        list.innerHTML = doctrines()
          .map((d) => doctrineChoice(d.title, checked.has(d.title)))
          .join("");
        choices = [
          ...modal.querySelectorAll<HTMLElement>("[data-doctrine-choice]"),
        ];
        updateChoices();
      });
    });
    const accountSelect =
      modal.querySelector<HTMLSelectElement>("[name=ai-account]")!;
    const modelSelect = modal.querySelector<HTMLSelectElement>("[name=model]")!;
    const nameInput = modal.querySelector<HTMLInputElement>("[name=name]")!;
    const signatureInput =
      modal.querySelector<HTMLInputElement>("[name=signature]")!;
    const computedSignature = () =>
      defaultSignature(nameInput.value, modelSelect.value);
    let lastSignature = computedSignature();
    // An untouched default follows the name and model; an edited one never moves.
    const syncSignature = () => {
      const next = computedSignature();
      if (signatureInput.value === lastSignature) signatureInput.value = next;
      lastSignature = next;
    };
    nameInput.addEventListener("input", syncSignature);
    modelSelect.addEventListener("change", syncSignature);
    modal.querySelector<HTMLButtonElement>("[data-reset-signature]")!.onclick =
      () => {
        signatureInput.value = lastSignature = computedSignature();
        signatureInput.focus();
      };
    const modelStatus = modal.querySelector<HTMLElement>(
      "[data-model-status]",
    )!;
    const statusLines = [
      modelStatus,
      ...modal.querySelectorAll<HTMLElement>(
        "[data-intelligence-status], [data-editor-accounts]",
      ),
    ];
    const markStatus = () => {
      for (const line of statusLines) {
        const text = line.textContent ?? "";
        line.dataset.state =
          text === "Loading"
            ? "loading"
            : /^(Unavailable|Reconnect required|Not connected|Lookup failed|Cancel failed)$/.test(
                  text,
                ) || /fail|error|denied|invalid/i.test(text)
              ? "error"
              : "info";
      }
    };
    const fix = modal.querySelector<HTMLButtonElement>("[data-fix-account]")!;
    const markFix = () => {
      markStatus();
      fix.hidden = !statusLines.some(
        (line) =>
          line.dataset.state === "error" &&
          /^(Reconnect required|Not connected)$/.test(line.textContent ?? ""),
      );
    };
    fix.onclick = () => {
      modal.querySelector<HTMLButtonElement>(".dialog-head button")?.click();
      requestAnimationFrame(() => {
        if (!modal.open) openFromHome("copilot");
      });
    };
    const statusObserver = new MutationObserver(markFix);
    for (const line of statusLines)
      statusObserver.observe(line, {
        childList: true,
        characterData: true,
        subtree: true,
      });
    markFix();
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
            ? `<option selected disabled value="${escape(selected)}">${escape(selected)} - ${catalogLoaded ? "unsupported" : "retained"}</option>`
            : "") +
          values
            .map(
              (value) =>
                `<option value="${escape(value)}" ${selected === value ? "selected" : ""} ${supported(value) ? "" : "disabled"}>${escape(label(value))}${supported(value) ? "" : " - unsupported"}</option>`,
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
        ? "Unavailable"
        : !ready
          ? "Choose a model"
          : intelligenceError(model, intelligence()) ||
            (model?.capabilities?.limits?.max_context_window_tokens !==
            undefined
              ? `Max ${model.capabilities.limits.max_context_window_tokens.toLocaleString("en-US")} tokens`
              : "");
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
          if (modal.isConnected) modelStatus.textContent = "Cancel failed";
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
          ? "Reconnect required"
          : "Choose an account";
        return;
      }
      const lookup = { accountId, requestId: newIdentity() };
      pendingLookup = lookup;
      cancelModels.hidden = false;
      modelStatus.textContent = "Loading";
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
            ? `<option selected disabled value="${escape(selected)}">${escape(selected)} - unavailable</option>`
            : "") +
          models
            .map(
              (m) =>
                `<option value="${escape(m.id)}" ${m.id === selected ? "selected" : ""} ${modelSelectable(m) ? "" : "disabled"}>${escape(m.name)} (${escape(m.id)})${modelSelectable(m) ? "" : " - unavailable"}</option>`,
            )
            .join("");
        modelSelect.disabled = !models.some(modelSelectable);
        syncSignature();
        describeModel();
      } catch (cause) {
        if (modal.isConnected && current === request)
          modelStatus.textContent = reason(cause);
      } finally {
        if (current === request) {
          pendingLookup = undefined;
          cancelModels.hidden = true;
        }
      }
    }
    function describeModel() {
      const model = catalog.find((m) => m.id === modelSelect.value);
      modelStatus.textContent = !catalog.length
        ? "No models"
        : modelSelect.value && (!model || !modelSelectable(model))
          ? "Model unavailable"
          : `${catalog.length} model${catalog.length === 1 ? "" : "s"}`;
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
      modelStatus.textContent = "Cancelled";
    };
    let selectedWasConnected = copilotAccounts.some(
      (a) => a.account_id === accountSelect.value && a.state === "connected",
    );
    const updateAccounts = () => {
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
          ? "Reconnect required"
          : "Choose an account";
      } else if (!selectedWasConnected) {
        void loadModels();
      }
      selectedWasConnected = connected;
    };
    updateAgentAccounts = updateAccounts;
    if (onSaved) {
      const previousRefresh = refreshAgentAccounts;
      const status = modal.querySelector<HTMLElement>(
        "[data-editor-accounts]",
      )!;
      let accountRequest = 0;
      const refreshAccounts = () => {
        if (!modal.isConnected) return;
        const request = ++accountRequest;
        status.textContent = "Loading";
        void invoke<CopilotAuth>("copilot_auth_state").then(
          (view) => {
            if (!modal.isConnected || request !== accountRequest) return;
            copilotAccounts = view.accounts;
            updateAccounts();
            status.textContent = copilotAccounts.some(
              (account) => account.state === "connected",
            )
              ? "Connected"
              : "Not connected";
          },
          () => {
            if (!modal.isConnected || request !== accountRequest) return;
            copilotAccounts = [];
            updateAccounts();
            status.textContent = "Unavailable";
          },
        );
      };
      refreshAgentAccounts = refreshAccounts;
      modal.querySelector<HTMLButtonElement>("[data-retry-accounts]")!.onclick =
        refreshAccounts;
      modal.addEventListener(
        "pr-sniper:dialog-closed",
        () => {
          if (refreshAgentAccounts === refreshAccounts)
            refreshAgentAccounts = previousRefresh;
          if (updateAgentAccounts === updateAccounts)
            updateAgentAccounts = undefined;
        },
        { once: true },
      );
      refreshAccounts();
    }
    void loadModels();
    if (onSaved) releaseDiscardGuard = warnBeforeDiscard(modal, "Agent");
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
          throw "Name, prompt and signature required";
        if (
          agents().some(
            (a) =>
              a !== existing && a.name.toLowerCase() === name.toLowerCase(),
          )
        )
          throw "Name already used";
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
          throw "Account and model required";
        if (!modelAvailable && !sameIntelligence)
          throw "Capabilities unavailable";
        if (modelAvailable) {
          const invalid = intelligenceError(selectedModel, intelligence());
          if (invalid) throw invalid;
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
            expected,
            value: values,
          },
          modal,
        );
        finish(values);
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
      <div class="repository-list native-repository-list repository-disconnected" data-repository-disconnected hidden></div>
      ${conflict ? '<p role="alert">Settings changed</p><button data-repository-reload>Reload</button>' : ""}
      <h2>Browse from an account</h2><div data-repository-accounts class="native-repository-list"></div></section>`;
    const rows = () => {
      const list = content.querySelector<HTMLElement>(".repository-list");
      const accounts = content.querySelector<HTMLElement>(
        "[data-repository-accounts]",
      );
      if (!list || !accounts) return;
      const restore = rememberControl(content);
      const disconnected = content.querySelector<HTMLElement>(
        "[data-repository-disconnected]",
      )!;
      const rowsFor = (disconnectedGroup: boolean) =>
        repositories()
          .filter((repository) => {
            const account = githubAccounts.find(
              (a) => a.account_id === repository.provider_account_id,
            );
            const isDisconnected =
              repositoryAccountsState === "ready" &&
              account?.state !== "connected";
            return isDisconnected === disconnectedGroup;
          })
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
                ? `${repository.name} as ${account?.login ?? "Unavailable"}`
                : repository.name;
            return `<div class="native-repository-row repository-monitoring-row"><button type="button" class="repository-open" aria-label="${escape(label)}" data-dirty="${!sameResource(
              repository,
              saved.repositories?.find((r) => r.id === repository.id),
            )}" data-repository="${escape(repository.id)}" data-focus-key="repository:${escape(repository.id)}:settings">
          <span class="settings-row-copy"><strong>${escape(repository.name)}</strong><small>${repository.provider === "github" ? "GitHub" : "Azure DevOps"} / ${escape(account?.login ?? "Unavailable")}</small><small data-repository-monitoring-detail="${escape(repository.id)}">${escape(repositoryMonitoringDetail(committed))}</small></span>
          <span class="settings-row-value" data-monitoring-state="${state.toLowerCase()}">${state}${
            sameResource(
              repository,
              saved.repositories?.find((r) => r.id === repository.id),
            )
              ? ""
              : " / Draft"
          }</span><span aria-hidden="true">›</span></button><button type="button" class="repository-monitoring-toggle" role="switch" aria-checked="${committed.enabled}" aria-label="Monitor ${escape(label)}" data-toggle-repository="${escape(repository.id)}" data-focus-key="repository:${escape(repository.id)}:monitoring" ${busy ? "disabled" : ""}>${committed.enabled ? "Disable" : "Enable"}</button></div>`;
          })
          .join("");
      disconnected.innerHTML = rowsFor(true);
      disconnected.hidden = !disconnected.innerHTML;
      list.innerHTML = rowsFor(false);
      list.hidden = !list.innerHTML;
      accounts.innerHTML =
        repositoryAccountsState === "unavailable"
          ? '<p role="alert">Unavailable</p><button data-retry-accounts>Retry</button>'
          : repositoryAccountsState === "loading"
            ? '<p role="status">Loading</p>'
            : githubAccounts
                .map(
                  (account) =>
                    `<div class="native-repository-row"><span class="settings-row-copy"><strong>${escape(account.login)}</strong><small>GitHub${account.state === "connected" ? "" : " / Reconnect required"}</small></span><button type="button" data-browse-account="${escape(account.account_id)}" aria-label="Browse repositories as ${escape(account.login)}" ${account.state === "connected" ? "" : "disabled"}><svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="10" cy="10" r="6"/><path d="m15 15 6 6"/></svg></button></div>`,
                )
                .join("");
      accounts
        .querySelector<HTMLButtonElement>("[data-retry-accounts]")
        ?.addEventListener("click", () =>
          window.dispatchEvent(
            new Event("pr-sniper:refresh-provider-accounts"),
          ),
        );
      const lists = [list, disconnected];
      lists
        .flatMap((l) => [
          ...l.querySelectorAll<HTMLButtonElement>("[data-toggle-repository]"),
        ])
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
      lists
        .flatMap((l) => [
          ...l.querySelectorAll<HTMLButtonElement>("[data-repository]"),
        ])
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
      throw "Reconnect required";
    let repository = repositories().find(
      (r) =>
        r.provider === "github" &&
        r.provider_account_id === accountId &&
        r.provider_repository_id === resolved.repository.id,
    );
    if (!repository) {
      const value: ConfiguredRepository = {
        id: newIdentity(),
        name: resolved.repository.name,
        enabled: false,
        provider: "github",
        provider_account_id: accountId,
        provider_repository_id: resolved.repository.id,
      };
      await commitResource(
        repositoryEdit(value),
        modal,
        resolved.account_generation,
      );
      pendingSetup.add(value.id);
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

  function editRepository(opener: HTMLElement) {
    const modal = dialog(
      "Add repository by URL",
      `<form><label>Repository URL<input name="repository" required placeholder="https://github.com/owner/repository" autocomplete="off" /></label>
      <div class="repository-actor"><p role="status" id="repository-actor-status"></p><button type="button" data-change-account hidden>Change</button></div>
      <label data-account-choice hidden>Acting GitHub account<select name="account" aria-describedby="repository-actor-status"></select></label>
      <div class="settings-actions"><button type="button" data-recover-account hidden>Connect GitHub account</button><button type="button" data-retry-account hidden>Retry reading accounts</button></div>
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
    let selectedId = "";
    let selectedLogin = githubAccounts.find(
      (a) => a.account_id === selectedId,
    )?.login;
    let choosing = false;
    let choiceRequired = false;
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
        alert.textContent = "Changed";
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
          ? "Coming soon"
          : provider === "invalid"
            ? "Invalid URL"
            : "Unsupported"
        : repositoryAccountsState === "unavailable"
          ? "Unavailable"
          : !ready
            ? "Loading"
            : actor
              ? `GitHub / ${actor.login}${actor.warning ? " / Retry verification" : ""}`
              : selectedId
                ? `${selectedLogin ?? "Saved account"}: ${unavailable?.reason ? githubAccountFailureMessage(unavailable.reason) : "Reconnect required"}`
                : available.length
                  ? "Choose an account"
                  : reconnect
                    ? "Reconnect required"
                    : "Not connected";
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
        alert.textContent = "Unavailable";
        alert.hidden = false;
        return;
      }
      const recovery = dialog(
        "GitHub repository accounts",
        "<div data-repository-auth></div>",
        recover,
      );
      compactEditor(recovery, "Back");
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
          throw "Unsupported";
        if (repositoryAccountsState !== "ready" || !actor)
          throw "Not connected";
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
          throw "Reconnect required";
        await addAndConfigure(accountId, resolved, modal, opener);
      } catch (cause) {
        if (modal.open && read === generation) {
          submitting = false;
          alert.textContent = reason(cause);
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
      `<label>Repository owner<select data-owner aria-label="Repository owner" disabled><option value="">Choose an owner</option></select></label>
      <label>Find a repository<input type="search" data-owner-search placeholder="Search" disabled /></label>
      <p role="status">Loading</p><p role="alert" hidden></p><button data-retry hidden>Retry</button>
      <div data-owner-results class="native-repository-list"></div>`,
      opener,
    );
    compactEditor(modal, "Back");
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
          ? `${results.length} repositories; incomplete`
          : !results.length
            ? "None"
            : !filtered.length
              ? "No matches"
              : `${results.length} repositories`;
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
              if (!connected()) throw "Reconnect required";
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
                throw "Repository changed";
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
      status.textContent = loaded ? "Lookup failed" : "Unavailable";
      alert.textContent = sameBinding ? reason(cause) : "Failed";
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
          ? "Refreshing"
          : "Loading"
        : "Loading";
      try {
        if (!connected()) throw "Reconnect required";
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
          status.textContent = incomplete ? "Incomplete" : "Choose an owner";
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
      repository.name,
      `<section class="repository-identity"><h3>${escape(repository.name)}</h3><p>${escape(repository.provider === "github" ? `GitHub / ${githubAccounts.find((account) => account.account_id === repository.provider_account_id)?.login ?? (repository.provider_account_id ? "Unavailable" : "No account")}` : "Azure DevOps")}</p></section>
        <section class="repository-group"><label class="repository-check"><input type="checkbox" role="switch" aria-label="Monitor ${escape(repository.name)}" data-repository-enabled ${repository.enabled || (pendingSetup.has(repository.id) && !setupMonitoringOff.has(repository.id)) ? "checked" : ""} /><span>Monitor this repository</span></label><p data-repository-monitoring-state role="status"></p><p data-repository-monitoring-detail></p><label class="repository-check"><input type="checkbox" role="switch" aria-label="Review drafts" data-review-drafts ${repository.review_drafts ? "checked" : ""} /><span>Review drafts</span></label></section>
        <section class="repository-group admission-group"><label class="repository-check" data-admission-all><input type="checkbox" role="switch" aria-label="All pull requests" data-admission-all-input /><span>All pull requests</span></label><button type="button" class="doctrine-row" data-open-filters><span>Filtered pull requests</span><span class="doctrine-summary" data-filter-summary></span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg></button></section>
        <div class="admission-view" data-admission-view hidden></div>
        <section class="repository-group"><div class="section-actions"><h2>Agents on this repository</h2><button class="primary" data-assign-agent>Assign agent</button></div>
        <div class="assignment-list"></div>
        </section><section class="repository-group" data-global-schedule><h2>Saved global schedule</h2><p>${schedule.kind === "cron" ? `<code>${escape(schedule.expression)}</code>` : `Every ${schedule.minutes} minutes`} / ${escape(schedule.timezone)}</p></section>
        <details class="repository-group"><summary>Repository</summary><dl class="repository-binding-details"><dt>Account ID</dt><dd>${escape(repository.provider_account_id ?? "Unbound")}</dd><dt>Repository ID</dt><dd>${escape(repository.provider_repository_id ?? "Not verified")}</dd></dl><div class="settings-actions"><button data-unbind-repository ${repository.provider_account_id ? "" : "disabled"}>Unbind account</button></div><div class="repository-remove"><input type="text" data-remove-confirm aria-label="Type repository name to confirm" placeholder="${escape(repository.name)}" autocomplete="off" /><button type="button" id="remove-repository" aria-label="Remove repository" title="Remove repository" disabled>${icon("trash")}</button></div><div class="connection"></div></details>
        <p role="status" data-explicit-pr-status ${pendingPull ? "" : "hidden"}>${pendingPull ? `PR #${pendingPull.number} queued on Save` : ""}</p>
        <p role="alert" data-earlier-intake-error hidden></p>
        <p role="alert" data-resource-error ${pendingPull?.failure ? "" : "hidden"}>${escape(pendingPull?.failure ?? "")}</p><div class="resource-actions"><button class="primary" data-save-repository>Save repository</button><button data-cancel-repository aria-label="Cancel repository changes">Cancel</button></div>`,
      opener,
    );
    compactEditor(modal, "Back");
    modal.classList.add("repository-editor");
    const currentEditor = () =>
      modal.open &&
      modal.isConnected &&
      !app.closest("[hidden]") &&
      !modal.closest('[aria-hidden="true"]');
    const updateIntake = (
      id: string,
      intent: ExplicitPullRequestIntent,
      message: string,
      failed: boolean,
    ) => {
      if (!currentEditor()) return false;
      if (
        id !== repository.id ||
        intent !== pendingPull ||
        pendingPullRequests.get(id) !== intent
      ) {
        if (!failed) return false;
        const notice = modal.querySelector<HTMLElement>(
          "[data-earlier-intake-error]",
        )!;
        notice.textContent = message;
        notice.hidden = false;
        return true;
      }
      const alert = modal.querySelector<HTMLElement>("[data-resource-error]")!;
      if (failed) {
        alert.textContent = message;
        alert.hidden = false;
      } else {
        const status = modal.querySelector<HTMLElement>(
          "[data-explicit-pr-status]",
        )!;
        status.textContent = message;
        status.hidden = false;
        alert.hidden = true;
      }
      return true;
    };
    updateRepositoryIntake = updateIntake;
    modal.addEventListener(
      "pr-sniper:dialog-closed",
      () => {
        if (updateRepositoryIntake === updateIntake)
          updateRepositoryIntake = undefined;
      },
      { once: true },
    );
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
          ? "Starts on Save"
          : "Disabled"
        : repositoryMonitoringDetail(committed);
    };
    const refreshSavedMonitoring = () => {
      updateMonitoring();
      refreshRepositoryRows?.();
    };
    updateMonitoring();
    void refreshRepositoryAutomation(updateMonitoring);
    modal.querySelector<HTMLInputElement>("[data-review-drafts]")!.onchange = (
      event,
    ) => {
      if ((event.target as HTMLInputElement).checked)
        repository.review_drafts = true;
      else delete repository.review_drafts;
      changed();
    };
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
    modal.querySelector<HTMLButtonElement>("[data-save-repository]")!.onclick =
      async () => {
        const requestedPull = pendingPullRequests.get(repository.id);
        const saveRoute = routeGeneration;
        try {
          const proposed = clone(repository);
          proposed.enabled = monitoring.checked;
          await commitResource(repositoryEdit(repository, proposed), modal);
          pendingSetup.delete(repository.id);
          setupMonitoringOff.delete(repository.id);
          refreshSavedMonitoring();
          if (requestedPull) {
            if (pendingPullRequests.get(repository.id) !== requestedPull)
              throw `PR #${requestedPull.number} replaced`;
            const committed = saved.repositories?.find(
              (r) => r.id === repository.id,
            );
            if (
              !committed ||
              committed.provider_account_id !== requestedPull.accountId ||
              committed.provider_repository_id !== requestedPull.repositoryId
            )
              throw "Account changed";
            const saveButton = modal.querySelector<HTMLButtonElement>(
              "[data-save-repository]",
            )!;
            saveButton.disabled = true;
            try {
              const admission = await invoke<{
                queued: boolean;
                message: string;
              }>("admit_explicit_pull_request", {
                expected: committed,
                number: requestedPull.number,
                accountGeneration: requestedPull.generation,
              });
              if (pendingPullRequests.get(repository.id) === requestedPull) {
                if (
                  requestedPull.failure &&
                  error.textContent === requestedPull.failure
                )
                  clearError();
                delete requestedPull.failure;
                updateRepositoryIntake?.(
                  repository.id,
                  requestedPull,
                  admission.queued
                    ? `PR #${requestedPull.number} queued`
                    : `PR #${requestedPull.number} pending`,
                  false,
                );
                if (admission.queued) pendingPullRequests.delete(repository.id);
              }
            } catch (cause) {
              const ownsIntent =
                pendingPullRequests.get(repository.id) === requestedPull;
              const message = `PR #${requestedPull.number}: ${reason(cause)}`;
              if (ownsIntent) requestedPull.failure = message;
              if (
                !updateRepositoryIntake?.(
                  repository.id,
                  requestedPull,
                  message,
                  true,
                )
              )
                showError(message);
              if (repositorySessionFailure(cause))
                window.dispatchEvent(
                  new Event("pr-sniper:refresh-provider-accounts"),
                );
            } finally {
              saveButton.disabled = false;
            }
            return;
          }
          if (currentEditor() && saveRoute === routeGeneration) {
            modal.close();
            render();
          }
        } catch (cause) {
          if (currentEditor() && saveRoute === routeGeneration) {
            const alert = modal.querySelector<HTMLElement>(
              "[data-resource-error]",
            )!;
            alert.textContent = reason(cause);
            alert.hidden = false;
          } else {
            showError(`${repository.name}: ${reason(cause)}`);
          }
        }
      };
    modal.querySelector<HTMLButtonElement>(
      "[data-cancel-repository]",
    )!.onclick = () => {
      if (pendingPull && pendingPullRequests.get(repository.id) === pendingPull)
        pendingPullRequests.delete(repository.id);
      acceptResource(draft, clone(saved), repositoryEdit(repository));
      if (!repositories().some((item) => item.id === repository.id))
        opener.dataset.focusKey = `repository:${repository.name}:settings`;
      modal.close();
      render();
    };
    renderAssignments();
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
    else modal.querySelector(".connection")!.textContent = "Unsaved";
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
    const removeButton =
      modal.querySelector<HTMLButtonElement>("#remove-repository")!;
    const removeConfirm = modal.querySelector<HTMLInputElement>(
      "[data-remove-confirm]",
    )!;
    removeConfirm.oninput = () => {
      removeButton.disabled = removeConfirm.value.trim() !== repository.name;
    };
    removeButton.onclick = async () => {
      if (removeConfirm.value.trim() !== repository.name) return;
      try {
        await commitResource(repositoryEdit(repository, null), modal);
        pendingPullRequests.delete(repository.id);
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
      "[data-unbind-repository]",
    )!.onclick = (event) => {
      const persisted = saved.repositories?.find((r) => r.id === repository.id);
      if (!persisted || !sameResource(repository, persisted)) {
        const alert = modal.querySelector<HTMLElement>(
          "[data-resource-error]",
        )!;
        alert.textContent = "Unsaved changes";
        alert.hidden = false;
        return;
      }
      const confirm = dialog(
        "Unbind repository account?",
        `<p>Unbind ${escape(repository.name)}?</p><p role="alert" hidden></p><button class="primary" data-confirm-unbind>Unbind account</button>`,
        event.currentTarget as HTMLButtonElement,
      );
      compactEditor(confirm, "Back");
      confirm.querySelector<HTMLButtonElement>(
        "[data-confirm-unbind]",
      )!.onclick = async () => {
        const next = clone(persisted);
        delete next.provider_account_id;
        delete next.provider_repository_id;
        try {
          await commitResource(repositoryEdit(repository, next), confirm);
          pendingPullRequests.delete(repository.id);
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
        list.innerHTML = "";
        restoreFocus();
        return;
      }
      list.innerHTML = "";
      for (const assignment of assignments) {
        const agent = agents().find((a) => a.id === assignment.agent_id);
        const row = document.createElement("div");
        row.className = "assignment-row";
        const primary = primaryAssignmentId(repository) === assignment.id;
        row.innerHTML = `<div><strong>${escape(agent?.name ?? "Deleted agent")}</strong><p>${primary ? `Primary${assignments.length === 1 ? " (automatic)" : ""} \u00b7 ` : "Secondary \u00b7 "}${assignment.comment ? "Publish Comment" : "Initial findings local"}${primary && assignment.actions?.reply ? " \u00b7 Reply Comment" : ""}${primary && assignment.actions?.merge ? " \u00b7 Approve & Merge" : primary && assignment.actions?.approve ? " \u00b7 Approve" : ""}${assignment.tools?.length ? ` \u00b7 ${assignment.tools.length} Tools` : ""}</p><p>${escape(agent?.model ?? "Unavailable")}</p></div><button data-edit aria-label="Edit assignment">Edit</button><button data-remove aria-label="Remove assignment" title="Remove">${icon("trash")}</button>`;
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
    const defaultAdmission = (): Admission => {
      const authors = repository.watched_authors ?? [];
      const rule = (
        kind: AdmissionKind,
        enabled: boolean,
        extra: Partial<AdmissionRule> = {},
      ): AdmissionRule => ({ kind, enabled, ...extra });
      return {
        mode: authors.length ? "filtered" : "all",
        rules: [
          rule("assigned", true),
          rule("mentioned", true),
          rule("include_users", authors.length > 0, { users: [...authors] }),
          rule("exclude_users", false, { users: [] }),
          rule("include_paths", false, { paths: [] }),
          rule("exclude_paths", false, { paths: [] }),
        ],
      };
    };
    // Reads never mutate the draft; the first edit materializes the default.
    let shown: Admission | undefined;
    const admission = (): Admission =>
      repository.admission ?? (shown ??= defaultAdmission());
    const editAdmission = (): Admission =>
      (repository.admission ??= shown ?? defaultAdmission());
    const kindLabels: Record<AdmissionKind, string> = {
      assigned: "Assigned to me for review",
      mentioned: "Mentioning me in comments",
      include_users: "Include users",
      exclude_users: "Exclude users",
      include_paths: "Include paths",
      exclude_paths: "Exclude paths",
    };
    const valueKind = (kind: AdmissionKind) =>
      kind.endsWith("_users") || kind.endsWith("_paths");
    const ruleCount = (rule: AdmissionRule) =>
      rule.users?.length ?? rule.paths?.length ?? 0;
    const chevron =
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg>';

    function setupAdmission() {
      const allInput = modal.querySelector<HTMLInputElement>(
        "[data-admission-all-input]",
      )!;
      const openFilters = modal.querySelector<HTMLButtonElement>(
        "[data-open-filters]",
      )!;
      const view = modal.querySelector<HTMLElement>("[data-admission-view]")!;
      const route: string[] = [];
      const sync = () => {
        const a = admission();
        allInput.checked = a.mode === "all";
        openFilters.disabled = a.mode === "all";
        const on = a.rules.filter((r) => r.enabled).length;
        modal.querySelector("[data-filter-summary]")!.textContent =
          a.mode === "all" ? "" : `${on} on`;
      };
      const dropTop = () => {
        for (let i = editorStack.length - 1; i >= 0; i--)
          if (editorStack[i].modal === modal && editorStack[i].admission) {
            editorStack.splice(i, 1);
            break;
          }
        announceEditor();
      };
      const pop = () => {
        route.pop();
        if (options.embedded) dropTop();
        const next = route[route.length - 1];
        if (!next) {
          view.hidden = true;
          view.replaceChildren();
          delete modal.dataset.view;
          sync();
          openFilters.focus({ preventScroll: true });
        } else if (next === "filters") {
          renderFilters();
          if (!options.embedded) {
            const back = document.createElement("button");
            back.type = "button";
            back.className = "admission-back";
            back.setAttribute("aria-label", "Back from Filtered pull requests");
            back.innerHTML = `${icon("back")}<span>Filtered pull requests</span>`;
            back.onclick = () => pop();
            view.prepend(back);
          }
        }
      };
      const push = (title: string, name: string, render: () => void) => {
        route.push(name);
        view.hidden = false;
        modal.dataset.view = "admission";
        render();
        if (!options.embedded) {
          const back = document.createElement("button");
          back.type = "button";
          back.className = "admission-back";
          back.setAttribute("aria-label", `Back from ${title}`);
          back.innerHTML = `${icon("back")}<span>${escape(title)}</span>`;
          back.onclick = () => pop();
          view.prepend(back);
        }
        if (options.embedded) {
          editorStack.push({ modal, title, back: pop, admission: true });
          announceEditor();
        }
      };
      allInput.onchange = () => {
        editAdmission().mode = allInput.checked ? "all" : "filtered";
        changed();
        sync();
      };

      function renderFilters() {
        const a = admission();
        view.replaceChildren();
        const group = document.createElement("section");
        group.className = "repository-group";
        a.rules.forEach((rule, index) => {
          const label = kindLabels[rule.kind];
          const row = document.createElement("div");
          row.className = "admission-rule";
          row.dataset.rule = rule.kind;
          row.innerHTML = `<button type="button" class="admission-open" ${valueKind(rule.kind) ? "" : "disabled"} aria-label="${escape(label)}${valueKind(rule.kind) ? ": edit values" : ""}"><span class="admission-label">${escape(label)}</span><span class="doctrine-summary">${valueKind(rule.kind) ? String(ruleCount(rule)) : ""}</span>${valueKind(rule.kind) ? chevron : ""}</button><button type="button" class="admission-move" data-up aria-label="Move ${escape(label)} up" title="Move up" ${index === 0 ? "disabled" : ""}>${icon("up")}</button><button type="button" class="admission-move" data-down aria-label="Move ${escape(label)} down" title="Move down" ${index === a.rules.length - 1 ? "disabled" : ""}>${icon("down")}</button><input type="checkbox" role="switch" class="admission-switch" aria-label="${escape(label)}" ${rule.enabled ? "checked" : ""} />`;
          row.querySelector<HTMLInputElement>(".admission-switch")!.onchange = (
            event,
          ) => {
            editAdmission();
            rule.enabled = (event.target as HTMLInputElement).checked;
            changed();
          };
          const move = (delta: number) => {
            editAdmission();
            const target = index + delta;
            [a.rules[index], a.rules[target]] = [
              a.rules[target],
              a.rules[index],
            ];
            changed();
            renderFilters();
            view
              .querySelector<HTMLElement>(
                `[data-rule="${rule.kind}"] ${delta < 0 ? "[data-up]" : "[data-down]"}`,
              )
              ?.focus({ preventScroll: true });
          };
          row.querySelector<HTMLElement>("[data-up]")!.onclick = () => move(-1);
          row.querySelector<HTMLElement>("[data-down]")!.onclick = () =>
            move(1);
          if (valueKind(rule.kind))
            row.querySelector<HTMLElement>(".admission-open")!.onclick = () =>
              push(label, `rule:${rule.kind}`, () => renderRule(rule));
          group.append(row);
        });
        view.append(group);
      }

      function renderRule(rule: AdmissionRule) {
        view.replaceChildren();
        const group = document.createElement("section");
        group.className = "repository-group";
        const list = document.createElement("div");
        list.className = "admission-values";
        const users = rule.kind.endsWith("_users");
        const draw = () => {
          list.replaceChildren();
          const values: string[] = users
            ? (rule.users ?? []).map((u) => `@${u.login}`)
            : (rule.paths ?? []);
          values.forEach((value, index) => {
            const row = document.createElement("div");
            row.className = "watchlist-row";
            row.innerHTML = `<span>${users ? escape(value) : `<code>${escape(value)}</code>`}</span><button type="button" data-remove aria-label="Remove ${escape(value)}" title="Remove">${icon("trash")}</button>`;
            row.querySelector<HTMLButtonElement>("[data-remove]")!.onclick =
              () => {
                editAdmission();
                if (users)
                  rule.users = (rule.users ?? []).filter((_, i) => i !== index);
                else
                  rule.paths = (rule.paths ?? []).filter((_, i) => i !== index);
                changed();
                draw();
                // The removed row's trash button is gone: land on the add control.
                view
                  .querySelector<HTMLElement>(
                    ".admission-add, .admission-path-form input",
                  )
                  ?.focus({ preventScroll: true });
              };
            list.append(row);
          });
        };
        group.append(list);
        if (users) {
          const add = document.createElement("button");
          add.type = "button";
          add.className = "primary admission-add";
          add.textContent = "Add people";
          add.onclick = () =>
            addPersonDialog(add, repository, rule.users ?? [], (identity) => {
              editAdmission();
              rule.users = [...(rule.users ?? []), identity];
              rule.enabled = true;
              changed();
              draw();
            });
          view.append(group, add);
        } else {
          const form = document.createElement("form");
          form.className = "admission-path-form";
          form.innerHTML = `<input name="path" aria-label="Path pattern" placeholder="docs/ or src/**/*.ts" autocomplete="off" /><button type="submit" class="primary">Add</button><p role="alert" hidden></p>`;
          form.onsubmit = (event) => {
            event.preventDefault();
            const input = form.querySelector<HTMLInputElement>("[name=path]")!;
            const alert = form.querySelector<HTMLElement>("[role=alert]")!;
            const value = input.value.trim();
            const problem = !value
              ? "Path required"
              : value.length > 256 ||
                  value.startsWith("/") ||
                  value.split("/").includes("..")
                ? "Invalid path"
                : (rule.paths ?? []).includes(value)
                  ? "Already added"
                  : "";
            alert.hidden = !problem;
            alert.textContent = problem;
            if (problem) return;
            editAdmission();
            rule.paths = [...(rule.paths ?? []), value];
            rule.enabled = true;
            changed();
            input.value = "";
            draw();
          };
          view.append(group, form);
        }
        draw();
      }

      openFilters.onclick = () =>
        push("Filtered pull requests", "filters", renderFilters);
      sync();
    }
    setupAdmission();
  }

  function assignAgentDialog(
    opener: HTMLElement,
    repository: ConfiguredRepository,
    onSaved: () => void,
    existing?: Assignment,
  ) {
    const chevron =
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 5 7 7-7 7"/></svg>';
    const assignmentId = existing?.id ?? newIdentity();
    const sole =
      (repository.assignments?.length ?? 0) + (existing ? 0 : 1) === 1;
    const isPrimary = sole || primaryAssignmentId(repository) === assignmentId;
    const modal = dialog(
      existing ? "Edit assignment" : "Assign agent",
      `<form><input type="hidden" name="agent" value="${escape(existing?.agent_id ?? "")}" />
        <section class="repository-group" data-assign-main><button type="button" class="doctrine-row" data-open-agent aria-label="Choose Agent"><span>Agent</span><span class="doctrine-summary" data-agent-summary></span>${chevron}</button>
        <label class="repository-check"><input type="checkbox" role="switch" name="primary" aria-label="Primary" ${isPrimary ? "checked" : ""} ${sole ? "disabled" : ""} /><span>Primary</span></label>
        <button type="button" class="doctrine-row" data-open-permissions aria-label="Permissions"><span>Permissions</span><span class="doctrine-summary" data-permission-summary></span>${chevron}</button>
        <button type="button" class="doctrine-row" data-open-tools aria-label="Tools"><span>Tools</span><span class="doctrine-summary" data-tool-summary></span>${chevron}</button></section>
        <section class="repository-group" data-assign-agents hidden><div data-agent-choices role="radiogroup" aria-label="Agent"></div><div class="resource-actions"><button type="button" data-create-agent>Create new Agent</button></div></section>
        <section class="repository-group" data-assign-permissions hidden>
        <label class="repository-check"><input type="checkbox" role="switch" name="comment" aria-label="Publish Comment" ${existing?.comment ? "checked" : ""} /><span>Publish Comment</span></label>
        <div data-primary-permissions ${isPrimary ? "" : "hidden"}><label class="repository-check"><input type="checkbox" role="switch" name="reply" aria-label="Reply Comment" ${isPrimary && existing?.actions?.reply ? "checked" : ""} /><span>Reply Comment</span></label>
        <div class="permission-row-line"><span>Approval</span><div class="permission-choices" role="radiogroup" aria-label="Approval and merge" data-value="${isPrimary && existing?.actions?.merge ? "merge" : isPrimary && existing?.actions?.approve ? "approve" : "none"}"><label class="segment"><input type="radio" name="action" value="none" aria-label="Neither" ${!isPrimary || (!existing?.actions?.approve && !existing?.actions?.merge) ? "checked" : ""} /><span>off</span></label><label class="segment"><input type="radio" name="action" value="approve" aria-label="Approve" ${isPrimary && existing?.actions?.approve && !existing?.actions?.merge ? "checked" : ""} /><span>Approve</span></label><label class="segment"><input type="radio" name="action" value="merge" aria-label="Approve &amp; Merge" ${isPrimary && existing?.actions?.merge ? "checked" : ""} /><span>&amp; Merge</span></label></div></div>
        <label class="repository-check" data-include-self ${isPrimary && existing?.actions?.merge ? "" : "hidden"}><input type="checkbox" role="switch" name="include_self" aria-label="Include self" ${existing?.actions?.include_self ? "checked" : ""} /><span>Include self</span></label></div></section>
        <section class="repository-group" data-assign-tools hidden>${(Object.keys(toolRows) as PacketTool[]).map((id) => `<label class="repository-check tool-row" data-tool-row="${id}"><input type="checkbox" role="switch" name="tool" value="${id}" aria-label="${escape(toolRows[id].name)}" ${existing?.tools?.includes(id) ? "checked" : ""} /><span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${toolRows[id].path}</svg><span class="tool-copy"><span class="tool-name">${escape(toolRows[id].name)}</span><small class="tool-detail" data-tool-detail>${escape(toolRows[id].detail)}</small></span></span></label>`).join("")}</section>
        <p role="alert" hidden></p><div class="resource-actions" data-assign-actions><button class="primary">${existing ? "Save assignment" : "Assign agent"}</button><button type="button" data-cancel-resource>Cancel</button></div></form>`,
      opener,
    );
    resourceEditor(modal);
    compactEditor(modal, "Back");
    modal.classList.add("repository-editor", "assignment-editor");
    const form = modal.querySelector<HTMLFormElement>("form")!;
    const agentInput = modal.querySelector<HTMLInputElement>("[name=agent]")!;
    const openAgent =
      modal.querySelector<HTMLButtonElement>("[data-open-agent]")!;
    const openPermissions = modal.querySelector<HTMLButtonElement>(
      "[data-open-permissions]",
    )!;
    const openTools =
      modal.querySelector<HTMLButtonElement>("[data-open-tools]")!;
    const choices = modal.querySelector<HTMLElement>("[data-agent-choices]")!;
    const sections = {
      tools: modal.querySelector<HTMLElement>("[data-assign-tools]")!,
      agent: modal.querySelector<HTMLElement>("[data-assign-agents]")!,
      permissions: modal.querySelector<HTMLElement>(
        "[data-assign-permissions]",
      )!,
    };
    const permissionCount = () =>
      modal.querySelectorAll(
        "[name=comment]:checked, [name=reply]:checked, [name=action]:checked:not([value=none])",
      ).length;
    const viewTitles = {
      agent: "Agent",
      permissions: "Permissions",
      tools: "Tools",
    };
    // Unset tools show the role default until a row is toggled.
    let toolsExplicit = !!existing?.tools;
    // Tools follow role and grants: out-of-role rows hide, ungranted rows clear.
    const syncTools = () => {
      const isPrimaryNow =
        modal.querySelector<HTMLInputElement>("[name=primary]")!.checked;
      const checked = (name: string) =>
        modal.querySelector<HTMLInputElement>(`[name=${name}]`)!.checked;
      const action = modal.querySelector<HTMLInputElement>(
        "[name=action]:checked",
      )!.value;
      const granted: Partial<Record<PacketTool, boolean>> = {
        leave_comments: checked("comment"),
        approve_pull_request: action !== "none",
        approve_and_merge: action === "merge",
      };
      const defaults = defaultTools(isPrimaryNow, {
        comment: !!granted.leave_comments,
        approve: !!granted.approve_pull_request,
        merge: !!granted.approve_and_merge,
      });
      let count = 0;
      for (const row of modal.querySelectorAll<HTMLElement>(
        "[data-tool-row]",
      )) {
        const id = row.dataset.toolRow as PacketTool;
        const scope = toolRows[id].scope;
        const input = row.querySelector<HTMLInputElement>("input")!;
        if (!toolsExplicit) input.checked = defaults.includes(id);
        row.hidden =
          (scope === "primary" && !isPrimaryNow) ||
          (scope === "secondary" && isPrimaryNow);
        input.disabled = granted[id] === false;
        if (row.hidden || input.disabled) input.checked = false;
        if (input.checked) count += 1;
      }
      modal.querySelector<HTMLElement>("[data-include-self]")!.hidden =
        !isPrimaryNow || action !== "merge";
      if (action !== "merge")
        modal.querySelector<HTMLInputElement>("[name=include_self]")!.checked =
          false;
      openTools.querySelector("[data-tool-summary]")!.textContent =
        String(count);
    };
    const refreshSelection = (selected = agentInput.value) => {
      syncTools();
      agentInput.value = selected;
      const agent = agents().find((agent) => agent.id === selected);
      const rows = new Map(
        [...choices.querySelectorAll<HTMLElement>("[data-agent-id]")].map(
          (row) => [row.dataset.agentId!, row],
        ),
      );
      choices.querySelector("[data-agent-missing]")?.remove();
      for (const a of agents()) {
        let row = rows.get(a.id);
        if (!row) {
          const holder = document.createElement("div");
          holder.innerHTML = `<div class="agent-choice" data-agent-choice data-agent-id="${escape(a.id)}"><label class="repository-check"><span></span><input type="radio" name="agent-choice" value="${escape(a.id)}" /></label><button type="button" data-edit-agent="${escape(a.id)}" title="Edit Agent">${icon("pencil")}</button></div>`;
          row = holder.firstElementChild as HTMLElement;
          choices.append(row);
        }
        rows.delete(a.id);
        row.querySelector("span")!.textContent = a.name;
        const radio = row.querySelector<HTMLInputElement>("input")!;
        radio.setAttribute("aria-label", a.name);
        radio.checked = a.id === selected;
        row
          .querySelector("[data-edit-agent]")!
          .setAttribute("aria-label", `Edit Agent ${a.name}`);
      }
      for (const stale of rows.values()) stale.remove();
      if (selected && !agent)
        choices.insertAdjacentHTML(
          "beforeend",
          `<div class="agent-choice" data-agent-missing><label class="repository-check"><span>Unavailable</span><input type="radio" name="agent-choice" checked disabled /></label></div>`,
        );
      openAgent.querySelector("[data-agent-summary]")!.textContent =
        selected && !agent ? "Unavailable" : (agent?.name ?? "");
      const checkedAction = modal.querySelector<HTMLInputElement>(
        "[name=action]:checked",
      );
      if (checkedAction)
        modal.querySelector<HTMLElement>(".permission-choices")!.dataset.value =
          checkedAction.value;
      openPermissions.querySelector("[data-permission-summary]")!.textContent =
        String(permissionCount());
    };
    refreshSelection(existing?.agent_id ?? "");
    const closeView = () => {
      const view = form.dataset.view as keyof typeof sections | undefined;
      if (!view) return;
      delete form.dataset.view;
      for (const section of Object.values(sections)) section.hidden = true;
      modal.querySelector<HTMLElement>("[data-assign-main]")!.hidden = false;
      modal.querySelector<HTMLElement>("[data-assign-actions]")!.hidden = false;
      dropEditor((e) => e.modal === modal && e.title === viewTitles[view]);
      refreshSelection();
      ({ agent: openAgent, permissions: openPermissions, tools: openTools })[
        view
      ].focus({
        preventScroll: true,
      });
    };
    const openView = (view: keyof typeof sections, title: string) => {
      form.dataset.view = view;
      modal.querySelector<HTMLElement>("[data-assign-main]")!.hidden = true;
      modal.querySelector<HTMLElement>("[data-assign-actions]")!.hidden = true;
      sections[view].hidden = false;
      sections[view].querySelector<HTMLElement>("[data-back]")?.remove();
      if (options.embedded) {
        editorStack.push({ modal, title, back: closeView });
        announceEditor();
      } else {
        const back = document.createElement("button");
        back.type = "button";
        back.className = "admission-back";
        back.dataset.back = "";
        back.setAttribute("aria-label", `Back from ${title}`);
        back.innerHTML = `${icon("back")}<span>${escape(title)}</span>`;
        back.onclick = closeView;
        sections[view].prepend(back);
      }
      (
        sections[view].querySelector<HTMLElement>("input:checked") ??
        sections[view].querySelector<HTMLElement>("input, button")
      )?.focus({ preventScroll: true });
    };
    openAgent.onclick = () => openView("agent", "Agent");
    openPermissions.onclick = () => openView("permissions", "Permissions");
    openTools.onclick = () => openView("tools", "Tools");
    const agentSaved = (agent?: Agent) => {
      if (!modal.isConnected) return;
      refreshSelection(agent?.id ?? agentInput.value);
      onSaved();
    };
    choices.onchange = (event) => {
      const input = event.target as HTMLInputElement;
      if (input.name !== "agent-choice") return;
      refreshSelection(input.value);
      closeView();
    };
    choices.onclick = (event) => {
      const pencil = (event.target as Element).closest<HTMLButtonElement>(
        "[data-edit-agent]",
      );
      const selected = agents().find((a) => a.id === pencil?.dataset.editAgent);
      if (pencil && selected) editAgent(pencil, selected, agentSaved);
    };
    modal.querySelector<HTMLButtonElement>("[data-create-agent]")!.onclick = (
      event,
    ) =>
      editAgent(
        event.currentTarget as HTMLButtonElement,
        undefined,
        (agent) => {
          agentSaved(agent);
          if (agent) closeView();
        },
      );
    const primary = modal.querySelector<HTMLInputElement>("[name=primary]")!;
    modal.addEventListener("change", (event) => {
      if ((event.target as HTMLInputElement).name === "tool")
        toolsExplicit = true;
      refreshSelection();
    });
    primary.onchange = () => {
      modal.querySelector<HTMLElement>("[data-primary-permissions]")!.hidden =
        !primary.checked;
      if (!primary.checked) {
        modal.querySelector<HTMLInputElement>("[name=reply]")!.checked = false;
        modal.querySelector<HTMLInputElement>(
          "[name=action][value=none]",
        )!.checked = true;
      }
    };
    const releaseDiscardGuard = warnBeforeDiscard(modal, "assignment");
    modal.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
      try {
        const agentId = agentInput.value;
        if (!agentId) throw "Agent required";
        if (!agents().some((agent) => agent.id === agentId))
          throw "Agent unavailable";
        const action = modal.querySelector<HTMLInputElement>(
          "[name=action]:checked",
        )!.value;
        const value: Assignment = {
          id: assignmentId,
          agent_id: agentId,
          schedule: existing?.schedule ?? clone(saved.defaults.schedule),
          comment:
            modal.querySelector<HTMLInputElement>("[name=comment]")!.checked,
          approve: existing?.approve ?? false,
          actions: {
            reply:
              primary.checked &&
              modal.querySelector<HTMLInputElement>("[name=reply]")!.checked,
            approve: primary.checked && action !== "none",
            merge: primary.checked && action === "merge",
            ...(primary.checked &&
            action === "merge" &&
            modal.querySelector<HTMLInputElement>("[name=include_self]")!
              .checked
              ? { include_self: true }
              : {}),
          },
        };
        const tools = [
          ...modal.querySelectorAll<HTMLInputElement>("[name=tool]:checked"),
        ].map((input) => input.value as PacketTool);
        if (toolsExplicit && tools.length) value.tools = tools;
        const next = clone(repository);
        if (existing)
          next.assignments = (next.assignments ?? []).map((a) =>
            a.id === value.id ? value : a,
          );
        else (next.assignments ??= []).push(value);
        if (primary.checked && !sole) next.primary_assignment_id = value.id;
        else if (!primary.checked && next.primary_assignment_id === value.id)
          delete next.primary_assignment_id;
        for (const assignment of next.assignments ?? []) {
          if (primaryAssignmentId(next) !== assignment.id && assignment.actions)
            assignment.actions = { reply: false, approve: false, merge: false };
        }
        await commitResource(repositoryEdit(repository, next), modal);
        Object.assign(repository, next);
        releaseDiscardGuard();
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
    people: WatchedIdentity[],
    onAdd: (identity: WatchedIdentity) => void,
  ) {
    const availableAccounts = githubAccounts.filter(
      (account) =>
        account.state === "connected" &&
        (!repository.provider_account_id ||
          account.account_id === repository.provider_account_id),
    );
    const picker = dialog(
      "Add people",
      `<form class="person-lookup"><label>Acting GitHub account<select name="account" required>${option("", "Choose a GitHub account", repository.provider_account_id ?? "")}${availableAccounts.map((account) => option(account.account_id, `${account.login} (${account.account_id})`, repository.provider_account_id ?? "")).join("")}</select></label><label>GitHub login<input name="login" placeholder="octocat" autocomplete="off" required /></label><p role="alert" hidden></p><div class="resource-actions"><button type="submit" class="primary" ${availableAccounts.length ? "" : "disabled"}>Add person</button><button type="button" data-cancel-resource>Cancel</button></div></form>`,
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
          throw "Settings changed";
        if (people.some((p) => p.id === identity.id)) throw "Already added";
        picker.close();
        onAdd(identity);
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
      ? "Updating"
      : `${saved.launch_at_login ? "On" : "Off"}; registration ${snapshot.login_registration ?? "unavailable"}${snapshot.isolated ? "; isolated" : ""}`;
  }

  function renderPreferences() {
    const schedule = draft.defaults.schedule;
    content.innerHTML = `<div class="preferences-view">
      <div class="preferences-boundary"><h2>Saved preferences</h2></div>
      <div class="settings-group preferences-saved"><fieldset aria-label="Global polling and capacity"><legend>Global polling and capacity</legend>
      <label class="preferences-capacity">AI capacity<input id="global-capacity" type="number" min="1" max="4294967295" step="1" value="${draft.capacity}" /></label>
      <div class="preferences-schedule">
      <label>Cron expression<input id="global-cron" value="${escape(schedule.kind === "cron" ? schedule.expression : "")}" placeholder="*/15 * * * *" /></label>
      <label>Schedule helper<select id="cron-helper"><option value="">Custom</option><option value="*/15 * * * *">Every 15 minutes</option><option value="0 * * * *">Every hour</option><option value="0 9 * * MON-FRI">Weekdays at 09:00</option></select></label>
      <label>Time zone<input id="global-timezone" value="${escape(schedule.timezone)}" /></label>
      </div></fieldset></div>
      <div class="preferences-boundary"><h2>Immediate controls</h2></div>
      <div class="settings-group" id="automation-settings"></div>
      <div class="settings-group" id="notification-settings"></div>
      <div class="settings-group"><fieldset aria-label="Startup"><legend>Startup</legend><label class="setting-row"><span>Open PR Sniper at login</span><input id="login" type="checkbox" role="switch" aria-describedby="login-status" disabled /></label><p id="login-status" role="status"></p><p id="login-error" role="alert" hidden></p></fieldset></div>
      <div class="settings-group"><fieldset aria-label="Status and recovery"><legend>Status and recovery</legend><button id="preferences-status" type="button">Open status and recovery</button>
      <button id="diagnostics" type="button">Open redacted diagnostics</button>
      <details id="preferences-readiness" class="preferences-readiness"><summary>Saved setup readiness</summary><p data-readiness role="status">Loading</p></details></fieldset></div></div>`;
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
        readiness.textContent = state.configuration_ready
          ? "Complete"
          : `Incomplete: ${issues.join("; ")}`;
      },
      (cause) => {
        if (readiness.isConnected) readiness.textContent = reason(cause);
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
          typeof cause === "string" ? cause : "Startup change failed",
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
          else if (!fresh.settings) errors.push("Unavailable");
        } catch {
          snapshot.login_registration = null;
          errors.push("Registration unavailable");
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
          showError(typeof cause === "string" ? cause : "Failed");
        }
      };
    content.querySelector<HTMLButtonElement>("#diagnostics")!.onclick = async (
      event,
    ) => {
      (event.currentTarget as HTMLButtonElement).focus({ preventScroll: true });
      try {
        await invoke("open_diagnostics");
      } catch {
        showError("Failed");
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
        showError("Changed elsewhere");
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
      showError("Close editors first");
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
        showError(state.error ?? "Unavailable");
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
        pendingPullRequests.clear();
        conflict = false;
      }
      catalogConflict = false;
      if (state.error) showError(state.error);
    } catch {
      showError("Reload failed");
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
      pendingPullRequests.clear();
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
        showError(state.error ?? "Unavailable");
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
      showError("Unavailable");
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
        showError("Not ready");
        return false;
      }
      if (dialogs.hasOpen()) {
        showError("Editor open");
        return false;
      }
      guidance = origin;
      returnGenie.hidden = false;
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
import { isWindows } from "./platform";
import { terseError } from "./errors";
