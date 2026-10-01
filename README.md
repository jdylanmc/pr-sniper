# PR Sniper

A macOS menu-bar application for human-owned pull request review. Built with
Tauri 2, Rust and vanilla TypeScript. Left-click the tray crosshair for one
retained panel with **Queue**, **Running**, **Reviewed** and **Settings**.
The secondary/right-click menu retains **Status**, **Review Queue**,
**Check Now**, **Settings**, **Diagnostics** and **Quit PR Sniper**.

Settings can explicitly verify a configured GitHub connection and read complete
pull-request metadata. The active tray process also polls enabled, account-bound
GitHub repositories and persists eligible revisions in the Review Queue.
Assigned Copilot Agents can review each admitted revision manually or through
the automatic-start gate, with validated results kept locally. A separate
publication gate can submit one revision-bound GitHub `COMMENT` review.
New external comments in verified owned threads can receive an evidence-backed
follow-up, or remain quiet/wait for human judgment.
Settings manages independent Copilot
AI accounts with browser sign-in and per-Agent account/model selection; see
[Copilot Settings](docs/copilot-settings.md). Product scope lives in the
[approved specification](docs/agent/specs/pr-sniper-mvp.nano.md), not this
implementation summary.

## vNext design prototype

The [standalone vNext POC](prototypes/v2/README.md) includes configured and
fresh-install demos, guided Genie setup, and mock review workflows. Open
`prototypes/v2/index.html` in a browser; no app build is needed. Its screenshots,
local checks, and limitations are documented alongside it. This is design
evidence, not production application behavior.

## Install on macOS

For Apple Silicon Macs running macOS 13.5 or later:

```sh
brew install --cask jdylanmc/pr-sniper/pr-sniper
```

