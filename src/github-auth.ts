import { invoke } from "@tauri-apps/api/core";

type GithubAuthState =
  | { state: "disconnected" }
  | {
      state: "connecting";
      user_code: string;
      verification_uri: string;
      expires_in_seconds: number;
      interval_seconds: number;
    }
  | { state: "connected"; account_id: string; login: string }
  | { state: "reconnect_required" };

export function renderGithubAuth(root: HTMLElement) {
  root.innerHTML = `<div class="github-auth-card"><div><h2>GitHub account</h2><p role="status">Reading connection state...</p></div><div class="github-auth-actions"></div></div>`;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const actions = root.querySelector<HTMLElement>(".github-auth-actions")!;

  function button(label: string, command: string) {
    const control = document.createElement("button");
    control.type = "button";
    control.textContent = label;
    control.addEventListener("click", async () => {
      setBusy(true);
      try {
        render(await invoke<GithubAuthState>(command));
      } catch {
        status.textContent =
          "GitHub connection could not be updated. No authorization or automation was assumed.";
      } finally {
        setBusy(false);
      }
    });
    actions.append(control);
  }

  function setBusy(busy: boolean) {
    actions
      .querySelectorAll<HTMLButtonElement>("button")
      .forEach((control) => (control.disabled = busy));
  }

  function render(state: GithubAuthState) {
    actions.replaceChildren();
    if (state.state === "disconnected") {
      status.textContent =
        "Disconnected. Connect through the PR Sniper GitHub App. This does not enable reviews, comments, notifications, or merging.";
      button("Connect GitHub", "begin_github_auth");
      return;
    }
    if (state.state === "reconnect_required") {
      status.textContent =
        "Reconnect required. The previous authorization was denied, expired, revoked, unusable, or could not be stored safely.";
      button("Reconnect GitHub", "begin_github_auth");
      return;
    }
    if (state.state === "connected") {
      status.textContent = `Connected as ${state.login} (${state.account_id}). Stable identity verified; repository access is not implied. No automation was enabled.`;
      return;
    }
    status.replaceChildren();
    status.append("Connecting. Open ");
    const link = document.createElement("a");
    link.href =
      state.verification_uri === "https://github.com/login/device"
        ? state.verification_uri
        : "https://github.com/login/device";
    link.target = "_blank";
    link.rel = "noreferrer";
    link.textContent = "GitHub device activation";
    status.append(
      link,
      ` and enter ${state.user_code}. GitHub requires at least ${state.interval_seconds} seconds between checks.`,
    );
    button("Check authorization", "poll_github_auth");
    button("Cancel", "cancel_github_auth");
  }

  void invoke<GithubAuthState>("github_auth_state").then(render, () => {
    status.textContent =
      "GitHub connection state is unavailable. No connection is assumed.";
    actions.replaceChildren();
  });
}
