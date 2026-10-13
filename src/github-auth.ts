import { invoke } from "@tauri-apps/api/core";

type GithubAuthFailure =
  | "disconnect_pending"
  | "disconnect_failed"
  | "expired"
  | "denied"
  | "device_flow_disabled"
  | "network"
  | "rate_limited"
  | "provider"
  | "invalid_response"
  | "browser_open"
  | "cancelled"
  | "timeout"
  | "wrong_identity"
  | "missing_scope"
  | "authentication_changed"
  | "credentials_unavailable";

export interface GithubAccount {
  provider: "github";
  account_id: string;
  login: string;
  state: "connected" | "reconnect_required";
  connection_generation?: number;
  reason?: GithubAuthFailure;
  warning?: GithubAuthFailure;
}

type GithubAuthView = {
  accounts: GithubAccount[];
  flow:
    | { state: "idle" }
    | {
        state: "connecting";
        expected_account_id?: string;
        user_code?: string;
        verification_uri?: string;
      }
    | {
        state: "pending_account_confirmation";
        account_id: string;
        login: string;
        confirmation_error?: GithubAuthFailure;
      }
    | { state: "failed"; reason: GithubAuthFailure };
};

export function renderGithubAuth(
  root: HTMLElement,
  accountsChanged?: (accounts: GithubAccount[] | null) => void,
) {
  root.innerHTML = `<div class="github-auth-card account-connection" data-account-role="repository"><header class="account-connection-heading"><h2>GitHub accounts</h2><p>Repository access and publication identity</p></header><div class="account-connection-state"><p role="status" tabindex="-1">Loading</p><div class="github-auth-flow"></div></div><div class="github-auth-accounts"></div><div class="github-auth-repositories"></div></div>`;
  const card = root.querySelector<HTMLElement>(".account-connection")!;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const actions = root.querySelector<HTMLElement>(".github-auth-flow")!;
  const accountList = root.querySelector<HTMLElement>(".github-auth-accounts")!;
  let renderedAccountIds = new Set<string>();
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let renderedView: string | undefined;
  let busy = false;
  let autoConfirmed = "";
  let actionGeneration = 0;
  let stateRead = 0;

  function setBusy(value: boolean) {
    busy = value;
    root
      .querySelectorAll<HTMLButtonElement>("button")
      .forEach((control) => (control.disabled = busy));
  }

  async function refreshAfterFailure(cause: unknown) {
    const failure = String(cause).toLowerCase();
    const request = actionGeneration;
    const read = ++stateRead;
    try {
      const view = await invoke<GithubAuthView>("github_auth_state");
      if (
        !root.isConnected ||
        request !== actionGeneration ||
        read !== stateRead
      )
        return;
      render(view);
    } catch {
      if (
        !root.isConnected ||
        request !== actionGeneration ||
        read !== stateRead
      )
        return;
      card.dataset.flowState = "failed";
      publishAccountStates([]);
      accountsChanged?.(null);
      status.textContent = "Unavailable";
      actions.replaceChildren();
      commandButton(
        actions,
        "Retry reading GitHub accounts",
        "github_auth_state",
      );
      return;
    }
    status.textContent = failure.includes("saved securely")
      ? "Storage failed"
      : failure.includes("missing_scope")
        ? "Reconnect required"
        : failure.includes("organization_policy_denied")
          ? "Blocked by organization"
          : failure.includes("network")
            ? "Offline"
            : "Failed";
  }

  function actionButton(
    container: HTMLElement,
    label: string,
    action: () => Promise<void>,
    key = label,
  ) {
    const control = document.createElement("button");
    control.type = "button";
    control.textContent = label;
    control.dataset.accountAction = key;
    control.addEventListener("click", async () => {
      if (busy || !root.isConnected) return;
      actionGeneration++;
      status.focus({ preventScroll: true });
      setBusy(true);
      try {
        await action();
      } catch (cause) {
        await refreshAfterFailure(cause);
      } finally {
        setBusy(false);
      }
    });
    container.append(control);
    return control;
  }

  function commandButton(
    container: HTMLElement,
    label: string,
    command: string,
    args?: Record<string, unknown>,
  ) {
    return actionButton(
      container,
      label,
      async () => {
        const view = await invoke<GithubAuthView>(command, args);
        stateRead++;
        if (root.isConnected) render(view);
      },
      `${command}:${args?.accountId ?? args?.expectedAccountId ?? ""}`,
    );
  }

  function publishAccountStates(accounts: GithubAccount[]) {
    const current = new Set(accounts.map((account) => account.account_id));
    for (const account of accounts)
      window.dispatchEvent(
        new CustomEvent("pr-sniper:provider-account-state", {
          detail: {
            provider: account.provider,
            account_id: account.account_id,
            available: account.state === "connected",
          },
        }),
      );
    for (const accountId of renderedAccountIds)
      if (!current.has(accountId))
        window.dispatchEvent(
          new CustomEvent("pr-sniper:provider-account-state", {
            detail: {
              provider: "github",
              account_id: accountId,
              available: false,
            },
          }),
        );
    renderedAccountIds = current;
  }

  function render(view: GithubAuthView) {
    if (!root.isConnected) return;
    clearTimeout(refreshTimer);
    const snapshot = JSON.stringify(view);
    if (view.flow.state === "connecting") {
      refreshTimer = setTimeout(refresh, 250);
      // Keep keyboard focus and clipboard feedback while the provider is pending.
      if (snapshot === renderedView) return;
    }
    const focused = document.activeElement;
    const focusKey =
      focused instanceof HTMLElement && root.contains(focused)
        ? focused.dataset.accountAction
        : undefined;
    renderedView = snapshot;
    card.dataset.flowState = view.flow.state;
    card.dataset.empty = String(view.accounts.length === 0);
    actions.replaceChildren();
    accountList.replaceChildren();
    publishAccountStates(view.accounts);
    accountsChanged?.(view.accounts);

    status.textContent = view.accounts.length
      ? `${view.accounts.length} ${view.accounts.length === 1 ? "account" : "accounts"}`
      : "Not connected";

    for (const account of view.accounts) {
      const item = document.createElement("article");
      item.className = "github-account";
      item.dataset.accountState = account.state;
      item.setAttribute("aria-label", `GitHub account ${account.login}`);
      const description = document.createElement("div");
      const heading = document.createElement("strong");
      heading.textContent = `${account.login} (${account.account_id})`;
      const state = document.createElement("p");
      state.textContent =
        account.state === "connected"
          ? account.warning
            ? `Connected; ${githubAccountFailureMessage(account.warning)}`
            : "\u2713 Connected"
          : githubAccountFailureMessage(account.reason);
      description.append(heading, state);
      const accountActions = document.createElement("div");
      accountActions.className = "github-auth-actions";
      if (account.state === "connected") {
        const disconnect = commandButton(
          accountActions,
          "Disconnect",
          "disconnect_github_auth",
          { accountId: account.account_id },
        );
        disconnect.setAttribute("aria-label", `Disconnect ${account.login}`);
        disconnect.title = "Disconnect";
        disconnect.classList.add("account-disconnect");
        disconnect.innerHTML =
          '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 7V4M15 7V4M7 7h10v4a5 5 0 0 1-10 0V7ZM12 16v4M3 3l18 18"/></svg>';
        if (account.warning)
          commandButton(
            accountActions,
            "Reconnect",
            "start_github_browser_auth",
            { expectedAccountId: account.account_id },
          ).setAttribute("aria-label", `Reconnect ${account.login}`);
      } else {
        commandButton(
          accountActions,
          "Reconnect",
          "start_github_browser_auth",
          { expectedAccountId: account.account_id },
        ).setAttribute("aria-label", `Reconnect ${account.login}`);
        if (account.reason === "disconnect_failed")
          commandButton(
            accountActions,
            "Retry disconnect",
            "disconnect_github_auth",
            { accountId: account.account_id },
          ).setAttribute("aria-label", `Retry disconnect ${account.login}`);
      }
      item.append(description, accountActions);
      accountList.append(item);
    }

    if (view.flow.state === "connecting") {
      if (view.flow.user_code && view.flow.verification_uri) {
        const userCode = view.flow.user_code;
        status.textContent = "Waiting";
        const device = document.createElement("div");
        device.className = "github-device";
        const guidance = document.createElement("p");
        guidance.textContent = view.flow.verification_uri;
        const codeRow = document.createElement("div");
        codeRow.className = "github-device-code-row";
        const code = document.createElement("code");
        code.className = "github-device-code";
        code.textContent = userCode;
        const feedback = document.createElement("p");
        feedback.className = "github-copy-feedback";
        feedback.setAttribute("aria-live", "polite");
        const copy = document.createElement("button");
        copy.type = "button";
        copy.dataset.accountAction = "copy-code";
        copy.className = "primary";
        copy.textContent = "Copy code";
        let copying = false;
        copy.addEventListener("click", async () => {
          if (copying) return;
          copying = true;
          copy.setAttribute("aria-disabled", "true");
          try {
            await navigator.clipboard.writeText(userCode);
            feedback.removeAttribute("role");
            feedback.textContent = "Copied";
          } catch {
            feedback.setAttribute("role", "alert");
            feedback.textContent = "Copy failed";
          } finally {
            copying = false;
            copy.removeAttribute("aria-disabled");
          }
        });
        codeRow.append(code, copy);
        device.append(guidance, codeRow, feedback);
        actions.append(device);
      } else {
        status.textContent = "Requesting code";
      }
      commandButton(actions, "Cancel", "cancel_github_auth");
    } else if (view.flow.state === "pending_account_confirmation") {
      status.textContent =
        view.flow.confirmation_error === "credentials_unavailable"
          ? `Storage failed: ${view.flow.login} (${view.flow.account_id})`
          : `${view.flow.login} (${view.flow.account_id})`;
      const confirm = commandButton(
        actions,
        "Confirm",
        "confirm_github_account",
      );
      confirm.className = "primary";
      // Browser sign-in already proved the identity; confirm once automatically
      // and fall back to the manual button if that attempt errors.
      if (
        !view.flow.confirmation_error &&
        autoConfirmed !== view.flow.account_id
      ) {
        autoConfirmed = view.flow.account_id;
        queueMicrotask(() => {
          if (root.isConnected && confirm.isConnected && !confirm.disabled)
            confirm.click();
        });
      }
      commandButton(
        actions,
        "Use a different account",
        "start_github_browser_auth",
        { selectAccount: true },
      );
      commandButton(actions, "Cancel", "cancel_github_auth");
    } else if (view.flow.state === "failed") {
      status.textContent = githubAccountFailureMessage(view.flow.reason);
      commandButton(
        actions,
        "Try GitHub sign-in again",
        "start_github_browser_auth",
      );
    } else {
      autoConfirmed = "";
      commandButton(
        actions,
        "Add GitHub account",
        "start_github_browser_auth",
      ).className = "primary";
    }
    if (focusKey && focused instanceof HTMLElement && !focused.isConnected) {
      const replacement = root.querySelector<HTMLElement>(
        `[data-account-action="${CSS.escape(focusKey)}"]`,
      );
      (replacement ?? status).focus({ preventScroll: true });
    }
  }

  const refresh = () => {
    // A parked or hidden card keeps its listeners: re-attaching it or
    // reopening the panel must re-read native state rather than show a stale
    // flow or account.
    if (!root.isConnected) {
      clearTimeout(refreshTimer);
      return;
    }
    if (busy) {
      refreshTimer = setTimeout(refresh, 250);
      return;
    }
    const request = actionGeneration;
    const read = ++stateRead;
    const current = () =>
      root.isConnected && request === actionGeneration && read === stateRead;
    void invoke<GithubAuthView>("github_auth_state").then(
      (view) => {
        if (current()) render(view);
      },
      () => {
        if (!current()) return;
        renderedView = undefined;
        card.dataset.flowState = "failed";
        publishAccountStates([]);
        accountsChanged?.(null);
        status.textContent = "Unavailable";
        actions.replaceChildren();
        commandButton(
          actions,
          "Retry reading GitHub accounts",
          "github_auth_state",
        );
      },
    );
  };
  window.addEventListener("pr-sniper:refresh-provider-accounts", refresh);
  window.addEventListener("focus", refresh);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") refresh();
  });
  refresh();
}

export function githubAccountFailureMessage(reason?: GithubAuthFailure) {
  return (
    {
      disconnect_pending: "Disconnecting",
      disconnect_failed: "Disconnect failed",
      expired: "Reconnect required",
      denied: "Denied",
      device_flow_disabled: "Device sign-in disabled",
      network: "Offline",
      rate_limited: "Rate limited",
      provider: "Verification failed",
      invalid_response: "Invalid response",
      browser_open: "Browser failed",
      cancelled: "Cancelled",
      timeout: "Timed out",
      wrong_identity: "Wrong account",
      missing_scope: "Reconnect required",
      authentication_changed: "Reconnect required",
      credentials_unavailable: "Storage unavailable",
    }[reason ?? "provider"] ?? "Reconnect required"
  );
}
