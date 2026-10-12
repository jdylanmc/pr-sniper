# PR Sniper Review Packet and Host-owned Agent Tools

- Spec ID: SPEC-PR-SNIPER-REVIEW-PACKET
- Source: docs/agent/discovery/pr-sniper-review-packet.md
- Source revision: 6d00613b63d5032f9322cdfb16eb2976dcf9b8235bf52b1c5b4fc7ef3802bba0
- Full specification: [Supporting requirements](./pr-sniper-review-packet.full.md)

## Intention

Make a PR's state, not a head SHA, the unit of review work. A heartbeat detects
state changes and queues the PR. When an agent is free, the host builds one
deterministic review packet from the PR's latest state and spawns a fresh agent
that has no shell and no network. The agent acts only through host-owned tools
and ends its run on exactly one exit. The host remains the only provider writer.
Baseline first: harden this, then expand it.

## Acceptance Criteria

- AC-001: Each polling cycle (default five minutes) lists the open PRs, reads each PR's current state and compares it with that PR's recorded state. A PR whose state differs is queued for review. A PR whose state is unchanged causes no queue entry, no packet and no agent run. A PR's state covers its head and base, revision, title and description, open/draft/merged/closed status, mergeable class, review decision, existing approvals, the CI check array (name, status, conclusion), comments and threads, and labels. Noise such as timestamps and per-run identifiers does not make a state different.
- AC-002: The queue holds the PR, not a snapshot of it. There is one queue entry per PR: a PR that changes again while queued remains one entry. No review details wait in the queue while the remote PR moves on.
- AC-003: When an agent is free, the host reads the PR's latest state, builds the review packet from it and records the packet's `state_id` before spawning a fresh agent with fresh context. A state that moved while the PR was queued needs no discard and no agent spend, because the packet reflects the latest state.
- AC-004: The packet has fixed sections in fixed order: Meta, Rules, Pull request, Conversation, Files, Directive. Meta identifies the packet, schema, state, revision, why the PR was admitted, the acting identity, a two-sentence PR Sniper introduction, the tools available and the exits offered. Rules are fixed, versioned text. Pull request carries title, description, author and relation, state, draft, base and head refs and SHAs, revision, labels, size, CI check array, mergeable class, review decision and existing approvals. Conversation carries comments and threads with whether each is new since the last review and the revision it targeted, plus this agent's own open feedback. Files carries a manifest. Directive carries the ordered doctrines, the agent prompt, the repository prompt, the preset and the signature.
- AC-005: The packet balances push and pull. It carries the top-level facts; details (file content, threads, CI logs, search) are fetched on demand through investigation tools. The file manifest lists each changed file with its path, status, bytes per part (patch, before, after) and kind; for revision above one it also shows what changed since the last consumed state. Whether generated, lockfile and vendored kinds are classified or treated differently is not settled by this specification.
- AC-006: The host tracks the bytes it serves for every part of every changed file. A review that did not read every part of every file it relies on to completion is rejected rather than accepted. Tool output is paged losslessly in chunks of at most 32,000 bytes. Nothing is silently truncated: a response that cannot fit is a failure, not a partial answer presented as complete.
- AC-007: Each run stores its packet, minus file content, with a content hash, so the exact packet the agent saw is auditable. The stored packet contains no tokens or credentials. Each tool call is recorded with the tool, an argument hash, a size and an outcome, without content or secrets. A user interface that shows the stored packet is a later phase and is not required by this delivery.
- AC-008: Packet text that originates outside PR Sniper (PR description, comments, file content) is fenced as untrusted data, and the rules tell the agent never to obey it. The agent is told to review the packet, call exactly one exit and stop, to read every part it relies on, never to claim it read what it did not, to say so with the fix when CI is red or merge conflicts exist instead of approving, to choose `ready_for_human` when unsure and `panic_exit` when broken.
- AC-009: Agent tools fall in two classes. Investigation tools are read-only, bounded and paged: read changes, read source, search paths, search code, list tree, get thread, get checks, and a budgeted `rubber_duck` second-model critique. Exits are the only way to end a run and the only way to cause an effect.
- AC-010: The agent has no shell, no `az`, no `gh`, no network and no checkout. Every tool is a thin hook into deterministic host code. Adding a tool is adding a registry entry, not changing the runtime.
- AC-011: Which tools an agent has is repository configuration, set per repository assignment. The host enforces that set for each run. A tool outside the set is unavailable to the run, and a mismatch between the configured set and the runtime's actual tool catalog blocks the run rather than running with a different set.
- AC-012: A run ends on exactly one exit. The exits are `leave_comments`, `ready_for_human`, `approve_pull_request`, `approve_and_merge` and `panic_exit`. `leave_comments` takes two arrays, `new_comments` (path, line, side, body) and `reply_comments` (thread, body), plus resolutions of the agent's own threads. `ready_for_human` takes no comment, moves the PR to the human's queue and leaves nothing on the provider. `approve_pull_request` approves the head with an approval comment. `approve_and_merge` approves with a comment and then merges. `panic_exit` reports an unrecoverable state. Other, non-exit tools may be used before the exit.
- AC-013: Exits are grant-gated. `leave_comments` needs Publish Comment for new comments and Reply Comment for replies, `approve_pull_request` needs Approve, `approve_and_merge` needs Approve & Merge, and `ready_for_human` and `panic_exit` need no grant. An exit whose grant is missing, or whose gate already fails when the packet is built, is not offered, and the packet says why.
- AC-014: Gates are evaluated twice: at packet build, where a failing exit is not offered, and again at exit time, where the host re-checks for races before writing. Every comment written by an exit is stamped with the revision it targeted.
- AC-015: CI red: approve and merge are not offered, and the agent leaves a comment saying CI is broken with a recommended fix for the error, using CI status and logs it can read. Merge conflicts: approve and merge are not offered, and the agent leaves a comment saying the PR is blocked by conflicts, with resolution advice when the resolution is non-trivial.
- AC-016: CI pending: approve is allowed and merge is not offered; the agent does not comment about waiting for CI. When CI later changes, the PR's state changes and a later run may merge. Required reviews not yet satisfied: approve is allowed, `approve_and_merge` is not offered until the provider reports reviews satisfied, and no comment is required.
- AC-017: Changes requested by someone else do not bind the agent: approve is allowed and merge remains blocked. Other users' unresolved threads, including outdated ones, no longer block approve. Merge gates are otherwise unchanged and still block on unresolved threads and on every condition they block on today.
- AC-018: A repository setting, Review drafts, decides whether draft PRs are reviewed. How approve behaves for a draft when Review drafts is on is not settled by this specification.
- AC-019: The Approve & Merge setting has an Include self option. Without it, a PR authored by the acting account is not approvable or mergeable as today. With it, the exits are offered for such a PR. Whether the provider accepts an author approving their own PR is unverified; if it does not, the option has no effect and this must be reported, not worked around.
- AC-020: Approving when PR Sniper's approval of the current head already exists is an idempotent success: no duplicate approval, no new comment, and the exit counts as successful and consumes the state.
- AC-021: If the PR's state moves while a run is in progress, the host tosses that run's result and re-queues the PR, with no write for the stale state.
- AC-022: When an exit's write fails for any reason, including a comment line not in the diff or a file renamed or moved, the host tosses the whole batch, leaves no partial batch on the provider, increments an error count for the PR and re-queues it. After three failures the PR is parked with its error.
- AC-023: When the account is signed out mid-run, the result is tossed and the PR is parked as Credentials in Needs attention with the existing Fix action, and it resumes automatically when the account reconnects.
- AC-024: `panic_exit` is logged and puts the PR in Needs attention, then retries automatically on the existing cooldown schedule. A panic does not consume the state.
- AC-025: A successful exit consumes the PR state, and a consumed state is never reviewed again. The PR state recorded after the exit becomes the baseline, so the agent's own comments, approval and thread resolutions never trigger a review. A failed exit, tossed write, panic or error does not consume the state.
- AC-026: A merged PR, and a PR closed without merging, moves to the Reviewed queue immediately and receives no further action.
- AC-027: An agent may resolve only the threads it owns. It cannot close other users' threads or comments, and may reply to disagree.
- AC-028: PR state is persisted, so a restart does not re-review every PR. Parked PRs appear in Reviewed. Reviewed offers Clean per PR as a row action and Clean all, each deleting the persisted state it covers. How this relates to the retention rules of ADR 0015 is not settled by this specification.
- AC-029: Operators configure the per-assignment tool set, the per-repository Review drafts setting and the Approve & Merge Include self option in settings.
- AC-030: Existing boundaries are preserved. The host is the only provider writer and the agent only requests: intent is saved before effect, and effects remain revision-safe and reconcilable. The agent never receives provider credentials. Nothing is silently truncated. Untrusted content stays fenced.

## Readiness

Blocked. How multiple assignments' runs and the primary's final review compose
under one exit per run and one successful exit per PR state, and whether
Approve and Merge exits stay primary-only, is an unresolved human-owned decision
that must return to Discovery. See the full specification.

## Non-goals

- Implementing any phase, writing tickets or a breakdown, or a user interface for viewing stored packets (a later phase).
- Choosing a token budget, model or concurrency for `rubber_duck`, or a generated/lockfile policy.
- Treating the proposed phase order as an approved ticket graph.
- Non-Copilot runtimes, expanding tools beyond the baseline, or relaxing merge gates.