The [v0.1.1 release](https://github.com/jdylanmc/pr-sniper/releases/tag/v0.1.1)
is Developer ID-signed, notarized and stapled. The published cask references its
verified checksum. Quit PR Sniper before running `brew upgrade --cask pr-sniper`;
ordinary upgrades/uninstall preserve saved application state and credentials.
Homebrew owns updates; no in-app updater is enabled.

## Develop on macOS

Requirements: macOS 13.5+, Xcode Command Line Tools (or full Xcode), Node
24.20.0 and Rust 1.98.1 via rustup. `.node-version`, `rust-toolchain.toml`,
`package-lock.json` and `src-tauri/Cargo.lock` pin the baseline.
Install prerequisites yourself using their official installers; these commands
do not install global tools.

The application's macOS 13.5 minimum matches the bundled official Copilot
runtime's deployment target.

```sh
npm ci
npm run tauri -- dev
```

If Homebrew's rustup is present but `cargo`/`rustc` are not on PATH, add the
installed toolchain's `bin` directory to your current shell. Do not confuse
missing shell shims with a missing Rust installation.

No main window opens at startup. Click the crosshair in the macOS menu bar.
Outside click, Escape, the panel's Close button and native close hide the same
panel; background work continues. **Quit PR Sniper** ends the process.
Navigation, editor drafts, scroll and keyboard focus survive panel dismissal
and destination changes. Back returns to the originating list and row; opening
another PR/job replaces the single detail layer. Native notification and legacy
Settings/Diagnostics entry points use retained routes, never a webview reload.
Unavailable exact PR/iteration/job identities show a missing destination, not
another item. Existing profile-scoped PR selection survives a restart; unsaved
editor drafts are in-process state, not restart persistence.

Placement uses the tray's monitor work area and physical coordinates, clamping
the approximately 400px-wide panel for small screens and mixed scaling. Native
folder selection holds focus dismissal until it returns. External browser
sign-in may hide the panel; reopening retains the connecting flow and draft.
Running shows the native shared-capacity jobs (including stopping work);
Reviewed exposes existing completed/terminal evidence in the current projection,
not a new paged history or purge backend. Status retains schedule health,
notification history and recovery; Diagnostics remains redacted.

The [native acceptance procedure](tests/macos-acceptance.md) and compiled
test-owned smoke harness cover the actual window boundary separately from
browser/geometry tests. Bundle compilation alone is not native acceptance.
The local frontend server is only for development; terminate the development
command too when finished.

For isolated runs, use an absolute application-data directory:

```sh
PR_SNIPER_DATA_DIR="$(mktemp -d)" \
PR_SNIPER_KEYCHAIN_SERVICE="com.jdylanmc.pr-sniper.tests.dev-$(uuidgen)" \
npm run tauri -- dev
```

That override never changes login items and disables the startup checkbox.
Launch-at-login registration requires an explicit Settings change in a normal
installed-app run. The checkbox shows your saved request, not effective macOS
state. The separate registration status validates the owned plist and current
executable; even a valid registration may be disabled by macOS Login Items.
Startup only inspects registration: it does not enable, disable or reapply the
saved preference.

## Check and bundle

```sh
npm run format:check
npm run lint
npm test
npm run bundle
```

`npm run format` formats owned application sources. `npm run build` checks and
builds the frontend. CI executes the checks and builds a real macOS `.app`;
native interactive proof is separate from compile and filesystem tests.
See [native acceptance](tests/macos-acceptance.md) for reproducible
install/launch/close/quit and scoped visual checks, including capability limits.

The bundle is `src-tauri/target/release/bundle/macos/PR Sniper.app`.
For a local user installation, copy it with Finder to `~/Applications` (create
that directory if needed), then open it. Quit any earlier PR Sniper instance
first. Local/CI bundles are ad-hoc signed with their bundle identity so macOS can
authorize notifications. This does not use a Developer ID certificate and is
not a notarized distribution:
Gatekeeper may require explicit approval under Privacy & Security for a
downloaded CI artifact. Do not disable Gatekeeper globally. The separate
[macOS release workflow](docs/releases.md) adds Developer ID signing,
notarization and a Homebrew cask for explicitly tagged Apple Silicon releases.
A local/CI ad-hoc bundle is not the notarized distribution.

## Windows native convergence

[Windows native application checks](.github/workflows/windows.yml) runs on
pull requests and `main` pushes using a GitHub-hosted Windows runner. It restores
locked npm dependencies, runs the production TypeScript/Vite build and tests
portable release logic plus both platforms' workflow contracts, then compiles
and tests the full Rust application, offline Copilot runtime and production
browser UI/Store bridge on Windows. It builds a standalone executable and uploads
only that executable plus version/commit/hash metadata. It needs no signing,
publishing or provider credentials.
macOS CI and signed release gates remain unchanged.

To reproduce these checks on Windows, install Node 24.20.0 (the version in
`.node-version`) and Python 3.13, with `node`, `npm` and `python` on PATH, then
run in PowerShell:

```powershell
npm ci
npm run build
npm run test:release:windows
```

The Windows release runner selects `test_release.py`; macOS retains the complete
release suite, including the macOS-path signing tests and hosted native Keychain
check. Release tests use fixtures and mocked provider/native operations, not
real signing or publication.

**CI is not interactive native acceptance or a signed release.**
The real credential/runtime, tray/login and notification adapters are integrated.
The normal Windows workflow checks the actual app; the focused foundation
harness remains available for targeted adapter checks, not a replacement host.
See [Windows prerequisites and checks](docs/windows-development.md) and
[the persistence boundary](docs/adr/0004-windows-persistence-foundations.md)
for MSVC/Rust/WebView2 setup, native test commands, permission behavior and
native acceptance boundaries. Native portability
([#58](https://github.com/jdylanmc/pr-sniper/issues/58),
[#59](https://github.com/jdylanmc/pr-sniper/issues/59),
[#60](https://github.com/jdylanmc/pr-sniper/issues/60)) and full native checks
with a launch-verified Windows artifact
([#61](https://github.com/jdylanmc/pr-sniper/issues/61)) remain separate.
Rust/MSVC is required for native foundation checks, but not the three
frontend/shared commands above. WebView2 is needed for eventual app launch;
Chocolatey distribution is a separate follow-up.

`npm run build:windows` builds
`src-tauri\target\release\pr-sniper.exe` with embedded production assets and no
Vite server. It leaves the macOS `npm run bundle` command unchanged.
See the [Windows application acceptance procedure](docs/windows-development.md#windows-application-acceptance)
before claiming tray/window/quit or login-launch proof, and the
[artifact retrieval procedure](docs/windows-development.md#windows-ci-artifacts)
for exact-commit metadata and SHA-256 verification. Windows startup uses
only the application's named current-user Run value; registration does not
override a disabled entry in Windows Startup Apps.

## Local data and diagnostics

The default data root is
`~/Library/Application Support/com.jdylanmc.pr-sniper/` on macOS and
`%LOCALAPPDATA%\com.jdylanmc.pr-sniper` on Windows.
`config/settings.json` stores nonsecret repository configuration and the startup
preference. Settings accepts `owner/repository` or an HTTPS `github.com` URL,
normalizes case and clone suffixes, and prevents duplicates. Rename, disable,
re-enable and confirmed removal operate on stable local repository identities;
they do not contact GitHub. There is no application-defined repository-count
limit. Failed configuration writes are visible, not reported as successful saves.
If configuration commits but recording diagnostics fails, Settings shows the
committed state with a separate warning, rather than reporting a failed save.

Settings retains the compact sidebar design, with **Integrations**, **Doctrines**,
**Agents**, and **Preferences**. In Integrations, choose a local root folder to discover GitHub remotes and select
repositories with searchable checkboxes, or add a repository manually. Discovery
reads bounded Git metadata only: no Git commands, hooks, includes or repository
code execute. It skips nested symlinks, stops descending at repositories, and
reports unreadable metadata and depth/resource limits. Linked worktrees and
ambiguous remotes can be added manually. Saved roots are not automatically
scanned. Multiple clones of the same GitHub repository share one monitoring
checkbox and saved identity; every discovered clone path remains searchable and
available in the row's local-clone details.

**Save agent**, **Save doctrine**, and **Save repository** persist only that
resource immediately; assignment saves commit their owning repository.
**Save preferences** saves global preferences without committing repository
drafts. **Reset changes** discards remaining unsaved changes, not successful
resource saves. Compare-and-save rejects conflicting edits to the same resource
without rejecting unrelated saves. Failed writes retain the editor and last
valid state; **Discard draft and reload** explicitly replaces the draft.
Controls and dialog dismissal are locked during saves. Startup registration is
separate and changes immediately on explicit choice in **Preferences**.
AI and repository connections also save immediately, independently of the
Settings draft. Closing a repository editor cancels its pending draft lookup;
delayed replies cannot restore dismissed edits.
Focus refreshes preserve open repository and preset forms. Dialogs retain focus,
Escape/Close and background isolation when native dialog APIs are unavailable;
viewport sizing also falls back for older WebKit versions. Production JavaScript
syntax and CSS optimization target Safari 15, preserving viewport fallbacks
through minification. Browser tests inspect the emitted stylesheet and exercise
its layout with unsupported viewport units and dialog APIs. The app requires
macOS 13.5 or later; build targets do not polyfill runtime APIs, and these simulations
are not native acceptance evidence.

Repository **Settings** assigns reusable Agents with independent comment,
Approve and Merge choices, and resolves watched people using the repository's
explicit GitHub account. Existing global defaults and overrides remain
preserved in storage. Review start and comment publication have separate
automatic/manual gates. A sole assignment is primary automatically; multiple
assignments permit one explicit primary or none. Primary selection never opts
into actions. Legacy inert `approve` flags remain preserved but are not grants;
new choices live in assignment `actions`. Merge is effective only for the
primary, and no primary means no effective approval or merge. Opted-in provider
actions require aggregate clearance and the primary final-review path below.

**Preferences** exposes one global five-field cron expression, default
`*/15 * * * *` in `UTC`, an expression helper, explicit IANA time-zone semantics,
and saved AI capacity (default four). Legacy interval choices and scoped
schedules remain readable, without scoped polling editors. An incompatible
legacy global interval remains visible as a setup issue until explicitly
replaced, never silently converted. The global scheduler consumes this saved
cron; the shared AI dispatcher consumes the positive capacity independently.

**Doctrines** manages plain-text review principles. A fresh configuration
persists all 23 bundled doctrines on first load, before any Settings tab is
visited. The canonical local doctrine documents are embedded in the app; no
download or local source checkout is needed at runtime. Edits, additions and
deletions (including deleting the whole library) survive restart. Existing
libraries are never topped up or replaced. Legacy settings without a doctrine
field remain empty, since the older format also omitted deliberately empty
libraries; subsequent saves record an explicit empty list.

**Agents** selects a Copilot
account and a real model returned by that account, alongside an optional
ordered list of zero or more doctrines, prompt and signature. Existing
single-doctrine selections remain compatible; an explicit empty list means
none. Multiple bodies compose in selected order, independent of library order.
Renaming a doctrine updates references atomically; referenced doctrines and
Agents cannot be deleted until their references are explicitly repaired.
Provider and model are distinct. Old Agents
without AI account bindings remain unconfigured until explicitly updated.
Disconnecting an AI account preserves dependent Agents and assignments.
Never put credentials in doctrines, prompts or other configuration fields.

Review details expose planned configuration before execution and captured
Agent/model/account, prompt, doctrine bodies, repository and assignment
authority afterward. Captured authority is evidence, not a current grant.
Library edits and restart never rewrite completed snapshots; legacy missing
fields are labeled unavailable rather than reconstructed from current settings.
Interrupted jobs retain their prior execution configuration and separately show
the current plan, which is revalidated before retry.

Settings and future setup flows share `saved_resources`, `validate_resource`
and `save_resource` over the existing Store. `ResourceEdit` addresses one Agent,
doctrine, repository or global-preference resource with its expected value;
`value: null` deletes, and `expected: null` creates. Readiness describes saved
configuration only: account verification, current model access, monitoring
scope, trust and provider capability remain independent gates. No setup wizard
or monitoring activation is added by this surface.

Saving validates the effective policy on the Rust storage boundary, including
positive whole-minute intervals, five-field cron syntax, IANA time zones,
unique positive account IDs, nonempty prompts/selectors and supported adapter
and selector shapes. Recognized GitHub token patterns are rejected without
echoing them. This is not a general-purpose secret detector: all configuration
must remain nonsecret. Invalid input does not replace the last valid saved
configuration; malformed or unreadable files are reported rather than reset.
`state/diagnostics.jsonl` records timestamped, fixed-schema host events, capped at
256 KiB plus one rotated file. **Settings > Preferences**
opens
an in-app reader, not an arbitrary filesystem or shell interface.
Invalid settings are reported rather than silently reset or overwritten.

## Scheduled monitoring

### Human-handoff inbox

**Review Queue** puts PRs ready for your final review and items needing your
input ahead of routine work. It combines the current assigned reviews,
publication receipts, owned-thread follow-ups and monitoring health into one
account/repository/revision-bound item. Every currently assigned Agent must
finish before the item can be machine-cleared; adding an assignment does not
inherit another Agent's result.

- **Ready for your final review:** all current assigned reviews have signed off,
  and enabled comment publication has a confirmed completed receipt. An
  intentionally review-only assignment needs no publication. Automated review
  completed; you still inspect the current PR and decide whether to merge on
  GitHub. This is not GitHub approval, a mergeability/checks guarantee, or evidence
  that a human personally reviewed it.
- **Waiting for PR author:** review findings or questions were successfully
  published. The author needs to respond or update the PR. A machine-sign-off
  summary alone does not create an author wait.
- **Needs your input:** a follow-up requires human judgment, findings remain
  local, or some findings could not be published. PR Sniper does not imply the
  author saw unpublished feedback.
- Confirmation, queued/reviewing, pending publication, blocked, failed and stale
  states remain distinct. A local sign-off never overrides a lost response,
  rejected publication, failed final verification, or current monitoring failure.
  Old published revisions retain a **Stale after publication** warning.
  Polls retain the observed target base as well as the head; a moved base also
  invalidates readiness. Older saved detections wait for a successful poll to
  establish that base before becoming ready.

**Evidence and actions** opens that exact item's complete ordered file guide,
findings, conversations, acting account, publication receipts and gated
start/confirmation/retry controls. **Open Settings**, **Open Diagnostics** and
schedule recovery remain available. The selected item survives reopening and
restart through private native `state/queue-selection.json` storage, including
isolated app profiles; unavailable identities produce a visible
missing-destination message, never selection of a different PR. Stable queue IDs
are derived from persisted provider, account, local/remote repository, PR,
revision and trigger-policy identities, not list positions or mutable logins.

**Open PR on GitHub** and each guide's file link resolve against saved native
queue identity. File links open the matching file anchor in GitHub's current PR
diff, including deleted or renamed files; they do not pretend that the live diff
is the immutable saved review. Check the displayed reviewed head against GitHub's
current revision, browser account and merge requirements. Navigation never
confirms a start, publishes comments, approves or merges.

### Native macOS notifications

In **Settings > Preferences > Notifications**, explicitly opt in to notifications.
This requests real macOS authorization and saves immediately, separately from
the Settings draft. Notifications default off; adding a GitHub account or
enabling review execution does not opt in. Existing attention states become
eligible once on. Use the packaged `.app`; a development web server is not a
native notification host.
The bundle signature must bind Info.plist and match the application identifier;
the compiler's executable-only signature is not sufficient. `npm run bundle`
supplies the required local ad-hoc signature without a signing certificate.

PR Sniper notifies the operator about confirmation, human input, readiness for
final review and failures. Routine queued/reviewing work and author waits do not
notify. The queue's authoritative state determines the category: local sign-off
cannot erase publication failures or stale evidence. A click opens the exact
saved queue item, or Settings for a scheduling/account failure without a PR. It
never confirms, starts, publishes, approves or merges anything.

Banner text is generic: no PR title, repository name, author name, code or
provider error text. Requests use the PR Sniper bundle identity and ordinary
UserNotifications alerts, grouped by an opaque repository/account identity.
They do not request critical or time-sensitive authorization or bypass Focus.
macOS authorization is per application, even when testing an isolated profile.

Private `state/notifications.json` retains profile-scoped request identities,
observed transitions, send intent, outcomes and exact destinations. Unchanged
polls/restarts do not repeat a transition. A genuinely new cause or re-entry after
leaving an attention state can notify again. Send intent is saved before the OS
request; interruption or a lost callback becomes **Outcome unknown**, never a
blind resend. Turning notifications off prevents new submissions; an already
accepted request may still appear. Events known denied or failed also remain in
history rather than being automatically replayed after permission changes.

**Review Queue > Notifications** distinguishes queued, submitting, accepted but
unconfirmed, unknown, denied, failed and no-longer-current requests. OS acceptance
does not prove a banner appeared: Focus or notification settings may suppress
it. **Destination opened** records navigation, not human acknowledgment, review,
approval or merge. Missing/foreign-profile destinations fail visibly without
opening a different PR. Native activation, acceptance, failure and navigation
also have fixed-schema diagnostics events.

**Send test notification** uses the chosen exact queue item or Settings, without
running a review or publishing anything. For denial, check **System Settings >
Notifications > PR Sniper**. Notification categories, per-repository preferences,
Windows support and update notices from the broad historical issue remain
outside the approved macOS P11 slice.
See [native notification acceptance](tests/macos-notifications.md) for isolated
OS delivery/click-through verification and its evidence limits.

### Polling and detection

While the menu-bar process is active, one global five-field cron schedule scans
enabled, scope-confirmed repositories in the saved IANA time zone. Each scan
captures its repository assignments. One read per account/repository binding
fans out to individual Agent jobs; assignment timers are not used. Cron times
skipped by a spring daylight-saving jump run at the first valid local time;
repeated fall-back times run once at their first occurrence. Sleep or missed
ticks cause one check, not a catch-up burst. **Check Now** coalesces one pending
global scan; repeated requests during a read coalesce. Bindings addressing the
same remote repository drain serially. Check Now never bypasses Retry-After or
an existing retry budget.

Each repository poll persists its provider/account/repository/policy identity,
attempt count and 15-minute retry deadline before the provider read begins.
Timeout, rate-limit, network and provider-server failures receive at most three
automatic retries with bounded backoff. Restart preserves the same operation
and budget; interruption after the deadline and non-retryable failures require
an explicit **Retry** from Review Queue. Poll operations record no attempted
mutation or provider receipt because polling does not
publish comments.

An enabled, account-bound repository does not begin new detection until its
monitoring scope is explicitly confirmed in Settings. The preview reads the
real matching open pull requests, shows the count before confirmation, selects
no existing pull requests by default, and offers **New pull requests only** or
an explicitly selected subset of existing pull requests plus new pull requests.
Scope confirmation saves immediately as a separate native operation and requires
relevant repository/filter drafts to be saved first. Existing configured
repositories require this confirmation after upgrade; their queue history is
retained. The native activation record
stores the account/repository binding, effective author/reviewer filter, latest
all-state pull-request number, immutable initial heads, observed heads and
durably admitted heads. Missing, stale or invalid activation blocks timer and
**Check Now** detection rather than enabling a backlog.

Filter-only edits preserve the confirmed creation watermark, baseline heads,
initial selection and admitted heads; every poll applies the current effective
author/reviewer filter for new admission. An unchanged excluded author-matched
head remains excluded when a filter widens, while its later matching head
qualifies. An explicit reviewer request can admit an older/unwatched PR despite
that initial backlog boundary. Preview/apply and
already-running polls still pin the filter they started with and reject stale
results.

Polling reads paginated open pull-request metadata through the repository's
connected OAuth account. A populated watched-author filter or a request for the
signed-in account as reviewer admits a non-draft revision. An empty effective
watched-author filter matches all authors only after scope activation; it does
not establish trust. Reviewer-only, all-author, fork and otherwise untrusted
work waits for explicit confirmation. Admission then remains sticky after
watchlist/reviewer removal, without granting trust or bypassing current account,
repository, start or publication gates. Repeated scans reuse each assignment's
normal job for the same PR iteration, regardless of later filter changes. Adding
an Agent creates its missing job at the next scan without repeating completed
unchanged passes. A new head supersedes old work; verified reopening creates a
new iteration even at the same head.

The complete open listing is followed by explicit provider reads for active,
admitted PRs absent from it. Only returned lifecycle
fields establish closure/merge; absence, 404, failed or incomplete reads are not
terminal evidence. Closed/merged iterations remain visible as provider-confirmed
history while later open listings can discover reopening, without refetching
every terminal PR's details. Poll pages use stable creation
order. Schedule health identifies each account/repository read and retains
pending, success and failure state.

`state/queue.json` atomically stores tracked lifecycle, iterations, immutable
normal-work identity/admission cause, queue order and per-Agent/PR pass ordinal.
Legacy arrays remain readable. Review/publication keys, receipts and old queue
destinations survive adoption; embedded originating reviews prevent replay when
an older queue/review file is missing. Completed execution snapshots are never
rewritten by scans. Reply work has its own ordinal and shares the durable queue
order allocator; retries retain their separate operation attempt counts.
Detection never
clones a repository, starts an agent, executes repository code, mutates GitHub
or publishes a review.

## Local Copilot reviews

Configure an Agent's Copilot account, returned model, prompt and optional
doctrine, then assign it to a repository. **Review Queue > Agent reviews**
shows each assignment's detected revisions. Existing pre-review detections
remain history until the next global scan; opening the queue does not
silently start legacy work.

**Start review** is explicit when automatic start is disabled. Enabling the
**Preferences > Start eligible reviews automatically** default (or **Review
start** in a repository's Settings) admits trusted, eligible assignment detections
to the shared-capacity review runner. Forks and authors outside the trusted watchlist
always require a checkbox confirmation for that exact revision. All-author
monitoring is not trust. **Cancel review** stops inference and requires an
explicit retry; changing the account, Agent, prompt, doctrine, repository or
start gate invalidates affected in-flight work.

The host reads complete paginated changed-file metadata and immutable base/head
trees through the bound repository account. Copilot gets only three host-owned
tools: batch changed-file reads, exact source reads, and literal path search.
No repository is checked out, no symlink is followed, and no target commands,
builds, tests, hooks, installs, or provider mutations are exposed. Before
inference, the pinned runtime's actual tool catalog must exactly match that
allowlist. Missing enforcement blocks the review even after trust confirmation.
This is a constrained current-user process, **not an operating-system sandbox**.

The independent AI account/model and configured review lens are pinned for the
attempt. Head/base revisions, lifecycle, triggers, repository state and start/
trust gates are rechecked before invocation; stale results are rejected.
Validated results contain a one-sentence synopsis, an ordered guide for every
changed file, structured findings, a machine decision, and runtime/session/
token metadata. Every changed file must actually have been read. Missing,
duplicate or invalid file entries fail validation; only invalid ordering falls
back to bytewise ascending paths. Machine-cleared never means human approval.

Review requests persist before waiting for capacity. The first 15-minute AI
budget begins when the worker actually starts, not when the request joins the
queue. Restart preserves operation identity, attempt count, confirmations and
an already-started budget, with at most
three retries for recognized transient failures. Expired budgets and permanent
failures require **Retry review**, which creates a new operation without
deleting history. Results remain local until the separate publication gate
admits them. Provider file/tree limits and the explicit 1 MiB per-tool-response
limit fail visibly rather than returning silently truncated context.

App-owned GitHub and Copilot token pairs use separate account-addressed macOS
Keychain services, never config, state or diagnostics. Neither Settings
connection copies terminal credentials. A green Copilot check verifies sign-in
only, not a subscription, seat or inference request.
See the [bounded architecture decision](docs/adr/0001-macos-foundation.md).

### Shared AI capacity and pause

Normal reviews and owned-thread analysis share one machine-wide capacity,
default **4**, configured in **Settings > Preferences**. Values such as **1**
and **20** are independent of saved Agent/repository counts. Work retains its
canonical FIFO order across retries and pauses. Blocked older work keeps its
reason and order without blocking eligible waiters; completion immediately
fills free slots across both kinds without another repository poll.

**Pause automation**, available in the queue and Settings, persists separately
in `state/automation.json`. It blocks new polling, AI admission and provider
writes without changing repository enablement or permissions. Active AI workers
are signalled to stop; their slots remain occupied and visibly **stopping**
until runtime and blocking work have ended. Rapid resume cannot reuse a slot or
operation while teardown is pending. Reducing capacity stops the newest excess
workers and keeps the oldest permitted workers running.

Partial AI output is discarded after pause/reduction. Durable interruption
metadata restores the pre-attempt retry budget only for intentional cancellation
or discarded successful output; a real failure racing pause remains a failure.
Once inference reports a failure, abort and runtime teardown cannot replace it
with a refundable cancellation; the slot stays occupied until cleanup finishes.
Prior failure counts/deadlines are not reset or extended by resume. Initial
unexecuted requests may wait beyond 15 minutes and still receive their first
budget. Individual cancellation withdraws work until explicit retry. Completed
reviews, pass/reply ordinals, execution snapshots and provider receipts remain
unchanged.

Owner replies and primary mentions fence every analysis-worker save by operation
identity. A manual retry accepted while the cancelled worker is stopping keeps
its new operation, history and manual-start intent; stale progress/completion
cannot overwrite it. The old worker releases only its own reservation after
teardown, then the accepted retry can start.

Higher AI capacity does not increase provider-write concurrency: the existing
serial publication and reply-publication coordinators remain separate from AI
slots. Pause cannot undo an already-started remote request. Its original
mutation intent and receipts are retained for reconciliation; a pending batch
is not deleted merely because automation paused. Storage failures are visible;
a worker whose final outcome cannot be saved retains a blocked stopping slot
until storage is repaired and the application restarted.

Native `automation_snapshot` returns saved pause/capacity plus actual occupied,
stopping, waiting and blocked work. `set_automation_paused` persists the gate and
signals current workers. Normal, primary-final, owned-reply and mention adapters
join the same candidate order and `capacity::Coordinator::reserve` path.

## Primary final review and provider actions

The existing queue projection supplies current normal-pass/feedback clearance.
Every current assignment must have its own completed pass for the tracked
iteration; adding/replacing an assignment cannot authorize an action from old
evidence. Pending conversations, held findings, human-input decisions, incomplete
conversation admission and unknown mutations block the action path.

When Approve or Merge is explicitly opted in, the primary receives a distinct
**full** constrained review after clearance and fresh provider observations.
It reads every changed file and sees the peer results, owned feedback, complete
bounded human discussion/review evidence and actual policy observations.
`state/actions.json` retains its own immutable basis, FIFO identity, operation,
attempt history and result without replacing normal review history.
It uses shared AI capacity, start/trust gates, pause accounting and worker
teardown. Findings or human judgment stop actions; they require human handling
or a new iteration rather than repeated same-iteration AI attempts to obtain a
different answer.

A cancelled final worker keeps its capacity reservation until teardown. If an
explicit retry is saved before teardown finishes, the old callback leaves that
retry's identity, intent and history untouched and releases only its own
reservation before refilling capacity. A real outcome-persistence failure still
retains a visible stopping slot; it is not treated as a superseded attempt.

Approval and merge are independent native operations, never Agent tools.
Approval is one acting-account vote, not one vote per configured Agent. It can
contribute before the provider has collected its other required approvals, but
cannot self-approve a GitHub PR or replace an existing account vote. Merge is
primary-only and additionally requires current-head green checks, satisfied
provider reviews/rules, conflict-free **CLEAN** readiness, no unresolved threads
(even outdated ones), no required/active merge queue, and a provider-selected
enabled merge method. Admin-bypass capability is neither queried nor used.
Absent, partial, unsupported or inaccessible policy/check evidence blocks merge
explicitly; it is not assumed green.

Each effect freezes action/account/head/attribution and persists intent before
its external request. Approval pins `commit_id`; merge pins `expectedHeadOid`
and an explicit provider-selected method. Head/base, role, scope, permissions,
feedback and human context are rechecked around work. A valid final can serve
both actions; its own exact confirmed approval receipt does not invalidate it.
Other relevant changes require fresh final evidence.

Lost responses and crashes reconcile the **original** effect, never a blind
replacement. Approval reconciliation requires its exact signed body, actor,
commit and review receipt. A later read proving merge after a lost response
establishes terminal provider state, **not attribution** to PR Sniper.
Automatic reconciliation is bounded to three observations within the original
window; **Reconcile original action (no resend)** requests another read.
Definitively rejected/stale/cancelled effects are not automatically replaced.
All comment, reply, approval and merge writers share one native mutation owner,
independently of AI capacity. Pause/cancel cannot undo an accepted remote write.

Every cleared unmerged PR retains a personal-review handoff, including after
confirmed automatic approval. Merge does not require a handoff acknowledgment.
Confirmed merged PRs become terminal history. Notifications and read-only queue
details distinguish normal clearance, final review, provider approval/merge,
unknown outcomes and personal review; automation never asserts personal review.

An action-evidence read failure remains visible after Approve and Merge are
turned off, but does not suppress an otherwise valid personal-review handoff
when no provider effect needs resolution. Pending or uncertain effects, current
review/feedback blockers and stale revisions still block clearance. **Refresh /
retry provider evidence** uses the native pump's eligibility and is disabled
with a reason when no work can consume the request, during backoff or while
paused. Original-effect reconciliation remains separate from admitting a new
action, including in the retained panel detail.

Provider limits remain explicit: final observations fail closed beyond 100
threads/comments per GraphQL connection or 100 check contexts; REST review and
discussion reads are bounded. `HAS_HOOKS`, unavailable merge rules, merge queues
and absent green-CI evidence do not take a direct-merge fallback. GitHub exposes
an atomic expected-head merge condition, not an expected-base condition; base
movement is detected by fresh pre/post observations, and confirmed effects are
retained rather than falsely undone. These offline-tested paths are not live
approval/merge acceptance evidence.

## Revision-safe comment publication

An assignment must allow **Comment**. The **Preferences > Publish review
comments automatically** default and each repository's **Comment publication**
override choose automatic normal publication or **off/local-only** evidence.
Review Queue shows the acting repository GitHub account, exact head, local
findings, publication state and confirmed provider receipts. Off does not create
an author-wait state or an invented manual-publication task. Genuine historical
pending batches retain their checked reconciliation/withdrawal controls.
Copilot credentials never
publish to GitHub, and the read-only agent adapter has no mutation tools.

The host freezes the validated output, maps findings only to verified diff
lines, creates one pending review with an explicit commit SHA, then submits it
with `COMMENT`. Findings outside the available diff remain visible locally;
the summary reports their count rather than silently dropping them or posting
them at guessed locations. Summaries and machine sign-off end with the canonical
` PR Sniper` and explicitly request final human review. The existing saved
custom Agent signature is not applied by this slice; signature customization
and its revised default remain separate #30 work. This comment-only publisher
does not approve or merge; those independent operations use the final-review
path above. Owned-thread replies use the conversation workflow below.

Before and after mutations the host rechecks account/repository identity, head
and reviewed target base, open/non-draft lifecycle, eligibility, active monitoring
scope, trust and current publication permission. Disabling Comment or changing
the publication gate stops the attempt; a queued confirmation is not a permanent
grant. **Withdraw publication confirmation** revokes that attempt and removes
only its exact owned pending batch when it can be verified. Cleanup can remove
that batch after revocation or a stale head, but cannot publish or modify another
pending review. A confirmed visible batch cannot be undone by cancellation.

A changed head requests a new scoped monitoring check only when currently
eligible; the normal admission/deduplication path queues it. Eligibility loss
does not requeue ineligible work. A head or gate change after visible submission
retains the receipt and reports **stale after publication**. Target-base movement
without a new head requires an explicit **Review again**, not publication against
a different diff. Older local results without a persisted reviewed base also
require another review. New local review attempts cannot bypass an existing
pending, uncertain or published batch for the same revision and assignment.

Private `state/publications.json` stores immutable output, mutation intent,
pending-review identity, confirmed receipts and retry budgets before subsequent
effects. Restart reconciles the complete remote review and inline comment set,
including acting identity and exact content, before any repeat mutation. A
missing receipt after an uncertain create never authorizes another create;
it remains visibly unresolved even after manual retry. Another pending review
owned by the signed-in account is never silently submitted or deleted.

Transient failures use the shared limit of three retries within 15 minutes,
preserving the operation and deadline across restart. Explicit rejection,
configuration and permission errors require correction. **Reconcile / retry
publication** starts a fresh budget but retains the original publication identity
and uncertainty. Provider acceptance and local machine sign-off remain distinct.
If GitHub confirms submission but the final eligibility check fails, the receipt
is retained and retry only reconciles that existing review. Starting another
local review cannot hide an active, uncertain or confirmed publication.
Live mutation acceptance requires an explicitly authorized disposable PR and
cleanup; deterministic provider fixtures do not claim a live publication pass.

## Owned-thread follow-ups

Each repository read also checks all captured assignments' unresolved threads
rooted in their confirmed PR Sniper inline comments. Ownership requires the saved review and
comment receipts, repository/account identity, original commit and exact root
body; a matching username alone is not sufficient. GitHub GraphQL supplies
resolution state and complete paginated published conversations. Unpublished
pending comments are excluded, so private drafts cannot trigger public replies.
Comment discovery failures use the existing poll operation's retry budget.

The latest published external comment creates a durable key containing provider,
account, configuration, owned-thread ID, comment ID and original reviewed head. Multiple
new comments between polls are coalesced into that latest trigger while the full
published conversation remains context. PR Sniper's own signed replies cannot
trigger themselves; an actual human comment through the same account can.
Repeated polling/restart does not create another job for the same key. A push
does not replay the same comment through a new owner.

**Review Queue > Thread follow-ups** shows the conversation, acting account,
revision, draft/evidence and separate analysis/publication states. Existing
automatic-start and automatic-comment defaults/overrides apply, and the
assignment must allow **Comment**. With those gates off, use **Start follow-up**
and a separate checked **Publish reply** confirmation. **Cancel follow-up**
stops inference or withdraws permission before a reply; it cannot remove an
already confirmed comment.

Analysis reuses the same pinned Copilot account/model, current review lens,
isolated runtime and three immutable read tools. It still reads every changed
file. A publishable reply is bounded, contains substantive new information and
quotes exact verified immutable source lines. Normalized repeats,
acknowledgment-only filler, speculative phrasing and unsubstantiated answers
stay quiet. Invalid or fabricated source citations fail visibly. Semantic review
quality still depends on the model and human review; citations are not proof of
every inference.

Human-judgment questions enter **human input required** with no publishable
body. That state pauses automatic follow-ups for the thread; a later external
comment can be started explicitly after the human decision. Quiet and human-input
results have no publication action. Resolved, changed, superseded or stale
threads stop rather than responding to an outdated conversation. Each follow-up
stores an immutable `target` (original publication/review/root) separately from
its `context` (current iteration, captured selection and trust). A new author
reply may analyze a later admitted iteration through the same original Agent;
it never rewrites the old review/head or transfers ownership to a replacement.
Normal passes on that iteration still fan out independently.

Replies are posted only to the verified root comment, end with ` PR Sniper`,
and never resolve a thread, approve a PR or merge. Before and after the mutation,
the host revalidates the live thread, revision, lifecycle, eligibility, account,
trust, scope and publication gate. A late change preserves the confirmed receipt
as **stale after publication**, without posting again.

Private atomic `state/follow-ups.json` records frozen input/output, both operation
budgets, attempted mutation, original review/thread/trigger identity and reply
receipt. Analysis and publication each receive the existing three-retry,
15-minute budget when started. Restart preserves those budgets.
**Reconcile / retry reply** retains the original key and body: a lost response
must be reconciled against the full remote thread before any further effect.
An absent uncertain reply never authorizes another POST, even after manual
retry. Live mutation acceptance still requires a separately approved disposable
PR; provider fixtures do not claim a live reply pass.

### Owned feedback across iterations

`state/feedback.json` records verified owned roots, the original owner and
publication, current-head observations, missing-root errors and durable closure
tombstones. Observing earlier roots is independent of authorizing their current
owner: removing an assignment retains unresolved feedback and visibly blocks
clearance. Only actual provider closure settles a discussion; no Agent
resolve/reopen mutation exists. Missing, tampered, failed or incomplete
observations never mean resolved.

New full reviews receive earlier open and closed feedback as untrusted context.
They must reassess every earlier open concern owned by their Agent using stable
feedback IDs. Reply analysis may reassess only its own concern; an explicit
clearance requires rationale and exact verified source evidence. An explanation
can therefore clear a concern without a push or another full review. Merely
analyzing, replying, or choosing quiet never clears it. Local reassessment is
displayed separately from provider thread closure.

Existing feedback cannot be emitted again as a new finding. Closed identities
remain tombstoned across reopening/new heads. As a conservative ambiguity gate,
new findings on the same or renamed file as a closed concern are retained as
**held locally** for human judgment, not republished; no finding is discarded
as if it never existed. Semantic interpretation still requires human review.
Pending conversations, human-input decisions, unavailable owners and unresolved
mutation outcomes prevent false machine clearance. Original published receipts
and local-only findings remain distinct.

Once a publication is confirmed complete, later feedback changes affect current
readiness, not the immutable publication's mutation-freshness check. Closure or
source-validated same-head owner reassessment can therefore restore readiness
after a later iteration publishes. Current assignment/account/revision gates,
open concerns, human-input decisions and pending or uncertain mutations still
block. New and pending publications retain their captured-feedback freshness
checks; local clearance never resolves a GitHub thread.

### Primary acting-account mentions

For already tracked open PRs, scans read bounded, complete top-level issue-comment
pages (up to 1,000 comments and 1 MiB of bodies). A standalone `@login` matching
the live repository acting GitHub identity routes one response to the current
primary; it never addresses the separate Copilot identity or all Agents.
No primary, unavailable model/account selection or missing current iteration is
visible blocked work with retained FIFO order. Mentions do not admit new PRs.

Mention intent is keyed by provider/account/repository/PR/comment identity, not
primary identity. Once assigned, it is not replayed through a later primary.
Signed machine output cannot loop; legitimate same-account human comments remain
eligible. Edited/deleted triggers block new replies, but an existing uncertain
reply can still reconcile its exact signed body and acting-account receipt.
Top-level responses link the original comment and reuse the constrained reply
schema, shared `Kind::Mention` AI capacity, explicit trust/start/comment gates,
serial reply publication, pause handling and no-blind-repost recovery. No fake
review or publication is created for a mention.

Admission first saves feedback observations, then saves mention identity and FIFO
intent before writing its follow-up execution, and only then saves the committed
execution link. Monitoring records success only after all admission writes
succeed. Failure leaves a durable conversation-admission blocker across restart
and manual retry until a successful check. Saved unlinked intents recover on the
next check without requiring remote rediscovery; a committed execution is linked,
not replaced. Missing linked history and uncertain replies never authorize replay.

Legacy follow-up records load into the typed target/context shape without
writing history during reads. When the original publication is available, its
retained review supplies immutable provenance; the legacy captured selection
remains the actual analysis context.

## Read-only GitHub connection

In **Integrations > Git repositories**, connect an account through the PR Sniper
GitHub OAuth App and confirm its stable identity. Repository OAuth requests
the broad `repo` scope for public/private access. Explicitly select the acting
account and repository, then save. In its **Settings > Repository and
connection**, verify the GitHub connection. A different account fails visibly
instead of silently switching identities. Connections use saved bindings,
not unsaved rename drafts. Copilot credentials do not determine the repository
acting identity.

The application verifies `/user`, repository metadata and an actual PR read.
Read access is separate from comment capability: classic OAuth `repo` or
`public_repo` scopes are checked against the repository visibility and archived
state. Scope availability is **not publication authorization** or proof that an
individual PR is unlocked or unrestricted. Credentials without scope evidence
(including fine-grained tokens) show comment permission **unverified**, never
fabricated write permission. No mutation is used to probe permissions.

**Read PR metadata** rechecks the verified account and remote repository ID,
then reads every PR page, each PR's full changed-file pages and requested users
and teams. It includes closed/merged PRs and explicit deleted-account/fork states.
Head changes, malformed pages, duplicate entries, incomplete file counts,
GitHub's 3,000-file cap and failed pages produce an error, not partial success.
No file is checked out or executed. This is metadata, not a guarantee of complete
diff text. Retargeting a saved repository invalidates its previous connection.

Authentication, CLI, permission, rate-limit, timeout, network and provider failures
remain visible. No automatic retry or automation enablement occurs. Diagnostics
record only fixed connection/read success or failure events, never raw provider
bodies, tokens or subprocess output. HTTPS credentials travel only in a sensitive
authorization header; redirects are disabled.

For an explicitly authorized legacy CLI-credential read-only diagnostic using
the native provider client (not the Settings OAuth connection):

```sh
cargo run --manifest-path src-tauri/Cargo.toml --locked --example github_read -- \
  jdylanmc/pr-sniper 6954990 --metadata
```

Use your own expected account ID when appropriate. This prints verified public
identity, capability and aggregate metadata counts, never a credential.

`npm run test:settings` exercises the production Settings UI against real Rust
storage with temporary data and fresh process reads. See
[Settings behavioral tests](tests/settings/README.md) for setup and the boundary
between browser proof and native macOS verification.

## Crosshair assets

The canonical crosshair is original vector geometry, not a private-use font.
`src/crosshair.svg` serves the local surfaces, and the native tray uses a
transparent template PNG that macOS adapts to its appearance.
Regenerate checked-in PNG/ICNS assets on macOS with
`swift scripts/generate-icons.swift`.
