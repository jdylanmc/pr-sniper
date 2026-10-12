# PR Sniper Review Packet and Host-owned Agent Tools - Supporting Requirements

- Spec ID: SPEC-PR-SNIPER-REVIEW-PACKET
- Source: docs/agent/discovery/pr-sniper-review-packet.md
- Source revision: 6d00613b63d5032f9322cdfb16eb2976dcf9b8235bf52b1c5b4fc7ef3802bba0
- Nano authority: [Product intention and acceptance criteria](./pr-sniper-review-packet.nano.md)

## Authority

The nano sibling is the product authority for this scoped capability under
[#172](https://github.com/jdylanmc/pr-sniper/issues/172). This document supplies
context and elaboration; it cannot override the nano. Requirement and decision
bullets below name the nano criterion they elaborate.

The existing [MVP](./pr-sniper-mvp.nano.md) and
[vNext](./pr-sniper-vnext.nano.md) specifications stay in force except where
the Contradictions section records a proposed supersession. A proposed
supersession takes effect only through a human merge of this pair.

This pair is a candidate until a human merges its specification PR to the
repository's default branch. Discovery alignment, a valid pair and independent
review are not specification approval. A human merge approves these
requirements; it does not implement them, verify GitHub behavior or resolve the
blocking open decision below.

This pair is not ready for implementation: one human-owned decision blocks
readiness (OQ-01). Specification of the rest does not settle it.

This document follows the Specify outline. Mapping: Source and alignment, Purpose
and outcomes (Outcomes and Success), Users and scenarios (Problem and Users),
Functional requirements (Product Requirements), Nonfunctional requirements,
Constraints and agreed decisions (Constraints and Dependencies, Product
Decisions), Non-goals (Scope and Non-goals), Acceptance criteria (nano),
Testing and verification expectations, Assumptions and unresolved decisions
(Assumptions, Contradictions, Open Questions), Traceability.

## Source and alignment

- Discovery artifact: [pr-sniper-review-packet.md](../discovery/pr-sniper-review-packet.md), a verbatim copy of the session foundation for [#172](https://github.com/jdylanmc/pr-sniper/issues/172). Owner: Dylan (human). Later sections supersede earlier ones (its section 21).
- Revision: SHA-256 `6d00613b63d5032f9322cdfb16eb2976dcf9b8235bf52b1c5b4fc7ef3802bba0` of the foundation body (below the copy's header separator). Verified against the copy by recomputing the hash. The copy is a narrative copy, not the schema-2 tool format; it has no tool-verified digests.
- Alignment evidence: the foundation records human confirmation on 2026-10-12 of findings, exits, gates and PR state, "ADR 0021 looks good. aligned." and "171 looks good." for the proposal PR [#171](https://github.com/jdylanmc/pr-sniper/pull/171). The human invoked Specify on 2026-10-12.
- Supporting sources read: [review-packet.md](../../review-packet.md), [ADR 0021](../../adr/0021-review-packet.md) with the proposed-change sections of [ADR 0008](../../adr/0008-one-review-per-head-sha.md) and [ADR 0020](../../adr/0020-constrained-host-owned-review-tools.md), [ADR 0010](../../adr/0010-native-mutation-owner-intent-before-effect.md), [0015](../../adr/0015-local-only-results-and-retention-journal.md), [0016](../../adr/0016-revision-safe-publication.md), [0017](../../adr/0017-opt-in-provider-actions.md), [provider-writes.md](../../provider-writes.md), [review-queue.md](../../review-queue.md), [CONTEXT.md](../../../CONTEXT.md), the MVP and vNext pairs and the VM-regression pair as the pattern.
- Not verified: the foundation's code citations (file and line numbers into `src-tauri/src`) were not re-read here; they remain claims of the foundation.
- Tracked ADR 0021 and `docs/review-packet.md` are the proposal documents. This pair is derived from the foundation and cross-checked with them; where they differ, the foundation governs and the difference is recorded below.

## Problem and Users

Today the agent prompt is one JSON string with limited context (no PR
description, author, CI, labels, full conversation), no statement of permitted
actions and no stored record of what the agent saw. The result is a JSON
decision the host interprets, not a deliberate exit. A tool-output truncation
incident showed completeness cannot be proven without a manifest. Review is keyed
by head SHA, and comments and CI changes follow separate paths. Findings are in
the foundation's section 2.

Users and actors:

| Actor                               | Goal                                                                                                |
| ----------------------------------- | --------------------------------------------------------------------------------------------------- |
| Human owner                         | Keep the last word; trust agent output; audit exactly what an agent saw; see ready PRs in his queue |
| Host (PR Sniper deterministic code) | Detect change, build packets, enforce tools, grants and gates, write to the provider                |
| Agent                               | Review the packet and end its run on one deterministic exit; never act directly                     |
| Provider (GitHub)                   | Source of PR state; target of host writes                                                           |
| Copilot runtime                     | Runs the fresh agent session                                                                        |

Primary scenario: heartbeat sees a changed PR, queues it, an agent takes it,
receives a packet, reads files, and calls one exit; the host writes and consumes
the state. Failure and recovery scenarios are those in the gate table
(RP-014 to RP-023): write failure, stale state, signed-out account, panic,
repeated failure.

## Outcomes and Success

Success is observable behavior in the nano: no work when nothing changed, one
queue entry per PR, one stored deterministic packet per run, a fresh agent with
only host-owned tools, one exit per run, gates applied twice, and own writes
never re-triggering review.

The human's priorities, unranked: review quality (more context), auditability
(store exactly what the agent saw), speed to ship, extensibility, deterministic
outs. Stance: harden the baseline, then expand. No numeric success measure
(token cost, latency, acceptable API cost per tick) was settled. Unknown
measures stay unknown.

## Scope and Non-goals

In scope: the PR state record and heartbeat diff, queue-holds-PR and consume-time
packet, packet content and storage, tool classes and per-assignment tool set,
the five exits and their gating, the gate table, consume semantics, the Reviewed
Clean actions, own-thread resolution and the three settings.

Non-goals are in the nano. Additionally, this pair does not create tickets or a
breakdown, and the Rollout proposal below is not a ticket graph.

## Constraints and Dependencies

Agreed constraints (foundation sections 1, 5, 7, 15, 21):

- The host is the only provider writer. The model requests; the host authorizes and writes. ADR 0010, 0016 and 0017 intent-before-effect, reconcile and revision-safety remain.
- No shell, `az`, `gh` or network for the agent; no yolo mode.
- No silent truncation. Untrusted text is fenced as data.
- The cadence is the debounce: no separate settle window.
- Delivery: balanced push and pull. Route R3 (phased hybrid) is the leading candidate; the foundation says the human's explicit selection of R3 is not yet recorded.

Dependencies from evidence (foundation sections 2 and 6): SDK `github-copilot-sdk 1.0.14` supports custom tools and terminal tools (a successful call ends the turn; a failed call leaves the loop running). Terminal-tool behavior in the real runtime is unproven (OQ-02). The SDK allowlist and hook limit tool exposure; they are not an OS or network sandbox.

## Confirmed Facts

Facts the foundation records as evidence (by source, not re-verified here):

- Current approve gate: open, non-draft, write authority, not the author, no pending review, no unresolved threads (including outdated), not CHANGES_REQUESTED, no duplicate approval, head pinned, primary final review machine sign-off. No CI, mergeability or required-review check for approve. Merge adds merge queue, method, green checks, MERGEABLE and CLEAN, required reviews and confirmed current-head approval.
- Grants (ADR 0017): Publish Comment any assignment; Reply, Approve and Merge primary-only; grants live on repository assignments.
- ADR 0020: three tools, paged at no more than 32,000 bytes, byte coverage of every part of every changed file; the runtime redirects larger output to an unreadable file.
- Revision is a local iteration that increments when the tracked head SHA changes. No stored last-reviewed revision.
- Cost spread across eight real runs: 21,119 to 2,966,832 input tokens; cause of the outlier unverified.

### Human decisions (confirmed; foundation section in brackets)

- Chron queues work; the queue runs N agents; each agent is fresh with one packet [1]. No shell, no yolo; tools are PR Sniper hooks [1].
- Two tool classes: investigation and permission-based [1]. Balanced push and pull [7].
- Five exits, one per run, with their arguments; approve and merge leave an approval comment; `ready_for_human` leaves no comment [8, 9]. `leave_comments` has `new_comments` and `reply_comments` [10].
- Tool set is repository configuration per assignment [10].
- Own-thread resolution only; others' threads cannot be closed [11]. Gates twice and revision stamps at exit [11]. Write failure: toss, error count, re-queue; three failures park [11, 19].
- Scenario decisions [12, 13, 17, 18, 19]: CI red, merge conflicts, CI pending, required reviews unmet, changes requested by others, others' unresolved threads, draft setting, Include self option, approval exists, bad comment line, signed out.
- PR state model: no change, nothing queued; a successful exit consumes the state; post-exit baseline; cadence is the debounce; merged and closed mean done; CI is an array; persist; Clean per PR and Clean all; review decision stays in state [14, 15, 16].
- Queue holds the PR; packet built at consume time from the latest state [21]. Panic: log, Needs attention, auto-retry on the existing cooldown [9].

## Source Claims

Claims and agent-derived proposals the foundation marks as unconfirmed. They are
recorded, not agreed:

- Exits grant-gated with a missing grant meaning not offered; `ready_for_human` and `panic_exit` need no grant; a denied terminal call returns an error and the loop continues (section 8, "agent-proposed, not yet confirmed"). The tracked proposal documents state grant gating and not-offered; the human reviewed them as "looks good", which this pair treats as acceptance of grant gating and not-offered but not of the denied-call behavior.
- A newer version supersedes older queued or running work; reply and review stop being separate run kinds (section 10, agent-derived). Partly settled by section 21 (queue holds the PR) and unspecified for runs of kinds reply and final (OQ-01).
- Re-approve must be idempotent (agent-derived in 13, confirmed in 19). Merge not offered while CI pending (13 agent-derived; confirmed by the human's CI-pending answer: may not merge until green).
- Bucket taxonomy A-E (agent-proposed, not objected to).
- Closed unmerged treated like merged: assumed in section 15, confirmed in 16.
- Per-tick state reads cost API calls for every tracked PR (agent note).

## Assumptions

Agent-derived by this specification, not agreed:

- A-01: When Review drafts is off, draft PRs are not reviewed. The foundation says only that it is a setting.
- A-02: Include self defaults to off (the current behavior); the foundation does not state a default.
- A-03: Re-queue after tossing re-enters the existing cooldown or retry budget; the foundation says only "re-queue".
- A-04: "Park after 3" counts consecutive failures per PR and state, and a consumed state resets the count. The foundation does not define the scope of the count.
- A-05: The heartbeat interval is configurable only as the existing polling cadence; the human said every five minutes.
- A-06: Settings placement is in the existing assignment, repository and Approve & Merge controls; no UI design is agreed.

## Contradictions

Evidence conflicts kept visible. Each needs a human decision; none is silently replaced.

| #    | Existing text                                                                                                                                                                                                            | Foundation direction                                                                                                                                                                                           | Status                                                                                                                                                                                                                                      |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C-01 | vNext: "self-approval remains forbidden"; [provider-writes](../../provider-writes.md) and [CONTEXT.md](../../../CONTEXT.md): "No self-approve on GitHub"; approve gate blocks when the acting account is the author      | Human wants an Include self option on Approve & Merge (section 18); reports having approved his own PRs from the same account                                                                                  | Proposed supersession of that vNext sentence. Needs human merge. GitHub acceptance of self-approval is UNVERIFIED and needs a probe on a disposable PR; the docs' "No self-approve" is itself an unchecked claim against the human's report |
| C-02 | ADR 0008: one review per head SHA                                                                                                                                                                                        | One successful exit per PR state                                                                                                                                                                               | Proposed ADR 0008 amendment (already in its proposed-change section)                                                                                                                                                                        |
| C-03 | ADR 0020: exactly three host-owned tools; allowlist must equal catalog                                                                                                                                                   | Per-assignment tool set plus grant-gated exits                                                                                                                                                                 | Proposed amendment; allowlist drift still blocks                                                                                                                                                                                            |
| C-04 | Approve gate blocks on any unresolved thread (including outdated) and on CHANGES_REQUESTED                                                                                                                               | Approve allowed despite others' unresolved threads and others' changes requested                                                                                                                               | Human decision; gate change. Merge gates unchanged (they block on unresolved threads and need required reviews and green CI)                                                                                                                |
| C-05 | [provider-writes](../../provider-writes.md) and [CONTEXT.md](../../../CONTEXT.md): agents never resolve or reopen threads; resolution is provider closure only (Concern reassessment "never resolves the GitHub thread") | Agents may resolve their own threads only                                                                                                                                                                      | Human decision; supersedes those lines for owned threads. Others' threads unchanged                                                                                                                                                         |
| C-06 | vNext AC-009 and ADR 0017: primary-only Reply, Approve and Merge; distinct primary final review before approve or merge; multiple assignments compose                                                                    | Foundation defines one exit per run and one successful exit per PR state, and does not say how multiple assignments and the primary final review compose, or whether Approve and Merge exits stay primary-only | UNRESOLVED, human-owned, BLOCKS readiness (OQ-01)                                                                                                                                                                                           |
| C-07 | ADR 0010: "Approve/merge never exposed as Agent tools"                                                                                                                                                                   | Approve and merge are exits                                                                                                                                                                                    | Agent-observed tension. Foundation treats exits as requests that the host validates; the host still writes. Needs human confirmation that this satisfies ADR 0010's intent, together with OQ-01                                             |
| C-08 | vNext AC-010: draft PRs cannot be approved or merged                                                                                                                                                                     | Review drafts setting; draft plus approve when the setting is on is unresolved                                                                                                                                 | Open (OQ-07)                                                                                                                                                                                                                                |
| C-09 | Foundation section 15 and 11/17: queue holds changed states; pre-run discard                                                                                                                                             | Section 21: queue holds the PR; packet at consume time                                                                                                                                                         | Superseded by the later section; this pair follows section 21                                                                                                                                                                               |

The foundation's section 4 run kinds (review, reply, final) and "version" wording
are superseded by PR state (section 21); the older wording is not carried here.

## Alternatives and Examples

Routes from the foundation's Scout comparison (section 5), for context, not
selection: R1 richer JSON prompt, R2 packet plus terminal action tools, R3
phased hybrid (leading candidate), R4 non-terminal writing tools (in tension
with ADR 0010 and 0016), R0 do nothing (ruled out by owner intent). The human
has not recorded an explicit R3 selection. Push-all versus pull-via-manifest is
settled as balanced (section 7).

Example: CI turns red on a PR the agent earlier left ready. The heartbeat sees a
changed CI array, queues the PR, the agent gets a packet where approve is not
offered with the reason, reads the failing check and calls `leave_comments`
saying CI is broken with a fix.

## Product Requirements

- RP-001 [AC-001]: Each cycle compares each open PR's normalized current state to its recorded baseline. Equal state means no entry, packet or run. Normalization removes timestamps and per-run IDs. The CI array is part of state; the approach to normalizing it so flapping runs do not look like a new state is open (OQ-05).
- RP-002 [AC-002]: A PR changing again while queued does not add an entry or carry old details.
- RP-003 [AC-003]: The packet is built at consume time from the latest state, with its `state_id` recorded. The packet is not stored in the queue.
- RP-004 [AC-004, AC-005]: Packet sections, order and content as in the nano. The manifest and the pull tools make details retrievable. Where the PR description lives in the stored pull request is unverified (OQ-10).
- RP-005 [AC-006]: Coverage is bytes served; a gap rejects the review. A response that cannot fit the limit is a failure of that run, never a model decision, as under ADR 0020.
- RP-006 [AC-007]: Store the packet minus file content with a hash, and a per-call audit record. The view over this record is later phase work.
- RP-007 [AC-008]: Rules are fixed text versioned with the schema; untrusted data is fenced.
- RP-008 [AC-009, AC-010]: Investigation tools are read-only, bounded and paged; the agent has no shell or network. `rubber_duck` uses a second model session with a budget; its model, budget, payer and concurrency are open (OQ-06).
- RP-009 [AC-011]: Tool set is per assignment. Allowlist drift still blocks a run.
- RP-010 [AC-012]: Exactly one exit per run. A run that ends without an exit is not a successful exit and does not consume the state; what happens next is not settled here beyond the existing failure and retry behavior (proposal).
- RP-011 [AC-013]: Exits are gated by the assignment's grants and by gates at build. Which exact exit needs which grant follows the tracked proposal. Whether a host-denied terminal call returns an error and lets the run continue is a runtime behavior to prove (proposal, OQ-02).
- RP-012 [AC-014]: Gates at packet build and at exit; revision stamp on every comment the exit writes. The stamp design is open (OQ-11).
- RP-013 [AC-015, AC-016, AC-017]: The gate table: CI red and conflicts block approve and merge and the agent comments; CI pending and unmet required reviews offer approve but not merge; changes requested by others and others' threads allow approve; merge gates unchanged.
- RP-014 [AC-018]: Review drafts is a per-repository setting.
- RP-015 [AC-019]: Include self is an option on Approve & Merge. It cannot be relied on until the GitHub probe answers.
- RP-016 [AC-020]: Existing approval of the current head makes re-approve an idempotent success.
- RP-017 [AC-021]: A state that moves during a run causes toss and re-queue with no write.
- RP-018 [AC-022]: A failed write tosses the whole batch, increments the error count and re-queues; three failures park.
- RP-019 [AC-023]: Signed out parks as Credentials in Needs attention with Fix and auto-resumes.
- RP-020 [AC-024]: `panic_exit` logs, parks in Needs attention and retries on the cooldown; it does not consume.
- RP-021 [AC-025]: Only a successful exit consumes; the post-exit state is the baseline, absorbing own writes.
- RP-022 [AC-026]: Merged and closed unmerged move to Reviewed immediately.
- RP-023 [AC-027]: Agents resolve only their own threads.
- RP-024 [AC-028]: PR state is persisted. Reviewed has Clean per PR and Clean all. Whether Clean deletes only persisted PR state or also retention receipts, and its relation to ADR 0015's provider-confirmed cleanup rule, is open (OQ-09).
- RP-025 [AC-029]: Settings for assignment tools, Review drafts and Include self.
- RP-026 [AC-030]: Preserve ADR 0010, 0016 and 0017 intent-before-effect and revision-safety, no silent truncation, no credentials in the packet and fenced untrusted content.

## Nonfunctional Requirements

- Auditability (agreed priority): the stored packet and hash plus per-call records allow reconstructing what an agent saw, without secrets or file content duplicated.
- Security: no shell, `az`, `gh` or network for the agent; the tool allowlist and hook are not an OS sandbox, which this specification does not claim.
- Reliability: persisted state survives restart; failures and panics never consume a state; no partial batch remains on the provider.
- Extensibility (agreed priority): tools are registry-driven; adding a tool adds a registry entry. Expansion beyond the baseline is later.
- Idempotency: re-approve is an idempotent success; uncertain writes never authorize a duplicate create (ADR 0010).
- Cost and performance: no target settled. Token cost, per-tick API cost for every open PR and provider concurrency are unmeasured (OQ-08, OQ-06). Accessibility, privacy beyond the no-secrets rule and latency: no requirement was agreed; none invented.

## Product Decisions

- PD-001 [AC-001, AC-002, AC-003]: PR state is the unit of work; the queue holds the PR; the packet is built at consume time (sections 14, 21).
- PD-002 [AC-005]: Balanced push and pull (section 7).
- PD-003 [AC-009, AC-011]: Two tool classes; per-assignment tool set (sections 1, 10).
- PD-004 [AC-012]: Five exits, one per run; `leave_comments` has two arrays (sections 8-10).
- PD-005 [AC-014]: Gates twice, comments stamped with revision (section 11).
- PD-006 [AC-015 to AC-023]: Scenario outcomes as decided (sections 9, 12, 13, 17, 18, 19).
- PD-007 [AC-025]: A successful exit consumes; post-exit baseline (sections 14, 15).
- PD-008 [AC-026, AC-028]: Merged and closed are done; persist; Clean per PR and all (sections 15, 16).
- PD-009 [AC-027]: Own-thread resolution only (section 11).
- PD-010 [AC-029]: Settings: assignment tools, Review drafts, Include self (section 18).

Proposed, not decided: the ADR 0008, ADR 0020 and ADR 0021 amendments. They stay "proposed" until a human merges #171 and this pair.

## Testing and Verification Expectations

Agreed by the human (relayed by the specification request). Prefer existing
high-level behavioral seams; no new seam is proposed.

- Rust integration tests on existing seams in `src-tauri/tests/*.rs`: heartbeat state diff and "no change, no work"; consume semantics; the gate table, table-driven; idempotent approve; toss, error count, re-queue and park at three.
- The fake runtime fixture `src-tauri/tests/support/review-runtime.mjs` is extended to call exits and to receive denials.
- Playwright settings tests in `tests/settings/*.spec.mjs`: assignment tool set, Review drafts, Include self, Reviewed Clean per PR and Clean all, panic in Needs attention.
- Manual or proof-of-concept evidence, not unit tests: terminal tools in the real Copilot runtime, GitHub self-approval, token cost.

Limitations: fixtures and the fake runtime do not prove real-runtime terminal-tool behavior, GitHub acceptance of self-approval or token cost. Live mutation acceptance needs an explicitly authorized disposable PR (provider-writes). The existing complementary checks remain obligations. The user-interface packet view has no test expectation because it is a later phase.

## Traceability

| Nano criterion | Source basis   | Supporting requirement | Agreed verification                            |
| -------------- | -------------- | ---------------------- | ---------------------------------------------- |
| AC-001         | 14, 15, 17, 18 | RP-001                 | Rust: heartbeat diff                           |
| AC-002         | 21             | RP-002                 | Rust: queue entry per PR                       |
| AC-003         | 21             | RP-003                 | Rust: consume semantics                        |
| AC-004         | 1, 7           | RP-004                 | Rust: packet content (proposal; no named test) |
| AC-005         | 7              | RP-004                 | Rust (proposal)                                |
| AC-006         | 2, ADR 0020    | RP-005                 | Rust (proposal; existing coverage tests)       |
| AC-007         | 1; 4           | RP-006                 | Rust (proposal)                                |
| AC-008         | 1, 4           | RP-007                 | Rust (proposal)                                |
| AC-009, AC-010 | 1, 2           | RP-008                 | Fake runtime; manual POC                       |
| AC-011         | 10             | RP-009                 | Playwright: assignment tool set                |
| AC-012         | 8, 9, 10       | RP-010                 | Fake runtime exits; manual POC                 |
| AC-013         | 8 (proposed)   | RP-011                 | Fake runtime denials; Rust gate table          |
| AC-014         | 11             | RP-012                 | Rust gate table                                |
| AC-015         | 9, 12          | RP-013                 | Rust gate table                                |
| AC-016         | 13, 17         | RP-013                 | Rust gate table                                |
| AC-017         | 12, 17         | RP-013                 | Rust gate table                                |
| AC-018         | 18             | RP-014                 | Playwright: Review drafts                      |
| AC-019         | 18             | RP-015                 | Playwright: Include self; manual probe         |
| AC-020         | 19             | RP-016                 | Rust: idempotent approve                       |
| AC-021         | 9, 21          | RP-017                 | Rust: toss and re-queue                        |
| AC-022         | 11, 19         | RP-018                 | Rust: toss, count, park at 3                   |
| AC-023         | 19             | RP-019                 | Rust (proposal; existing behavior)             |
| AC-024         | 9              | RP-020                 | Playwright: panic in Needs attention           |
| AC-025         | 14, 15         | RP-021                 | Rust: consume semantics                        |
| AC-026         | 15, 16         | RP-022                 | Rust (proposal)                                |
| AC-027         | 11             | RP-023                 | Rust (proposal)                                |
| AC-028         | 15, 16         | RP-024                 | Playwright: Clean per PR and all               |
| AC-029         | 18             | RP-025                 | Playwright                                     |
| AC-030         | 5, 7, 21       | RP-026                 | Existing complementary checks                  |

Coverage gaps: the human did not name tests for packet content, coverage tests,
resolve-own-threads or merged/closed handling; those rows are marked
(proposal) and need human confirmation. Source conflicts: C-01 to C-09.

## Rollout (proposal)

Agent-proposed from [review-packet.md](../../review-packet.md); not approved,
not a ticket graph. Early phases change content, not behavior.

1. State record, heartbeat diff, consume-time packet, store packet; current fields.
2. Packet content: description, author, CI array, conversation, manifest. Blocked on the generated/lockfile policy (OQ-04).
3. Exits replace the JSON `decision`. Blocked on OQ-01 and the terminal-tool POC (OQ-02).
4. Own-thread resolution and per-comment revision stamps.
5. Per-assignment tool set, packet view, Reviewed Clean.

## Open Questions

Blocking:

| ID    | Owner / later work       | Question                                                                                                                                                                                                                                                                                             | Impact                                                 |
| ----- | ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| OQ-01 | Human, back to Discovery | How do multiple assignments' runs and the primary final review compose under one exit per run and one successful exit per PR state? Do Approve and Merge exits stay primary-only (ADR 0017, vNext AC-005/AC-008/AC-009)? Also confirm exits satisfy ADR 0010's "never exposed as Agent tools" (C-07) | Blocks readiness of the whole pair and the exits phase |
| OQ-02 | POC owner                | Terminal tools in the real runtime: does a successful call end the turn, payload shape, does a failed call loop back                                                                                                                                                                                 | Blocks the exits phase (phase 3)                       |
| OQ-04 | Human and evidence       | Generated, lockfile and vendored file policy                                                                                                                                                                                                                                                         | Blocks the manifest phase (phase 2)                    |

Non-blocking unless stated:

| ID    | Owner / later work       | Question                                                                                                               | Impact                                                 |
| ----- | ------------------------ | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| OQ-03 | Probe on a disposable PR | Does GitHub accept an author approving their own PR?                                                                   | Decides whether Include self has effect; verifies C-01 |
| OQ-05 | Design                   | CI-array normalization so flapping runs are not new states                                                             | Re-review noise and cost                               |
| OQ-06 | Human and POC            | `rubber_duck` model, budget, payer and provider concurrency                                                            | Tool availability and cost                             |
| OQ-07 | Human                    | Draft plus approve when Review drafts is on                                                                            | Gate behavior for drafts                               |
| OQ-08 | Measurement              | API cost of reading every open PR each tick; batching, conditional requests; token cost of the push packet versus pull | Cost; cadence                                          |
| OQ-09 | Human                    | Reviewed Clean scope versus ADR 0015 (per PR and all; what is deleted)                                                 | Retention and evidence loss                            |
| OQ-10 | Read                     | Where the PR description lives in `PullRequest`                                                                        | Packet phase 2                                         |
| OQ-11 | Design                   | Per-comment targeted-revision stamping design                                                                          | Phase 4                                                |
| OQ-12 | Human                    | Does `ready_for_human` store a local, unposted summary?                                                                | Auditability; the exit leaves no provider comment      |
| OQ-13 | Human                    | Explicit selection of route R3; assumptions A-01 to A-06                                                               | Rollout shape                                          |
