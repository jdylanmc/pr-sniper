# Review Queue

Reference for Queue states, evidence, local Agent reviews, retry/parking and notifications.

## States

- Handoff states: **Ready for your final review**, **Waiting for PR author**, **Needs your input**.
- External prerequisites: **Waiting on CI**, **Waiting on draft**, **Waiting on reviews** appear in Running as PR rows, outside AI occupancy and human handoff counts. Activity records **Wait** and the concrete prerequisite; no ready/failure notification.
- Distinct, never merged: queued/reviewing, pending publication, failed, stale. No user-facing Blocked state ([ADR-0007](adr/0007-retry-then-park-single-inbox.md)).
- Packet runs read the PR fresh when their turn comes. Head moved or PR closed since queueing: superseded, free (no attempt spent), retried shortly. A state that moves during the run: Toss, counted, retried through the budget, parks at 3.
- Machine sign-off is not approval. Ready for you = signed off and not approved by PR Sniper. A confirmed approval of the current head (`State::Approved`) leaves the Queue for Reviewed (**Approved**); a new head voids it. Unsettled or failed approval stays in Needs attention.
- Retry: transient failures retry with backoff, 3 failed attempts, then **park** (`MAX_FAILED_ATTEMPTS`, `RETRY_WINDOW_SECONDS` = 15 min in `src-tauri/src/monitoring.rs`). Credential, permission and permanent failures park immediately.
- Auto-resume: a parked Network/Provider/Timeout/RateLimited/Permission failure gets a fresh 3-attempt budget after a cooldown of 30m, 1h, 2h, 4h, 8h (`RESUME_COOLDOWNS_SECONDS`; `resume_cycle`, `resume_at` on the operation). After five cycles it waits for a human. Credential failures resume when the GitHub or Copilot account connects again. Explicit Retry starts a new operation (cycle 0): a poll failure retries the repository poll; a review restarts only if its own run failed with nothing published for that revision. Publication operations resume the same way (reconcile the original). Permanent failures never resume. Superseded, closed and merged work is never resumed.
- Parked failures live in Queue > **Needs attention**: cause chip, **Fix** (credentials → the failing Copilot or GitHub account) and **Retry**, **Retry all** pill (`retry_all_parked`; `Item.retry_all` marks the rows it re-arms: packet parks, failed review runs, parked polls, and publications unless paused or withdrawn; permanent, panic and provider-action failures stay parked). A clock + time shows the next automatic retry. Failed detail lists Cause, Attempts, Last attempt, Next retry, Agent, Revision, Account and the raw error. Running holds active + queued jobs and external-wait PR rows without AI slots.
- Teammates: teammate fail = PR-state fail. One Needs attention row per PR, chip = agent name + typed cause (`failed_teammate`; icons, no prose); detail adds Teammate row. Primary waits (`blocked_by`, Running chip = agent name + clock) until all teammates have report for same state. Good reports reused on retry. State move re-runs all. See [ADR-0021](adr/0021-review-packet.md).
- Packet exits (`Item.exit`, `Item.progress`, typed `failure_cause`):

  | Exit / host phase                                                             | Queue                                          | Chip                                                              |
  | ----------------------------------------------------------------------------- | ---------------------------------------------- | ----------------------------------------------------------------- |
  | `ready_for_human`                                                             | Ready for you                                  | —                                                                 |
  | `wait`                                                                        | out of human Queue; Running PR row, no AI slot | **Waiting on CI** / **Waiting on draft** / **Waiting on reviews** |
  | `approve_pull_request`, `approve_and_merge`                                   | Ready for you                                  | **Approving** → **Merging**                                       |
  | approval confirmed / merged                                                   | Reviewed (Approved / Merged)                   | —                                                                 |
  | `leave_comments` writes running (exit_writes Planned/Verified, batch pending) | out of Queue (`awaiting_publication`)          | **Publishing**                                                    |
  | writes confirmed + consumed                                                   | out of Queue (`waiting_for_author`)            | —                                                                 |
  | write Tossed, budget left                                                     | re-queued (`queued`)                           | —                                                                 |
  | toss budget spent (`error_count` ≥ 3)                                         | Needs attention                                | **Tossed**, Retry                                                 |
  | write Failed after an effect                                                  | Needs attention                                | **Write failed**, Retry                                           |
  | `panic_exit`                                                                  | Needs attention                                | **Panic** + cooldown clock                                        |

  Retry on Tossed / Write failed: `retry_packet_state` resets `error_count`, re-queues the review. Panic Retry restarts the run.

