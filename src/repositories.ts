import type { Assignment, PolicyOverrides, WatchedIdentity } from "./policy";

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

export function primaryAssignmentId(
  repository: Repository,
): string | undefined {
  return repository.assignments?.length === 1
    ? repository.assignments[0].id
    : repository.primary_assignment_id;
}
