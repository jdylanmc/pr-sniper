export type Schedule =
  | { kind: "interval"; minutes: number; timezone: string }
  | { kind: "cron"; expression: string; timezone: string };
export type Selector =
  { kind: "default" } | { kind: "model" | "agent"; value: string };
export interface Policy {
  schedule: Schedule;
  watched_authors: { id: string; login: string }[];
  reviewer_assignment: boolean;
  adapter: "copilot";
  selector: Selector;
  prompt: string;
  automatic_agent_start: boolean;
  automatic_comment_publication: boolean;
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
/** A reusable review profile: model + optional doctrine + prompt +
 * signature. Agents are referenced by id from repository assignments. */
export interface Agent {
  id: string;
  name: string;
  model: string;
  doctrine?: string;
  prompt: string;
  signature: string;
}
/** One agent running on one repository, with its own timer and
 * permissions. `approve` is stored but never executed. */
export interface Assignment {
  id: string;
  agent_id: string;
  schedule: Schedule;
  comment: boolean;
  approve: boolean;
}

export function effectivePolicy(
  defaults: Policy,
  overrides: PolicyOverrides,
): Policy {
  return {
    schedule: overrides.schedule ?? defaults.schedule,
    watched_authors: overrides.watched_authors ?? defaults.watched_authors,
    reviewer_assignment:
      overrides.reviewer_assignment ?? defaults.reviewer_assignment,
    adapter: overrides.adapter ?? defaults.adapter,
    selector: overrides.selector ?? defaults.selector,
    prompt: overrides.prompt ?? defaults.prompt,
    automatic_agent_start:
      overrides.automatic_agent_start ?? defaults.automatic_agent_start,
    automatic_comment_publication:
      overrides.automatic_comment_publication ??
      defaults.automatic_comment_publication,
  };
}
