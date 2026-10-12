# Hand each agent one review packet and let it end on one exit

Status: proposed. Discovery: [#172](https://github.com/jdylanmc/pr-sniper/issues/172). Spec: [Review packet](../review-packet.md).

## Context

Today the agent prompt is one JSON string built in `src-tauri/src/review/runtime.rs` (`prompt()`).

- No description, author, CI, labels, commits or full conversation.
- No statement of which actions are permitted.
- Result is a JSON `decision` the host interprets, not a deliberate exit.
- Nothing stores what the agent saw.
- A tool-output truncation incident showed completeness cannot be proven without a manifest.
- Review is keyed by head SHA ([ADR 0008](0008-one-review-per-head-sha.md)). Comments and CI changes are handled by separate paths.

## Decision

- **Unit of work = PR state.** A heartbeat diffs each open PR's normalized state against the recorded one. Changed = the PR is queued for review. Unchanged = nothing. The queue holds the PR, one entry per PR, never a snapshot.
- **Packet built at consume time**, from the PR's latest state, when an agent is free. No stale review details wait in the queue. The packet records its `state_id`.
- One **review packet** per run: versioned, labeled, deterministic. Balanced push and pull: top-level facts in the packet, details through tools.
- The agent has **no shell, `az`, `gh` or network**. Its only hands are PR Sniper tools, thin hooks into deterministic host code.
- **Investigation tools** are read-only, bounded, paged. **Tool set is repository configuration**, per assignment.
- A run ends on **one exit**: `leave_comments` (`new_comments`, `reply_comments`, own-thread resolutions), `ready_for_human`, `approve_pull_request`, `approve_and_merge`, `panic_exit`. Exits are grant-gated.
- Gates run at packet build (exit not offered) and at exit (re-check). Host saves intent before effect and stamps comments with their revision.
- A **successful exit consumes the state**. Post-exit state is the baseline. Failures and panics do not consume.
- Packet stored with the run, minus file content, with `packet_hash`.

## Consequences

- Reviews are reproducible and auditable. A state is reviewed once. No re-review on identical state.
- Revisions, comments, edits and CI changes use one mechanism.
- Larger packet per run. Reading every open PR each tick costs API calls.
- Replaces the JSON `decision` with exits. Needs a `schema` version and a migration for stored results.
- Agents may resolve their own threads, never others'. Reverses the earlier "agent never resolves" rule.
- Approve gate relaxes: others' unresolved threads and changes requested no longer block approve.
- Extends [ADR 0020](0020-constrained-host-owned-review-tools.md) and [ADR 0008](0008-one-review-per-head-sha.md). Keeps [ADR 0010](0010-native-mutation-owner-intent-before-effect.md), [0016](0016-revision-safe-publication.md), [0017](0017-opt-in-provider-actions.md): the model requests, the host writes.
- Delivered in phases; early phases change content, not behavior.
