import { invoke } from "@tauri-apps/api/core";

export interface CopilotAccount {
  provider: "copilot";
  account_id: string;
  login: string;
  state: "connected" | "reconnect_required";
  reason?: string;
}
export interface CopilotAuth {
  accounts: CopilotAccount[];
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
        confirmation_error?: string;
      }
    | { state: "failed"; reason: string };
}
export interface CopilotModel {
  id: string;
  name: string;
  policy?: { state: string; terms?: string };
  warningText?: { dataRetention?: string };
  infoMessages?: { message: string }[];
}
export const modelSelectable = (model: CopilotModel) =>
  !model.policy || ["enabled", "unconfigured"].includes(model.policy.state);

export const copilotFailure = (reason?: string) =>
  ({
    disconnected: "Disconnected. Reconnect to use this account again.",
    expired: "The credential is missing or expired. Reconnect this account.",
    denied: "GitHub authorization was denied. Try again when ready.",
    network: "Cannot reach GitHub to verify sign-in. Retry verification.",
    provider:
      "GitHub could not verify this identity. Retry verification or reconnect.",
    invalid_response: "GitHub returned an invalid response. Retry.",
    browser_open: "Could not open your default browser. Retry sign-in.",
    cancelled: "Sign-in cancelled.",
    timeout: "Sign-in timed out. Retry.",
    wrong_identity:
      "GitHub returned an unexpected or already configured account. Reconnect the matching account or choose another identity.",
    credentials_unavailable:
      "Secure storage is unavailable. Retry or reconnect.",
    verification_pending: "Verifying saved sign-in...",
    verification_required: "Verify saved sign-in or reconnect this account.",
    device_flow_disabled:
      "Device sign-in is disabled for the PR Sniper OAuth App. A maintainer must enable it.",
  })[reason ?? "provider"] ?? "Verify or reconnect this account.";

