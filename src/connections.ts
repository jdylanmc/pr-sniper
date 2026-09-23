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
  name: string;
  connection?: Connection;
  message: string;
  pulls?: PullRequest[];
}

const observations = new Map<string, Observation>();
const failures: Record<string, string> = {
  missing_cli: "The optional development GitHub CLI probe is unavailable.",
  broken_cli: "The optional development GitHub CLI probe is broken.",
  signed_out:
    "The PR Sniper GitHub App authorization is missing, expired, or rejected. Reconnect GitHub.",
  wrong_identity:
    "The authenticated GitHub account does not match the expected account ID. No repository action was taken.",
  missing_read_permission:
    "GitHub denied repository or pull-request read access, or the repository does not exist. Check repository access and credential permissions.",
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

export function renderConnection(root: HTMLElement, repository: Repository) {
  let observation = observations.get(repository.id);
  if (observation?.name !== repository.name) {
    observations.delete(repository.id);
    observation = undefined;
  }
  root.innerHTML = `
    <p class="settings-hint">Uses the connected PR Sniper GitHub App account and the explicitly selected installation repository.</p>
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
  status.textContent =
    observation?.message ??
    "Not verified. Connect GitHub and explicitly select this repository from an App installation; no provider changes are performed.";
  read.disabled = !observation?.connection;

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
      : "Verifying GitHub CLI identity and access...";
    details.replaceChildren();
    try {
      if (metadata && pinned) {
        const result = await invoke<{
          connection: Connection;
          pull_requests: PullRequest[];
        }>("read_github_metadata", {
          id: repository.id,
          expectedAccountId: pinned.identity.id,
          expectedRepositoryId: pinned.repository.id,
        });
        if (!root.isConnected) return;
        const pulls = result.pull_requests;
        observation = {
          name: repository.name,
          connection: result.connection,
          pulls,
          message: `${describe(result.connection)} Complete metadata: ${pulls.length} PRs, ${pulls.reduce((sum, pull) => sum + pull.files.length, 0)} changed files. Last read ${new Date().toLocaleTimeString()}.`,
        };
        renderPulls(pulls);
      } else {
        const connection = await invoke<Connection>(
          "verify_github_connection",
          { id: repository.id },
        );
        if (!root.isConnected) return;
        observation = {
          name: repository.name,
          connection,
          message: `${describe(connection)} Last verified ${new Date().toLocaleTimeString()}.`,
        };
      }
    } catch (cause) {
      if (!root.isConnected) return;
      observation = {
        name: repository.name,
        message:
          typeof cause === "string" && Object.hasOwn(failures, cause)
            ? failures[cause]
            : "Connection read failed. No raw error details or partial metadata are exposed.",
      };
    } finally {
      unlock();
      if (root.isConnected) {
        if (observation) {
          observations.set(repository.id, observation);
          status.textContent = observation.message;
        }
        read.disabled = !observation?.connection;
      }
    }
  }
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void run(false);
  });
  read.addEventListener("click", () => void run(true));
}
