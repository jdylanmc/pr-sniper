# Discovery Foundation

- Subject: https://github.com/jdylanmc/pr-sniper/issues/172
- Slug: pr-sniper-review-packet
- Alignment: confirmed by the human, 2026-10-12 (findings, exits, gates, PR state; #171 reviewed "looks good"); composition decision 2026-10-12 19:00 (section 22) and teammate reports decision 2026-10-12 19:10 (section 23) and report format, heartbeat, consume, delivery, sandbox decisions 2026-10-12 19:40 (section 24)
- Format: narrative copy of the session discovery foundation, not the schema-2 tool format; no tool-verified digests
- Source revision: 757d17c13aa8e919fa0d6921cc5932b4d47951f8d2731df98f42e73c14fcc36c (SHA-256 of the session artifact `foundation-new.md`, which is the body below)

The body is verbatim. Later sections supersede earlier ones where they conflict (see sections 21-24). The human's 2026-10-12 ~19:20 teammate-failure correction (a teammate failure blocks the primary; failure taxonomy) came after this copy and is recorded in the specification, not here.

---

# Discovery foundation: review packet and host-owned agent tools

Issue: https://github.com/jdylanmc/pr-sniper/issues/172
Proposal PR (docs only): https://github.com/jdylanmc/pr-sniper/pull/171 (`docs/review-packet.md`, `docs/adr/0021-review-packet.md`, status: proposed)
Date: 2026-10-12. Lifetime: this session's workspace folder (survives checkpoints, not a repo artifact).

## 1. Question and boundaries

**Question.** How should every agent run get one review packet and act only through host-owned tools?

**Desired learning.** What the current pipeline, SDK and GitHub data allow; which design routes are viable; what to probe next.

**Out of scope.** Implementation, tickets/breakdown, UI for viewing packets, spec writing.

**Owner statements (human, 2026-10-12).**
- Chron finds work and queues it. The queue runs N agents at a time. Each agent spawns fresh with fresh context and gets one review packet.
- Packet = Meta (2-sentence PR Sniper intro, tools, caveman instructions to review the packet and pick the right tool), Pull request (title, description, author, state/status, revision #, file list, other metadata), Directive (doctrines, prompt).
- The agent gets no `az`/shell and no yolo mode. It acts only through PR Sniper methods: thin hooks into deterministic host code.
- Two tool classes: investigation (rubber-duck own evaluation, get file contents, search tokens, search other code in the repo) and permission-based (approve a PR, start a review and comment).
- Ready for you = agents reviewed the latest revision, have nothing meaningful to add, lack Approve permission. Approved PRs leave the queue.
- Alignment 2026-10-12: findings confirmed. "Get this baseline hardened up and we'll expand it." Priorities: review quality (more context), auditability (store exactly what the agent saw), speed to ship, extensibility. Wants more deterministic "outs": ways for the agent to end its turn. Next: save this, then hone tools/capabilities.

## 2. Findings (cited; facts only unless marked)

Evidence packets: three read-only explore runs (pipeline, SDK, GitHub/gates). Paths relative to `src-tauri/src` unless noted.

### Current pipeline (fact)
- Run kinds: normal full review (`FullReview`), follow-up reply/mention (`ReplyTask`, `follow_up/task.rs:56-87`), primary final review (`actions/host.rs:155-164`). `review/runtime.rs:46-66` Task trait.
- Prompt = one JSON string sent as the first user message (`review/runtime.rs:703-741, 950`). System message is `replace` mode and forbids commands, tests, hooks, publication (`:686-689`).
- Prompt fields: head, base, target sha, title, review lens (agent prompt, repo prompt, preset, doctrine), changed_files (path, status, previous_path, parts), result_schema, coverage note (`:733-743`). Prior feedback is added for full reviews.
- Selection (agent, policy, doctrine, preset) is frozen per run; lens hash = 8 bytes of SHA-256 over agent, doctrine, preset, policy prompt, adapter (`review.rs:79-91`).
- Output contract: JSON `synopsis`, `files`, `findings`, `decision` (`machine_sign_off` | `human_input_required`); validated host-side (`review::validate_output`). No SDK structured-output enforcement; `session.idle` marks end (`:963-969`).
- Host gates: byte-coverage of every part of every changed file (ADR 0020); evidence validation.

### Authority today (fact)
- Comment publication, reply publication, approve and merge are host-decided. Model gives a result; host checks grants/freshness, saves intent, writes, reconciles (`publication.rs:278-283`; `actions.rs:184-231, 593-612`; `actions/host.rs:486-605`).
- Final-review prompt: model must never approve/merge; host checks policy separately (`actions/host.rs:155-164`).
- Grants (ADR 0017): Publish Comment any assignment; Reply, Approve, & Merge primary-only; grants live on repository assignments.
- Approve gate (`github/actions.rs:83-130`, `actions.rs:534-623`): open, non-draft, write authority, not author, no pending review, no unresolved threads (incl. outdated), not CHANGES_REQUESTED, no duplicate approval, head pinned (`approve_exact`), primary final review MachineSignOff with no findings/held findings/conflict. **No CI, mergeability or required-review check for Approve.**
- Merge adds: no merge queue, merge method, `checks == SUCCESS`, MERGEABLE + CLEAN, required reviews, confirmed current-head approval (`github/actions.rs:133-171`).
- Correction: #269's "not a complete clearance when findings are unmapped" text was conditional boilerplate with "0 finding(s) could not be mapped". The approval matched the code. The Approve-without-CI question stands.

### SDK `github-copilot-sdk 1.0.14` (fact unless marked)
- Custom tools: `Tool::new` + `ToolHandler::call` (async), JSON-schema params (`tool.rs:183-229`). Current runtime registers 3 read tools via `available_tools` allowlist (`review/runtime.rs:672-679`) plus a `PreToolUse` hook denying others.
- **Terminal tools exist:** `Tool.is_terminal`; a successful call ends the turn, a failed call leaves the loop running (`types.rs:350-365, 488-505`).
- Per-tool `instructions` field exists (`types.rs:400-410`).
- No `response_format`/structured output use found. Context controls: `reasoning_effort`, `context_tier`; model verified after session creation (`review/runtime.rs:1108-1140`).
- Output over ~50 KB is redirected by the runtime to an unreadable file; the host pages at <=32,000 bytes (`review/runtime.rs:35-43`; ADR 0020).
- Second session/model: SDK supports per-session config; host already lists models. Nothing in code limits it; provider concurrency unknown.
- Usage events (`assistant.usage`) are recorded as category only; no token accumulation or budget enforcement (`review/diagnostics.rs:103-110`). Watchdog and abort exist.
- The allowlist and hook enforce tool exposure. This is not an OS or network sandbox.

### GitHub data (fact)
- Already fetched: state, draft, author, base/head refs+SHAs, mergeable, mergeStateStatus, reviewDecision, branch protection (required reviews), CI via `statusCheckRollup` (100 contexts, fail closed), reviews, top-level comments (1000 cap), review threads with isResolved/isOutdated (`github/actions.rs:229-321`, `github/conversation.rs:95-119`, `github/threads.rs:293-317`), files (REST, 3000 cap, `github/review.rs:109-132`), base/head trees (recursive, fail if truncated).
- Not found: PR body/description exposure in the prompt context (REST detail is fetched; whether `PullRequest` stores `body` unverified), labels, linked issues, CODEOWNERS, full commit list, per-file bytes/SHA-256, generated/lockfile/vendored classification, incremental delta, packet-level "new since last review" baseline.
- Revision # = local `iteration`, incremented when the tracked head SHA changes (`monitoring.rs:2450-2504`). No stored `last_reviewed_revision`.

### Cost evidence (8 real runs)
Input tokens 21,119 to 2,966,832 (most 21k-273k); output 373-6,261; tool calls 4-69. Outlier xgang-harness#269 (2.97M in, 69 calls); cause unverified (paging vs. PR size). Changed-file counts and durations are not stored.

### Hypotheses (not facts)
- H1: Terminal tools can express "outs" (`request_human`, `notify_ready`, `no_reply`) with host as authority. Needs a POC for runtime behavior.
- H2: Paged reading multiplies tokens for large files; a manifest + selective reading may cut cost. Unmeasured.
- H3: A second session for `rubber_duck` works under the same account. Unverified.

### Unknowns
Terminal-tool behavior in the real runtime; parallel tool-call scheduling; provider concurrency/rate limits; usage-event token reliability; where PR description lives in `PullRequest`; real changed-file counts.

## 3. Alignment record

Human confirmed the findings as aligned (2026-10-12). Correction: harden the baseline first, expand later. Priorities and next step as in section 1. Pull vs. push packet delivery: human unsure; explained as: push = everything in the first message; pull = short manifest plus tools to fetch on demand. No selection made.

## 4. Domain model (aligned findings only)

**Actors.** Human owner. Host (PR Sniper deterministic code). Agent (fresh model session per run). Provider (GitHub). Copilot runtime.

**Concepts and terms.**
- **Run**: one fresh agent session for one unit of work. Kinds: review, reply, final.
- **Review packet**: the single versioned, deterministic document an agent receives; stored with the run (minus file content) with a hash.
- **Tool**: a host-owned hook the agent may call. Classes: **investigation** (read-only, bounded, paged, always listed) and **action** (grant-gated).
- **Terminal action / out**: a tool call that ends the run. A request, not an effect: the host validates it, applies grants and gates, saves intent before effect, then acts.
- **Grant**: Publish Comment, Reply Comment, Approve, & Merge on a repository assignment.
- **Manifest / coverage**: per-file parts with sizes (and proposed hashes); host tracks bytes served; a gap rejects the review.
- **Revision**: the tracked head SHA iteration. **Lens**: agent+doctrines+preset+prompts hash.
- **Audit record**: per tool call: tool, args hash, size, outcome. No content or secrets.

**States and events.** Run: queued, running, ended by terminal action, failed/parked. Events: packet built, tool called, terminal action requested, validated, executed or denied.

**Boundaries.** The model requests; the host authorizes and writes. The packet carries untrusted text (PR body, comments, file content) fenced as data. One review per head SHA (ADR 0008). Provider writes follow ADR 0010/0016/0017.

**Relationships.** Queue schedules Runs; a Run gets exactly one Packet and a Tool set filtered by Grants; Terminal actions produce Host actions; Host actions produce Receipts; Receipts feed Ready/Approved/Needs attention states.

**Unresolved interpretations.** What exactly counts as an "out" and whether a run may end on several; whether "reply" is a Run kind or a second packet envelope.

## 5. Scout comparison

**Decision.** The agent's interface: what the packet contains and how results leave the run.

**Hard constraints (from evidence/owner).** Host stays the write authority (ADR 0010/0016/0017); no shell/network for the agent; one review per head SHA; no silent truncation; grants gate action tools.

**Criteria (owner priorities, unranked):** review quality, auditability, speed to ship, extensibility, deterministic outs.

| Route | Hypothesis | Fit | Evidence for / against | Status |
|---|---|---|---|---|
| R1 Richer JSON prompt, keep result schema | Quality and audit improve with data alone | Speed high; extensibility, outs low | For: data already fetched. Against: no tool-based outs; owner wants outs | viable, partial fit |
| R2 Packet + terminal action tools (host authority) | Outs and extensibility come from tools | Outs, extensibility, audit high; speed medium | For: SDK `is_terminal`, allowlist+hook exist. Against: runtime behavior unproven; biggest rewrite (schemas, validation, tests) | viable, untested |
| R3 Phased hybrid: enrich packet + store it first; add few terminal outs; expand tools later | Hardening baseline first, then expand | Speed high, extensibility medium-high | For: matches "baseline then expand"; builds on R1/R2 parts | viable, leading candidate |
| R4 Non-terminal tools that write immediately, several per run | Max flexibility | Quality medium; safety low | Against: conflicts with ADR 0010/0016 (batch, intent before effect) | in tension with constraints; not ruled out, untested |
| R0 Do nothing | status quo | none | Against: no outs, thin context, no audit | ruled out by owner intent |

**Delivery sub-decision (push vs pull).** Push all: simple, large fixed cost. Pull via manifest: smaller packet, more tool calls. Evidence: cost spread 21k-2.97M; H2 unmeasured. Unselected.

**Stopping case.** Evidence now distinguishes R3/R2 over R1/R4 on stated priorities; remaining uncertainty is runtime behavior, discoverable by a bounded POC. Confidence: moderate. Doctrine cited: scout / Principles / "Seek meaningfully different routes", "Buy the cheapest useful evidence", "Keep selection human-owned". Not explored: provider-side concurrency, non-Copilot runtimes. Recommendation is not a selection.

## 6. Frontier

**Known.** Pipeline, SDK capabilities (by source), data coverage, gate semantics, cost spread, owner intent.

**Unknown / next move.**
1. Terminal tools in the real runtime (ends turn? payload? failed-call loop?). Needs POC.
2. Second session for `rubber_duck`: concurrency, cost. Needs POC.
3. Token cost with packet enrichment and manifest vs. full paging. Needs measurement on real PRs.
4. Which "outs" exist and their exact semantics (one or several per run; host action per out). Human + hone.
5. Approve gate: require green CI / mergeable / complete clearance? Human decision.
6. Generated/lockfile policy; prior feedback scope; reply packet shape; `search_code` scope; packet header budget. Human + evidence.
7. Where PR body lives in `PullRequest`; "new since last review" baseline design. Research/read.

**Blocked.** None. **Ready for inquiry.** 1 (POC), 3 (measure), 4 (hone with human).

**Next inquiry recommended.** Hone tools/capabilities with the human (item 4), then a bounded POC for terminal tools (item 1).

## 7. Decisions recorded after alignment (human, 2026-10-12 16:33)

- ADR 0021 (review packet) reviewed by the human: "looks good. aligned." Repo status still `proposed` until an approved edit.
- **Delivery: balanced push + pull.** Packet carries the top-level facts (title, description, author, state, revision, CI summary, file manifest, directive, rules, tools). Details are fetched on demand through investigation tools (file content, threads, CI logs, search). Settles the push-vs-pull sub-decision; leading route stays R3 (human selection of R3 not yet explicit).
- Frontier item 4 (outs and their semantics) is next; item 1 (terminal-tool POC) waits for an agreed scope.

## 8. Exit ("out") design, human statement 2026-10-12 16:43

A run ends on its outs. Named exits:
- `leave_comments`: PR has outstanding issues the agent found. Args: array of 1+ comments, each with target line(s) and body; replies to existing threads also pass through this exit.
- `ready_for_human`: no new outstanding issues; move the PR to the human's queue. Leaves **no comment**.
- `approve_pull_request`: approve. Leaves an **approval comment**.
- `approve_and_merge`: approve then merge. Leaves an approval comment.
- `panic_exit`: unrecoverable state. Host logs it and throws the PR to the Needs attention queue.
Typically the agent uses one of `ready_for_human`, `approve_pull_request`, `approve_and_merge`.

Host interpretation (agent-proposed, not yet confirmed): exits are grant-gated (`leave_comments` needs Publish Comment and Reply Comment for replies; approve needs Approve; merge needs & Merge); a missing grant means the exit is not offered. `ready_for_human` and `panic_exit` need no grant and no provider write. SDK note: a failed terminal tool call leaves the loop running (`types.rs:350-365`), so a host-denied exit can return an error and let the agent choose another.

## 9. Exit semantics, human answers 2026-10-12 16:50

- **One exit per run.** The run ends on its exit. The agent may also have other, non-exit tools (read_file etc.).
- **Tool access is operator-granted, not fixed.** "If the operator/human has provided tool access to the agent then it can use it." (Implication, agent-derived: the fixed 3-tool allowlist of ADR 0020 becomes a per-run set computed from the agent/assignment configuration plus grants. Not yet confirmed.)
- **Denied exits, by scenario:**
  - CI red: the agent does not approve. If it would have approved but CI is red, it leaves a comment that CI must be fixed, with a recommendation for the error and fix. (Needs CI status and logs available to the agent.)
  - Head advances / files change: throw the review away and re-queue a new one (host action; consistent with ADR 0008 new head = new review).
  - Every other scenario: to be worked out with the human, one at a time.
- **panic_exit:** log, send to Needs attention, then auto-retry on the existing cooldown schedule.
- **Reply runs:** human suggests splitting comments into new comments and reply comments, and does not want to be restrictive. (Agent proposal, unconfirmed: one exit `leave_comments` with two arrays, `new_comments` and `reply_comments`, which keeps "one exit per run".)

## 10. Answers 2026-10-12 17:00

- `leave_comments` = one exit with two arrays: `new_comments` and `reply_comments`. Confirmed.
- Tool set is **part of repository configuration**: "this agent has these tools" (per repository assignment). Confirmed (resolves the ADR 0020 allowlist question: per-assignment tool set, plus grant-gated exits).
- Denied-exit scenarios are walked one at a time, organized as ways to fail fast and fail gracefully.
- **Version model (human):** "comments and revisions are just versions that get queued up to get reviewed." A unit of work is a *version* of a PR (a new head revision, or a new comment set). Each queued version gets one run and one exit. (Agent-derived implications, unconfirmed: reply and review stop being separate run kinds; a newer version supersedes an older queued or running one; the packet shows the PR at that version plus what changed since the last reviewed version; ADR 0008 "one review per head SHA" generalizes to "one run per version".)

## 11. Failure handling, human answers 2026-10-12 17:10

- **Superseded is a deterministic pre-run check**, done before the agent gets a chance to run (no agent spend). Needs a **comment metadata structure** that tracks which revision each comment targeted.
- **Agents may resolve their OWN threads/comments.** They cannot close others' threads or comments; they may reply if they disagree. (This reverses the working rule "agent never resolves or reopens threads" that appears in docs/review-packet.md; also check CONTEXT.md, ADR 0010/0016 and docs/provider-writes.md.)
- **Gates run twice, confirmed:** gate at packet build time. At exit time all comments are stamped with their revision.
- **Write failure at exit:** if a file was renamed or moved meanwhile, or PR Sniper fails to write for any reason: toss the batch, increment an error count, re-queue another run. (Agent-derived: needs bounded error count so it parks; and the host must not leave a partial batch on the provider.)
- Buckets A-E (agent-correctable, superseded, infrastructure, human-needed, not applicable) proposed by the agent; not objected to.

## 12. Scenarios decided 2026-10-12 17:20

- **Scenario 2 merge conflicts:** do not approve. Leave a comment stating the PR is blocked with merge conflicts; recommend how to resolve them if non-trivial. (Same shape as CI red: fail fast, approve not offered, agent comments.)
- **Others' unresolved threads:** approve is still allowed. Only the human's own resolution matters. (Changes the current host gate, which blocks on any unresolved thread incl. outdated. To record as a gate change.)

## 13. Scenario 3 CI pending, human answer 2026-10-12 17:30

- CI queued/running is fine: the agent may review, comment and approve. It may not merge until CI is green.
- The PR is later picked up by polling as an open, unmerged PR; the agent gets another pass, where it can merge.
- While CI runs, the agent's only exits are approve (again) or leave a comment. No comment like "CI gate must be clear before merging". Acknowledge CI only when it is broken.
- (Agent-derived, unconfirmed: re-approve must be idempotent when our approval already exists on the head; approve_and_merge is not offered while CI is pending; re-pass cost needs a trigger rule.)

## 14. Central model: PR state, human answer 2026-10-12 17:40

- Maintain the state of each PR deterministically: current revision, open comments, any edits, "all the stuff".
- The heartbeat cron looks for changes. **No change means nothing is queued.** If CI state has not changed since the last run, there is nothing to review.
- For every PR picked up, keep a **state** of that PR in memory. If any agent **successfully calls an exit against a PR state, that state is consumed** and can never be re-reviewed.
- This generalizes the "versions" idea: a version is a PR state. Revisions, comments, edits and CI/mergeability changes are all state changes that create a new state to review. The second (merge) pass is just a new state (CI turned green).

Agent-derived implications to confirm: (1) our own writes (comments, approval, resolved threads) change the PR state, so the post-exit state must be recorded as the baseline for that consumed state, or the agent would re-review its own output; (2) "in memory" must be backed by durable storage or a restart re-reviews everything (the current tracked-PR state is already persisted); (3) noisy fields (timestamps, per-check flapping) need normalization into stable classes; (4) a successful exit is the only thing that consumes a state; failures, panics and tossed writes do not; (5) cost: each state change is one run, so a settle window may be needed for bursts; (6) replaces ADR 0008's "one review per head SHA" with "one successful exit per PR state".

## 15. PR state details, human answers 2026-10-12 17:55

- Own writes do not trigger a re-review: the post-exit state is the baseline. Confirmed.
- **No separate settle window.** Changes are only detected on the repository polling cadence, so the cadence is the debounce. (Agent note: a state observed mid-burst is still reviewed; a later change makes a new state and another run.)
- **Merged means done.** A merged PR moves to the done queue (Reviewed) immediately; no further action.
- **CI is an array** of checks, not a single value. State must carry the array (name, status, conclusion), normalized to avoid timestamp noise.
- "Review decision" was unclear to the human. Explanation given: GitHub's aggregate review verdict (APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED) derived from reviews and branch protection. Otherwise the field list is accepted.
- **Persist the state.** Parked PRs sit in the Reviewed queue, which gets a **"clean" button that deletes all the persisted stuff**. (Agent note: scope, all PRs or per PR, unspecified; ADR 0015 currently cleans only on provider-confirmed terminal state.)

### Consolidated model v2 (agent synthesis of sections 8-15; unconfirmed items marked)
- **PR state** = normalized snapshot: head/base SHA, title+description hash, open|draft|merged|closed, mergeable class, CI check array, review decision, existing approvals, comments and threads (id, last edit, resolved, targeted revision), labels.
- **Heartbeat** (repo polling cadence) diffs current state against the last consumed baseline. No diff, no run.
- **Queue** holds changed states. Pre-run deterministic check: if a newer state exists, discard and re-queue without spending an agent.
- **Run**: fresh agent, one packet, per-assignment tool set, one exit. Exits: `leave_comments` (`new_comments`, `reply_comments`, own-thread resolutions), `ready_for_human` (no comment), `approve_pull_request` (+approval comment, idempotent), `approve_and_merge`, `panic_exit`.
- **Gates run twice**: at packet build (exit not offered; the packet says why) and at exit time (re-check; comments stamped with revision).
- **Consume**: a successful exit consumes the state; the post-exit state becomes the baseline. Failed or tossed writes, panics and errors do not consume (error count, re-queue, cooldown/park).
- **Merged** ends the PR: Reviewed queue, no more action. Closed unmerged: unconfirmed (assumed same).
- **Failure buckets**: A agent-correctable, B superseded, C infrastructure, D human-needed, E not applicable.
- Scenarios decided: CI red, merge conflicts, others' unresolved threads (approve allowed), CI pending (approve ok, merge only when green), head advanced, write failure.
- Scenarios still open: required reviews missing, changes requested by someone else, draft, closed, acting account is the author, approval already exists, signed out mid-run, comment line not in the diff.

## 16. Answers 2026-10-12 18:05

- Reviewed queue gets **Clean per PR (row action) plus Clean all**.
- A PR closed without merging: same as merged (done queue, no more action).
- Review decision stays in the PR state.

## 17. Answers 2026-10-12 18:15

- **Scenario 5 changes requested by someone else:** approve still allowed; their request does not bind the agent; only merge is blocked.
- **Scenario 4 required reviews not satisfied:** approve allowed; approve_and_merge not offered until the provider says reviews are satisfied (like CI pending, no comment).
- **Heartbeat loop (human restatement):** every 5 minutes, for each PR: read the current state; if it differs from the recorded state, queue a review for that state (the work packet, identified by the state); if not, go to the next PR.
- (Agent notes: the packet is built when the run is dequeued from fresh state, so a state that moved again is discarded before agent spend; per-tick state reads cost API calls for every tracked PR, so batching/conditional requests may be needed.)

## 18. Answers 2026-10-12 18:25

- Heartbeat (human restatement): every 5 minutes, list which PRs are open, grab all their states, compare to the recorded states.
- **Acting account is the PR author:** the human reports he has approved every PR raised in this work from the same account. Wants an explicit option on the **Approve & Merge** setting, e.g. an **"Include self"** checkbox. (Evidence note: today's gate blocks when the acting account equals the author, `github/actions.rs:127-130`. Unverified: whether GitHub accepts an author approving their own PR; to be probed on a disposable PR, otherwise the option is moot.)
- **Draft PRs:** a repository setting, "review drafts" or not.
- Settings surface so far: per-assignment tool set; repository setting "review drafts"; Approve & Merge option "Include self".

## 19. Answers 2026-10-12 18:35 (last listed scenarios)

- **Approve when our approval already exists:** idempotent success. No duplicate approval, no new comment; the exit counts as successful and consumes the state.
- **Comment line not in the diff (or file renamed/moved):** whole batch tossed; error count +1; re-queue; after 3 failures park with the error.
- **Signed out mid-run:** bucket D: tossed, parked as Credentials in Needs attention with Fix; auto-resumes on reconnect (existing behavior).

## 20. Frontier after this pass

**Decided scenarios (13):** CI red, merge conflicts, others' unresolved threads, CI pending, head advanced, write failure, required reviews unmet, changes requested by others, draft (setting), acting account is author (setting), approval exists, bad comment line, signed out.

**Open evidence needs.**
1. POC: terminal tools in the real runtime (does a successful exit end the turn; payload shape; a failed call loops).
2. Probe: does GitHub accept an author approving their own PR (disposable PR); decides the "Include self" option.
3. Measure: token cost of the push packet vs. pull on real PRs.
4. POC: second session for `rubber_duck`.
5. Read: where PR body lives in `PullRequest`; per-comment revision stamp design; state normalization for the CI array.

**Open human decisions.** Whether to select route R3 explicitly; tool set shape per assignment (names, config UI); `rubber_duck` model/budget; generated/lockfile policy; draft + approve behavior when review drafts is on; scope of the Reviewed Clean vs. ADR 0015.

**Repo documents to update (needs approval):** `docs/review-packet.md`, ADR 0021 (state-based units of work; exits; own-thread resolution; gates twice), ADR 0008 (one successful exit per PR state), ADR 0020 (per-assignment tool set), `docs/provider-writes.md` (approve gate relaxations: others' threads, changes requested), CONTEXT.md terms (PR state, exit, consumed state).

**Recommended next action.** Run one bounded POC for items 1 and 2 (terminal tools + self-approval probe) in isolation, then hand the full artifact to `specify`. Discovery owns this recommendation; nothing advances without the human.

## 21. Final statements before Specify, 2026-10-12 18:00-18:10

- **Queue holds the PR.** If a review is queued, the PR is queued for review. When an agent is ready to take it, the system generates the packet from the PR's **latest state** and hands it over. This eliminates review details sitting in the queue while the remote PR advances. (Supersedes the earlier "queue holds changed states / drop and re-queue" note in section 15 and the pre-run-discard wording in sections 11 and 17: with consume-time packets a moved state needs no discard; a state that moves during a run is still tossed and re-queued.)
- #171 (spec + ADR 0021, reworked around PR state, consume-time packets and exits; ADR 0008/0020 amended as proposed changes) reviewed by the human: "171 looks good."
- Superseded in this artifact: section 4 "Run kinds: review, reply, final" and the "version" wording (now PR state); the constraint "one review per head SHA" (now one successful exit per PR state; ADR 0008 proposed amendment).
- Specify invoked by the human on 2026-10-12.

## 22. Composition decision and delivery mode, human 2026-10-12 19:00

- **Multi-agent composition (resolves spec C-06 / OQ-01):** with several assignments, change the model. **The primary agent executes last.** All non-primary agents run first and each generates a **concise report for the primary agent**. Then the primary gets its turn and has **the actual exit tools**. (Agent reading, to confirm in the spec: non-primary runs end by submitting a report, not an exit; only the primary is offered the exits; a sole assignment is primary and gets no reports; the separate primary "final review" run is replaced by this primary turn; Publish Comment/Reply Comment/Approve/& Merge grants matter only on the primary.)
- **Delivery mode:** treat this as an **ad hoc session**; reconcile existing specs and ADRs as needed; "completely get this working end to end." (Implementation authorized in this session; formal ticket breakdown skipped by the human. Agent-made decisions where Discovery left gaps must be labeled as ad hoc and flagged for owner review.)

## 23. Teammate reports, human 2026-10-12 19:10

Example: the operator creates a SOLID agent and a PRAGMATIC agent (non-primary, each with its own doctrine lens) and both run. Their reports feed the PRIMARY agent's work packet. The primary must realize these are **reports from its configured teammates** on their findings, and it can take their findings into account. (Agent reading: the Reports section names each teammate and its lens; the packet rules frame them as colleagues' findings to weigh, not commands; the primary decides and verifies what it relies on.)

## 24. Report format, heartbeat, consume time, delivery, taxonomy, sandbox, human 2026-10-12 19:40

- **Teammate report format.** Teammates **always use caveman** for inter-agent reports. A teammate has a **single exit tool, `submit_report`**, like a sub-agent evaluation. It accepts an **array of findings**; each finding has `file`, `line` (number), `nano` (short description, 100 words or less), `full` (full description, up to 4 KB) and `fix` (recommended fix, up to 4 KB). (Agent reading, to confirm: teammates keep their configured investigation tools; "single tool" means the single exit tool.) The primary's Teammate reports section renders the findings (nano by default; full and fix available), fenced as agent output. All agent-to-agent text is caveman.
- **Heartbeat.** The default stays the current default (not changed to 5 minutes). Each heartbeat only checks whether something needs to be **added** to the queue or **removed** from it (PR closed or merged before its turn).
- **Consume time.** When a PR's turn comes up in the queue it refreshes itself with fresh state. No drop-and-requeue for a state that moved while queued: "at worst there are no changes; if there are changes, pick them up automatically". Only a state that moves **during a run** is tossed and re-queued.
- **Delivery.** One big PR, no incremental PR per phase: build packet mode end to end, verify it works (sandbox repo run), then remove the old path. No "legacy until Tools is set" idea: packet mode replaces the old path for all assignments once verified. Internal milestones are fine.
- **Taxonomy confirmed by the human.** Permission is recoverable by cooldown; schema failures are permanent. The taxonomy items are no longer ad hoc.
- **Sandbox.** A private repository `jdylanmc/pr-sniper-sandbox` with disposable PRs will be used for the self-approval probe and the live run.
