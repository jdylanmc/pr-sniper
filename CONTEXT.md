# PR Sniper

PR Sniper monitors configured provider repositories and prepares review work while keeping provider identity, repository access, review execution, and publication authority explicit.

## Language

### Identity and access

**Provider Account**:
A stable user identity within one source-control provider. Provider name and provider-issued account ID together form its identity. Carries repository and publication identity only; independent of **Copilot AI Account**. Connect saves identity on browser sign-in completion, assigns no repositories, picks no model, enables no automation. Disconnect forgets the account; reconnect allowed.
_Avoid_: Active account, current account

**Copilot AI Account**:
Independent GitHub Copilot identity used only for AI model access. Each **Agent** selects one account and one model explicitly. Never decides **Acting Identity**. Direct Claude, Codex, Grok integrations are disabled; models reached through Copilot are distinct from them. Disconnecting keeps dependent Agents and assignments.
_Avoid_: AI login, Provider Account (for Copilot)

**Repository Account Binding**:
The explicit association between one configured repository and the provider account used for every provider action on that repository. Never auto-picks an account, never falls back to another account or CLI login. Unbind keeps assignments, grants and completed evidence; provider actions need a new explicit binding. Same remote may be bound to different accounts.
_Avoid_: Default account, inferred account, fallback account

**Acting Identity**:
The provider account visibly named at the point where PR Sniper performs or proposes a repository action. Chosen explicitly in Settings; a different account fails visibly.
_Avoid_: Logged-in user

**Provider Repository Identity**:
The provider-issued stable repository ID bound to one explicit Provider Account.
_Avoid_: Repository name

**Accessible Repository**:
A repository returned for a connected Provider Account through its granted authorization, including repositories owned by another user or organization. Accessibility does not configure or monitor the repository; organization, project, tenant and provider policy can further restrict it.
_Avoid_: Installed repository

**Needs Attention**:
Two linked meanings, one name. (1) Queue group listing every **Parked failure**, each with a cause chip (Credentials, Permission, Network, Failed) and one row action: **Fix** for credentials, **Retry** otherwise; **Retry all** retries the group. (2) Account-level cause behind a credentials failure: a repository binding or provider account that cannot authorize reads and needs explicit reconnect or rebind. Fix opens Accounts. No Blocked state exists.
_Avoid_: Blocked, Disconnected repository, Failures

**OAuth Device Authorization Attempt**:
A single bounded GitHub device-flow operation containing a provider-issued one-time user code, a secret device code retained only in the native host, browser launch, polling, cancellation, and expiry.
_Avoid_: Login session, callback flow

**One-Time User Code**:
The short provider-issued code shown to the user for manual entry at GitHub's verified device authorization page. It is transient UI data, not the secret device code used for token polling.
_Avoid_: Device token, access code

**Pending Account Confirmation**:
A validated stable GitHub account ID and login whose credentials remain only in memory until the user explicitly confirms.
_Avoid_: Connected account

### Agents and grants

**Agent**:
Saved review configuration: prompt, signature, ordered zero-or-more **Doctrines**, one **Copilot AI Account** and model. No count cap; **AI Capacity** limits concurrent work, not Agents. Missing, unavailable or legacy selections stay visible, never silently replaced. Referenced Agent cannot be deleted. Holds no permissions; grants live on repository assignments.
_Avoid_: Bot, reviewer, development agent

**Doctrine**:
Shared, editable plain-text review principle selected per Agent. Referenced Doctrine cannot be deleted; rename updates references. Edits and deletions, even an empty library, survive restart; the library is never topped up. Holds no credentials.
_Avoid_: Skill, prompt snippet

**Primary Agent**:
The repository assignment responsible for eligible conversation assessments and the final full review before permitted provider actions. Runs last, after every teammate report for the same PR state; its packet has the Teammate reports section. A sole assignment is primary automatically; multiple assignments have at most one explicit primary. No primary means no reply, approve or merge; normal reviews and permitted initial comments continue. Selecting primary grants no capabilities and does not change original feedback ownership.
_Avoid_: Global primary, thread owner as responder

**Tools** (assignment):
Packet tool ids an assignment may use: investigation tools plus exits. Every run is packet mode. Absent = role default: primary gets investigation tools, granted exits, `ready_for_human`, `panic_exit`; teammate gets investigation tools, `submit_report`, `panic_exit`. Set = validated explicit list. Effect exits primary only; `submit_report` teammate only; exits need their grant. Set by switches in the assignment's Tools list.
_Avoid_: Skills, plugins

