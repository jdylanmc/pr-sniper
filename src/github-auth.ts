import { invoke } from "@tauri-apps/api/core";

type GithubAuthFailure =
  | "disconnected"
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
  root.innerHTML = `<div class="github-auth-card account-connection" data-account-role="repository"><header class="account-connection-heading"><h2>GitHub accounts</h2><p>Repository access and publication identity</p></header><p class="account-scope">Requests broad public/private repository access, not Copilot AI access.</p><div class="account-connection-state"><p role="status" tabindex="-1">Reading connection state...</p><div class="github-auth-flow"></div></div><details class="account-consent"><summary>GitHub access and consent</summary><p>GitHub's broad <code>repo</code> scope grants access to public and private repositories available to the account. PR Sniper lists them for explicit selection and never starts monitoring every accessible repository automatically.</p><p>Only confirming the returned identity saves this connection. Repository selection and action permissions stay separate. Disconnect removes this role's local credential, not your GitHub authorization or Copilot connection.</p></details><div class="github-auth-accounts"></div><div class="github-auth-repositories"></div></div>`;
  const card = root.querySelector<HTMLElement>(".account-connection")!;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const actions = root.querySelector<HTMLElement>(".github-auth-flow")!;
  const accountList = root.querySelector<HTMLElement>(".github-auth-accounts")!;
  let renderedAccountIds = new Set<string>();
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let renderedView: string | undefined;
  let busy = false;
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
      status.textContent =
        "GitHub connection state is unavailable after the failed operation. No connection is assumed.";
      actions.replaceChildren();
      commandButton(
        actions,
        "Retry reading GitHub accounts",
        "github_auth_state",
      );
      return;
    }
    const currentState = status.textContent ?? "";
    status.textContent = failure.includes("saved securely")
      ? `GitHub credentials could not be saved securely. The account is still pending; retry Confirm or cancel without connecting it. ${currentState}`
      : failure.includes("missing_scope")
        ? "GitHub no longer grants the required repo scope. Reconnect and review the broad public/private repository access request."
        : failure.includes("organization_policy_denied")
          ? "GitHub organization policy or SAML single sign-on blocked repository access. Authorize the OAuth App for that organization or contact its administrator."
          : failure.includes("network")
            ? "The GitHub network request failed. No authorization or automation was assumed."
            : failure.includes("provider")
              ? "GitHub returned an error. No authorization or automation was assumed."
              : "GitHub connection could not be updated. No authorization or automation was assumed.";
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
    actions.replaceChildren();
    accountList.replaceChildren();
    publishAccountStates(view.accounts);
    accountsChanged?.(view.accounts);

    status.textContent = view.accounts.length
      ? `${view.accounts.length} GitHub ${view.accounts.length === 1 ? "account" : "accounts"} configured. Repository access and automation are not implied.`
      : "No GitHub accounts connected. Adding an account does not enable reviews, comments, notifications, or merging.";

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
            ? `Connected through the PR Sniper GitHub OAuth App. Last verification needs retry: ${failureMessage(account.warning)}`
            : "Connected through the PR Sniper GitHub OAuth App."
          : `Needs attention. ${failureMessage(account.reason)}`;
      description.append(heading, state);
      const accountActions = document.createElement("div");
      accountActions.className = "github-auth-actions";
      if (account.state === "connected") {
        commandButton(accountActions, "Disconnect", "disconnect_github_auth", {
          accountId: account.account_id,
        }).setAttribute("aria-label", `Disconnect ${account.login}`);
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
        status.textContent =
          "GitHub opened in your default browser. Copy the one-time code, paste it on GitHub, then return here.";
        const device = document.createElement("div");
        device.className = "github-device";
        const guidance = document.createElement("p");
        guidance.textContent = `Paste this code at ${view.flow.verification_uri}`;
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
            feedback.textContent = "Code copied. Paste it on GitHub.";
          } catch {
            feedback.setAttribute("role", "alert");
            feedback.textContent =
              "Could not copy. Select the code and copy it manually.";
          } finally {
            copying = false;
            copy.removeAttribute("aria-disabled");
          }
        });
        codeRow.append(code, copy);
        device.append(guidance, codeRow, feedback);
        actions.append(device);
      } else {
        status.textContent =
          "Requesting a one-time GitHub authorization code. The default browser will open when it is ready.";
      }
      commandButton(actions, "Cancel", "cancel_github_auth");
    } else if (view.flow.state === "pending_account_confirmation") {
      status.textContent =
        view.flow.confirmation_error === "credentials_unavailable"
          ? `GitHub credentials could not be saved securely. Confirm ${view.flow.login} (${view.flow.account_id}) again or cancel without connecting it.`
          : `Confirm ${view.flow.login} (${view.flow.account_id}) for repository access and publication. Credentials are not saved until you confirm.`;
      commandButton(actions, "Confirm", "confirm_github_account").className =
        "primary";
      commandButton(
        actions,
        "Use a different account",
        "start_github_browser_auth",
        { selectAccount: true },
      );
      commandButton(actions, "Cancel", "cancel_github_auth");
    } else if (view.flow.state === "failed") {
      status.textContent = failureMessage(view.flow.reason);
      commandButton(
        actions,
        "Try GitHub sign-in again",
        "start_github_browser_auth",
      );
    } else {
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
    if (!root.isConnected) {
      clearTimeout(refreshTimer);
      window.removeEventListener(
        "pr-sniper:refresh-provider-accounts",
        refresh,
      );
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
        status.textContent =
          "GitHub connection state is unavailable. No connection is assumed.";
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
  refresh();
}

function failureMessage(reason?: GithubAuthFailure) {
  return (
    {
      disconnected: "Disconnected. Reconnect to use this account again.",
      disconnect_pending:
        "Disconnecting. Secure credential deletion is not yet confirmed.",
      disconnect_failed:
        "Credential deletion could not be confirmed. Retry disconnect.",
      expired: "The authorization expired or can no longer be refreshed.",
      denied: "GitHub authorization was denied.",
      device_flow_disabled:
        "The PR Sniper GitHub OAuth App Device Flow registration is unavailable. A maintainer must enable Device Flow before sign-in can work.",
      network: "The GitHub network request failed.",
      rate_limited:
        "GitHub rate limited the verification request. Retry later.",
      provider: "GitHub rejected or could not validate the connection.",
      invalid_response: "GitHub returned an invalid authorization response.",
      browser_open: "PR Sniper could not open the default browser.",
      cancelled: "GitHub sign-in was cancelled.",
      timeout: "GitHub sign-in timed out.",
      wrong_identity:
        "GitHub returned an already connected or unexpected account.",
      missing_scope:
        "The GitHub OAuth authorization no longer grants the required repo scope.",
      authentication_changed:
        "Reconnect through the PR Sniper GitHub OAuth App. Superseded GitHub App credentials are not reused.",
      credentials_unavailable:
        "The credentials could not be restored or stored safely.",
    }[reason ?? "provider"] ?? "Reconnect this account."
  );
}
