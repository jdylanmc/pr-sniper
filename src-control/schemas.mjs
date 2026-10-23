import { z } from "zod";

export const id = z.string().min(1);
export const uuid = z.string().uuid();
export const revision = z.string().regex(/^[a-f0-9]{64}$/);
export const identity = z.strictObject({
  request_id: uuid.describe(
    "From prepare_request; reuse with issued_at after an uncertain reply.",
  ),
  issued_at: z
    .number()
    .int()
    .describe("Unix seconds returned by prepare_request."),
});
export const pageInput = {
  offset: z.number().int().min(0).optional(),
  limit: z.number().int().min(1).max(100).optional(),
};
const nullableOptional = (schema) => schema.nullable().optional();
const watched = z.strictObject({ id, login: id });
const schedule = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("interval"),
    minutes: z.number().int(),
    timezone: id,
  }),
  z.strictObject({ kind: z.literal("cron"), expression: id, timezone: id }),
]);
const selector = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("default") }),
  z.strictObject({ kind: z.literal("model"), value: id }),
  z.strictObject({ kind: z.literal("agent"), value: id }),
]);
const policyFields = {
  schedule: nullableOptional(schedule),
  watched_authors: z.array(watched),
  reviewer_assignment: z.boolean(),
  adapter: z.literal("copilot"),
  selector,
  prompt: z.string(),
  automatic_agent_start: z.boolean(),
  automatic_comment_publication: z.boolean(),
};
export const policy = z.strictObject(policyFields);
const overrides = z.strictObject(policyFields).partial();
export const doctrine = z.strictObject({
  id: uuid,
  title: id,
  body: z.string(),
});
export const voice = doctrine.extend({ guardrails: z.boolean() });
export const agent = z.strictObject({
  id: uuid,
  name: id,
  model: z.string(),
  intelligence: nullableOptional(
    z.strictObject({
      reasoning_effort: z.string().nullable(),
      context_tier: z.string().nullable(),
    }),
  ),
  ai_account: nullableOptional(
    z.strictObject({ provider: z.literal("copilot"), account_id: id }),
  ),
  doctrine: nullableOptional(z.string()),
  doctrines: nullableOptional(z.array(z.string())),
  doctrine_ids: z.array(id).optional(),
  voice_id: nullableOptional(id),
  prompt: z.string(),
  signature: z.string(),
});
export const tools = z.enum([
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
]);
export const assignment = z.strictObject({
  id: uuid,
  agent_id: uuid,
  schedule: nullableOptional(schedule),
  comment: z.boolean(),
  approve: z.boolean().optional(),
  actions: nullableOptional(
    z.strictObject({
      reply: z.boolean().optional(),
      approve: z.boolean(),
      merge: z.boolean(),
      include_self: z.boolean().optional(),
    }),
  ),
  tools: nullableOptional(z.array(tools)),
  rubber_duck: nullableOptional(id),
});
export const repository = z.strictObject({
  id: uuid,
  provider: z.enum(["github", "azure_devops"]),
  name: id,
  enabled: z.boolean(),
  cron: z.string().optional(),
  provider_account_id: nullableOptional(id),
  provider_repository_id: nullableOptional(id),
  overrides: overrides.optional(),
  review_preset: nullableOptional(id),
  watched_authors: z.array(watched).optional(),
  admission: nullableOptional(
    z.strictObject({
      mode: z.enum(["all", "filtered", "explicit"]),
      rules: z.array(
        z.strictObject({
          kind: z.enum([
            "assigned",
            "mentioned",
            "include_users",
            "exclude_users",
            "include_paths",
            "exclude_paths",
          ]),
          enabled: z.boolean(),
          users: nullableOptional(z.array(watched)),
          paths: nullableOptional(z.array(z.string())),
        }),
      ),
    }),
  ),
  assignments: z.array(assignment).optional(),
  primary_assignment_id: nullableOptional(id),
  review_drafts: z.boolean().optional(),
});
export const preferences = z.strictObject({
  defaults: policy,
  capacity: z.number().int().min(1).max(4294967295),
  review_inactivity_minutes: z.number().int().min(1).max(4294967295),
  time_zone: id,
  root_folder: z.string().nullable(),
  presets: z.array(z.strictObject({ id, name: id, body: z.string() })),
  default_review_preset: id.nullable(),
  rubber_duck: z.strictObject({
    max_calls: z.number().int().min(1).max(32),
    call_timeout_seconds: z.number().int().min(30).max(600),
    primary_window_minutes: z.number().int().min(5).max(120),
  }),
});
export const requestOverride = z.strictObject({
  agent_id: id.nullable(),
  doctrine_ids: z.array(id).nullable(),
});
const operationState = z.enum([
  "queued",
  "running",
  "interrupted",
  "completed",
  "failed",
  "manual_retry",
]);
const failureClass = z.enum(["credentials", "permission", "network", "other"]);
export const notificationCategory = z.enum([
  "confirmation",
  "human_input",
  "ready",
  "failure",
  "test",
  "approved",
  "merged",
  "final_review_cleared",
]);
export const notificationSettings = z
  .object({
    platform: z.enum(["macos", "windows"]),
    enabled: z.boolean(),
    permission: z
      .object({
        authorization: z.string(),
        alerts_enabled: z.boolean().nullable(),
        center_enabled: z.boolean().nullable(),
      })
      .nullable(),
    error: z.string().nullable(),
    categories: z.array(
      z.strictObject({ category: notificationCategory, enabled: z.boolean() }),
    ),
    replayed: z.boolean().optional(),
  })
  .passthrough();
