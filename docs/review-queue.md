# Review Queue

Reference for Queue states, evidence, local Agent reviews, retry/parking and notifications.

## States

- Handoff states: **Ready for your final review**, **Waiting for PR author**, **Needs your input**.
- Distinct, never merged: queued/reviewing, pending publication, failed, stale. No user-facing Blocked state ([ADR-0007](adr/0007-retry-then-park-single-inbox.md)).
- Machine sign-off is not approval. Human decides approve and merge.
- Retry: transient failures retry with backoff, 3 failed attempts, then **park** (`MAX_FAILED_ATTEMPTS`, `RETRY_WINDOW_SECONDS` = 15 min in `src-tauri/src/monitoring.rs`). Credential, permission and permanent failures park immediately.
- Auto-resume: a parked Network/Provider/Timeout/RateLimited/Permission failure gets a fresh 3-attempt budget after a cooldown of 30m, 1h, 2h, 4h, 8h (`RESUME_COOLDOWNS_SECONDS`; `resume_cycle`, `resume_at` on the operation). After five cycles it waits for a human. Credential failures resume when the GitHub or Copilot account connects again. Explicit Retry starts a new operation (cycle 0): a poll failure retries the repository poll; a review restarts only if its own run failed with nothing published for that revision. Publication operations resume the same way (reconcile the original). Permanent failures never resume. Superseded, closed and merged work is never resumed.
- Parked failures live in Queue > **Needs attention**: cause chip, **Fix** (credentials → Accounts) or **Retry**, **Retry all** pill. A clock + time shows the next automatic retry. Failed detail lists Cause, Attempts, Last attempt, Next retry, Agent, Revision, Account and the raw error. Running holds active + queued only.
- Stale only on new head or base. Completed reviews stay Ready despite config drift or a disabled repo. Merged/closed PRs move to Reviewed; tracked PRs of disabled repos keep a light lifecycle refresh.
- Queue identity is derived from stored tracking, not position.

## Evidence and actions

- Evidence: ordered changed-file guide, findings, conversations, acting account, receipts, mutation recovery. Human comments and reviews stay on GitHub.
- Gated start, confirm and retry controls. **Open Settings**, **Open Diagnostics**, schedule recovery stay available.
- Selection persists in private `state/queue-selection.json` (isolated profiles too). Unavailable identity: missing-destination message.
- Open-PR and file links resolve from saved native queue identity. File links open the anchor in the current PR diff (deleted/renamed ok). They are not the immutable saved review: compare the displayed reviewed head with GitHub's current revision, browser account and merge requirements.
- One handoff per account/repo PR. Confirmed approval is not a personal review. Confirmed-merged PRs are not actionable.

## Local Agent reviews

- Agent = Copilot account + returned model + prompt + ordered 0..many Doctrines, assigned per repo. **Agent reviews** lists detected revisions. Pre-review detections stay as history until the next scan.
- ONE review per head SHA ([ADR-0008](adr/0008-one-review-per-head-sha.md)). A new revision permits another review. Same revision: Agent only replies to new comments. Agent context = description + changed files + all comments + prior feedback.
- Requests persist before capacity wait. Unexecuted initial requests may wait over 15 min and still get a first budget.
- **Cancel review** stops inference; **Retry review** is explicit. Changing account, Agent, prompt, Doctrine, repo or effective inputs invalidates affected in-flight work.
- Pause: **Pause automation** (Queue or Settings) halts polling, AI and writes. `automation_snapshot` returns saved pause/capacity plus occupied, stopping, waiting work. `set_automation_paused` persists the gate and signals workers. State: `state/automation.json`.
- Reserve path: `capacity::Coordinator::reserve` (`src-tauri/src/capacity.rs`).
- Keychain: GitHub and Copilot tokens in separate account-addressed services. Never in config, state or diagnostics. See [ADR-0001](adr/0001-macos-foundation.md).
- Green Copilot check = sign-in only. No seat, subscription or inference proof.
- Review tools: three host-owned read-only tools, not an OS sandbox.

## Notifications

macOS. Windows: [windows-notifications.md](windows-notifications.md). Needs the packaged app; `npm run bundle` applies the local ad-hoc signature that binds `Info.plist` to the app identifier.

- Opt in: Settings > Preferences > Notifications. Saves immediately, separate from the draft. Adding an account or enabling reviews never opts in. Existing attention states notify once after opt-in.
- Notifies: confirmation, human input, ready for final review, failures. Not: routine queued/reviewing, author waits.
- Click opens the exact saved queue item, or Settings for a scheduling/account failure with no PR.
- Banner text is generic. Grouped by opaque repo/account identity. Disabling stops new submissions; an accepted request may still show.
- `state/notifications.json`: profile-scoped request identities, observed transitions, send intent, outcomes, destinations. Same transition never repeats on poll/restart; a new cause or re-entry may notify.
- Review Queue > Notifications states: queued, submitting, accepted-unconfirmed, unknown, denied, failed, no-longer-current. Acceptance is not banner display; Focus may suppress. Missing or foreign-profile destination fails visibly. Denied/failed events stay in history, not replayed after permission change.
- **Send test notification**: chosen queue item or Settings; runs no review. Denied: System Settings > Notifications > PR Sniper.
- Isolated profiles still need per-app macOS authorization. Procedure: [tests/macos-notifications.md](../tests/macos-notifications.md).
