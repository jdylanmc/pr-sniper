import { invoke } from "@tauri-apps/api/core";
import type { AgentIntelligence } from "./policy";

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
  supportedReasoningEfforts?: string[];
  defaultReasoningEffort?: string;
  supportedContextTiers?: string[];
  capabilities?: {
    supports?: { reasoningEffort?: boolean };
    limits?: {
      max_context_window_tokens?: number;
      max_output_tokens?: number;
    };
  };
  billing?: {
    tokenPrices?: {
      maxPromptTokens?: number;
      contextMax?: number;
      longContext?: { maxPromptTokens?: number; contextMax?: number };
    };
  };
}
export const modelSelectable = (model: CopilotModel) =>
  !model.policy || ["enabled", "unconfigured"].includes(model.policy.state);

export const reasoningEfforts = (model?: CopilotModel) =>
  model?.capabilities?.supports?.reasoningEffort === false
    ? []
    : (model?.supportedReasoningEfforts ?? []);

export function contextTiers(model?: CopilotModel): string[] {
  const prices = model?.billing?.tokenPrices;
  return [
    ...new Set([
      ...(model?.supportedContextTiers ?? []),
      ...(prices?.maxPromptTokens !== undefined ||
      prices?.contextMax !== undefined
        ? ["default"]
        : []),
      ...(prices?.longContext ? ["long_context"] : []),
    ]),
  ];
}

// These are the pinned SDK's context wire values, not a model capability catalog.
export const runtimeContextTier = (value: string) =>
  value === "default" || value === "long_context";

export function contextCapacity(model?: CopilotModel, tier = "default") {
  const prices = model?.billing?.tokenPrices;
  const prompt =
    tier === "long_context"
      ? (prices?.longContext?.maxPromptTokens ??
        prices?.longContext?.contextMax)
      : tier === "default"
        ? (prices?.maxPromptTokens ?? prices?.contextMax)
        : undefined;
  const output = model?.capabilities?.limits?.max_output_tokens;
  const total =
    prompt !== undefined && output !== undefined ? prompt + output : undefined;
  return total !== undefined
    ? `${total.toLocaleString("en-US")} tokens`
    : prompt !== undefined
      ? `${prompt.toLocaleString("en-US")} prompt tokens`
      : "";
}

export function intelligenceError(
  model: CopilotModel | undefined,
  choice: AgentIntelligence | undefined,
) {
  if (
    choice?.reasoning_effort &&
    !reasoningEfforts(model).includes(choice.reasoning_effort)
  )
    return "Effort unsupported";
  if (
    choice?.context_tier &&
    (!contextTiers(model).includes(choice.context_tier) ||
      !runtimeContextTier(choice.context_tier))
  )
    return "Context unsupported";
  return "";
}

export const actualIntelligence = (value?: AgentIntelligence | null) =>
  value
    ? `Effort: ${value.reasoning_effort ?? "Default"}; context: ${value.context_tier ?? "Default"}`
    : "Not recorded";

export const copilotFailure = (reason?: string) =>
  ({
    disconnect_pending: "Disconnecting",
    disconnect_failed: "Disconnect failed",
    expired: "Reconnect required",
    denied: "Denied",
    network: "Offline",
    rate_limited: "Rate limited",
    missing_scope: "Reconnect required",
    authentication_changed: "Reconnect required",
    provider: "Verification failed",
    invalid_response: "Invalid response",
    browser_open: "Browser failed",
    cancelled: "Cancelled",
    timeout: "Timed out",
    wrong_identity: "Wrong account",
    credentials_unavailable: "Storage unavailable",
    verification_pending: "Verifying",
    verification_required: "Verify required",
    device_flow_disabled: "Device sign-in disabled",
  })[reason ?? "provider"] ?? "Reconnect required";

