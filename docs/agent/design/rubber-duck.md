# Rubber duck plan

Status: implemented on `feat/rubber-duck` with [ADR 0023](../../adr/0023-rubber-duck-verification.md) (accepted 2026-10-14). Limits are Saved Preferences; duck setup failure is a job error. No live model run yet.

## Slices

Each slice lands green on its own (`npm test`, `npm run lint`, `npm run format:check`; UI slices add `npm run test:settings`, sharded).

| #   | Slice               | Files                                                                                                                                                                                                                 | Tests                                                                                                                                                                                                                                                                         |
| --- | ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Config and snapshot | `storage.rs` (`Assignment.rubber_duck`, `PACKET_TOOLS`, `assignment_tools`, readiness, Agent delete guard), `review.rs` (`ExecutionConfiguration.rubber_duck`, `resolve`, `same_execution`), `src/policy.ts` mirror   | legacy settings without field load and round-trip unchanged; unknown field still rejected; readiness: tool without duck, teammate with tool, duck without account/model; duck Agent undeletable; snapshot immutable after Agent edit; `same_execution` differs on duck change |
| 2   | Contract            | new `review/duck.rs` (input, `Verdict`, `DuckFailure`, `DuckCall`, bounds), `exits.rs` (`SubmitVerdict` duck-only), `review/tools.rs` (`rubber_duck` definition)                                                      | schema shape; each bound at limit and limit+1; `revise` requires `revised_body`; enum rejection; no cut text                                                                                                                                                                  |
| 3   | Duck packet         | `packet.rs` (`Role::Duck`, `SCHEMA = 5`), `packet/rules.rs` (duck rules, primary duck rule), `packet/sections.rs` (Claim section, fencing), goldens                                                                   | duck golden; same inputs same `duck_packet_hash`; claim fenced and defused; primary golden with and without `rubber_duck`; teammate golden unchanged except schema                                                                                                            |
| 4   | Duck runner         | `review/duck.rs` (`DuckRunner` trait: policy-owned contract), `review/runtime.rs` (duck `Task`, tool handler, serialization, call count, deadline), `copilot.rs` (credential + dedicated thread runtime for the duck) | fake runner: serialization of parallel calls, `limit_reached`, `budget_exhausted`, `timed_out`, `exit_taken`, `rate_limited` records gate, `unavailable`; real-config test: duck session catalog = granted reads + `submit_verdict`, no `rubber_duck`                         |
| 5   | Wire into primary   | `review/packet_run.rs` (offer when snapshot has duck), `review/host.rs` (inject runner, cancellation, `ReviewResult.ducks`), `storage/diagnostics.rs` (`ToolName::RubberDuck`, events)                                | host: capacity 1 primary + duck completes (no second reservation); pause cancels duck and slot stays stopping until both end; duck failure leaves primary exit intact; moved state tosses run with verdicts; diagnostics content-free                                         |
| 6   | Settings UI         | `src/settings.ts` (Rubber duck drill-in, Tools row sync, summary), `src/settings.css`, `prototypes/v2/`                                                                                                               | Playwright: row hidden for teammate; choose/clear duck; Tools row follows; focus restore; Agent delete blocked; WebKit run                                                                                                                                                    |
| 7   | Activity            | `src/activity.ts` (duck calls with run)                                                                                                                                                                               | Playwright: verdict icons, counts, failure kinds                                                                                                                                                                                                                              |
| 8   | Docs                | `CONTEXT.md` (Rubber Duck, Duck call, Verdict), `docs/review-packet.md` (tool, limits, rules, OQ-06 closed), `docs/settings.md`, changelog proposal                                                                   | `npm run format:check`, `npm run test:skills`                                                                                                                                                                                                                                 |

Live: one sandbox run on a disposable `jdylanmc/pr-sniper-sandbox` PR, only with owner authorization.

## Migration

- All new fields `#[serde(default, skip_serializing_if = "Option::is_none")]` / `Vec::is_empty`; structs keep `deny_unknown_fields`.
- No data rewrite. Absent duck = feature off. Legacy snapshots and results show no duck, never rebuilt.
- Downgrade: older builds reject files carrying the new fields (same as earlier additive fields).

## Rebase notes

- Settings cleanup: new row lives in the assignment editor main group only; no Status/Diagnostics coupling.
- Voices: duck packet has no Voice section; primary Voice section unaffected; order Claim after Files, before Directive.
- Notifications, themes: no overlap.

## UI sketch

Assignment editor, main group (primary only):

```text
Agent            Security Bot      >
Primary          [on]
Rubber duck      Duck Bot          >
Permissions      Publish, Reply    >
Tools            9 Tools           >
```

Rubber duck drill-in:

```text
( ) None
(o) Duck Bot        gpt-x
( ) SOLID Bot       gpt-y
```

Tools drill-in adds one row:

```text
[duck]  Rubber duck     Second Agent checks one finding   [on]
```

Activity, primary run:

```text
Rubber duck   3   ✓2  ✎1  ✗0
  ✓ src/a.rs:12
  ✎ src/b.rs:40
  ✓ thread 991
```
