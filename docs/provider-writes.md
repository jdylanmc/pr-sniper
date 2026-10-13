# Provider writes

Reference for every GitHub write PR Sniper can make: comment publication, thread replies, mention replies, Approve and Merge. Reviews are always on; each write is a per-assignment opt-in grant. Agents never write; the native host does ([ADR-0010](adr/0010-native-mutation-owner-intent-before-effect.md), [ADR-0016](adr/0016-revision-safe-publication.md), [ADR-0017](adr/0017-opt-in-provider-actions.md)).

## Rules

- Intent persists before every provider write. Reconcile the original; never resend.
- Live mutation acceptance only on an explicitly authorized disposable PR with cleanup. Fixtures never claim a live pass.
- Failure origin comes from typed provenance, never message text.
- Publication failures stay on the PR with cause and next action, not a global banner. Host persistence/read/coordination failures keep infra provenance and raise a host warning.

## Comment publication

- Grant: **Publish Comment** on the assignment. Off: findings stay local; no wait state, no manual task. Retired global defaults and repository policy dropdowns have no authority.
- Flow: host freezes validated output → maps findings to verified diff lines → one pending review with explicit commit SHA → submit `COMMENT`.
- Out-of-diff findings stay local; the summary gives a count. Sign-off ends ` PR Sniper` and requests final human review. Custom Agent signature is not applied to the review body.
- Before and after mutation recheck: account/repo identity, head, reviewed target base, open and non-draft, eligibility, monitoring scope, publication permission. Disabling Comment or a gate change stops the attempt.
- **Withdraw publication**: revokes the attempt; removes only the exact owned pending batch when verifiable. Never touches another pending review. A confirmed visible batch is not undone.
- Stale: head change queues a new check only if eligible. Change after a visible submission keeps the receipt marked **stale after publication**. Base move without a new head: explicit **Review again**. A new local review cannot bypass a pending, uncertain or published batch for the same revision + assignment.
- Ledger `state/publications.json` (private): immutable output, mutation intent, pending-review identity, confirmed receipts, retry budgets. Restart reconciles the whole remote review and inline comment set (identity, exact content) before repeating. A missing receipt after an uncertain create never allows another create. Another pending review of the signed-in account is never submitted or deleted.
- Pending comments may have null line/side. New batches keep a verified diff position per finding. Reconcile checks position, path, body, original commit, author, review identity, comment count. Older batches: read-only comparison of frozen revisions; missing or truncated patch stops verification.
- Unusable batch (owned pending review whose inline comments differ from the frozen batch, changed lens, withdrawn): the exact owned pending review is deleted and the publication is **discarded**; no replacement batch, no Needs attention row. The head is re-reviewed once per lens (open PR, current head, nothing published) and its new batch publishes normally. A submitted review with differing comments stays the receipt. Transient failures auto-resume on the review cooldowns; startup re-observes publications parked by older builds once.
- Retry: 3 retries within 15 min, shared. Operation and deadline survive restart. Rejection, config and permission failures need a fix. **Reconcile / retry publication** = fresh budget, same identity and uncertainty. After GitHub-confirmed submission with failed final eligibility, the receipt is kept and retry only reconciles.

## Conversation follow-ups

- Primary Agent only. Each repo read checks unresolved threads rooted in confirmed PR Sniper inline comments (saved review + comment receipts, repo/account identity, original commit, exact root body). Pending comments excluded. Discovery failures use the poll retry budget.
- Primary also assesses new eligible other-user comments on admitted open PRs: unowned, secondary-authored and non-mention too. First complete observation is the baseline; history is context only.
- Trigger key: provider/account/configuration/repository/PR/thread + trigger identity. No role, no later head. Comments between polls coalesce to the latest. Own signed replies, same-account comments and signed machine output never trigger. Poll, restart or push never duplicates a job.
- Analysis: same pinned Copilot account/model, current review lens, isolated runtime, three immutable read tools, reads what it needs. Publishable reply: bounded, substantive new information, exact verified source-line quotes. Quiet: repeats, ack-only filler, speculation, unsubstantiated answers. Fabricated citations fail visibly. Citations are not proof of every inference.
- Human-judgment question → **human input required**: no publishable body; auto follow-ups for that thread pause.
- Resolved, changed, superseded or stale threads stop. Replies end ` PR Sniper`; never resolve, approve or merge. Legacy only: Packet-mode assignments admit no Reply or Mention follow-up; existing records drain.
- Grant: **Reply Comment** (primary). Off: analysis still runs; responses stay local with a visible limitation. Enabling later never replays; new comments make new work.
- UI: Review Queue > **Thread follow-ups**. **Cancel follow-up** stops inference or withdraws permission before reply; cannot remove a confirmed comment.
- State `state/follow-ups.json`: analysis and publication each get 3 retries / 15 min; restart preserves. **Reconcile / retry reply**: same key and body, reconcile the full remote thread; never re-POST an uncertain reply.
- Feedback `state/feedback.json`: only provider closure settles a GitHub thread. Legacy agents never resolve or reopen; a Packet-mode exit may resolve own threads (below). Full reviews get earlier feedback as untrusted context and must reassess their own open concerns. Findings against the same or renamed file of a closed concern are **held locally**, never discarded. Unassign keeps unresolved feedback and blocks clearance.
- Confirmed publication is immutable. Later feedback changes affect current readiness only.

## Mentions

