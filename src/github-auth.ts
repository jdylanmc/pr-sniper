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
  | {
      state: "reconnect_required";
      reason:
        | "denied"
        | "expired"
        | "network"
        | "provider"
        | "invalid_response"
        | "credentials_unavailable";
    };

export interface GithubInstalledRepository {
  installation_id: string;
  repository: { id: string; name: string };
}

export function renderGithubAuth(
  root: HTMLElement,
  selectRepository?: (repository: GithubInstalledRepository) => void,
) {
  root.innerHTML = `<div class="github-auth-card"><div><h2>GitHub account</h2><p role="status">Reading connection state...</p><div class="github-auth-repositories"></div></div><div class="github-auth-actions"></div></div>`;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const actions = root.querySelector<HTMLElement>(".github-auth-actions")!;
  const repositories = root.querySelector<HTMLElement>(
    ".github-auth-repositories",
  )!;

  function actionButton(label: string, action: () => Promise<void>) {
    const control = document.createElement("button");
    control.type = "button";
    control.textContent = label;
    control.addEventListener("click", async () => {
      setBusy(true);
      try {
        await action();
      } catch (cause) {
        const failure = String(cause).toLowerCase();
        status.textContent = failure.includes("network")
          ? "The GitHub network request failed. No authorization or automation was assumed."
          : failure.includes("provider")
            ? "GitHub returned an error. No authorization or automation was assumed."
            : "GitHub connection could not be updated. No authorization or automation was assumed.";
      } finally {
        setBusy(false);
      }
    });
    actions.append(control);
  }

  function button(label: string, command: string) {
    actionButton(label, async () => render(await invoke(command)));
  }

  function setBusy(busy: boolean) {
    actions
      .querySelectorAll<HTMLButtonElement>("button")
      .forEach((control) => (control.disabled = busy));
  }

  function render(state: GithubAuthState) {
    actions.replaceChildren();
    repositories.replaceChildren();
    window.dispatchEvent(
      new CustomEvent("pr-sniper:github-auth-state", {
        detail: {
          account_id: state.state === "connected" ? state.account_id : null,
        },
      }),
    );
    if (state.state === "disconnected") {
      status.textContent =
        "Disconnected. Connect through the PR Sniper GitHub App. This does not enable reviews, comments, notifications, or merging.";
      button("Connect GitHub", "begin_github_auth");
      return;
    }
    if (state.state === "reconnect_required") {
      const reason = {
        denied: "The GitHub authorization was denied.",
        expired:
          "The GitHub authorization expired or can no longer be refreshed.",
        network: "The GitHub network request failed.",
        provider: "GitHub returned an error while validating the connection.",
        invalid_response: "GitHub returned an invalid authorization response.",
        credentials_unavailable:
          "The GitHub credentials could not be restored or stored safely.",
      }[state.reason];
      status.textContent = `Reconnect required. ${reason}`;
      button("Reconnect GitHub", "begin_github_auth");
      return;
    }
    if (state.state === "connected") {
      status.textContent = `Connected as ${state.login} (${state.account_id}). Stable identity verified; repository access is not implied. No automation was enabled.`;
      button("Disconnect GitHub", "disconnect_github_auth");
      actionButton("Load installed repositories", async () => {
        const result = await invoke<{
          identity: { id: string; login: string };
          repositories: GithubInstalledRepository[];
        }>("list_github_repositories");
        if (result.identity.id !== state.account_id)
          throw new Error("GitHub account changed");
        repositories.replaceChildren();
        if (!result.repositories.length) {
          repositories.textContent =
            "No repositories are available through this GitHub App installation.";
          return;
        }
        const list = document.createElement("ul");
        for (const installed of result.repositories) {
          const item = document.createElement("li");
          const use = document.createElement("button");
          use.type = "button";
          use.textContent = `Use ${installed.repository.name}`;
          use.addEventListener("click", () => selectRepository?.(installed));
          item.append(use);
          list.append(item);
        }
        repositories.append(list);
      });
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
