export type Schedule =
  | { kind: "interval"; minutes: number; timezone: string }
  | { kind: "cron"; expression: string; timezone: string };
export type Selector =
  { kind: "default" } | { kind: "model" | "agent"; value: string };
export interface Policy {
  schedule: Schedule;
  watched_authors: { id: string; login: string }[];
  reviewer_assignment: boolean;
  watch?: WatchChoices;
  adapter: "copilot";
  selector: Selector;
  prompt: string;
  /** Legacy readable configuration/evidence, not an execution preference. */
  automatic_agent_start: boolean;
  automatic_comment_publication: boolean;
}
export interface WatchChoices {
  all_pull_requests: boolean;
  by_user: boolean;
  mentions: boolean;
}

export function effectiveWatchedAuthors(
  policy: Policy,
  repositoryAuthors: WatchedIdentity[] = [],
): WatchedIdentity[] {
  const identities = new Map<string, WatchedIdentity>();
  for (const person of [...policy.watched_authors, ...repositoryAuthors])
    if (!identities.has(person.id)) identities.set(person.id, person);
  return [...identities.values()].sort((a, b) => a.id.localeCompare(b.id));
}

export function effectiveWatch(
  policy: Policy,
  authors: WatchedIdentity[],
): WatchChoices {
  return (
    policy.watch ?? {
      all_pull_requests: authors.length === 0,
      by_user: true,
      mentions: true,
    }
  );
}

export function watchSummary(
  policy: Policy,
  authors: WatchedIdentity[],
): string {
  const watch = effectiveWatch(policy, authors);
  const choices = [
    watch.all_pull_requests ? "All authors" : "",
    watch.by_user && !watch.all_pull_requests
      ? authors.length
        ? authors.map((author) => `@${author.login}`).join(", ")
        : "By user: no users selected"
      : "",
    policy.reviewer_assignment
      ? "Review requested from your GitHub account"
      : "",
    watch.mentions ? "@Mentions of your GitHub account" : "",
  ].filter(Boolean);
  return choices.join("; ") || "No new pull requests from watch choices.";
}
export type PolicyOverrides = Partial<Policy>;
export interface WatchedIdentity {
  id: string;
  login: string;
}
/** A named review principle. Its title is also its slug -- there is no
 * separate id. Plain text, ~500 words recommended, never enforced. */
export interface Doctrine {
  title: string;
  body: string;
}
/** A reusable review profile: model + ordered doctrines + prompt +
 * signature. Agents are referenced by id from repository assignments. */
export interface Agent {
  id: string;
  name: string;
  model: string;
  intelligence?: AgentIntelligence;
  ai_account?: AiAccount;
  doctrine?: string;
  doctrines?: string[];
  prompt: string;
  signature: string;
}
export interface AgentIntelligence {
  reasoning_effort: string | null;
  context_tier: string | null;
}
export interface AiAccount {
  provider: "copilot";
  account_id: string;
}
/** Repository-scoped permissions. Legacy schedule/approve are retained for
 * compatibility; only explicit actions opt into future provider operations. */
export interface Assignment {
  id: string;
  agent_id: string;
  schedule: Schedule;
  comment: boolean;
  approve: boolean;
  actions?: { reply?: boolean; approve: boolean; merge: boolean };
}

export const doctrineTitles = (agent: Agent): string[] =>
  agent.doctrines ?? (agent.doctrine ? [agent.doctrine] : []);

export function effectivePolicy(
  defaults: Policy,
  overrides: PolicyOverrides,
): Policy {
  return {
    schedule: overrides.schedule ?? defaults.schedule,
    watched_authors: overrides.watched_authors ?? defaults.watched_authors,
    reviewer_assignment:
      overrides.reviewer_assignment ?? defaults.reviewer_assignment,
    ...((overrides.watch ?? defaults.watch)
      ? { watch: overrides.watch ?? defaults.watch }
      : {}),
    adapter: overrides.adapter ?? defaults.adapter,
    selector: overrides.selector ?? defaults.selector,
    prompt: overrides.prompt ?? defaults.prompt,
    automatic_agent_start: true,
    automatic_comment_publication:
      overrides.automatic_comment_publication ??
      defaults.automatic_comment_publication,
  };
}
