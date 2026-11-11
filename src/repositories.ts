import type { Assignment, PolicyOverrides, WatchedIdentity } from "./policy";

export type AdmissionKind =
  "assigned" | "mentioned" | "include_users" | "exclude_users";
export interface AdmissionRule {
  kind: AdmissionKind;
  enabled: boolean;
  users?: WatchedIdentity[];
}
/** All pull requests, or the first enabled matching rule (in order) decides. */
export interface Admission {
  mode: "all" | "filtered" | "explicit";
  rules: AdmissionRule[];
}

export interface Repository {
  id: string;
  provider: "github" | "azure_devops";
  name: string;
  enabled: boolean;
  /** Five-field cron, evaluated in the saved time zone. */
  cron: string;
  provider_account_id?: string;
  provider_repository_id?: string;
  overrides?: PolicyOverrides;
  /** Optional per-repository watchlist, shared across this repository's
   * assignments. Exact login + stable id, no wildcards. */
  watched_authors?: WatchedIdentity[];
  admission?: Admission;
  /** N agents with repository-scoped permissions. */
  assignments?: Assignment[];
  primary_assignment_id?: string;
  review_preset?: string;
  /** Include draft pull requests in review. Absent = off. */
  review_drafts?: boolean;
  /** Path globs: include globs and `!` excludes. */
  paths?: string[];
  /** How `paths` combine with admission. Absent = `and` (gate). */
  paths_mode?: "and" | "or";
}

/** One path glob, as `paths::validate` checks it; `""` when valid. */
export function pathProblem(value: string, existing: string[]): string {
  const glob = value.startsWith("!") ? value.slice(1) : value;
  if (!glob) return "Path required";
  if (
    [...glob].length > 256 ||
    glob.startsWith("/") ||
    glob.split("/").includes("..") ||
    /[\u0000-\u001f\u007f-\u009f]/.test(glob)
  )
    return "Invalid path";
  if (existing.includes(value)) return "Already added";
  if (existing.length >= 200) return "Limit 200";
  return "";
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

/** Lowercase Azure DevOps organization of a pasted repository URL. */
export function azureDevopsOrganization(input: string): string | undefined {
  try {
    const url = new URL(input.trim());
    const host = url.hostname.toLowerCase();
    const organization =
      host === "dev.azure.com"
        ? url.pathname.split("/").filter(Boolean)[0]
        : host.endsWith(".visualstudio.com")
          ? host.slice(0, -".visualstudio.com".length)
          : undefined;
    return organization?.toLowerCase() || undefined;
  } catch {
    return undefined;
  }
}

export function primaryAssignmentId(
  repository: Repository,
): string | undefined {
  return repository.assignments?.length === 1
    ? repository.assignments[0].id
    : repository.primary_assignment_id;
}
