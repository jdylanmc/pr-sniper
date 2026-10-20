export type Schedule =
  | { kind: "interval"; minutes: number; timezone: string }
  | { kind: "cron"; expression: string; timezone: string };
export type Selector =
  { kind: "default" } | { kind: "model" | "agent"; value: string };
export interface Policy {
  /** Legacy global schedule; moved onto repositories on load, never saved. */
  schedule?: Schedule;
  watched_authors: { id: string; login: string }[];
  reviewer_assignment: boolean;
  adapter: "copilot";
  selector: Selector;
  prompt: string;
  /** Legacy readable configuration/evidence, not an execution preference. */
  automatic_agent_start: boolean;
  automatic_comment_publication: boolean;
}
export type PolicyOverrides = Partial<Policy>;
export interface WatchedIdentity {
  id: string;
  login: string;
}
/** A review principle: stable id, display title and plain-text body
 * (~500 words recommended, never enforced). Agents reference the id. The id is
 * absent only on job evidence recorded before ids. */
export interface Doctrine {
  id: string;
  title: string;
  body: string;
}
/** A writing style for human-facing text: stable id, title, body.
 * `guardrails` adds the host's fixed tone guard; default on. */
export interface Voice {
  id: string;
  title: string;
  body: string;
  guardrails: boolean;
}
/** A reusable review profile: model + ordered doctrines + optional Voice +
 * prompt + signature. Agents are referenced by id from repository assignments. */
export interface Agent {
  id: string;
  name: string;
  model: string;
  intelligence?: AgentIntelligence;
  ai_account?: AiAccount;
  /** Legacy title references; only in job evidence recorded before ids. */
  doctrine?: string;
  doctrines?: string[];
  /** Ordered Doctrine ids; an id missing from the library is unavailable. */
  doctrine_ids?: string[];
  voice_id?: string;
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
  /** Legacy; never written for new assignments. */
  schedule?: Schedule;
  comment: boolean;
  approve: boolean;
  actions?: {
    reply?: boolean;
    approve: boolean;
    merge: boolean;
    include_self?: boolean;
  };
  /** Absent = the role default (`defaultTools`). */
  tools?: PacketTool[];
  /** Rubber Duck Agent id. Honored on the primary only. */
  rubber_duck?: string;
}

/** Mirrors `PACKET_TOOLS` in src-tauri/src/storage.rs (checked by a Rust test). */
export const PACKET_TOOLS = [
  "read_changes",
  "read_source",
  "get_thread",
  "get_checks",
  "read_check_log",
  "list_tree",
  "search_code",
  "rubber_duck",
  "leave_comments",
  "ready_for_human",
  "wait",
  "approve_pull_request",
  "approve_and_merge",
  "panic_exit",
  "submit_report",
] as const;
export type PacketTool = (typeof PACKET_TOOLS)[number];

/** Role default when `tools` is unset. Mirrors `Repository::assignment_tools` in storage.rs. */
export function defaultTools(
  primary: boolean,
  grants: { comment: boolean; approve: boolean; merge: boolean },
  duck = false,
): PacketTool[] {
  const tools: PacketTool[] = [
    ...PACKET_TOOLS.slice(0, PACKET_TOOLS.indexOf("rubber_duck")),
  ];
  if (primary && duck) tools.push("rubber_duck");
  if (primary) {
    if (grants.comment) tools.push("leave_comments");
    tools.push("ready_for_human");
    tools.push("wait");
    if (grants.approve) tools.push("approve_pull_request");
    if (grants.merge) tools.push("approve_and_merge");
  } else tools.push("submit_report");
  tools.push("panic_exit");
  return tools;
}

export const doctrineIds = (agent: Agent): string[] => agent.doctrine_ids ?? [];

/** Agent-ordered Doctrine titles; a dangling reference shows as unavailable. */
export const doctrineTitles = (agent: Agent, library: Doctrine[]): string[] =>
  doctrineIds(agent).map(
    (id) => library.find((d) => d.id === id)?.title ?? `${id} (unavailable)`,
  );

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
    automatic_agent_start: true,
    automatic_comment_publication:
      overrides.automatic_comment_publication ??
      defaults.automatic_comment_publication,
  };
}
