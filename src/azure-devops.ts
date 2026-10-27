import { invoke } from "@tauri-apps/api/core";

type Identity = { id: string; display_name: string; unique_name?: string };

export interface AzureDevopsSetup {
  cli:
    | { state: "unknown" | "missing" }
    | { state: "too_old" | "ready"; version: string }
    | { state: "installing"; version: string | null };
  minimum: string;
  installer: "homebrew" | "winget" | "page";
  organization:
    | { state: "unset" }
    | {
        state: "unknown" | "not_found" | "tenant_unknown" | "offline";
        name: string;
      }
    | { state: "tenant_known"; name: string; tenant: string };
  account:
    | {
        state:
          | "unavailable"
          | "signed_out"
          | "signing_in"
          | "interaction_required"
          | "tenant_access_blocked"
          | "offline"
          | "failed";
      }
    | { state: "connected"; identity: Identity }
    | { state: "wrong_identity"; identity: Identity; expected: Identity };
  checking: boolean;
}

const cliValues: Record<AzureDevopsSetup["cli"]["state"], string> = {
  unknown: "Checking",
  missing: "Missing",
  too_old: "Too old",
  installing: "Installing",
  ready: "Ready",
};
const organizationValues: Record<
  AzureDevopsSetup["organization"]["state"],
  string
> = {
  unset: "",
  unknown: "Checking",
  not_found: "Not found",
  tenant_unknown: "No tenant",
  offline: "Offline",
  tenant_known: "",
};
const accountValues: Record<AzureDevopsSetup["account"]["state"], string> = {
  unavailable: "",
  signed_out: "Signed out",
  signing_in: "Signing in",
  interaction_required: "Sign-in required",
  tenant_access_blocked: "Access denied",
  offline: "Offline",
  failed: "Failed",
  connected: "Connected",
  wrong_identity: "Wrong account",
};
const failures: Record<string, string> = {
  busy: "Busy",
  not_ready: "Not ready",
  storage: "Storage failed",
  browser_open: "Browser failed",
};

