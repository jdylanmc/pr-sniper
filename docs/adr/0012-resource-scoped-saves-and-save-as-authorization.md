# Save per resource; Save is the authorization

## Context

One big Settings draft made unrelated edits conflict and lose work. Per-PR "Start" prompts and manual-start prefs added friction without adding safety.

## Decision

- Agent, doctrine, repository saves persist only that resource, at once. Assignment save commits its owning repo. Save preferences is independent.
- Shared seam over Store: `saved_resources`, `validate_resource`, `save_resource`. `ResourceEdit{value,expected}`: `value:null` deletes, `expected:null` creates. Compare-and-save per resource.
- AI/repo connections and startup registration save immediately.
- Rust validates at the storage boundary: cron, IANA tz, intervals, unique positive account IDs, non-empty selectors. Recognized GitHub token patterns rejected without echo. Invalid input never replaces last valid config.
- Saving repo config + assignments authorizes ongoing read-only reviews (forks, reviewer-requested, later revisions). No per-PR start. Human-approved amendment #120.
- Save writes an authorization receipt. No downgrade migration.
- Remaining gates: global pause, repo disabled, admission, account/assignment, capacity.
- Review never grants publication, approval, merge (see ADR 0017).

## Consequences

- Failed write keeps editor and last valid state. **Discard draft and reload** is explicit.
- No setup wizard. Readiness = saved config only.
- Disable or rebind invalidates authority.
- Legacy start flags readable, cannot block.
