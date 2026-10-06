import { invoke } from "@tauri-apps/api/core";
import type { Repository } from "./repositories";
import { lockSettings } from "./drafts";

interface Connection {
  identity: { id: string; login: string };
  repository: { id: string; name: string };
  capabilities: {
    read: boolean;
    comment: "available" | "unavailable" | "unknown";
  };
}

interface PullRequest {
  number: number;
  title: string;
  state: "open" | "closed" | "merged";
  draft: boolean;
  head_sha: string;
  author: { id: string; login: string } | null;
  requested_reviewers: { id: string; login: string }[];
  files: { path: string; status: string }[];
}

interface Observation {
  provider: Repository["provider"];
  name: string;
  account_id: string;
  repository_id?: string;
  connection?: Connection;
  message: string;
  pulls?: PullRequest[];
}

interface AccountAvailability {
  available: boolean;
  label?: string;
}

const observations = new Map<string, Observation>();
window.addEventListener("pr-sniper:provider-account-state", (event) => {
  const detail = (
    event as CustomEvent<{
      provider: Repository["provider"];
      account_id: string;
      available: boolean;
    }>
  ).detail;
  if (detail.available) return;
  for (const [id, observation] of observations)
    if (
      observation.provider === detail.provider &&
      observation.account_id === detail.account_id
    )
      observations.delete(id);
});
const failures: Record<string, string> = {
  signed_out:
    "The PR Sniper GitHub OAuth authorization is missing, expired, or rejected. Reconnect GitHub.",
  missing_scope:
    "The GitHub OAuth authorization no longer grants the required repo scope. Reconnect and review the requested public/private repository access.",
  scope_unverified:
    "GitHub did not provide the scope evidence needed to verify this read. No missing scope or grant is established. Retry this selected account; if it persists, check this app's authorization in GitHub.",
  organization_policy_denied:
    "GitHub organization policy or SAML single sign-on blocks this repository. Authorize the OAuth App for the organization or contact its administrator.",
  organization_policy_denied_with_missing_scope:
    "GitHub reported an organization authorization restriction and an authorization missing repo scope. Ask the organization administrator to approve the PR Sniper OAuth App; also reconnect this selected account in Accounts with repository access. Reconnecting alone cannot bypass organization policy.",
  wrong_identity:
    "The authenticated GitHub account does not match the expected account ID. No repository action was taken.",
  missing_read_permission:
    "GitHub denied repository or pull-request read access, or the repository does not exist. Check repository access and credential permissions.",
  repository_unavailable:
    "GitHub could not find this repository or hides it from the selected account. Check the URL, repository access, and the PR Sniper OAuth App's organization approval or single sign-on authorization. A 404 does not prove the repository is absent.",
  rate_limited:
    "GitHub rate limited this read. Wait for the provider limit to reset before retrying.",
  network: "Could not reach GitHub securely. Check your network and try again.",
  timeout: "GitHub timed out. No complete result was accepted.",
  provider_failure:
    "GitHub could not complete this read. Try again after checking provider health.",
  invalid_response:
    "GitHub returned an invalid response. No complete result was accepted.",
  incomplete_read:
    "GitHub metadata is incomplete or exceeded a provider/resource limit. No partial result was accepted.",
  revision_changed:
    "A pull request changed during the read. No mixed-revision result was accepted; read again.",
  invalid_repository:
    "The saved repository is invalid. Correct its configuration before connecting.",
  repository_changed:
    "The repository changed or was retargeted. Verify its saved identity again.",
  configuration:
    "Saved repository settings are unavailable. Reload Settings and check local storage.",
};