export const account = z
  .object({
    state: z.enum(["connected", "reconnect_required"]),
    provider: z.string(),
    account_id: id,
    login: id,
    warning: z.string().optional(),
    reason: z.string().optional(),
    connection_generation: z.number().int().optional(),
  })
  .passthrough();
export const health = z
  .object({
    configuration_id: id,
    repository_name: id,
    provider_account_id: id.nullable(),
    provider_repository_id: id.nullable(),
    account_login: id.nullable(),
    enabled: z.boolean(),
    schedule_available: z.boolean(),
    in_flight: z.boolean(),
    operation_id: id.nullable(),
    state: operationState.nullable(),
    failure: z.string().nullable(),
    failure_class: failureClass.nullable(),
    retryable: z.boolean(),
    last_failure: z.string().nullable(),
    last_attempt: z.number().nullable(),
    last_success: z.number().nullable(),
    next_run: z.number(),
    next_attempt_at: z.number().nullable(),
  })
  .passthrough();
// Native ids; tests/control/fixtures/native-enums.json pins them to the app.
export const exitName = z.enum([
  "leave_comments",
  "ready_for_human",
  "wait",
  "approve_pull_request",
  "approve_and_merge",
  "panic_exit",
  "submit_report",
  "submit_verdict",
  "lgtm",
]);
export const progress = z.enum([
  "approving",
  "merging",
  "merge_pending_behind_base",
  "merge_pending_conflicts",
  "publishing",
]);
export const mergeBlock = z.enum(["behind_base", "conflicts"]);
export const disposition = z.enum([
  "still_applies",
  "fixed",
  "withdrawn",
  "unverifiable",
  "author_wont_fix",
  "signed_off",
  "no_longer_applies",
  "carried_forward",
]);
export const queueItem = z
  .object({
    id,
    configuration_id: id,
    number: z.number().int(),
    title: z.string(),
    state: id,
    failure_class: failureClass.nullable(),
    failure_cause: z.string().nullable(),
    dismiss_token: z.string().nullable(),
    resume_at: z.number().nullable(),
    exit: exitName.nullable(),
    progress: progress.nullable(),
    merge_pending: z
      .strictObject({ reason: mergeBlock, ci_not_run: z.boolean() })
      .nullable(),
    review_count: z.number().int(),
    reviews: z.array(
      z.strictObject({
        candidate_key: id,
        operation_id: id.nullable(),
        state: operationState.nullable(),
        error: z.string().nullable(),
        next_attempt_at: z.number().nullable(),
        exit: exitName.nullable(),
        exits_denied: z.array(
          z.strictObject({ exit: exitName, reason: z.string() }),
        ),
        dispositions: z.partialRecord(disposition, z.number().int()),
        duck_failures: z.array(
          z.strictObject({
            ordinal: z.number().int(),
            failure: z.string(),
            reason: z.string().nullable(),
          }),
        ),
      }),
    ),
  })
  .passthrough();
export const page = (item, config = false) =>
  z
    .object({
      values: z.array(item),
      total: z.number().int(),
      next_offset: z.number().int().nullable(),
      ...(config ? { revision } : {}),
    })
    .passthrough();
export const summary = z
  .object({
    revision,
    counts: z.strictObject({
      agents: z.number(),
      doctrines: z.number(),
      voices: z.number(),
      repositories: z.number(),
      provider_accounts: z.number(),
      copilot_accounts: z.number(),
      queue_jobs: z.number(),
      repository_health: z.number(),
      pending_watch_configuration: z.number(),
    }),
    automation: z.strictObject({ paused: z.boolean(), capacity: z.number() }),
    theme: z.enum(["light", "dark", "os"]),
    launch_at_login: z.boolean(),
    notifications: z.object({
      enabled: z.boolean(),
      muted: z.array(z.string()),
    }),
    login_registration: z.string().nullable(),
  })
  .passthrough();
export const saved = z
  .object({
    state: z.literal("saved"),
    revision,
    warning: z.string().nullable().optional(),
    replayed: z.boolean(),
  })
  .passthrough();
export const watch = z
  .object({
    state: z.enum(["not_queued", "queued", "waiting", "running", "completed"]),
    reason: z.string().nullable(),
    paused: z.boolean(),
    prioritized: z.number().int(),
    item_id: id.nullable(),
    replayed: z.boolean(),
  })
  .passthrough();
export const detail = z.strictObject({
  encoding: z.literal("json-text"),
  revision,
  text: z.string(),
  offset: z.number().int(),
  next_offset: z.number().int().nullable(),
  total: z.number().int(),
});
export const readiness = z.object({
  configuration_ready: z.boolean(),
  issues: z.array(z.string()),
  repositories: z.array(
    z.object({
      repository_id: id,
      primary_assignment_id: id.nullable(),
      issues: z.array(z.string()),
      assignments: z.array(
        z.tuple([
          id,
          z.strictObject({
            primary: z.boolean(),
            comment: z.boolean(),
            reply: z.boolean(),
            approve: z.boolean(),
            merge: z.boolean(),
          }),
        ]),
      ),
    }),
  ),
});
export const envelope = (result) =>
  z.object({
    protocol: z.literal(1),
    ok: z.boolean(),
    result: result.optional(),
    error: z.object({ code: id, detail: z.unknown() }).optional(),
    request: identity.optional(),
  });
export { z };