const svg = (paths: string) =>
  `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const icons = {
  cli: svg(
    '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m7 10 3 2-3 2M12 15h5"/>',
  ),
  organization: svg(
    '<path d="M4 20V6l8-3 8 3v14M9 20v-5h6v5M8 9h1M15 9h1M8 12h1M15 12h1"/>',
  ),
  account: svg(
    '<circle cx="12" cy="8" r="3.5"/><path d="M5 20a7 7 0 0 1 14 0"/>',
  ),
  check: svg('<path d="m5 12 5 5 9-10"/>'),
  refresh: svg(
    '<path d="M20 11a8 8 0 0 0-14-4M4 4v4h4M4 13a8 8 0 0 0 14 4m2 3v-4h-4"/>',
  ),
  forget: svg(
    '<path d="M9 7V4M15 7V4M7 7h10v4a5 5 0 0 1-10 0V7ZM12 16v4M3 3l18 18"/>',
  ),
};

const escape = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (character) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[character]!,
  );

let lastCheck = 0;
const RECHECK_AFTER_MS = 10_000;

export function azureDevopsSummary(view: AzureDevopsSetup) {
  return view.organization.state === "unset" ? "" : view.organization.name;
}

export function renderAzureDevops(
  root: HTMLElement,
  options: {
    confirmSignIn: (tenant: string, opener: HTMLElement) => Promise<boolean>;
    showError: (message: string) => void;
  },
) {
  root.innerHTML = `<div class="settings-overview azure-devops-setup"><section class="settings-home-group" aria-label="Azure DevOps"></section></div>`;
  const group = root.querySelector<HTMLElement>(".settings-home-group")!;
  let view: AzureDevopsSetup | undefined;
  let busy = false;
  let poll: ReturnType<typeof setTimeout> | undefined;
  let read = 0;
  let invalid = false;
  let organizationDraft: string | undefined;
  let rendered: string | undefined;
  // Focus taken before controls were disabled for a pending action.
  let focusMemo: { key?: string; row?: string } | undefined;

  const describe = (element: HTMLElement) => ({
    key:
      element.dataset.azureAction ??
      (element.matches("[name=organization]") ? "organization" : undefined),
    row: element.closest<HTMLElement>("[data-azure-row]")?.dataset.azureRow,
  });
  function currentFocus() {
    const element = document.activeElement;
    if (element instanceof HTMLElement && group.contains(element))
      return describe(element);
  }

  const pending = (current: AzureDevopsSetup) =>
    current.checking ||
    current.cli.state === "installing" ||
    current.account.state === "signing_in";

  const textAction = (action: string, label: string, name = label) =>
    `<button type="button" data-azure-action="${action}" aria-label="${escape(name)}">${escape(label)}</button>`;
  const iconAction = (action: string, name: string, icon: keyof typeof icons) =>
    `<button type="button" class="azure-devops-icon" data-azure-action="${action}" aria-label="${escape(name)}" title="${escape(name)}">${icons[icon]}</button>`;
  const skeleton = (key: "cli" | "organization" | "account", copy = "") =>
    `<div class="settings-home-row azure-devops-row" data-azure-row="${key}"><span class="settings-row-symbol">${icons[key]}</span><span class="settings-row-copy"><strong></strong>${copy}<span data-azure-details></span></span><span class="settings-row-value" data-azure-value="${key}"></span><span class="azure-devops-actions"></span></div>`;
  // The organization field persists across renders so typing and its caret
  // survive background updates.
  group.innerHTML =
    skeleton("cli") +
    skeleton(
      "organization",
      `<form class="azure-devops-organization"><input name="organization" aria-label="Organization" autocomplete="off" autocapitalize="off" spellcheck="false" maxlength="50"><button type="submit" class="azure-devops-icon" data-azure-action="organization-save" aria-label="Save organization" title="Save organization">${icons.check}</button></form>`,
    ) +
    skeleton("account");
  group.hidden = true;
  const form = group.querySelector<HTMLFormElement>(
    ".azure-devops-organization",
  )!;
  const input = form.querySelector<HTMLInputElement>("input")!;

  function fill(
    key: "cli" | "organization" | "account",
    state: string,
    title: string,
    details: string,
    value: string,
    actions: string,
  ) {
    const row = group.querySelector<HTMLElement>(`[data-azure-row="${key}"]`)!;
    row.dataset.state = state;
    row.querySelector("strong")!.textContent = title;
    row.querySelector("[data-azure-details]")!.innerHTML = details;
    row.querySelector("[data-azure-value]")!.textContent = value;
    row.querySelector(".azure-devops-actions")!.innerHTML = actions;
  }

  function render(next: AzureDevopsSetup) {
    if (!root.isConnected) return;
    clearTimeout(poll);
    if (pending(next)) poll = setTimeout(refresh, 500);
    const snapshot = JSON.stringify([next, busy, invalid]);
    if (snapshot === rendered) return;
    rendered = snapshot;
    const focused = document.activeElement;
    const idle = !focused || focused === document.body;
    const memo = currentFocus() ?? (idle ? focusMemo : undefined);
    focusMemo = undefined;
    view = next;
    group.hidden = false;
    const { cli, organization, account } = next;
    const cliVersion =
      "version" in cli && cli.version ? cli.version : undefined;
    fill(
      "cli",
      cli.state,
      "Azure CLI",
      `<small data-azure-version>${escape(cliVersion ?? "")}</small><small data-azure-minimum>\u2265 ${escape(next.minimum)}</small>`,
      cliValues[cli.state],
      cli.state === "missing"
        ? textAction("install", "Install", "Install Azure CLI")
        : cli.state === "too_old"
          ? textAction("install", "Update", "Update Azure CLI")
          : cli.state === "installing"
            ? textAction("cancel", "Cancel", "Cancel install")
            : iconAction("check", "Re-check", "refresh"),
    );
    const name = organization.state === "unset" ? "" : organization.name;
    if (organizationDraft === undefined && document.activeElement !== input)
      input.value = name;
    input.toggleAttribute("aria-invalid", invalid);
    if (invalid) input.setAttribute("aria-invalid", "true");
    fill(
      "organization",
      organization.state,
      "Organization",
      `<small data-azure-tenant>${escape(organization.state === "tenant_known" ? organization.tenant : "")}</small>`,
      organizationValues[organization.state],
      organization.state === "offline"
        ? textAction("check", "Retry", "Retry organization")
        : "",
    );
    const identity =
      account.state === "connected" || account.state === "wrong_identity"
        ? account.identity
        : undefined;
    const expected =
      account.state === "wrong_identity" ? account.expected : undefined;
    fill(
      "account",
      account.state,
      identity?.display_name ?? "Account",
      identity
        ? `<small data-azure-identity title="${escape(identity.unique_name ?? identity.id)}">${escape(identity.unique_name ?? identity.id)}</small>${expected ? `<small data-azure-expected title="${escape(expected.unique_name ?? expected.display_name)}">\u2260 ${escape(expected.unique_name ?? expected.display_name)}</small>` : ""}`
        : "",
      accountValues[account.state],
      [
        ["signed_out", "interaction_required", "wrong_identity"].includes(
          account.state,
        )
          ? textAction("sign-in", "Sign in", "Sign in to Azure DevOps")
          : "",
        ["offline", "failed", "tenant_access_blocked"].includes(account.state)
          ? textAction("check", "Retry", "Retry account")
          : "",
        account.state === "signing_in"
          ? textAction("cancel", "Cancel", "Cancel sign-in")
          : "",
        identity
          ? iconAction(
              "forget",
              `Forget ${identity.unique_name ?? identity.display_name}`,
              "forget",
            )
          : "",
      ].join(""),
    );
    group.toggleAttribute("aria-busy", pending(next));
    group.querySelectorAll<HTMLButtonElement>("button").forEach((button) => {
      button.disabled = busy;
    });
    bindActions();
    // Only focus that was already inside the setup moves with its control.
    if (memo && !currentFocus()) {
      const replacement =
        memo.key === "organization"
          ? input
          : memo.key
            ? group.querySelector<HTMLElement>(
                `[data-azure-row="${memo.row}"] [data-azure-action="${CSS.escape(memo.key)}"]`,
              )
            : null;
      (
        replacement ??
        group.querySelector<HTMLElement>(
          `[data-azure-row="${memo.row}"] button:not(:disabled)`,
        ) ??
        group.querySelector<HTMLElement>("button:not(:disabled)")
      )?.focus({ preventScroll: true });
    }
  }

  async function run(
    command: string,
    args?: Record<string, unknown>,
    control?: HTMLElement,
  ) {
    if (busy) return false;
    busy = true;
    rendered = undefined;
    // WebKit does not focus clicked buttons; the activator is the opener.
    focusMemo =
      currentFocus() ??
      (control && group.contains(control) ? describe(control) : undefined);
    const request = ++read;
    group
      .querySelectorAll<HTMLButtonElement>("button")
      .forEach((button) => (button.disabled = true));
    try {
      const next = await invoke<AzureDevopsSetup>(command, args);
      if (command === "azure_devops_check") lastCheck = Date.now();
      busy = false;
      if (request === read) render(next);
      else
        group
          .querySelectorAll<HTMLButtonElement>("button")
          .forEach((button) => (button.disabled = false));
      return true;
    } catch (cause) {
      busy = false;
      const code = String(cause);
      if (code === "invalid_organization") invalid = true;
      else options.showError(failures[code] ?? "Failed");
      if (view) render(view);
      else
        group
          .querySelectorAll<HTMLButtonElement>("button")
          .forEach((button) => (button.disabled = false));
      if (code === "invalid_organization") input.focus({ preventScroll: true });
      else if (control?.isConnected) control.focus({ preventScroll: true });
      return false;
    }
  }

  input.addEventListener("input", () => {
    organizationDraft = input.value;
    if (invalid) {
      invalid = false;
      rendered = undefined;
      input.removeAttribute("aria-invalid");
    }
  });
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const value = input.value.trim();
    if (!value) {
      invalid = true;
      rendered = undefined;
      input.setAttribute("aria-invalid", "true");
      return;
    }
    if (
      await run("azure_devops_set_organization", { organization: value }, input)
    ) {
      organizationDraft = undefined;
      if (view && view.organization.state !== "unset")
        input.value = view.organization.name;
    }
  });

  function bindActions() {
    group
      .querySelectorAll<HTMLButtonElement>(
        ".azure-devops-actions [data-azure-action]",
      )
      .forEach((button) => {
        const action = button.dataset.azureAction!;
        button.onclick = async () => {
          if (busy) return;
          if (action === "check") await run("azure_devops_check", {}, button);
          if (action === "install")
            await run("azure_devops_install", {}, button);
          if (action === "cancel") await run("azure_devops_cancel", {}, button);
          if (action === "forget")
            await run("azure_devops_forget_account", {}, button);
          if (action === "sign-in") {
            const organization = view?.organization;
            if (organization?.state !== "tenant_known") return;
            if (!(await options.confirmSignIn(organization.tenant, button)))
              return;
            await run("azure_devops_sign_in", {}, button);
          }
        };
      });
  }

  function refresh() {
    if (!root.isConnected) {
      clearTimeout(poll);
      return;
    }
    if (busy) {
      poll = setTimeout(refresh, 500);
      return;
    }
    const request = ++read;
    void invoke<AzureDevopsSetup>("azure_devops_setup_state").then(
      (next) => {
        if (request === read) render(next);
      },
      () => {
        if (request !== read || !root.isConnected) return;
        options.showError("Unavailable");
      },
    );
  }

  const request = ++read;
  void invoke<AzureDevopsSetup>("azure_devops_setup_state").then(
    (current) => {
      if (request !== read || !root.isConnected) return;
      render(current);
      if (
        !pending(current) &&
        (current.cli.state === "unknown" ||
          Date.now() - lastCheck > RECHECK_AFTER_MS)
      ) {
        lastCheck = Date.now();
        const checkRequest = ++read;
        void invoke<AzureDevopsSetup>("azure_devops_check").then(
          (next) => {
            if (checkRequest === read) render(next);
          },
          () => {
            if (checkRequest === read && root.isConnected)
              options.showError("Unavailable");
          },
        );
        // Show the probe as checking until it answers.
        render({ ...current, checking: true });
        clearTimeout(poll);
      }
    },
    () => {
      if (request === read && root.isConnected) {
        options.showError("Unavailable");
        const retry = document.createElement("div");
        retry.innerHTML = textAction("check", "Retry", "Retry Azure DevOps");
        root.querySelector(".azure-devops-setup")!.append(retry);
        retry.querySelector("button")!.onclick = async () => {
          if (await run("azure_devops_check")) retry.remove();
        };
      }
    },
  );
}