export function renderCopilotAuth(
  root: HTMLElement,
  accountsChanged: (accounts: CopilotAccount[]) => void,
) {
  root.innerHTML = `<div class="copilot-auth-card account-connection" data-account-role="ai"><header class="account-connection-heading"><h3>GitHub Copilot</h3><p>AI identity for Agent reviews</p></header><div class="account-connection-state"><p role="status" tabindex="-1">Loading</p><p class="copilot-error" role="alert" hidden></p><div class="copilot-flow"></div></div><div class="copilot-accounts"></div></div>`;
  const card = root.querySelector<HTMLElement>(".account-connection")!;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const error = root.querySelector<HTMLElement>("[role=alert]")!;
  const flow = root.querySelector<HTMLElement>(".copilot-flow")!;
  const accounts = root.querySelector<HTMLElement>(".copilot-accounts")!;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  let busy = false;
  let cancelling = false;
  let lastView = "";
  let autoConfirmed = "";
  let actionGeneration = 0;
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
    error.textContent = typeof cause === "string" ? cause : "Failed";
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
    button.dataset.accountAction = `${command}:${args?.accountId ?? args?.expectedAccountId ?? ""}`;
    const isCancel = command === "cancel_copilot_auth";
    const isAccountAction =
      command === "verify_copilot_account" ||
      command === "disconnect_copilot_account";
    button.dataset.cancelSignIn = String(isCancel);
    button.disabled = isCancel ? cancelling : busy || cancelling;
    button.onclick = async () => {
      if (cancelling || (!isCancel && busy)) return;
      status.focus({ preventScroll: true });
      if (isCancel) cancelling = true;
      else busy = true;
      const request = ++actionGeneration;
      const initialRead = stateRead;
      let needsRefresh = false;
      if (isCancel) clearTimeout(timer);
      updateButtons();
      if (!isCancel) error.hidden = true;
      try {
        const view = await invoke<CopilotAuth>(command, args);
        if (alive()) {
          const current =
            request === actionGeneration &&
            (!isCancel || initialRead === stateRead);
          // Reads started during this action predate its committed result.
          stateRead++;
          if (current) render(view);
          else needsRefresh = true;
        }
      } catch (cause) {
        if (alive()) {
          stateRead++;
          // Cancel invalidates flow snapshots, not another account's failure.
          if (request === actionGeneration || isAccountAction) showError(cause);
          needsRefresh = true;
        }
      } finally {
        if (isCancel) cancelling = false;
        else busy = false;
        if (alive()) updateButtons();
        if (alive() && (needsRefresh || isCancel)) void refresh();
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
    const focused = document.activeElement;
    const focusKey =
      focused instanceof HTMLElement && root.contains(focused)
        ? focused.dataset.accountAction
        : undefined;
    lastView = serialized;
    card.dataset.flowState = view.flow.state;
    card.dataset.empty = String(view.accounts.length === 0);
    accountsChanged(view.accounts);
    accounts.replaceChildren();
    flow.replaceChildren();
    status.textContent = view.accounts.length ? "" : "Not connected";
    for (const account of view.accounts) {
      const row = document.createElement("article");
      row.className = "github-account";
      row.dataset.accountState = account.state;
      row.setAttribute("aria-label", `Copilot account ${account.login}`);
      const description = document.createElement("div");
      const title = document.createElement("strong");
      title.textContent = `${account.login} (${account.account_id})`;
      const state = document.createElement("p");
      state.className =
        account.state === "connected" ? "copilot-signed-in" : "";
      state.textContent =
        account.state === "connected"
          ? "Connected"
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
      const disconnect = button(
        actions,
        "Disconnect",
        "disconnect_copilot_account",
        { accountId: account.account_id },
      );
      disconnect.setAttribute(
        "aria-label",
        `Disconnect Copilot ${account.login}`,
      );
      disconnect.title = "Disconnect";
      disconnect.classList.add("account-disconnect");
      disconnect.innerHTML =
        '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 7V4M15 7V4M7 7h10v4a5 5 0 0 1-10 0V7ZM12 16v4M3 3l18 18"/></svg>';
      row.append(description, actions);
      accounts.append(row);
    }
    const current = view.flow;
    if (current.state === "connecting") {
      status.textContent = current.user_code ? "Waiting" : "Requesting code";
      if (current.user_code) {
        const device = document.createElement("div");
        device.className = "github-device";
        const guidance = document.createElement("p");
        guidance.textContent = current.verification_uri ?? "";
        const codeRow = document.createElement("div");
        codeRow.className = "github-device-code-row";
        const code = document.createElement("code");
        code.className = "github-device-code";
        code.textContent = current.user_code;
        const copy = document.createElement("button");
        copy.type = "button";
        copy.textContent = "Copy code";
        copy.dataset.accountAction = "copy-code";
        const feedback = document.createElement("p");
        feedback.setAttribute("aria-live", "polite");
        copy.onclick = async () => {
          try {
            await navigator.clipboard.writeText(current.user_code!);
            feedback.textContent = "Copied";
          } catch {
            feedback.textContent = "Copy failed";
          }
        };
        codeRow.append(code, copy);
        device.append(guidance, codeRow, feedback);
        flow.append(device);
      }
      button(flow, "Cancel Copilot sign-in", "cancel_copilot_auth");
    } else if (current.state === "pending_account_confirmation") {
      status.textContent = `Connecting ${current.login}`;
      if (current.confirmation_error)
        showError(copilotFailure(current.confirmation_error));
      const confirm = button(
        flow,
        "Confirm Copilot account",
        "confirm_copilot_account",
      );
      confirm.className = "primary";
      // The browser sign-in already proved the identity; confirm once on its
      // own and fall back to the manual button if that attempt errors.
      const attempt = `${current.account_id}`;
      if (!current.confirmation_error && autoConfirmed !== attempt) {
        autoConfirmed = attempt;
        queueMicrotask(() => {
          if (alive() && confirm.isConnected && !confirm.disabled)
            confirm.click();
        });
      }
      // Keep the expected identity on reconnect; cancelling never changes its pin.
      button(flow, "Cancel Copilot sign-in", "cancel_copilot_auth");
    } else {
      autoConfirmed = "";
      if (current.state === "failed") showError(copilotFailure(current.reason));
      button(flow, "Connect Copilot account", "start_copilot_auth").className =
        "primary";
    }
    if (focusKey && focused instanceof HTMLElement && !focused.isConnected) {
      const replacement = root.querySelector<HTMLElement>(
        `[data-account-action="${CSS.escape(focusKey)}"]`,
      );
      (replacement ?? status).focus({ preventScroll: true });
    }
  }
  async function refresh() {
    if (!alive() || cancelling) return;
    const request = actionGeneration;
    const read = ++stateRead;
    try {
      const view = await invoke<CopilotAuth>("copilot_auth_state");
      if (request === actionGeneration && read === stateRead && alive())
        render(view);
    } catch (cause) {
      if (!alive() || request !== actionGeneration || read !== stateRead)
        return;
      lastView = "";
      card.dataset.flowState = "failed";
      if (error.hidden) showError(cause);
      status.textContent = "Unavailable";
      accounts.replaceChildren();
      flow.replaceChildren();
      button(flow, "Retry reading Copilot accounts", "copilot_auth_state");
    }
  }
  window.addEventListener("focus", refresh);
  window.addEventListener("pr-sniper:refresh-provider-accounts", refresh);
  void refresh();
  return () => {
    disposed = true;
    actionGeneration++;
    clearTimeout(timer);
    window.removeEventListener("focus", refresh);
    window.removeEventListener("pr-sniper:refresh-provider-accounts", refresh);
  };
}
import { isWindows, secureStoreName } from "./platform";