export function describeReadFailureDetails(error: unknown): string | undefined {
  if (!error || typeof error !== "object") return;
  if (
    "provider_rejected_status" in error &&
    typeof error.provider_rejected_status === "number" &&
    Number.isInteger(error.provider_rejected_status) &&
    error.provider_rejected_status >= 100 &&
    error.provider_rejected_status <= 599
  )
    return `GitHub rejected this lookup (HTTP ${error.provider_rejected_status}). Check the current repository URL/input and provider policy, then retry. This response does not establish a missing scope.`;
  if (!("rate_limited_with_context" in error)) return;
  const context = error.rate_limited_with_context;
  if (
    !context ||
    typeof context !== "object" ||
    !("retry_after_seconds" in context) ||
    !("reset_at" in context) ||
    !("organization_access_incomplete" in context) ||
    typeof context.organization_access_incomplete !== "boolean" ||
    !("missing_repo_scope" in context) ||
    typeof context.missing_repo_scope !== "boolean"
  )
    return;
  const { retry_after_seconds: delay, reset_at: reset } = context;
  const validTimestamp = (value: unknown) =>
    value === null ||
    (typeof value === "number" && Number.isSafeInteger(value) && value >= 0);
  if (!validTimestamp(delay) || !validTimestamp(reset)) return;
  const messages = [
    "GitHub rate limited this lookup. Wait before trying again.",
  ];
  if (typeof delay === "number") messages.push(`Retry after ${delay} seconds.`);
  if (typeof reset === "number")
    messages.push(
      `GitHub reports the rate-limit reset at Unix time ${reset} (UTC seconds).`,
    );
  if (context.organization_access_incomplete)
    messages.push(
      "Organization access is also restricted or incomplete. Check this app's organization approval or single sign-on authorization after the rate limit clears; discovery is not complete.",
    );
  if (context.missing_repo_scope)
    messages.push(
      "The selected authorization also lacks repo scope. After waiting, reconnect this selected account with repository access.",
    );
  if (context.missing_repo_scope && context.organization_access_incomplete)
    messages.push("Reconnecting alone cannot bypass organization policy.");
  return messages.join(" ");
}

export function readFailureMissingScope(error: unknown): boolean {
  if (
    error === "missing_scope" ||
    error === "organization_policy_denied_with_missing_scope"
  )
    return true;
  return !!(
    describeReadFailureDetails(error) &&
    error &&
    typeof error === "object" &&
    "rate_limited_with_context" in error &&
    error.rate_limited_with_context &&
    typeof error.rate_limited_with_context === "object" &&
    "missing_repo_scope" in error.rate_limited_with_context &&
    error.rate_limited_with_context.missing_repo_scope === true
  );
}

function describe(connection: Connection): string {
  const { identity, capabilities } = connection;
  const comment =
    capabilities.comment === "available"
      ? "Comment scope available; item restrictions must be rechecked"
      : capabilities.comment === "unavailable"
        ? "Comment permission unavailable"
        : "Comment permission unverified (this credential exposes no scope evidence)";
  return `Verified ${identity.login} (${identity.id}). Repository and PR read access verified. ${comment}. This is not publication authorization. No automation was enabled.`;
}

