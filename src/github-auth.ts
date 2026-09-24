import { invoke } from "@tauri-apps/api/core";

type GithubAuthFailure =
  | "expired"
  | "denied"
  | "device_flow_disabled"
  | "network"
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

export interface GithubAccessibleRepository {
  id: string;
  name: string;
}

export function renderGithubAuth(
  root: HTMLElement,
  selectRepository?: (
    account: GithubAccount,
    repository: GithubAccessibleRepository,
  ) => void,
  accountsChanged?: (accounts: GithubAccount[]) => void,
) {
  root.innerHTML = `<div class="github-auth-card"><h2>GitHub accounts</h2><p role="status">Reading connection state...</p><div class="github-auth-flow"></div><p class="settings-hint">GitHub's broad <code>repo</code> scope grants access to public and private repositories available to the account. PR Sniper lists them for explicit selection and never starts monitoring every accessible repository automatically.</p><div class="github-auth-accounts"></div><div class="github-auth-repositories"></div></div>`;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const actions = root.querySelector<HTMLElement>(".github-auth-flow")!;
  const accountList = root.querySelector<HTMLElement>(".github-auth-accounts")!;
  const repositories = root.querySelector<HTMLElement>(
    ".github-auth-repositories",
  )!;
  let renderedAccountIds = new Set<string>();
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let renderedView: string | undefined;

  function setBusy(busy: boolean) {
    root
      .querySelectorAll<HTMLButtonElement>("button")
      .forEach((control) => (control.disabled = busy));
  }

  async function refreshAfterFailure(cause: unknown) {
    const failure = String(cause).toLowerCase();
    try {
      render(await invoke<GithubAuthView>("github_auth_state"));
    } catch {
      status.textContent =
        "GitHub connection state is unavailable after the failed operation. No connection is assumed.";
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
  ) {
    const control = document.createElement("button");
    control.type = "button";
    control.textContent = label;
    control.addEventListener("click", async () => {
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
    return actionButton(container, label, async () =>
      render(await invoke<GithubAuthView>(command, args)),
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
    clearTimeout(refreshTimer);
    const snapshot = JSON.stringify(view);
    if (view.flow.state === "connecting") {
      refreshTimer = setTimeout(refresh, 250);
      // Keep keyboard focus and clipboard feedback while the provider is pending.
      if (snapshot === renderedView) return;
    }
    renderedView = snapshot;
    actions.replaceChildren();
    accountList.replaceChildren();
    repositories.replaceChildren();
    publishAccountStates(view.accounts);
    accountsChanged?.(view.accounts);

    status.textContent = view.accounts.length
      ? `${view.accounts.length} GitHub ${view.accounts.length === 1 ? "account" : "accounts"} configured. Repository access and automation are not implied.`
      : "No GitHub accounts connected. Adding an account does not enable reviews, comments, notifications, or merging.";

    for (const account of view.accounts) {
      const item = document.createElement("article");
      item.className = "github-account";
      item.setAttribute("aria-label", `GitHub account ${account.login}`);
      const description = document.createElement("div");
      const heading = document.createElement("strong");
      heading.textContent = `${account.login} (${account.account_id})`;
      const state = document.createElement("p");
      state.textContent =
        account.state === "connected"
          ? "Connected through the PR Sniper GitHub OAuth App."
          : `Needs attention. ${failureMessage(account.reason)}`;
      description.append(heading, state);
      const accountActions = document.createElement("div");
      accountActions.className = "github-auth-actions";
      if (account.state === "connected") {
        commandButton(accountActions, "Disconnect", "disconnect_github_auth", {
          accountId: account.account_id,
        }).setAttribute("aria-label", `Disconnect ${account.login}`);
        actionButton(accountActions, "Load repositories", async () => {
          const result = await invoke<{
            identity: { id: string; login: string };
            repositories: GithubAccessibleRepository[];
          }>("list_provider_repositories", {
            provider: "github",
            accountId: account.account_id,
          });
          if (result.identity.id !== account.account_id)
            throw new Error("GitHub account changed");
          repositories.replaceChildren();
          if (!result.repositories.length) {
            repositories.textContent = `No repositories are available to ${account.login} with the granted GitHub OAuth scope.`;
            return;
          }
          const list = document.createElement("ul");
          for (const repository of result.repositories) {
            const row = document.createElement("li");
            const use = document.createElement("button");
            use.type = "button";
            use.textContent = `Use ${repository.name} as ${account.login}`;
            use.addEventListener("click", () =>
              selectRepository?.(account, repository),
            );
            row.append(use);
            list.append(row);
          }
          repositories.append(list);
        }).setAttribute("aria-label", `Load repositories for ${account.login}`);
      } else {
        commandButton(
          accountActions,
          "Reconnect",
          "start_github_browser_auth",
          { expectedAccountId: account.account_id },
        ).setAttribute("aria-label", `Reconnect ${account.login}`);
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
        copy.className = "primary";
        copy.textContent = "Copy code";
        copy.addEventListener("click", async () => {
          copy.disabled = true;
          try {
            await navigator.clipboard.writeText(userCode);
            feedback.removeAttribute("role");
            feedback.textContent = "Code copied. Paste it on GitHub.";
          } catch {
            feedback.setAttribute("role", "alert");
            feedback.textContent =
              "Could not copy. Select the code and copy it manually.";
          } finally {
            copy.disabled = false;
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
          : `Confirm ${view.flow.login} (${view.flow.account_id}). Credentials are not saved until you confirm.`;
      commandButton(actions, "Confirm", "confirm_github_account");
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
      commandButton(actions, "Add GitHub account", "start_github_browser_auth");
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
    void invoke<GithubAuthView>("github_auth_state").then(render, () => {
      renderedView = undefined;
      status.textContent =
        "GitHub connection state is unavailable. No connection is assumed.";
      actions.replaceChildren();
    });
  };
  window.addEventListener("pr-sniper:refresh-provider-accounts", refresh);
  refresh();
}

function failureMessage(reason?: GithubAuthFailure) {
  return (
    {
      expired: "The authorization expired or can no longer be refreshed.",
      denied: "GitHub authorization was denied.",
      device_flow_disabled:
        "The PR Sniper GitHub OAuth App Device Flow registration is unavailable. A maintainer must enable Device Flow before sign-in can work.",
      network: "The GitHub network request failed.",
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