export function renderCopilotAuth(
  root: HTMLElement,
  accountsChanged: (accounts: CopilotAccount[]) => void,
) {
  root.innerHTML = `<div class="copilot-auth-card"><h3>GitHub Copilot</h3><p class="settings-hint">Supplies AI credentials only. Repository access and future comments use the separate GitHub repository connection. Sign-in requests profile access and refresh, not repository access; GitHub may retain permissions you previously granted this OAuth App. Model lookup requires macOS 13.5 or later.</p><p role="status">Reading Copilot accounts...</p><p class="copilot-error" role="alert" hidden></p><div class="copilot-flow"></div><div class="copilot-accounts"></div><p class="settings-hint">A check means verified sign-in, not a subscription, seat or model test. Connections save immediately in macOS Keychain, independently of Save changes. Disconnect removes this role's local credential, not your GitHub authorization or repository connection.</p></div>`;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const error = root.querySelector<HTMLElement>("[role=alert]")!;
  const flow = root.querySelector<HTMLElement>(".copilot-flow")!;
  const accounts = root.querySelector<HTMLElement>(".copilot-accounts")!;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  let busy = false;
  let cancelling = false;
  let lastView = "";
  let generation = 0;
  let stateRead = 0;
  const alive = () => !disposed && root.isConnected;
  const updateButtons = () =>
    root.querySelectorAll<HTMLButtonElement>("button").forEach((control) => {
      control.disabled =
        control.dataset.cancelSignIn === "true"
          ? cancelling
          : busy || cancelling;
    });
  const showError = (cause: unknown) => {
    error.textContent =
      typeof cause === "string"
        ? cause
        : "Copilot connection could not be updated. Retry.";
    error.hidden = false;
  };
  function button(
    parent: HTMLElement,
    label: string,
    command: string,
    args?: Record<string, unknown>,
  ) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    const isCancel = command === "cancel_copilot_auth";
    button.dataset.cancelSignIn = String(isCancel);
    button.disabled = isCancel ? cancelling : busy || cancelling;
    button.onclick = async () => {
      if (cancelling || (!isCancel && busy)) return;
      if (isCancel) cancelling = true;
      else busy = true;
      const request = ++generation;
      if (isCancel) clearTimeout(timer);
      updateButtons();
      error.hidden = true;
      try {
        const view = await invoke<CopilotAuth>(command, args);
        if (alive() && request === generation) render(view);
      } catch (cause) {
        if (alive() && request === generation) {
          showError(cause);
          try {
            const view = await invoke<CopilotAuth>("copilot_auth_state");
            if (request === generation) render(view);
          } catch {
            if (alive() && request === generation)
              status.textContent =
                "Copilot account state is unavailable. Retry reading accounts.";
          }
        }
      } finally {
        if (isCancel) cancelling = false;
        else busy = false;
        if (alive()) updateButtons();
        if (alive() && request !== generation) void refresh();
      }
    };
    parent.append(button);
    return button;
  }
  function render(view: CopilotAuth) {
    if (!alive()) return;
    clearTimeout(timer);
    const serialized = JSON.stringify(view);
    if (
      view.flow.state === "connecting" ||
      view.accounts.some((a) => a.reason === "verification_pending")
    )
      timer = setTimeout(() => void refresh(), 350);
    if (serialized === lastView) return;
    lastView = serialized;
    accountsChanged(view.accounts);
    accounts.replaceChildren();
    flow.replaceChildren();
    status.textContent = view.accounts.length
      ? "Choose an account and a returned model separately for each Agent."
      : "No Copilot accounts connected. Connect an account to configure an Agent.";
    for (const account of view.accounts) {
      const row = document.createElement("article");
      row.className = "github-account";
      row.setAttribute("aria-label", `Copilot account ${account.login}`);
      const description = document.createElement("div");
      const title = document.createElement("strong");
      title.textContent = `${account.login} (${account.account_id})`;
      const state = document.createElement("p");
      state.className =
        account.state === "connected" ? "copilot-signed-in" : "";
      state.textContent =
        account.state === "connected"
          ? `Signed in as ${account.login}. Sign-in verified only.`
          : copilotFailure(account.reason);
      if (account.state === "connected") {
        const check = document.createElement("span");
        check.className = "copilot-check";
        check.setAttribute("aria-hidden", "true");
        check.textContent = "\u2713";
        state.prepend(check);
      }
      description.append(title, state);
      const actions = document.createElement("div");
      actions.className = "github-auth-actions";
      button(actions, "Verify sign-in", "verify_copilot_account", {
        accountId: account.account_id,
      }).setAttribute(
        "aria-label",
        `Verify Copilot sign-in for ${account.login}`,
      );
      button(actions, "Reconnect", "start_copilot_auth", {
        expectedAccountId: account.account_id,
      }).setAttribute("aria-label", `Reconnect Copilot ${account.login}`);
      button(actions, "Disconnect", "disconnect_copilot_account", {
        accountId: account.account_id,
      }).setAttribute("aria-label", `Disconnect Copilot ${account.login}`);
      row.append(description, actions);
      accounts.append(row);
    }
    const current = view.flow;
    if (current.state === "connecting") {
      status.textContent = current.user_code
        ? "Complete sign-in in your default browser, then confirm the returned identity here."
        : "Requesting a one-time code. Your default browser will open.";
      if (current.user_code) {
        const device = document.createElement("div");
        device.className = "github-device";
        const guidance = document.createElement("p");
        guidance.textContent = `Paste this one-time code at ${current.verification_uri}`;
        const codeRow = document.createElement("div");
        codeRow.className = "github-device-code-row";
        const code = document.createElement("code");
        code.className = "github-device-code";
        code.textContent = current.user_code;
        const copy = document.createElement("button");
        copy.type = "button";
        copy.textContent = "Copy code";
        const feedback = document.createElement("p");
        feedback.setAttribute("aria-live", "polite");
        copy.onclick = async () => {
          try {
            await navigator.clipboard.writeText(current.user_code!);
            feedback.textContent = "Code copied. Paste it on GitHub.";
          } catch {
            feedback.textContent =
              "Could not copy. Select the code and copy it manually.";
          }
        };
        codeRow.append(code, copy);
        device.append(guidance, codeRow, feedback);
        flow.append(device);
      }
      button(flow, "Cancel Copilot sign-in", "cancel_copilot_auth");
    } else if (current.state === "pending_account_confirmation") {
      status.textContent = `Confirm ${current.login} (${current.account_id}) for AI. Credentials are not saved until you confirm.`;
      if (current.confirmation_error)
        showError(copilotFailure(current.confirmation_error));
      button(flow, "Confirm Copilot account", "confirm_copilot_account");
      // Keep the expected identity on reconnect; cancelling never changes its pin.
      button(flow, "Cancel Copilot sign-in", "cancel_copilot_auth");
    } else {
      if (current.state === "failed") showError(copilotFailure(current.reason));
      button(flow, "Connect Copilot account", "start_copilot_auth");
    }
  }
  async function refresh() {
    if (!alive() || cancelling) return;
    const request = generation;
    const read = ++stateRead;
    try {
      const view = await invoke<CopilotAuth>("copilot_auth_state");
      if (request === generation && read === stateRead && alive()) render(view);
    } catch (cause) {
      if (!alive() || request !== generation || read !== stateRead) return;
      lastView = "";
      showError(cause);
      status.textContent =
        "Copilot account state is unavailable. No connection is assumed.";
      flow.replaceChildren();
      button(flow, "Retry reading Copilot accounts", "copilot_auth_state");
    }
  }
  window.addEventListener("focus", refresh);
  void refresh();
  return () => {
    disposed = true;
    generation++;
    clearTimeout(timer);
    window.removeEventListener("focus", refresh);
  };
}
