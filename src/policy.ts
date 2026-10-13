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
  /** Legacy readable configuration/evidence, not an execution preference. */
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
  actions?: {
    reply?: boolean;
    approve: boolean;
    merge: boolean;
    include_self?: boolean;
  };
  /** Absent = the role default (`defaultTools`). */
  tools?: PacketTool[];
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
  "leave_comments",
  "ready_for_human",
  "approve_pull_request",
  "approve_and_merge",
  "panic_exit",
  "submit_report",
] as const;
export type PacketTool = (typeof PACKET_TOOLS)[number];

/** Role default when `tools` is unset. Mirrors `default_tools` in storage.rs. */
export function defaultTools(
  primary: boolean,
  grants: { comment: boolean; approve: boolean; merge: boolean },
): PacketTool[] {
  const tools: PacketTool[] = [
    ...PACKET_TOOLS.slice(0, PACKET_TOOLS.indexOf("leave_comments")),
  ];
  if (primary) {
    if (grants.comment) tools.push("leave_comments");
    tools.push("ready_for_human");
    if (grants.approve) tools.push("approve_pull_request");
    if (grants.merge) tools.push("approve_and_merge");
  } else tools.push("submit_report");
  tools.push("panic_exit");
  return tools;
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
    adapter: overrides.adapter ?? defaults.adapter,
    selector: overrides.selector ?? defaults.selector,
    prompt: overrides.prompt ?? defaults.prompt,
    automatic_agent_start: true,
    automatic_comment_publication:
      overrides.automatic_comment_publication ??
      defaults.automatic_comment_publication,
  };
}
