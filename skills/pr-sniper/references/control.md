# PR Sniper MCP tools

The app-bundled local stdio server is the agent-facing interface. Its typed
`tools/list` schemas define exact inputs and outputs. It calls the same native
operations as the desktop UI; annotations never grant app permissions.

| Need / native operation   | MCP tool                                                                                                |
| ------------------------- | ------------------------------------------------------------------------------------------------------- |
| Retry identity            | `prepare_request` (no app effect)                                                                       |
| Summary / `inspect`       | `inspect`                                                                                               |
| Resources / `resources`   | `list_agents`, `list_doctrines`, `list_voices`, `list_repositories`, `get_preferences`                  |
| Accounts / `accounts`     | `list_repository_accounts`, `list_copilot_accounts`                                                     |
| Queue / health / detail   | `list_queue`, `list_health`, `read_evidence`                                                            |
| Target / resolve / models | `inspect_target`, `resolve_repository`, `list_models`                                                   |
| Validate / save           | `validate_agent`, `save_agent` (likewise doctrine, voice, repository, preferences)                      |
| Preset / watch            | `apply_review_preset`, `watch_pull_request`                                                             |
| Pause / check / retry     | `set_pause`, `check_now`, `retry_poll`, `retry_review`, `retry_state`, `retry_all`                      |
| Review / publication      | `cancel_review`, `publish_review`, `cancel_publication`                                                 |
| Action / follow-up        | `cancel_action`, `reconcile_action`, `retry_action_observation`, `resume_follow_up`, `cancel_follow_up` |
| Local cleanup             | `clean_reviewed`, `dismiss_stale`                                                                       |
| Immediate preferences     | `set_theme`, `set_login_item`, `get_notifications`, `set_notifications`, `set_notification_category`    |

- Every mutation requires the identity from `prepare_request`. A new resource
  may use a separately prepared UUID as its stable resource ID.
- Replies expose the native envelope in `structuredContent` and text;
  `isError:true` preserves a typed native error, not success.
- On an uncertain reply, retry only the same tool with identical arguments and
  identity within the receipt lifetime. Cancellation after dispatch does not
  undo native work. A replay is an old observation; read current state afterward.
- No login, arbitrary shell, generic execute or second review engine.
- MCP registration is an explicit human Settings action; the server cannot
  register itself or edit another server. The skill is installed separately
  (`npx skills add jdylanmc/pr-sniper --skill pr-sniper -g -a github-copilot`).

## Internal diagnostic protocol 2

The CLI below is retained for diagnostics/acceptance, **not** the primary agent API.

Commands: `node src-control/pr-sniper.mjs [--app <executable>] describe|inspect|--version`; otherwise read
one JSON object from stdin. Output: `{protocol,ok,result}` or `{protocol,ok:false,error:{code,detail}}`.
Exit 0 means an operation returned successfully, not that reviews completed.
Exit 1 means typed application failure; 2 means local CLI/transport failure.

