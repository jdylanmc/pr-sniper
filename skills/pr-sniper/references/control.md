# Local control protocol 1

Commands: `node scripts/pr-sniper.mjs describe|inspect|--version`; otherwise read
one JSON object from stdin. Output: `{protocol,ok,result}` or `{protocol,ok:false,error:{code,detail}}`.
Exit 0 means an operation returned successfully, not that reviews completed.
Exit 1 means typed application failure; 2 means local CLI/transport failure.

| `operation`                      | Additional fields                                                                                                         |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `describe`                       | none                                                                                                                      |
| `inspect`                        | none; configuration, revision, safe account identities, queue, controls                                                   |
| `resources`                      | `kind`: `agents`, `doctrines`, `voices`, `repositories`, `preferences`; `offset` (default 0), `limit` (1–100, default 25) |
| `target`                         | `url`; local parsing and matching saved repositories                                                                      |
| `models`                         | `account_id`; already connected Copilot account                                                                           |
| `resolve`                        | `account_id`, `url`; connected GitHub account                                                                             |
| `validate`                       | `edit`; native ResourceEdit                                                                                               |
| `save`                           | `revision`, `edit`, optional `account_generation`, `allow_grants` (default false)                                         |
| `preset`                         | `revision`, `account_id`, `model`; additive SOLID/Security/Maintainer setup                                               |
| `watch`                          | `expected` repository, `url`, `account_generation`, `review_again` (default false), `overrides` (default `{}`)            |
| `pause`                          | `paused`                                                                                                                  |
| `check_now`                      | none                                                                                                                      |
| `retry`                          | `operation_id`                                                                                                            |
| `retry_state`                    | `item_id`                                                                                                                 |
| `cancel_review`                  | `operation_id`                                                                                                            |
| `publish`                        | `review_operation_id`; existing publication gates and receipts apply                                                      |
| `cancel_publication`             | `publication_id`                                                                                                          |
| `cancel_action`                  | `id`                                                                                                                      |
| `reconcile_action`               | `id`; observe the original uncertain action, never issue a replacement                                                    |
| `retry_action_observation`       | `item_id`                                                                                                                 |
| `follow_up` / `cancel_follow_up` | `id`; existing legacy reconciliation controls                                                                             |
| `clean`                          | optional `item_id`; terminal detail only                                                                                  |
| `dismiss_stale`                  | `item_id`, `expected_token`                                                                                               |
| `theme`                          | `theme`: `light`, `dark`, `os`                                                                                            |
| `login_item`                     | `enabled`; OS registration, not authentication                                                                            |
| `notifications`                  | `enabled`                                                                                                                 |
| `notification_category`          | `category`, `enabled`                                                                                                     |
| `installation`                   | none                                                                                                                      |
| `install`                        | `host`: `copilot_cli`, `expected`: installation revision or null                                                          |
| `uninstall`                      | `host`: `copilot_cli`, `expected`: installation revision                                                                  |

Every mutation additionally requires `request_id` (UUID) and `issued_at` (Unix
seconds). The CLI supplies these once if absent. Keep them when retrying a lost
reply. Receipts last five minutes; older requests fail `expired_request`.
An interrupted request remains `outcome_unknown`, never re-executed.
Mutation success includes `replayed`; a replay is the original observation,
not live status. Inspect again for current queue state. CLI responses include
the exact request ID and timestamp needed to retry a lost reply.

ResourceEdit: `{kind,id,expected,value}` for `agent`, `doctrine`, `voice`,
`repository`; preferences use `{kind:"preferences",expected,value}` without id.
Use `inspect.result.settings` as the source of exact native resource shapes.
New resources require UUID IDs, `expected:null`. `value:null` deletes.
New repositories are disabled, bound to a `resolve` result, and have no
assignments. Then save explicit assignments and enable the human-requested
scope. Repository schedules are five-field cron in the saved IANA time zone.
No generic settings replacement/import, credentials, authentication or shell
operations exist. Unknown fields and operation names are rejected.

Request overrides: `{ "<assignment UUID>": {"agent_id":"<saved Agent UUID>",
"doctrine_ids":["<saved Doctrine UUID>"]} }`. Either field may be null.
Only never-started work may receive overrides. Subsequent passes use shared
configuration unless explicitly overridden again. Running work is unchanged.

`saved` means committed configuration; `queued` means eligible pending work;
`waiting` means paused or prerequisite-gated; `running` means active;
`completed` means unchanged completed work was reused. Errors after intake
can include `admitted:true`; inspect before acting again.
An interrupted watch keeps a persistent pending-configuration hold: never-started
jobs do not run with unintended defaults. After inspecting, a new explicit
`watch` with the desired overrides (`{}` reuses existing work configuration;
null Agent/doctrine fields on an addressed assignment select shared configuration)
finishes configuration and releases it. Active jobs are not interrupted.

Default executable discovery: installed `launcher.json`, then standard macOS
Applications or Windows per-user PR Sniper install paths. No PATH shell lookup.
`PR_SNIPER_DATA_DIR` selects isolated data; background launch then also requires
an explicit `PR_SNIPER_KEYCHAIN_SERVICE` test scope. Neither variable is needed
for the installed app. Protocol mismatches never fall back to file mutation.
