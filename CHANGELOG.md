# Changelog

Notable changes are recorded here using [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Stable release tags use Semantic Versioning; see [release operations](docs/releases.md).

## Unreleased

### Fixed

- Make the sidebar regression explicitly wait for notification state: disabled
  while loading, enabled but opted out after loading. Stop treating the shipped
  notification control as a retired placeholder.

- Dispatch verified releases to the dedicated tap-owned publisher rather than
  running Homebrew under the application release wrapper. Confirm exact cask
  bytes before reporting success; retain an unconfirmed state on timeout.
  Tap CI audits and verifies a disposable install without accessing Apple
  signing credentials or replacing published release assets.

- Stop browser-test polling and drain accepted native Store calls before
  deleting each isolated fixture directory, preventing late writes from racing
  cleanup. Keep application behavior, assertions, retries and timeouts unchanged.

- Add the owned CI signing keychain to the user search list before signing,
  verify the change and verify restoration afterward. Report native operation,
  exit code and fixed error category without echoing credentials or raw native
  output. Prepare version 0.1.1 for the first-release recovery; the failed
  v0.1.0 tag is preserved and has no published release assets.

- Keep an assignment's frequency selector synchronized with advanced interval
  edits, so the displayed timer matches the draft that will be saved.

- Persist the complete 23-doctrine starter library on first settings load,
  independent of tab order or which section is saved first. Preserve edited,
  custom and deliberately empty libraries across restarts; existing legacy
  settings without a doctrine field remain unchanged.

- Keep GitHub accounts and sign-in controls within the Settings window, with
  readable account details and wrapping actions. Show a prominent selectable
  one-time code beside **Copy code**, preserve clipboard feedback and keyboard
  focus during authorization polling, and explain manual copying if clipboard
  access fails.

### Changed

- Replace separate native Queue/Settings/Status/Diagnostics windows with one
  hidden-at-start tray panel. Four retained destinations, one exact job-detail
  layer and Back preserve drafts, row/scroll/focus context and current evidence
  without reloading. Native notifications route to the saved identity or an
  explicit missing state, never another PR.
- Anchor the compact panel to the actual tray monitor/work area with DPI-aware
  clamping. Outside/Escape/close hide without stopping background work; native
  pickers retain focus ownership and tray toggles avoid the dismissal/reopen
  race. Secondary-menu recovery and explicit Quit remain available. Reviewed
  uses the existing evidence projection, without adding history storage/purge.
- Replace assignment timers with one durable global cron scan. Capture the scan's
  assignments and read each account/repository binding once before fanning out
  individual Agent jobs. Preserve retry budgets, manual-check coalescing and
  existing owned-thread polling.
- Keep PR admission after author/reviewer filters change; explicit reviewer
  requests can admit older PRs. Persist iteration identity, FIFO metadata and
  separate normal/reply ordinals. Verified closure/merge is terminal; reopening,
  even at the same head, creates fresh work. Preserve historical review keys,
  publication receipts and queue destinations without replaying completed passes.

- Require macOS 13.5 or later for the application, matching the bundled Copilot
  runtime's minimum.

- Anchor Settings to the approved compact sidebar, with repository selection,
  readable GitHub people lookup, preserved model selections, local review
  presets and separate review/publication switches. Settings drafts save together
  and detect conflicting updates; existing custom prompts, cron schedules and
  per-field overrides remain supported. Discovery reads only metadata beneath an
  explicitly chosen folder, never repository code. Review execution health
  remains unavailable; no automation or approval is enabled by this change.
  Dialogs include an accessible older-WebKit fallback with viewport sizing
  preserved in production builds; delayed replies preserve
  open forms and cannot restore dismissed repository edits. Multiple local
  clones share one repository checkbox while retaining searchable clone paths.
  Conflicting drafts now offer explicit discard-and-reload recovery, custom
  schedule controls stay synchronized, and repeated remote URLs preserve Git's
  fetch identity while ambiguous remotes remain unavailable.
  See the [approved Settings design](docs/agent/design/settings-default.md).

### Added

- Add primary final full reviews through shared AI capacity after current
  aggregate Agent/feedback clearance. Capture peer and human context, validate
  full-file coverage, and preserve separate final evidence and attempt history.
- Execute independent opt-in GitHub approval and primary-only exact-head merge
  through native capability/policy gates and one serialized mutation owner.
  Retain frozen intents, definitive receipts and bounded no-resend reconciliation;
  preserve personal-review handoff after approval and terminal merge history.
- Present publication-off normal findings as local-only evidence, without
  manufacturing author-wait or manual-publication tasks. Retain real pending
  publication recovery and distinguish action outcomes in notifications.

- Retain verified owned feedback across PR iterations, including closure
  tombstones and explicit evidence-backed reassessment. Keep immutable original
  publication/root provenance separate from current conversation analysis;
  missing or unavailable ownership never becomes clearance.
- Route top-level acting-account mentions to one repository primary through the
  existing constrained conversation engine and shared AI capacity. Preserve
  same-account human replies, stable deduplication, independent publication gates
  and exact lost-response reconciliation without replacement posts.
- Show current feedback disposition, blocked mention routing, captured context
  and held-local ambiguous findings in existing read-only evidence surfaces.

- Share AI capacity across normal reviews and owned-thread analysis, default
  four, with oldest-eligible ordering and completion-driven refill. Persist
  manual requests before capacity waiting and start the first AI retry window
  at actual execution.
- Add durable global pause/resume and real occupied/stopping/waiting/blocked
  controls. Pause and capacity reduction discard partial AI work without
  consuming failure retries, retain stopping reservations through teardown,
  preserve genuine failures and reconcile existing provider effects.

- Add resource-scoped Settings saves and validation over the existing Store,
  with a shared saved-configuration readiness contract for future setup flows.
  Preserve unrelated drafts, reject stale same-resource writes, guard referenced
  resource deletion and update doctrine references atomically on rename.
- Support ordered zero/one/many doctrines and immutable execution configuration
  evidence, with explicitly planned jobs and honest legacy missing snapshots.
- Save global cron/time-zone and capacity preferences, repository primary
  selection and independent approval/merge opt-ins. Preserve legacy settings
  without turning inert approval flags into grants. Global scheduling,
  concurrency, provider actions and Genie UI remain separate deliveries.

- Add the native Windows system-tray host, opt-in current-user startup
  registration and Windows notification activation, preserving the existing
  review interface and human-owned decisions. Windows CI now builds the full
  application, exercises native/browser checks and publishes unsigned,
  commit-identified executable artifacts. Public signing and Chocolatey
  distribution remain separate. Explicit notification opt-in initializes
  Windows' first-use status with a persisted, suppressed setup notice and
  exact cleanup. Runtime lifecycle checks wait for observed fixture stages
  without changing application deadlines. See
  [#59](https://github.com/jdylanmc/pr-sniper/issues/59),
  [#60](https://github.com/jdylanmc/pr-sniper/issues/60) and
  [#61](https://github.com/jdylanmc/pr-sniper/issues/61).

- Add Windows Credential Manager storage for independent repository and Copilot
  accounts, preserving shared token-pair and registry behavior with explicit
  native capacity errors without partially replacing a reconnect. Port the
  constrained Copilot runtime environment, bounded cleanup after process-tree
  termination and the bundled offline handshake. Full
  Windows application acceptance remains separate. See
  [#58](https://github.com/jdylanmc/pr-sniper/issues/58).

- Add Windows-native private persistence with current-user-only access controls,
  atomic file replacement and explicit permission/locking failures. Preserve
  denied-read errors without changing existing permissions; keep embedded
  doctrine bytes canonical across Windows checkouts. Windows CI now exercises
  the production persistence, policy and discovery foundations;
  the native application still requires its account, tray and notification
  adapters. See [#57](https://github.com/jdylanmc/pr-sniper/issues/57) and
  [Windows development](docs/windows-development.md).

- Add credential-free Windows pull-request and main-push checks for the
  production frontend and portable release/workflow contracts. Native app
  validation and runnable artifacts remain separate follow-ups. See
  [#65](https://github.com/jdylanmc/pr-sniper/issues/65) and
  [#61](https://github.com/jdylanmc/pr-sniper/issues/61).

- Add stable-tag macOS release automation for Developer ID signing,
  notarization/stapling, verified versioned archives and automatic dedicated
  Homebrew cask updates. Gate signing on exact-commit main CI; keep Apple and
  tap credentials separate from PR builds. Initial distribution targets
  Apple Silicon on macOS 13.5+. The first release/install remains a separate
  human-authorized acceptance step. See [release operations](docs/releases.md)
  and [#12](https://github.com/jdylanmc/pr-sniper/issues/12).

- Add an inspectable, standalone vNext design POC with configured and
  fresh-install demos, guided Genie setup, browser-local mock review workflows,
  and synthetic screenshots. No production integration. See
  [#52](https://github.com/jdylanmc/pr-sniper/issues/52).

- Add opt-in macOS notifications for operator confirmation, human input,
  final review and failure, with generic private banner text and exact saved
  destinations. Persist transition deduplication and pre-send intent across
  restart; show denied, failed, uncertain and OS-accepted-but-unconfirmed
  delivery in Review Queue. Notification clicks only navigate, never approve,
  publish or merge. Local/CI bundles use bundle-bound ad-hoc signing for macOS
  notification identity; Developer ID signing and notarization remain out of
  scope. See [#15](https://github.com/jdylanmc/pr-sniper/issues/15).

- Put machine-cleared PRs and operator-input work first in Review Queue, while
  published findings wait on the PR author. Aggregate all current assignments
  without hiding publication failures, uncertain outcomes or stale revisions.
  Preserve exact selection across restart, complete guide/file navigation,
  evidence and gated recovery actions. Fix assignment matching so one Agent's
  detection cannot mask another's review/publication/follow-up state. See
  [#2](https://github.com/jdylanmc/pr-sniper/issues/2).

- Follow published external comments in verified PR Sniper-owned unresolved
  threads with a constrained, evidence-backed reply, silence or a human-input
  wait. Preserve per-thread/comment/revision identities and separate bounded
  analysis/publication budgets; reconcile lost reply responses without blind
  duplicates. Review Queue exposes the complete conversation, source evidence,
  explicit start/publication gates, cancellation and recovery. Private drafts
  and machine replies cannot self-trigger. See
  [#24](https://github.com/jdylanmc/pr-sniper/issues/24).

- Publish validated review findings as one signed, revision-bound GitHub
  `COMMENT` batch through independent automatic/manual publication gates.
  Persist mutation intent and receipts, reconcile uncertain outcomes without
  blind duplicate posting, retain unmappable findings locally, and discard
  stale owned pending reviews. Review Queue exposes confirmation withdrawal,
  reconciliation/retry, and stale-after-publication status; provider approval
  and merging remain unavailable. See
  [#23](https://github.com/jdylanmc/pr-sniper/issues/23).

- Run assignment-bound Copilot reviews with the configured AI account, model,
  prompt and doctrine, manually or through the automatic-start gate. Require
  exact-revision trust confirmation for forks and untrusted authors; expose only
  verified immutable read tools, never target code execution or publication.
  Review Queue provides cancellation, durable bounded retries, complete ordered
  file guides, structured findings, machine decisions and session/usage details.
  Invalid, incomplete or stale results fail visibly. See
  [#22](https://github.com/jdylanmc/pr-sniper/issues/22).

- Persist each repository poll's operation identity, attempt count and
  15-minute retry deadline before provider work. Interrupted polls retain the
  same budget across restart; transient failures receive at most three bounded
  retries, while expired or non-retryable operations stop visibly for explicit
  retry from Review Queue. Detection-only operations explicitly contain no
  attempted mutation or provider receipt. See
  [#7](https://github.com/jdylanmc/pr-sniper/issues/7).

- Poll enabled, account-bound GitHub repositories from the active menu-bar
  process using each saved assignment's fixed interval or five-field cron
  schedule in an explicit time zone, with the effective repository policy as a
  legacy fallback when no assignments exist. **Check Now** covers every eligible
  account and assignment while serializing reads of the same remote repository.
  Require explicit monitoring-scope confirmation before any enabled repository
  admits new detections. Settings previews the real filtered backlog and lets
  the user choose new pull requests only or selected existing revisions plus
  new work; existing configurations pause safely after upgrade while retaining
  history. Native activation persists the bound account/repository, filter,
  creation watermark, initial/observed heads and durable admitted-head identity
  so unchanged unselected old revisions stay excluded while selected or matching
  new heads remain admitted across polls and restart. An empty watched-author
  filter means all authors after activation without making those authors trusted;
  requested-reviewer matching applies only when that trigger is enabled.
  Filter-only edits preserve the confirmed boundary and admitted-head history
  while each poll uses the current filter; stale previews and in-flight results
  remain fenced to their original filter.
  Persist acting-account and assignment schedule health plus revision-keyed
  detection history, deduplicate repeat observations, and retain superseded,
  ineligible or configuration-invalidated revisions as visibly non-actionable.
  A revision absent from one created-ascending open-pull-request scan remains a
  nonterminal "not seen" record and becomes active again if observed later.
  Health labels distinguish disabled, checking, blocked, unavailable and
  scheduled repositories instead of presenting an unusable schedule as active.
  Disconnects fence late provider results before queue or cursor commits, while
  offline startup, rate limits and ordinary provider failures remain visible
  and retryable without discarding a verified account; warnings retain an
  explicit reconnect escape, failed reconnect attempts preserve the usable
  stored grant, and successful verification clears the warning. Definite invalid,
  expired or revoked refresh grants still require reconnect, and repository
  retry warnings never make an unverified Copilot account appear connected or
  selectable. Reviewer-only work outside the trusted watchlist waits for
  confirmation; detection never starts an agent or publishes a review. See
  [#3](https://github.com/jdylanmc/pr-sniper/issues/3).

- Manage independent Copilot connections in Settings with browser sign-in,
  explicit account confirmation and Keychain storage. Each Agent selects its
  own connected account and a model returned by Copilot. Disconnect retains
  dependent Agents and assignments needing reconnect, without switching
  accounts. The green check verifies sign-in only; it does not test a
  subscription or run a prompt. Failed credential deletion stays visible and
  retryable, including when cancelling another sign-in. Late state reads cannot
  undo account actions, and failed refreshed-credential saves mark only the
  affected account unavailable.
  See [Copilot Settings](docs/copilot-settings.md).

- Connect through the registered PR Sniper GitHub OAuth App in the default browser
  using GitHub's secretless device authorization flow, a provider-issued one-time
  user code, bounded polling/cancellation/expiry, actionable
  disabled-registration failures and explicit stable-identity confirmation
  before persistence. Multiple provider accounts retain
  account-addressed credentials with absolute expirations in macOS Keychain and
  independent serialized rotation boundaries. Restart restores and validates
  every account, retry-safely migrates the prior active-account layouts without
  orphaning secrets, recovers interrupted first-time account additions, and
  removes only the selected account on disconnect. OAuth App credentials use a
  generation-specific Keychain namespace; superseded GitHub App credentials
  require explicit reconnect and are cleaned only after the new pair is
  durable. A cleanup failure remains explicit retryable debt without
  misreporting the already-confirmed account as pending. Each OAuth session requests
  the broad `repo` scope, explains its public/private repository consent, lists
  paginated affiliated repositories and can directly validate any known
  accessible repository with the selected account, requires explicit
  selection, and backs identity, people, repository verification and
  pull-request metadata reads without requiring GitHub CLI credentials. Failed
  operations retain the last confirmed credential state and report specific
  reconnect causes. Settings never treats sign-in as permission to review,
  publish, notify or merge. Deterministic provider fixtures and isolated native
  Keychain restart coverage protect this foundation; live browser sign-in
  remains separate acceptance. See
  [#5](https://github.com/jdylanmc/pr-sniper/issues/5).

- Verify configured GitHub repositories with the connected OAuth account and
  stable identities, distinguish read access from comment scope, and read
  complete paginated PR, reviewer and changed-file metadata in Settings.
  Every repository is explicitly bound to a provider account and stable
  repository identity; obsolete installation bindings become unbound and
  require explicit reselection. Overlapping access never auto-selects an acting
  account and can retain separate bindings and policies for each account.
  Account failure or removal marks only that account's bindings as needing
  attention, disables their provider actions, and clears their cached
  verification and metadata until reconnection or explicit rebinding. Identity,
  permission, rate-limit and incomplete-read failures remain visible; no
  provider mutation or automation enablement occurs.
  See [#5](https://github.com/jdylanmc/pr-sniper/issues/5).

- Manage watched GitHub repositories from Settings, with canonical duplicate
  protection, stable identities, rename/enable controls and confirmed removal
  persisted across restarts. Configure global review-policy defaults and
  per-repository overrides with visible inherited sources and independent
  automatic agent-start and comment-publication gates. This configures future monitoring;
  it does not connect to GitHub or start reviews. See [#4](https://github.com/jdylanmc/pr-sniper/issues/4)
  and [#11](https://github.com/jdylanmc/pr-sniper/issues/11).
  Invalid schedules, identities and supported credential patterns are rejected
  before saving; failed configuration writes preserve existing data. Committed
  settings remain visible if diagnostics fails, with an explicit warning.
  Unrelated saves and asynchronous focus refreshes preserve unsaved policy edits.

- Locally buildable macOS menu-bar foundation with a font-independent crosshair,
  queue/status/setup placeholders, persistent startup preference, explicit
  opt-in launch at login, and redacted host diagnostics accessible from Settings.
  Closing windows keeps the tray alive; Quit ends the host. Includes developer
  setup, storage regression coverage and a macOS build/check pipeline. This
  foundation does not yet monitor repositories or perform reviews. See [#12](https://github.com/jdylanmc/pr-sniper/issues/12).
