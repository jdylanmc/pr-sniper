# MCP ↔ UI parity

Audit of the 47 MCP tools (49 capabilities) in the MCP control surface ([agentic-control.md](agentic-control.md)) against the app UI, at `main` 0.5.3.
Tracking: [#276](https://github.com/jdylanmc/pr-sniper/issues/276).

Legend: **Yes** = same effect reachable in the UI; **Partial** = narrower, bulk-only or buried; **No** = agent-only.
MCP → native: `src-control/mcp-server.mjs` → `src-tauri/src/control/{dispatcher,operations}.rs`.

## Summary

| UI parity | MCP capabilities |
| --------- | ---------------- |
| Yes       | 40               |
| Partial   | 5                |
| No        | 3                |
| N/A       | 1                |

## Read

| MCP tool                   | Native                                      | UI      | UI evidence                                                                   | Notes                                                                 |
| -------------------------- | ------------------------------------------- | ------- | ----------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `prepare_request`          | none (MCP-only request identity)            | N/A     | —                                                                             | Protocol plumbing                                                     |
| `inspect`                  | `Inspect` (dispatcher.rs)                   | Yes     | `src/panel.ts:104-111` summary, `src/automation.ts:55` pause/capacity         | Spread over header + Running count                                    |
| `list_agents`              | `Resources{agents}` → saved settings        | Yes     | `src/resources.ts:140` `saved_resources`; Settings › Agents                   |                                                                       |
| `list_doctrines`           | `Resources{doctrines}`                      | Yes     | Settings › Doctrines `src/settings.ts:1371`                                   |                                                                       |
| `list_voices`              | `Resources{voices}`                         | Yes     | Settings › Voices `src/settings.ts:1500`                                      |                                                                       |
| `list_repositories`        | `Resources{repositories}`                   | Yes     | Settings › Repositories                                                       |                                                                       |
| `get_preferences`          | `Resources{preferences}`                    | Yes     | Settings › Preferences                                                        |                                                                       |
| `list_repository_accounts` | `Accounts{repository}` + Azure DevOps setup | Yes     | `src/settings.ts:1039,1074` (`azure_devops_setup_state`, `github_auth_state`) |                                                                       |
| `list_copilot_accounts`    | `Accounts{copilot}`                         | Yes     | `src/settings.ts:1075` `copilot_auth_state`                                   |                                                                       |
| `list_queue`               | `Queue` (dispatcher.rs)                     | Yes     | Queue / Running / Reviewed `src/panel.ts:122-129`; `monitoring_snapshot`      |                                                                       |
| `list_health`              | `Health` → `Monitor::snapshot`              | Partial | `src/queue-detail.ts:144-150,401-463` failure facts on a PR item              | No repository health view; a failed poll with no PR item is invisible |
| `read_evidence`            | `Detail` → pruned item evidence             | Partial | `src/queue.ts:665` `renderItemEvidence`; Details `src/monitoring.ts:337`      | Rendered subset; no full evidence view                                |
| `inspect_target`           | `Target` → saved repository match           | Partial | Settings › Repositories › Add by URL `src/settings.ts:2927`                   | Resolves only while adding                                            |
| `resolve_repository`       | `resolve_provider_repository`               | Yes     | `src/settings.ts:2927,2957,3079,3227`                                         |                                                                       |
| `list_models`              | `list_copilot_models`                       | Yes     | `src/settings.ts:2140`, `src/genie.ts:267`                                    |                                                                       |

## Configure

| MCP tool                                                  | Native                                             | UI  | UI evidence                                      | Notes                                           |
| --------------------------------------------------------- | -------------------------------------------------- | --- | ------------------------------------------------ | ----------------------------------------------- |
| `validate_agent` / `_doctrine` / `_voice` / `_repository` | `validate_resource`                                | Yes | `src/resources.ts:155`                           | Implicit on edit                                |
| `save_agent` / `_doctrine` / `_voice` / `_repository`     | `save_resource` (+ grants)                         | Yes | `src/resources.ts:161`; delete = trash icon      | Grants via toggles                              |
| `validate_preferences` / `save_preferences`               | `validate_resource` / `save_resource{preferences}` | Yes | `src/resources.ts:199`; Settings › Preferences   |                                                 |
| `apply_review_preset`                                     | `preset::apply`                                    | No  | —                                                | SOLID + Security + Maintainer team by hand only |
| `set_pause`                                               | `set_automation_paused`                            | Yes | `src/automation.ts:116` header Monitoring switch |                                                 |
| `set_theme`                                               | `save_theme`                                       | Yes | `src/settings.ts:4934`                           |                                                 |
| `set_login_item`                                          | `save_login`                                       | Yes | `src/settings.ts:5074`                           |                                                 |
| `get_notifications`                                       | `notification_snapshot`                            | Yes | `src/notifications.ts:152`                       |                                                 |
| `set_notifications`                                       | `set_notifications_enabled`                        | Yes | `src/notifications.ts:194`                       |                                                 |
| `set_notification_category`                               | `set_notification_category`                        | Yes | `src/notifications.ts:199`                       |                                                 |

## Act

| MCP tool                            | Native                                                       | UI      | UI evidence                                                                                                 | Notes                                             |
| ----------------------------------- | ------------------------------------------------------------ | ------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `watch_pull_request`                | `watch` → `admit_explicit_pull_request` + `work::prioritize` | Partial | Settings › Repositories › Add by URL → Save `src/settings.ts:2697,3836`                                     | Buried in Settings; not from Queue; no prioritize |
| `watch_pull_request` `review_again` | `work::request_again`                                        | No      | `src/monitoring.ts:762-776` only for stale publication / legacy runs                                        | Reviewed PR cannot be re-reviewed on demand       |
| `watch_pull_request` `overrides`    | `work::apply_overrides`                                      | No      | —                                                                                                           | Per-request Agent/doctrine override               |
| `check_now`                         | `check_now` / `start_checks`                                 | Yes     | Tray › Check Now `src-tauri/src/lib.rs:4564`; Retry fallback `src/queue-detail.ts:243`                      | Not in panel                                      |
| `retry_poll`                        | `Monitor::manual_retry_operation`                            | Partial | Retry on a PR item `src/queue-detail.ts:209-217`; Tray › Check Now retries all parked polls                 | Per-repository only through an item               |
| `retry_review`                      | `review::host::request_batch`                                | Yes     | Retry `src/queue.ts:509-526`, `src/queue-detail.ts:338`; Details › Retry review `src/monitoring.ts:926-942` |                                                   |
| `retry_state`                       | `retry_packet_state`                                         | Yes     | Retry for Tossed / Write failed `src/queue-detail.ts:191-200`                                               |                                                   |
| `cancel_review`                     | `cancel_review`                                              | Yes     | Details › Cancel review `src/monitoring.ts:628,934`                                                         | Per run, inside PR detail                         |
| `publish_review`                    | `publish_review`                                             | Yes     | Details › consent + Publish `src/monitoring.ts:658,738-760`                                                 |                                                   |
| `cancel_publication`                | `cancel_publication`                                         | Yes     | Details › Withdraw publication `src/monitoring.ts:654,727-736`                                              |                                                   |
| `cancel_action`                     | `cancel_provider_action`                                     | Yes     | `src/actions.ts:244`                                                                                        |                                                   |
| `reconcile_action`                  | `reconcile_provider_action`                                  | Yes     | `src/actions.ts:237`; Retry `src/queue-detail.ts:184-191`                                                   |                                                   |
| `retry_action_observation`          | `retry_action_observation`                                   | Yes     | `src/actions.ts:196-201`; Retry `src/queue-detail.ts:244-250`                                               |                                                   |
| `resume_follow_up`                  | `start_follow_up`                                            | Yes     | `src/follow-up.ts:97`                                                                                       |                                                   |
| `cancel_follow_up`                  | `cancel_follow_up`                                           | Yes     | `src/follow-up.ts:96`                                                                                       |                                                   |
| `dismiss_stale`                     | `dismiss_stale_review`                                       | Yes     | Queue row × icon `src/queue.ts:528-566`                                                                     | Only while `dismiss_token` is set                 |
| `clean_reviewed`                    | `clean_reviewed`                                             | Yes     | Reviewed › Clean / Clean all `src/panel.ts:657-664,690-702,829-842`                                         |                                                   |

## UI-only (no MCP)

| UI capability                                           | Tauri command(s)                                                                                                      | Status                         |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| GitHub / Copilot sign-in, reconnect, disconnect         | `start_github_browser_auth`, `disconnect_github_auth`, `start_copilot_auth`, `disconnect_copilot_account`             | Intentional (no login via MCP) |
| Azure DevOps setup (CLI install, sign-in, organization) | `azure_devops_install`, `azure_devops_sign_in`, `azure_devops_set_organization`, `azure_devops_use_signed_in_account` | Intentional                    |
| MCP install / repair / remove                           | `control_install`, `control_uninstall`                                                                                | Intentional                    |
| Repository browser, owner list                          | `list_provider_repositories`, `list_provider_repository_owners`                                                       | Gap (P3)                       |
| Watched-author people search                            | `search_provider_people`, `resolve_provider_person`                                                                   | Gap (P3)                       |
| Schedule preview, time zones                            | `preview_schedule`, `time_zones`, `describe_schedules`                                                                | Gap (P3); validate covers cron |
| Connection verify / provider metadata                   | `verify_provider_connection`, `read_provider_metadata`                                                                | Gap (P3)                       |
| Copy diagnostics                                        | `diagnostics`                                                                                                         | Gap (P3)                       |
| Open PR / file on provider                              | `open_queue_destination`                                                                                              | N/A (agent has URLs)           |
| Retry all failed (Needs attention)                      | per-item recovery fan-out `src/queue.ts:585-603`                                                                      | In flight: #274 / #275         |
| Panel navigation, queue selection, Genie, changelog     | `panel_navigate`, `select_queue_item`, `monitoring_setup_review`                                                      | N/A                            |

## Gaps

| #   | Rank | Gap                                            | MCP                                 | Proposed UI                                                     |
| --- | ---- | ---------------------------------------------- | ----------------------------------- | --------------------------------------------------------------- |
| G1  | P1   | Re-review a reviewed PR (#277)                 | `watch_pull_request` `review_again` | ↻ icon in PR detail status actions                              |
| G2  | P2   | Watch a PR by URL from the Queue               | `watch_pull_request`                | Orange + pill on Queue → URL field → prioritized                |
| G3  | P2   | Repository poll failure with no PR item        | `list_health`, `retry_poll`         | Needs attention repository card: cause chip + Retry / Fix icons |
| G4  | P2   | Review team preset                             | `apply_review_preset`               | Agents › preset pill (account + model pickers)                  |
| G5  | P3   | Full evidence                                  | `read_evidence`                     | `{}` icon in Details → read-only evidence view                  |
| G6  | P3   | Per-request Agent / doctrine overrides         | `watch_pull_request` `overrides`    | Override pickers in G2 dialog                                   |
| G7  | P3   | Per-repository health (last success, next run) | `list_health`                       | Status dot + times on Settings › Repositories rows              |

Excluded (owned elsewhere): Retry all in Needs attention, MCP changes not refreshing the UI, #273, #275.