| `operation`                      | Additional fields                                                                                                                                                                                                                                                                                                                                                          |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `describe`                       | none                                                                                                                                                                                                                                                                                                                                                                       |
| `inspect`                        | none; bounded counts, revision, pause/capacity, immediate control status                                                                                                                                                                                                                                                                                                   |
| `accounts`                       | `kind`: `repository` or `copilot`; `offset`, `limit`; safe identities and repository connection generations; repository pages add `azure_devops` setup state and the bound Azure DevOps account                                                                                                                                                                            |
| `queue`                          | `offset`, `limit`; bounded PR summaries, candidate/operation IDs, saved `error`/`next_attempt_at`, item `exit`/`progress`/`merge_pending`, review `exit` and `exits_denied` reasons, finding `dispositions` (incl. `author_wont_fix`, `no_longer_applies`, `signed_off`, `carried_forward`), failed `duck_failures` with typed `reason`, `failure_class` (`model` when an Agent's model refused) and `failed_model` (`agent_id`, `agent_name`, `model`, `refusal`, `status`; remedy: change that Agent's model), `failed_operation` (`repository_check`, `review`, `publication`, `exit_write`, `follow_up`, `provider_action`) and `remedy` (`sign_in`, `change_model`, `reconcile`, `retry`) (first 25 reviews; rest in detail) |
| `health`                         | `offset`, `limit`; bounded repository/account identities, polling state/cause, exact `operation_id`, and `retryable`                                                                                                                                                                                                                                                       |
| `detail`                         | `item_id`, character `offset`, `limit` (1–65536); JSON-text chunks with stable content revision; reviews, publications, follow-ups, exit writes; pruned PR: tombstone, `state: pruned`                                                                                                                                                                                     |
| `resources`                      | `kind`: `agents`, `doctrines`, `voices`, `repositories`, `preferences`; `offset` (default 0), `limit` (1–100, default 25)                                                                                                                                                                                                                                                  |
| `target`                         | `url`; local parsing and matching saved repositories                                                                                                                                                                                                                                                                                                                       |
| `models`                         | `account_id`; already connected Copilot account                                                                                                                                                                                                                                                                                                                            |
| `resolve`                        | `account_id`, `url`; connected GitHub account, or the bound Azure DevOps identity (`dev.azure.com`/`*.visualstudio.com` URL)                                                                                                                                                                                                                                               |
| `validate`                       | `edit`; native ResourceEdit                                                                                                                                                                                                                                                                                                                                                |
| `save`                           | `revision`, `edit`, optional `account_generation`, `allow_grants` (default false)                                                                                                                                                                                                                                                                                          |
| `preset`                         | `revision`, `account_id`, `model`; additive SOLID/Security/Maintainer setup                                                                                                                                                                                                                                                                                                |
| `watch`                          | `expected` repository, `url`, `account_generation`, `review_again` (default false), `overrides` (default `{}`)                                                                                                                                                                                                                                                             |
| `pause`                          | `paused`                                                                                                                                                                                                                                                                                                                                                                   |
| `check_now`                      | none                                                                                                                                                                                                                                                                                                                                                                       |
| `retry`                          | exact polling `operation_id` from `health`; native polling gates apply                                                                                                                                                                                                                                                                                                     |
| `retry_review`                   | `candidate_key`; only currently failed/cancelled AI work; completed work rejects with `review_again_required`                                                                                                                                                                                                                                                              |
| `retry_state`                    | `item_id`                                                                                                                                                                                                                                                                                                                                                                  |
| `retry_all`                      | none; retries parked packet states and failed AI work; skips permanent and panic; returns `retried`, `skipped`                                                                                                                                                                                                                                                             |
| `cancel_review`                  | `operation_id`                                                                                                                                                                                                                                                                                                                                                             |
| `publish`                        | `review_operation_id`; existing publication gates and receipts apply                                                                                                                                                                                                                                                                                                       |
| `cancel_publication`             | `publication_id`                                                                                                                                                                                                                                                                                                                                                           |
| `cancel_action`                  | `id`                                                                                                                                                                                                                                                                                                                                                                       |
| `reconcile_action`               | `id`; observe the original uncertain action, never issue a replacement                                                                                                                                                                                                                                                                                                     |
| `retry_action_observation`       | `item_id`                                                                                                                                                                                                                                                                                                                                                                  |
| `follow_up` / `cancel_follow_up` | `id`; existing legacy reconciliation controls                                                                                                                                                                                                                                                                                                                              |
| `clean`                          | optional `item_id`; terminal detail only                                                                                                                                                                                                                                                                                                                                   |
| `dismiss_stale`                  | `item_id`, `expected_token`                                                                                                                                                                                                                                                                                                                                                |
| `theme`                          | `theme`: `light`, `dark`, `os`                                                                                                                                                                                                                                                                                                                                             |
| `login_item`                     | `enabled`; OS registration, not authentication                                                                                                                                                                                                                                                                                                                             |
| `notifications`                  | `enabled`                                                                                                                                                                                                                                                                                                                                                                  |
| `notification_settings`          | none; bounded notification preferences and OS permission state                                                                                                                                                                                                                                                                                                             |
| `notification_category`          | `category`, `enabled`                                                                                                                                                                                                                                                                                                                                                      |
| `installation`                   | none                                                                                                                                                                                                                                                                                                                                                                       |
| `install`                        | `host`: `copilot_cli`, `expected`: installation revision or null                                                                                                                                                                                                                                                                                                           |
| `uninstall`                      | `host`: `copilot_cli`, `expected`: installation revision                                                                                                                                                                                                                                                                                                                   |