- Scan: [polling.md](polling.md#pr-metadata-read). A standalone `@login` of the acting GitHub identity in a top-level issue comment yields one response routed to the current primary. Never the Copilot identity, never all Agents.
- A mention can admit an otherwise unwatched PR. Reply permission alone never admits PRs.
- Intent key: provider/account/repo/PR/comment. Pending intent revalidates the primary; executed intent is immutable. Signed or same-account comments are loop-guarded. Edited or deleted comments block new work; reconcile stays allowed.
- Reply is a top-level comment linking the original, reusing the reply schema, `Kind::Mention` capacity, serial publication, pause and no-blind-repost. No fake review or publication.
- Admission write order: feedback observation → mention identity + FIFO intent → follow-up execution → execution link. Success recorded after all. A failure leaves a durable blocker. Unlinked intents recover; committed linked ones are not replaced.
- No primary, unavailable model or no iteration: work stays queued with its reason, FIFO.

## Packet exits

- Packet-mode runs write only through their exit; the host applies it. Gates (`exits::gates::offered`) decide what is offered; the exit-time check re-reads the PR.
- `leave_comments.new_comments`: one auto-created publication via `Batch::prepare_exit` (same ledger, pending review, reconcile as above). Every line must map to the diff; one unmappable line tosses the whole batch, nothing is posted. Needs Publish Comment.
- `leave_comments.reply_comments`, `resolve_own_threads`: host-executed, ledger `exit_writes` in `state/feedback.json`. See **Replies and resolutions**. Needs Reply Comment (both).
- `ready_for_human`: no GitHub write. Local summary, queue shows Ready for you.
- `approve_pull_request`, `approve_and_merge`: payload, gate and stored outcome only; effects not wired yet. Approval comment required. GitHub refuses self-approval; merge on own PRs needs Include self. Approve ignores others' threads and changes requested; merge does not.
- `panic_exit`: no write. Typed failure, parks at once, cooldown auto-resume.

## Replies and resolutions (packet exit)

- One exit = one ordered effect: **preflight** (read-only) → publication batch (`new_comments`) → replies → resolutions → confirmed. Publication is held until preflight passes (`exit_writes::holds_publication`).
- Preflight verifies every item or tosses the whole exit before any effect: head unchanged, thread exists, repliable root, resolutions owned. Nothing partial. Toss = error +1, re-queue (ADR 0007, park at 3).
- Ownership: root must verify against a saved publication proof (receipt, account, review, original commit, exact root body) of this PR. Others' threads: reply only. Already resolved own thread = done, no write.
- Intent saved before each effect (`Attempted`). Reply body frozen at preflight: agent text, marker `<!-- pr-sniper:reply:exit-<id>-<n> -->`, revision stamp, ` PR Sniper`. Reconcile reads the thread: marker present = done. Attempted and absent = **never re-POST**; retries read only, then park `Failed`.
- Resolve: GraphQL `resolveReviewThread` with `clientMutationId`; unconfirmed result re-reads the thread; idempotent retry.
- Failure after the first effect = `Failed` (needs human), never silent replay. Before it = `Tossed`.
- Revision stamp per comment: thread root `original_commit_id` mapped to a local iteration, else `unknown`.
- Not `FollowUp`: it needs a validated analysis result, queue kind and primary-only owned targets. Own ledger, same rules. Legacy follow-ups still drain.
- Confirmed exit calls `exit_writes::consume_after` (integration point): consume and re-baseline once, so own writes never retrigger.

## Approve and Merge

- Primary only. **Approval** pill `off | Approve | & Merge`. State `state/actions.json`. Uses shared AI capacity, execution gates, pause accounting, worker teardown.
- Clearance: every current assignment has its own completed pass for the tracked iteration. Blocked by pending conversations, held findings, human-input decisions, incomplete conversation admission, unknown mutations.
- Final review: distinct full primary review over all changed files, peer results, owned feedback, bounded human discussion and policy observations. Fail-closed.
- Rechecks: head/base, role, scope, permissions, feedback, human context. A valid final review serves both actions; the host's own confirmed approval receipt does not invalidate it. Other relevant change → fresh final.
- Approve: one acting-account vote, pins `commit_id`. No self-approve on GitHub. Never replaces an existing vote. Reconcile needs exact signed body, actor, commit, review receipt.
- Merge needs: confirmed current-revision approval from an eligible reviewer; green current-head checks; satisfied reviews and rules; `CLEAN`; no unresolved threads (outdated too); no required or active merge queue; provider-selected enabled method. Pins `expectedHeadOid` and explicit method. Admin bypass is never queried or used. Absent, partial, unsupported or inaccessible evidence blocks.
- Provider limits (`src-tauri/src/github/actions.rs`): fail closed beyond 100 threads, 100 comments per thread, or 100 check contexts. No direct-merge fallback for hooks, unavailable merge rules, merge queue or missing green CI.
- Reconcile: up to 3 automatic observations in the original window, then **Reconcile original action** (one more read). A merge proven after a lost response is terminal with no PR Sniper attribution. Rejected, stale or cancelled actions are never replaced automatically.
- Evidence read failure stays visible after Approve/Merge is turned off. **Retry provider evidence** follows native pump eligibility; disabled with a reason when no work, in backoff, or paused.
- Capacity: a cancelled final worker keeps its reservation until teardown. A real outcome-persistence failure keeps a visible stopping slot.
- Handoff: a cleared, unmerged PR keeps the personal-review handoff, also after auto-approval. Merge needs no acknowledgement. Merged = terminal history. Notifications and queue details stay separate for clearance, final review, approval/merge, unknown outcome, personal review.