**Packet**:
The one text an agent is given per run, built by the host from fresh PR state: Meta, fixed Rules, PR, Conversation, Files manifest, Teammate reports (primary), Directive, Exits. Untrusted content is fenced; nothing is cut without a marker. Same state, same text, same `packet_hash`. Stored content-free as the run's packet record. See [Review packet](docs/review-packet.md).
_Avoid_: Prompt

**Exit**:
The one terminal tool call that ends an agent run: `leave_comments`, `ready_for_human`, `approve_pull_request`, `approve_and_merge`, `panic_exit`, or a teammate's `submit_report`. Offered or denied by the gates at packet build, re-checked at exit. The host is the only writer; the agent never writes.
_Avoid_: Decision, verdict

**Teammate**:
A non-primary assignment of a repository. Runs first, ends with `submit_report`, never has an effect exit. Its report is untrusted data for the primary. Teammate fail = whole PR-state run set fail: primary blocked (`BlockedBy::Teammate`) until every teammate has a report for same state. Good reports kept on retry. State move re-runs all.
_Avoid_: Secondary agent, peer

**Review drafts** (repository):
Repository switch, default off. On: draft pull requests are reviewed too.

**Include self** (Approve & Merge):
Approve & Merge option, default off: the user's own PRs count. Inert until approve exits land.

**Assignment Capability**:
An explicit repository-scoped grant on an assignment: Publish Comment for any assignment; independent Reply Comment and Approve or Approve & Merge for the primary only. New assignments start with Comment, Approve and Merge off. Publish Comment off: findings stay local. Reply Comment gates exit replies and resolutions. Merge includes approval and follows confirmed current-head approval; an existing provider vote is not fabricated as a new PR Sniper vote. Obsolete secondary action grants show disabled, never auto-promoted. Legacy start, default and override settings are inert history. These product grants never authorize development agents to publish, approve or merge.
_Avoid_: Automation override, reusable Agent permission, merge-only grant

**Authorization Boundary**:
Saving a repository with Agent assignments authorizes ongoing read-only reviews of all open and future admitted PRs, including forks and later revisions. No scope or revision-trust prompt. Save is one atomic settings replacement and reads no backlog. Unrelated saves never enable an unconfigured or disabled repository.
_Avoid_: Scope prompt, trust prompt

### Admission and scheduling

**Admission rule**:
Per-repository decision of which open PRs become tracked **Iterations**. Modes: **All pull requests**, or **Filtered pull requests**: ordered, reorderable on/off rules; first enabled matching rule decides; no match rejects. Rules: Assigned to me, Mentioning me, Include users, Exclude users, Include paths (any changed file matches), Exclude paths (all changed files match). Paths are prefix or glob. Admission is sticky: an admitted PR stays tracked after rule edits. Still gated by account, pause, capacity and grants. Existing repositories migrated with monitoring off until reviewed.
_Avoid_: Reviewer requests, People you watch, watch filter

**Explicit PR Intake**:
PR URL entered at **+ URL** resolves its repository and that exact PR. After a valid repository Save the PR is admitted at once, outside admission rules, with no schedule wait. Same URL and same Iteration reuse existing work. Normal gates still apply. A plain repository URL creates no PR work. Failed intake never undoes a committed Save.
_Avoid_: Manual scan, force review

**Global Schedule**:
The single five-field cron plus explicit IANA time zone (default `*/15 * * * *`, `UTC`) scanning all enabled configured repositories. No per-repository or per-Agent schedule. One provider read per account/repository binding fans out to per-Agent jobs. Missed or sleep ticks collapse to one check; no catch-up. Legacy interval choices are readable only and stay a visible setup issue until replaced.
_Avoid_: Polling interval, assignment timer, per-Agent poll

**Check Now**:
Manual global scan. Coalesces to one pending scan. Never bypasses **Backpressure** or an existing **Retry budget**.

**Pause Automation**:
Persisted global gate, separate from repository enablement and grants. Blocks new polling, AI admission and provider writes. Active workers are signalled and keep their slot, **stopping**, until teardown ends. Partial AI output is discarded. An already-sent remote request is not undone; intents and receipts are kept.

**AI Capacity**:
Machine-wide count of concurrent AI job slots, positive integer, default 4. Shared by all review runs (primaries and teammates) in one FIFO order across retries and pauses. Primary-final, reply and mention kinds are history only. Limits work, not Agent count. A free slot fills at once. Reducing stops the newest excess workers. Does not raise provider-write concurrency.
_Avoid_: Concurrency limit

