import type { Assignment, PolicyOverrides, WatchedIdentity } from "./policy";
import {
  effectivePolicy,
  effectiveWatchedAuthors,
  type Policy,
} from "./policy";

export function repositoryWatch(defaults: Policy, repository: Repository) {
  const policy = effectivePolicy(defaults, repository.overrides ?? {});
  const people = effectiveWatchedAuthors(policy, repository.watched_authors);
  return { policy, people };
}

export interface Repository {
  id: string;
  provider: "github" | "azure_devops";
  name: string;
  enabled: boolean;
  provider_account_id?: string;
  provider_repository_id?: string;
  overrides?: PolicyOverrides;
  /** Optional per-repository watchlist, shared across this repository's
   * assignments. Exact login + stable id, no wildcards. */
  watched_authors?: WatchedIdentity[];
  /** N agents with repository-scoped permissions. */
  assignments?: Assignment[];
  primary_assignment_id?: string;
  review_preset?: string;
}

// This identifies the intake context, not repository access or PR admission.
// Native resolution still validates the complete input with the chosen account.
export function repositoryProviderContext(
  input: string,
): "github" | "azure_devops" | "unsupported" | "invalid" {
  const value = input.trim();
  if (!value.includes("://")) return "github";
  try {
    const host = new URL(value).hostname.toLowerCase();
    if (host === "github.com") return "github";
    if (host === "dev.azure.com" || host.endsWith(".visualstudio.com"))
      return "azure_devops";
  } catch {
    return "invalid";
  }
  return "unsupported";
}

export function primaryAssignmentId(
  repository: Repository,
): string | undefined {
  return repository.assignments?.length === 1
    ? repository.assignments[0].id
    : repository.primary_assignment_id;
}