export function renderConnection(
  root: HTMLElement,
  repository: Repository,
  account: AccountAvailability = { available: false },
) {
  let observation = observations.get(repository.id);
  if (
    observation?.provider !== repository.provider ||
    observation?.name !== repository.name ||
    observation?.account_id !== repository.provider_account_id ||
    observation?.repository_id !== repository.provider_repository_id
  ) {
    observations.delete(repository.id);
    observation = undefined;
  }
  root.innerHTML = `
    <p class="settings-hint">${repository.provider === "github" ? `Uses GitHub account ${account.label ?? repository.provider_account_id ?? "not selected"} and the explicitly selected repository.` : "Azure DevOps authentication is not implemented in this build."}</p>
    <form class="connection-form">
      <button type="submit">Verify GitHub connection</button>
      <button type="button" class="read-metadata">Read PR metadata</button>
    </form>
    <p role="status" class="connection-status"></p>
    <div class="pull-metadata"></div>`;
  const form = root.querySelector<HTMLFormElement>("form")!;
  form.dataset.draftKey = `connection:${repository.id}`;
  const status = root.querySelector<HTMLElement>("[role=status]")!;
  const read = root.querySelector<HTMLButtonElement>(".read-metadata")!;
  const details = root.querySelector<HTMLElement>(".pull-metadata")!;
  const authChanged = (event: Event) => {
    if (!root.isConnected) {
      window.removeEventListener(
        "pr-sniper:provider-account-state",
        authChanged,
      );
      return;
    }
    const detail = (
      event as CustomEvent<{
        provider: Repository["provider"];
        account_id: string;
        available: boolean;
      }>
    ).detail;
    if (
      detail.provider !== repository.provider ||
      detail.account_id !== repository.provider_account_id ||
      detail.available === account.available
    )
      return;
    window.removeEventListener("pr-sniper:provider-account-state", authChanged);
    renderConnection(root, repository, {
      available: detail.available,
      label: account.label ?? detail.account_id,
    });
  };
  window.addEventListener("pr-sniper:provider-account-state", authChanged);
  status.textContent =
    !account.available && repository.provider_account_id
      ? `Needs attention. GitHub account ${account.label ?? repository.provider_account_id} must be reconnected or this repository must be explicitly rebound before provider actions are available.`
      : (observation?.message ??
        (repository.provider_account_id
          ? `Not verified. Acting account ${repository.provider_account_id}; no provider changes are performed.`
          : "Needs attention. Explicitly select a provider account and repository before reading."));
  form.querySelector<HTMLButtonElement>("button")!.disabled =
    !account.available ||
    !repository.provider_account_id ||
    repository.provider === "azure_devops";
  read.disabled = !account.available || !observation?.connection;

  function renderPulls(pulls: PullRequest[]) {
    details.replaceChildren();
    for (const pull of pulls) {
      const item = document.createElement("details");
      const summary = document.createElement("summary");
      summary.textContent = `#${pull.number} ${pull.title} - ${pull.state}${pull.draft ? ", draft" : ""} - ${pull.files.length} files`;
      const metadata = document.createElement("p");
      metadata.textContent = `Head ${pull.head_sha}. Author: ${pull.author ? `${pull.author.login} (${pull.author.id})` : "deleted/unavailable"}. Requested reviewers: ${pull.requested_reviewers.map((reviewer) => `${reviewer.login} (${reviewer.id})`).join(", ") || "none"}.`;
      const files = document.createElement("ul");
      for (const file of pull.files) {
        const row = document.createElement("li");
        row.textContent = `${file.status}: ${file.path}`;
        files.append(row);
      }
      item.append(summary, metadata, files);
      details.append(item);
    }
  }
  if (observation?.pulls) renderPulls(observation.pulls);

  async function run(metadata: boolean) {
    const pinned = observation?.connection;
    if (metadata && !pinned) return;
    const unlock = lockSettings(root);
    status.textContent = metadata
      ? "Reading every PR and changed-file page..."
      : "Verifying GitHub OAuth identity and repository access...";
    details.replaceChildren();
    try {
      if (metadata && pinned) {
        const result = await invoke<{
          connection: Connection;
          pull_requests: PullRequest[];
        }>("read_provider_metadata", {
          id: repository.id,
          expectedAccountId: pinned.identity.id,
          expectedRepositoryId: pinned.repository.id,
        });
        if (!root.isConnected) return;
        const pulls = result.pull_requests;
        observation = {
          provider: repository.provider,
          name: repository.name,
          account_id: result.connection.identity.id,
          repository_id: repository.provider_repository_id!,
          connection: result.connection,
          pulls,
          message: `${describe(result.connection)} Complete metadata: ${pulls.length} PRs, ${pulls.reduce((sum, pull) => sum + pull.files.length, 0)} changed files. Last read ${new Date().toLocaleTimeString()}.`,
        };
        renderPulls(pulls);
      } else {
        const connection = await invoke<Connection>(
          "verify_provider_connection",
          { id: repository.id },
        );
        if (!root.isConnected) return;
        observation = {
          provider: repository.provider,
          name: repository.name,
          account_id: connection.identity.id,
          repository_id: repository.provider_repository_id!,
          connection,
          message: `${describe(connection)} Last verified ${new Date().toLocaleTimeString()}.`,
        };
      }
    } catch (cause) {
      if (!root.isConnected) return;
      observations.delete(repository.id);
      observation = undefined;
      status.textContent =
        describeReadFailureDetails(cause) ??
        (typeof cause === "string" && Object.hasOwn(failures, cause)
          ? failures[cause]
          : "Connection read failed. No raw error details or partial metadata are exposed.");
      if (
        readFailureMissingScope(cause) ||
        (typeof cause === "string" &&
          [
            "signed_out",
            "wrong_identity",
            "network",
            "rate_limited",
            "timeout",
            "provider_failure",
            "invalid_response",
            "missing_scope",
            "organization_policy_denied_with_missing_scope",
            "configuration",
          ].includes(cause))
      )
        window.dispatchEvent(new Event("pr-sniper:refresh-provider-accounts"));
    } finally {
      unlock();
      if (root.isConnected) {
        if (observation) {
          observations.set(repository.id, observation);
          status.textContent = observation.message;
        }
        read.disabled = !account.available || !observation?.connection;
      }
    }
  }
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void run(false);
  });
  read.addEventListener("click", () => void run(true));
}