- Activity (packet runs): **Read** N of M, exit row (icon + exit, counts New / Replies / Resolved, Approve / Merge, Write phase, Progress), **Packet** row (State id, Schema, Tools, Offered, Denied, Hash), teammate **Report** rows (agent, finding count; tap = `file:line` + nano). `ready_for_human` summary shows local, never posted.
- Stale only on new head or base. Completed reviews stay Ready despite config drift or a disabled repo. Merged/closed PRs move to Reviewed; tracked PRs of disabled repos keep a light lifecycle refresh.
- Queue identity is derived from stored tracking, not position.

## Evidence and actions

- Evidence: ordered changed-file guide, findings, conversations, acting account, receipts, mutation recovery. Human comments and reviews stay on GitHub.
- Item detail: **Activity** rows under the status block (Reviewed · files, Findings, Commented · inline, Replied, Final, Retried · Nx, Approved · head, Merged); tap expands key/value facts (IDs copy on tap), GitHub arrow opens the PR. Collapsed Details: Agent / Result / Provider key/value groups. CI is not in the snapshot, so no CI row.
- Gated start, confirm and retry controls. Check Now lives in the tray menu.
- Selection persists in private `state/queue-selection.json` (isolated profiles too). Unavailable identity: missing-destination message.
- Open-PR and file links resolve from saved native queue identity. File links open the anchor in the current PR diff (deleted/renamed ok). They are not the immutable saved review: compare the displayed reviewed head with GitHub's current revision, browser account and merge requirements.
- One handoff per account/repo PR. Confirmed approval is not a personal review. Confirmed-merged PRs are not actionable.

## Local Agent reviews

- Agent = Copilot account + returned model + prompt + ordered 0..many Doctrines, assigned per repo. **Agent reviews** lists detected revisions. Pre-review detections stay as history until the next scan.
- ONE review per head SHA ([ADR-0008](adr/0008-one-review-per-head-sha.md)). A new revision permits another review. Same revision: Agent only replies to new comments. Agent context = description + changed files + all comments + prior feedback.
- Every run is packet mode ([review-packet.md](review-packet.md)). Discarded batch: no automatic re-review (once-per-lens removed).
- Legacy runs (JSON decision) load as history. Unfinished ones are cancelled on startup (`Legacy review retired.`); **Retry review** starts a packet run.
- Requests persist before capacity wait. Unexecuted initial requests may wait over 15 min and still get a first budget.
- **Cancel review** stops inference; **Retry review** is explicit. Changing account, Agent, prompt, Doctrine, repo or effective inputs invalidates affected in-flight work.
- Pause: the panel header **Monitoring** switch (Pause/Resume automation) halts polling, AI and writes. No Settings pause card. `automation_snapshot` returns saved pause/capacity plus occupied, stopping, waiting work. `set_automation_paused` persists the gate and signals workers. State: `state/automation.json`.
- Reserve path: `capacity::Coordinator::reserve` (`src-tauri/src/capacity.rs`).
- Keychain: GitHub and Copilot tokens in separate account-addressed services. Never in config, state or diagnostics. See [ADR-0001](adr/0001-macos-foundation.md).
- Green Copilot check = sign-in only. No seat, subscription or inference proof.
- Review tools: host-owned read-only investigation tools plus exits, not an OS sandbox. Reads are paged in byte chunks and never truncated; coverage ("N of M files read") is recorded on the result, never enforced ([ADR-0020](adr/0020-constrained-host-owned-review-tools.md)). Older results may carry `byte_coverage` or an `incomplete_coverage` record; both still load.
- Rate limits: backpressure, not failure. `state/backpressure.json` gates the provider account; the row shows a clock and the local resume time ([ADR-0013](adr/0013-shared-ai-capacity.md)).