**Saved Preferences**:
Global cron, time zone, **AI Capacity** and comment-publication defaults, saved with Settings. **Immediate Controls**: pause, notification opt-in, launch at login; apply natively, not by Save or Reset. A saved request is shown apart from actual OS registration; unknown registration is never shown as enabled.
_Avoid_: Startup checkbox as effective state

### Reviews and iterations

**Iteration**:
One tracked revision of a PR. A new head SHA starts a new Iteration and supersedes older work; a verified reopen after cleanup starts a new one even at the same SHA. Earlier Iterations remain history. Merged or closed PRs move to Reviewed.
_Avoid_: Revision (as tracked unit; "revision" means the provider commit)

**One-review-per-head rule**:
Each head SHA gets one review per assigned Agent. A new revision is the only permission for another review. On the same revision an Agent only replies to new comments. Added Agent gets its missing job at next scan; completed unchanged passes never repeat. Agent context is PR description, changed files and all comments, plus prior feedback. Being replaced by **PR state** detection ([polling.md](docs/polling.md#heartbeat-pr-state)).
_Avoid_: Re-review, manual re-run

**PR state**:
Normalized, sorted, timestamp-free view of a PR: head/base, text, lifecycle, mergeable class, review decision, approvals, CI array, comments and threads, labels. Compared by hash. Persisted per PR in `pr-state.json`.

**Heartbeat**:
The scheduled read of PR state for tracked open PRs. It only adds a PR to the queue (hash differs from baseline and pending) or removes it (merged/closed before its turn). One queue entry per PR, whatever the number of changes while queued.

**Baseline**:
Hash of the last consumed PR state, including the host's own writes, so own comments never retrigger.

**Consume** / **Toss**:
A successful exit consumes the state it read and sets the Baseline. A state that moved during the run tosses the result: error count +1, re-queue, park at 3. Honoured when: `ready_for_human` completes, comments publish, approve/merge confirm. Baseline = fresh snapshot after the write. Head moved or PR closed before the run: superseded, no budget spent.

**Clean** (Reviewed):
Per-PR and Clean all. Deletes persisted PR state, packet and result detail of terminal PRs only. Receipts and uncertain-write records stay.

**Job**:
One unit of AI work for one Agent on one exact Iteration. Kind: normal (packet run). Legacy kinds load as history, never run. Running counts jobs, not triggers: 7 Agents at capacity 4 means 4 active, 3 waiting. Carries an immutable **Execution Snapshot**.
_Avoid_: Trigger, task

**Execution Snapshot**:
Immutable record of Agent, account, model, prompt, Doctrine bodies, repository and assignment authority at job start. Evidence, never current grant, never rewritten by edits or restart. Legacy missing fields show unavailable, never rebuilt. A never-started failure keeps its planned configuration and claims no execution.
_Avoid_: Current config, Execution Configuration

**Normal Pass**:
The completed normal review of an Iteration by one Agent. Retries are **Review Attempts**; completed retries do not add Normal Passes. Per-Agent pass ordinals are stored in the queue state; reply work has its own ordinal.
_Avoid_: Review run

**Review Attempt**:
Every try at a review, including retries. Fixed-schema bounded local **Diagnostic Attempt Record** excludes prompts, bodies, transcripts and credentials; logging failure never changes outcome.

**Host-owned tools**:
The only tools Copilot gets in a review: the run's investigation tools (changed-file reads, source reads, threads, checks, tree, code search) and its offered exits, over immutable base/head trees. No checkout, commands, builds, tests, hooks, installs or provider mutations. Runtime tool catalog must exactly equal the allowlist before inference or the review is blocked. Constrained current-user process, not an OS sandbox.
_Avoid_: Sandbox, agent tools

**Validated review result**:
The one exit the run ended on (or a teammate report), runtime metadata, informational **Read coverage**. Legacy results (synopsis, file guide, findings, machine decision) load as history only. Head, base, lifecycle and inputs are rechecked before invoke; stale results are rejected. Stays local until publication gate admits.

**Final Review**:
Record of the primary's approve exit (packet `FinalReview`) that authorizes Approve or Merge. No separate final review run. Legacy final-review records load as history, authorize nothing; unfinished ones are cancelled.
_Avoid_: Approval review, second pass

**Action Clearance**:
Gate before any provider action. Every current assignment has its own completed pass for the tracked Iteration. Old evidence never covers an added or replaced assignment. Pending conversations, held findings, human-input decisions, incomplete admission and unknown mutations block.
_Avoid_: Auto-merge approval

### Failure and retry

**Backpressure**:
A rate limit (GitHub 403/429, Copilot/model 429) is a signal to wait, not a failure. No attempt spent, no failure count, no Park, never Needs attention. The provider account is gated in `backpressure.json` until Retry-After/reset, else 30s x2^n (cap 15 min, deterministic jitter); a success clears it. Queue shows a quiet clock row with the local resume time.
_Avoid_: Rate-limit failure

**Read coverage**:
Recorded count of changed files the agent read in full ("N of M files read"). Informational only: never rejects, fails or retries a review.
_Avoid_: Coverage gate

**Retry budget**:
Persisted per operation: attempt count, retry deadline, state. Survives restart and resume without reset. Transient failures auto-retry with backoff; 3 failed attempts then **Park**. The AI review window starts when the worker starts, not at enqueue. Credentials, permission and permanent failures park at once. Parked retry starts a new operation, keeping history. Cooldown cycles (30m, 1h, 2h, 4h, 8h; `resume_cycle`, `resume_at`) grant each resumable failure a fresh budget, up to five.
_Avoid_: Retry count, unlimited retry

**Failure class**:
Cause label on a failed item: Credentials, Permission, Network, Failed. Decides the row action: Credentials gets **Fix**, others **Retry**.
_Avoid_: Error type

**Parked failure**:
Operation that exhausted its **Retry budget** or failed non-transiently. Listed in Queue under **Needs Attention**, never in Running. Auto-resumes: network, provider, timeout and permission failures after a cooldown (five cycles, then a human); credentials when the account connects again. Permanent failures wait for the user.
_Avoid_: Blocked, dead job

**Stopping slot**:
AI slot held by a worker told to stop until teardown ends. A worker whose final outcome cannot be saved keeps its slot until storage is repaired and the app restarted.

### Queue and handoff

**Queue Item**:
One Review Queue entry bound to account, repository, PR, revision and trigger policy. Stable ID derives from those identities, never list position or mutable login. Every currently assigned Agent must finish before **Machine-cleared**; a new assignment inherits no result. Navigation never confirms, starts, publishes, approves or merges. Detail shows a status block (PR title, short state or failure label, Fix, Retry or Open) and a collapsed Details disclosure for the rest.
_Avoid_: Row, list position

**Ready for you**:
Queue group of PRs where the Agents reviewed the latest head, have nothing to add (no findings, held findings or open concerns) and did **not** approve, because they lack Approve permission: the human decides. A PR the acting account approved (confirmed receipt for the current head) is not Ready: it leaves the Queue and shows **Approved** in Reviewed (**Merged** once merged). A new head voids that approval. No Agent lane. Active and waiting jobs belong in Running (active plus queued only). Completed reviews stay Ready despite config drift or a disabled repository; they go **Stale** only on a new head or base.
_Avoid_: Agent lane

**Handoff**:
Queue entry asking a human to act. One current handoff per account/repository PR. Human comments and reviews stay on GitHub. Confirmed approval is not personal review; confirmed merged PRs are not actionable. Marker on every cleared unmerged PR, kept after confirmed auto-approval. Automation never asserts personal review done.
_Avoid_: Human approval

**Machine-cleared**:
All current assigned Agents signed off. Automated sign-off only; never human approval, GitHub approval, mergeability or checks guarantee.
_Avoid_: Approved, reviewed by human

**Ready for your final review**:
Queue state. All current assigned reviews signed off and enabled comment publication has a confirmed receipt. Operator still inspects the PR and decides on GitHub.
_Avoid_: Approved, mergeable

**Waiting for PR author**:
Queue state. Findings or questions were successfully published; author must respond or update. Sign-off summary alone never creates it.

**Needs your input**:
Queue state. Follow-up needs human judgment, findings remain local, or some findings are unpublished. Never implies the author saw unpublished feedback.
_Avoid_: Needs attention (that term is the failure group)

**Stale**:
Review superseded by a new head or base. Stale after publication is the same condition on a **Publication**: receipt kept, labeled stale, no publish against a different diff. Base move without new head needs explicit Review again.
_Avoid_: Outdated review

**Destination**:
Exact PR, Iteration or job identity addressed by a panel route, notification or provider link. Unavailable exact identity is a missing destination; never substitutes another item.

### Publication and feedback

**Publication**:
Host-owned posting of frozen validated findings. Findings map only to verified diff lines. One pending review with explicit commit SHA, submitted as `COMMENT`. Out-of-diff findings stay local. Sign-off ends with canonical ` PR Sniper`. Never approves or merges. Copilot credentials never publish.
_Avoid_: Posting, sync

**Native Mutation Owner**:
Single native writer for every comment, reply, approval and merge, serial and independent of **AI Capacity**. Pause cannot undo an accepted remote write. Approval and merge are native operations, never Agent tools.
_Avoid_: Agent tool, publisher

**Approval**:
One acting-account provider vote, not one per Agent. Cannot self-approve a GitHub PR or replace an existing vote. Pins `commit_id`. Approve alone never merges.
_Avoid_: Agent vote

**Merge Readiness**:
Conditions for Approve & Merge: confirmed current-revision approval from an eligible reviewer, green current-head checks, satisfied provider rules, conflict-free CLEAN, no unresolved threads, no required merge queue, provider-selected enabled merge method. Admin bypass is neither queried nor used. Absent or partial evidence blocks. Merge pins `expectedHeadOid`.
_Avoid_: Mergeable

**Unknown Publication Outcome**:
Publication or reply whose result is unconfirmed. Keeps original reconciliation controls; never shown as Done. Reconcile keeps original key and body and checks the full remote thread before any effect. An uncertain reply never authorizes another POST.
_Avoid_: Resend

**Owned Feedback**:
Feedback rooted in a comment PR Sniper published, verified by saved review, receipts, repository/account identity, commit and exact root body. Username match is insufficient. Records original owner. Unassigning keeps unresolved feedback and blocks clearance. Missing, tampered or incomplete observation is never resolved. **Agent resolve**: a Packet-mode primary may resolve only owned threads, through its `leave_comments` exit and a host `resolveReviewThread` write; never others' threads, never reopen. Legacy follow-ups never resolved.
_Avoid_: Own thread by author name, thread owner as responder

**Feedback clearance**:
Local reassessment of an owned concern needing rationale and verified source evidence, recorded by legacy reviews (history). Shown apart from provider closure; a packet exit may resolve owned threads only. Closure leaves a durable **Tombstone**; a new finding on a closed concern's file is **Held locally** for human judgment, never republished.
_Avoid_: Resolved

**Conversation Trigger**:
Eligible other-user comment on an admitted open PR, assessed by the Primary Agent alone, including unowned and non-mention threads. First observation is a baseline; history is context, not backlog. Key excludes responder role and later head. Comments between polls coalesce to the latest. PR Sniper signed replies and same-account comments never trigger.
_Avoid_: Backlog

**Follow-up**:
Legacy reply work on one external comment thread. Retired: the packet reads comments and answers through exit `reply_comments`. None admitted; unanalyzed records cancelled on startup, analyzed replies still publish and reconcile. Immutable target apart from current context. Outcomes: reply, **Quiet response** (nothing to say, never clears a concern), or human input required (no publishable body, pauses auto follow-ups for the thread). Reply posts only to a verified root, ends ` PR Sniper`, never resolves, approves or merges. Host revalidates before and after mutation.
_Avoid_: Reply job

**Acting-account mention**:
Standalone `@login` of the repository acting identity in another user's top-level comment. Routes one response to the current Primary Agent; never the Copilot identity or all Agents. Mention intent is keyed by provider, account, repository, PR and comment, not by primary, and belongs to its exact Iteration. Executed evidence is never replayed under another Agent. May admit an otherwise unadmitted PR.
_Avoid_: Mention (as generic trigger)

**Receipt**:
Minimal surviving record of a publication, action or operation: provider ownership IDs, root-body fingerprints, sticky human closures. Not an archive.

**Terminal Cleanup**:
Removal of PR detail after provider-confirmed closure or merge, established only by returned lifecycle fields of an explicit read. Absence, 404, failed read, disabled config or lost account never qualify. Keeps receipts and records of uncertain external writes. Reopen before cutover cancels; after cutover starts a new Iteration.
_Avoid_: Archive, purge

**Comment permission unverified**:
State when classic OAuth scope evidence (`repo` or `public_repo`) cannot be checked against repository visibility, or token is fine-grained. Scope is not publication authorization. No mutation probe.