Every mutation additionally requires `request_id` (UUID) and `issued_at` (Unix
seconds). The CLI supplies these once if absent. Keep them when retrying a lost
reply. Receipts last five minutes; older requests fail `expired_request`.
An interrupted request remains `outcome_unknown`, never re-executed.
`control_busy` means the request was not executed (native queue full, or
`reason: expired` when it could not start within 25 seconds); retry it later
with the same identity. The client waits 120 seconds after sending. An
operation still running at 90 seconds returns `outcome_unknown` but finishes
before any other request starts; its receipt (same request ID) then holds the
real result. An operation that crashes the app also returns
`outcome_unknown`; the app exits and relaunches on the next call. Inspect the
receipt (same request ID) and current state before any retry.
Mutation success includes `replayed`; a replay is the original observation,
not live status. Read `queue` again for current queue state. CLI responses include
the exact request ID and timestamp needed to retry a lost reply.

ResourceEdit: `{kind,id,expected,value}` for `agent`, `doctrine`, `voice`,
`repository`; preferences use `{kind:"preferences",expected,value}` without id.
Use `resources.result.values` as the source of exact native resource shapes.
Resources/accounts/queue/health pages return `next_offset` (null at the end). Resource
pages are byte-bounded as well as count-bounded; follow the returned cursor,
not the requested count. Reject mixed resource revisions while assembling views.
For full evidence, concatenate `detail.text` chunks at the returned character
offsets, require identical `detail.revision`, then parse the complete JSON.
Polling health exists independently of PR items. An empty queue does not mean
repository polling succeeded. Use `health` even for a first-poll failure.
`retry_review` rechecks current eligibility under the Store lock: a stale client
that saw failure cannot rerun work that has since completed. Explicit
`watch.review_again` is the separate fresh-pass operation.
New resources require UUID IDs, `expected:null`. `value:null` deletes.
New repositories are disabled, bound to a `resolve` result, and have no
assignments. Azure DevOps repositories: `provider:"azure_devops"`,
`provider_repository_id` `{organization}/{project ID}/{repository ID}`, people
by identity GUID plus display name; their pull requests are polled, admitted, reviewed and published like GitHub ones; Approve is the reviewer vote; approve-and-merge is refused (`unsupported_provider_action`). Then save explicit assignments and enable the human-requested
scope. Repository schedules are five-field cron in the saved IANA time zone.
No generic settings replacement/import, credentials, authentication or shell
operations exist. Unknown fields and operation names are rejected.

Request overrides: `{ "<assignment UUID>": {"agent_id":"<saved Agent UUID>",
"doctrine_ids":["<saved Doctrine UUID>"]} }`. Either field may be null.
Only never-started work may receive overrides. Subsequent passes use shared
configuration unless explicitly overridden again. Running work is unchanged.

`saved` means committed configuration; `queued` means eligible pending work;
`waiting` means paused or prerequisite-gated; `running` means active;
`completed` means unchanged completed work was reused. `waiting` reasons:
`external_prerequisite`, `pending_configuration`, `paused`,
`feedback_observation` (retries after the next clean poll), `execution_gates`. Errors after intake
can include `admitted:true`; inspect before acting again.
An interrupted watch keeps a persistent pending-configuration hold: never-started
jobs do not run with unintended defaults. After inspecting, a new explicit
`watch` with the desired nonempty assignment override map finishes configuration
and releases it (null Agent/doctrine fields select shared configuration).
Ordinary `{}` watch requests and rejected reruns preserve pre-existing holds.
Active jobs are not interrupted.

Executable: the registration's `--app <absolute executable>`, else standard
macOS Applications or Windows per-user PR Sniper install paths. No PATH lookup.
`PR_SNIPER_DATA_DIR` selects isolated data; background launch then also requires
an explicit `PR_SNIPER_KEYCHAIN_SERVICE` test scope. Neither variable is needed
for the installed app. Protocol mismatches never fall back to file mutation.

The app owns only `mcpServers.pr-sniper` in `~/.copilot/mcp-config.json`:
`node <app>/mcp/mcp-server.mjs --app <executable>`. Other servers and root
fields are preserved; an unowned or customized entry is a conflict. A durable
intent survives interruption and status completes only authorized work. An
owned entry for another app path is `incomplete` (Repair); an installed app
re-points it at launch. A linked `~/.copilot` or `mcp-config.json` (macOS)
is written at its user-owned real target; retargeting it conflicts.
Native isolated profiles cannot read or mutate the real registration.
SDK 2.3.1 handles legacy initialization and modern 2026-07-28 stdio negotiation.