## Notifications

macOS. Windows: [windows-notifications.md](windows-notifications.md). Needs the packaged app; `npm run bundle` applies the local ad-hoc signature that binds `Info.plist` to the app identifier.

- Opt in: Settings > Preferences > Notifications. Saves immediately, separate from the draft. Adding an account or enabling reviews never opts in. Opt-in is from now forward: states that exist at opt-in, or arose while off, never notify. Turning on sends one confirmation (opens Settings).
- Notifies: confirmation, human input, ready for final review, failures, approved, merged, final review cleared. Not: routine queued/reviewing, author waits.
- Types: while on, each type has its own switch (default on, including older profiles). A type turned back on never replays what happened while it was off.
- Click opens the exact saved queue item, or Settings for a scheduling/account failure with no PR.
- Grouped by opaque repo/account identity. Disabling stops new submissions and marks unsent ones not sent; an accepted request may still show.
- Copy: title = state word, then `repo#123`; body = PR title, then at most one reason. Repo and PR title in banners are owner-approved (2026-10-23). Rendered at send time from the live queue item; `state/notifications.json` keeps no PR text. Title ≤ 48 chars, body ≤ 90; longer text ends in `…`. Line breaks, control, bidi-control and XML-invalid (U+FFFE/U+FFFF) characters are stripped. The live item matches by id or legacy alias. A Failure notice stores the item's typed failure cause when sent (`failure_cause`, no text); banner and history render its reason from that alone. Item gone: state word alone, body **Open the queue**.

| Type                 | Title                | Body                                                                                                   | Badge          |
| -------------------- | -------------------- | ------------------------------------------------------------------------------------------------------ | -------------- |
| Confirmation         | Confirm · repo#123   | PR title                                                                                               | Amber `!`      |
| Human input          | Needs you · repo#123 | PR title                                                                                               | Amber `!`      |
| Ready                | Ready · repo#123     | PR title · Needs your review                                                                           | Blue crosshair |
| Final review cleared | Cleared · repo#123   | PR title · Needs your review                                                                           | Blue crosshair |
| Approved             | Approved · repo#123  | PR title · Needs your review                                                                           | Green check    |
| Merged               | Merged · repo#123    | PR title                                                                                               | Green check    |
| Failure              | Failed · repo#123    | PR title [· Publish failed / Publish unconfirmed / Review crashed / Retries exhausted / Review failed] | Red `×`        |
| Failure, no PR       | Failed · Repo check  | Open Settings                                                                                          | Red `×`        |
| Opt-in confirmation  | Notifications on     | PR alerts show here                                                                                    | Blue crosshair |

- Badges: `src-tauri/icons/notifications/*.png` (256 px RGBA, white rim for dark and light), embedded in the binary; regenerate with `swift scripts/generate-notification-badges.swift`. Shape and title word carry the state, not color alone. macOS: `macos/*.png` variant (disc padded to half the canvas, 64 px inset, so the thumbnail stays smaller than the app icon); per-request copy in `<data root>/notification-badges/` attached as the thumbnail (the app icon stays the main icon); UserNotifications moves it into its store, leftovers are deleted after the request. Windows: `appLogoOverride` from a stable `<data root>/notification-badges/<state>.png`. A badge that cannot be written is skipped; the notice still sends.
- `state/notifications.json`: profile-scoped request identities, observed transitions, send intent, outcomes, destinations. Same transition never repeats on poll/restart; a new cause or re-entry may notify.
- Review Queue > Notifications states: queued, submitting, accepted-unconfirmed, unknown, denied, failed, no-longer-current. Acceptance is not banner display; Focus may suppress. Missing or foreign-profile destination fails visibly. Denied/failed events stay in history, not replayed after permission change.
- Settings card: the switch only while healthy. A problem shows one row: **Blocked by macOS/Windows** (denied or disabled) or **Muted in macOS** (banners and Notification Center both off). Unknown channels are never shown as muted.
- Denied: System Settings > Notifications > PR Sniper.
- Isolated profiles still need per-app macOS authorization. Procedure: [tests/macos-notifications.md](../tests/macos-notifications.md).
