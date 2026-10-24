import { McpServer } from "@modelcontextprotocol/server";
import {
  serveStdio,
  StdioServerTransport,
} from "@modelcontextprotocol/server/stdio";
import { randomUUID } from "node:crypto";
import { invoke, protocol, useExecutable } from "./control-client.mjs";
import * as s from "./schemas.mjs";

const { z } = s;
const version =
  typeof PR_SNIPER_VERSION === "string" ? PR_SNIPER_VERSION : "development";
const simpleResult = z
  .object({ state: z.string(), replayed: z.boolean() })
  .passthrough();
const mutation = (shape) => ({ ...s.identity.shape, ...shape });

export function createServer() {
  const server = new McpServer(
    { name: "pr-sniper", version },
    { maxToolInputElements: 20_000 },
  );
  server.registerTool(
    "prepare_request",
    {
      title: "Prepare a retry-safe request",
      description:
        "Create an identity for one explicitly requested native mutation. Reuse both fields if its reply is lost. Does not launch or change the app.",
      inputSchema: z.strictObject({}),
      outputSchema: s.identity,
      annotations: {
        readOnlyHint: true,
        destructiveHint: false,
        openWorldHint: false,
      },
    },
    async () => {
      const result = {
        request_id: randomUUID(),
        issued_at: Math.floor(Date.now() / 1000),
      };
      return {
        content: [{ type: "text", text: JSON.stringify(result) }],
        structuredContent: result,
      };
    },
  );

  function register(
    name,
    description,
    input,
    output,
    command,
    changes = false,
    openWorld = false,
  ) {
    server.registerTool(
      name,
      {
        description,
        inputSchema: z.strictObject(input),
        outputSchema: s.envelope(output),
        annotations: {
          readOnlyHint: !changes,
          destructiveHint: changes,
          idempotentHint: true,
          openWorldHint: openWorld,
        },
      },
      async (args, context) => {
        if (context.mcpReq.signal.aborted)
          return {
            isError: true,
            content: [
              {
                type: "text",
                text: "Request cancelled before native dispatch.",
              },
            ],
          };
        let reply;
        try {
          reply = await invoke(command(args));
        } catch (error) {
          reply = {
            protocol,
            ok: false,
            error: {
              code: error.code ?? "bridge_unavailable",
              detail: error.detail ?? null,
            },
            ...(args.request_id
              ? {
                  request: {
                    request_id: args.request_id,
                    issued_at: args.issued_at,
                  },
                }
              : {}),
          };
        }
        return {
          isError: !reply.ok,
          content: [{ type: "text", text: JSON.stringify(reply) }],
          structuredContent: reply,
        };
      },
    );
  }

  register(
    "inspect",
    "Read bounded native application counts, pause/capacity and preferences. Starts the app if closed without resuming an explicit pause.",
    {},
    s.summary,
    () => ({ operation: "inspect" }),
  );
  for (const [name, kind, schema] of [
    ["list_agents", "agents", s.agent],
    ["list_doctrines", "doctrines", s.doctrine],
    ["list_voices", "voices", s.voice],
    ["list_repositories", "repositories", s.repository],
    ["get_preferences", "preferences", s.preferences],
  ])
    register(
      name,
      `Read saved ${kind} and configuration revision. Follow next_offset until null; never mix revisions.`,
      s.pageInput,
      s.page(schema, true),
      (args) => ({ operation: "resources", kind, ...args }),
    );
  for (const [name, kind] of [
    ["list_repository_accounts", "repository"],
    ["list_copilot_accounts", "copilot"],
  ])
    register(
      name,
      `List existing connected ${kind} identities safely; no credentials or login flow. Repository accounts include the generation required for validated binding/watch commands.`,
      s.pageInput,
      s.page(s.account),
      (args) => ({ operation: "accounts", kind, ...args }),
    );
  register(
    "list_queue",
    "Read bounded PR state and exact native candidate/operation identities. Completed work is not retryable; explicit review again uses watch_pull_request.",
    s.pageInput,
    s.page(s.queueItem),
    (args) => ({ operation: "queue", ...args }),
  );
  register(
    "list_health",
    "Read repository polling health, failures and exact retry operation IDs, including failed first polls with no PR items.",
    s.pageInput,
    s.page(s.health),
    (args) => ({ operation: "health", ...args }),
  );
  register(
    "read_evidence",
    "Read revision-checked JSON-text chunks of one PR item's full evidence. Concatenate chunks only while revision matches.",
    {
      item_id: s.id,
      offset: z.number().int().min(0).optional(),
      limit: z.number().int().min(1).max(65536).optional(),
    },
    s.detail,
    (args) => ({ operation: "detail", ...args }),
  );
  register(
    "inspect_target",
    "Parse a GitHub PR/repository URL and find saved repository IDs before choosing scope or an acting account.",
    { url: s.id },
    z.object({
      target: z.object({
        name: s.id,
        pull_request_number: z.number().nullable(),
      }),
      repository_ids: z.array(s.id),
      revision: s.revision,
    }),
    (args) => ({ operation: "target", ...args }),
  );
  register(
    "resolve_repository",
    "Resolve a URL through one explicitly selected already-connected repository account. Returns verified repository identity and connection generation; does not configure or enable it.",
    { account_id: s.id, url: s.id },
    z
      .object({
        identity: z.object({ id: s.id, login: s.id }),
        repository: z.object({ id: s.id, name: s.id }),
        account_generation: z.number().int(),
        pull_request: z
          .object({ id: s.id, number: z.number() })
          .passthrough()
          .nullable()
          .optional(),
      })
      .passthrough(),
    (args) => ({ operation: "resolve", ...args }),
    false,
    true,
  );
  register(
    "list_models",
    "List models from one already-connected Copilot account. Never performs login or chooses an account/model implicitly.",
    { account_id: s.id },
    z.array(z.object({ id: s.id, name: z.string() }).passthrough()),
    (args) => ({ operation: "models", ...args }),
    false,
    true,
  );
  for (const [kind, schema] of [
    ["agent", s.agent],
    ["doctrine", s.doctrine],
    ["voice", s.voice],
    ["repository", s.repository],
  ]) {
    const edit = {
      id: s.uuid,
      expected: schema.nullable(),
      value: schema
        .nullable()
        .describe(
          "Exact replacement, or null for explicit deletion. References and collisions are validated by the app.",
        ),
    };
    register(
      `validate_${kind}`,
      `Validate one ${kind} edit without committing it.`,
      edit,
      s.readiness,
      (args) => ({ operation: "validate", edit: { kind, ...args } }),
    );
    register(
      `save_${kind}`,
      `Commit one ${kind} using native validators and optimistic revision checks. Preserve unrelated fields. Grant expansion requires explicit human intent and allow_grants:true.`,
      mutation({
        revision: s.revision,
        ...edit,
        allow_grants: z.boolean().optional(),
        account_generation: z.number().int().optional(),
      }),
      s.saved,
      ({
        request_id,
        issued_at,
        revision,
        allow_grants,
        account_generation,
        ...edit
      }) => ({
        operation: "save",
        request_id,
        issued_at,
        revision,
        allow_grants,
        account_generation,
        edit: { kind, ...edit },
      }),
      true,
    );
  }
  register(
    "save_preferences",
    "Save exact non-login preferences through native conflict validation. Immediate controls use their separate tools.",
    mutation({
      revision: s.revision,
      expected: s.preferences,
      value: s.preferences,
    }),
    s.saved,
    ({ request_id, issued_at, revision, expected, value }) => ({
      operation: "save",
      request_id,
      issued_at,
      revision,
      edit: { kind: "preferences", expected, value },
    }),
    true,
  );
  register(
    "validate_preferences",
    "Validate exact non-login preferences without committing.",
    { expected: s.preferences, value: s.preferences },
    s.readiness,
    (args) => ({
      operation: "validate",
      edit: { kind: "preferences", ...args },
    }),
  );
  register(
    "apply_review_preset",
    "Explicitly add missing shipped SOLID/Security teammates and Maintainer primary components, preserving customizations. Uses selected connected Copilot account/model; returns zero-grant assignment suggestions.",
    mutation({ revision: s.revision, account_id: s.id, model: s.id }),
    z.object({
      state: z.literal("saved"),
      revision: s.revision,
      replayed: z.boolean(),
      assignments: z.array(s.assignment),
      primary_assignment_id: s.id,
      agents: z.array(s.agent),
    }),
    (args) => ({ operation: "preset", ...args }),
    true,
    true,
  );
  register(
    "watch_pull_request",
    "Watch the exact PR using an existing enabled repository snapshot. Does not enable scope or grants. Reuses/prioritizes eligible work; never interrupts running work or bypasses Wait. Set review_again only on explicit human rerun intent.",
    mutation({
      expected: s.repository,
      url: s.id,
      account_generation: z.number().int(),
      review_again: z.boolean().optional(),
      overrides: z.record(s.uuid, s.requestOverride).optional(),
    }),
    s.watch,
    (args) => ({ operation: "watch", ...args }),
    true,
    true,
  );
  for (const [name, operation, shape, description] of [
    [
      "set_pause",
      "pause",
      { paused: z.boolean() },
      "Set explicit global pause/resume. Never resume for a generic watch request.",
    ],
    [
      "check_now",
      "check_now",
      {},
      "Request checks of enabled repositories through normal schedule/pause/backpressure gates.",
    ],
    [
      "retry_poll",
      "retry",
      { operation_id: s.id },
      "Retry only the failed poll identified by list_health, not every repository.",
    ],
    [
      "retry_review",
      "retry_review",
      { candidate_key: s.id },
      "Retry currently failed/cancelled AI work. Completed work rejects; use explicit watch review_again for a new pass.",
    ],
    [
      "retry_state",
      "retry_state",
      { item_id: s.id },
      "Retry a parked packet/exit state through native recovery.",
    ],
    [
      "cancel_review",
      "cancel_review",
      { operation_id: s.id },
      "Cancel the exact native AI operation; never infer immediate teardown.",
    ],
    [
      "publish_review",
      "publish",
      { review_operation_id: s.id },
      "Explicitly request existing validated publication; current identity/grants/freshness and receipts still gate effects.",
    ],
    [
      "cancel_publication",
      "cancel_publication",
      { publication_id: s.id },
      "Cancel the exact publication through native safety gates.",
    ],
    [
      "cancel_action",
      "cancel_action",
      { id: s.id },
      "Cancel an existing provider-action intent.",
    ],
    [
      "reconcile_action",
      "reconcile_action",
      { id: s.id },
      "Observe/reconcile an existing uncertain action; never replace its provider identity.",
    ],
    [
      "retry_action_observation",
      "retry_action_observation",
      { item_id: s.id },
      "Retry native action observation for the exact item.",
    ],
    [
      "resume_follow_up",
      "follow_up",
      { id: s.id },
      "Resume an existing legacy follow-up through its native gates.",
    ],
    [
      "cancel_follow_up",
      "cancel_follow_up",
      { id: s.id },
      "Cancel the exact legacy follow-up.",
    ],
    [
      "dismiss_stale",
      "dismiss_stale",
      { item_id: s.id, expected_token: s.id },
      "Acknowledge one exact stale local state without deleting history or provider receipts.",
    ],
    [
      "set_theme",
      "theme",
      { theme: z.enum(["light", "dark", "os"]) },
      "Set the saved native application theme.",
    ],
    [
      "set_login_item",
      "login_item",
      { enabled: z.boolean() },
      "Set OS launch-at-login registration through the app. This is not account authentication.",
    ],
  ])
    register(
      name,
      description,
      mutation(shape),
      simpleResult,
      (args) => ({ operation, ...args }),
      true,
      [
        "check_now",
        "retry",
        "publish",
        "reconcile_action",
        "retry_action_observation",
        "follow_up",
      ].includes(operation),
    );
  register(
    "clean_reviewed",
    "Remove terminal local detail only; native receipts and uncertain writes remain protected.",
    mutation({ item_id: s.id.nullable().optional() }),
    z.object({ value: z.number(), replayed: z.boolean() }),
    (args) => ({ operation: "clean", ...args }),
    true,
  );
  // Native notification snapshots vary by OS; expose the stable controls and
  // retain the app's complete platform result as structured native data.
  register(
    "get_notifications",
    "Read notification preferences and actual OS permission status without requesting permission.",
    {},
    s.notificationSettings,
    () => ({ operation: "notification_settings" }),
  );
  register(
    "set_notifications",
    "Set notification opt-in through native OS permission handling.",
    mutation({ enabled: z.boolean() }),
    s.notificationSettings,
    (args) => ({ operation: "notifications", ...args }),
    true,
  );
  register(
    "set_notification_category",
    "Set a native notification category without changing unrelated preferences.",
    mutation({ category: s.notificationCategory, enabled: z.boolean() }),
    s.notificationSettings,
    (args) => ({ operation: "notification_category", ...args }),
    true,
  );
  return server;
}

// The app registers `node <app resources>/mcp/mcp-server.mjs --app <executable>`.
const app = process.argv.indexOf("--app");
if (app !== -1) useExecutable(process.argv[app + 1]);

serveStdio(createServer, {
  legacy: "serve",
  transport: new StdioServerTransport(undefined, undefined, {
    maxBufferSize: 2 * 1024 * 1024,
  }),
  onerror: () =>
    process.stderr.write("[pr-sniper-mcp] protocol transport error\n"),
  maxSubscriptions: 1,
});
