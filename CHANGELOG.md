# Changelog

Notable changes are recorded here using [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Stable release tags use Semantic Versioning; see [release operations](docs/releases.md).

## Unreleased

### Added

- Configure Agents with advertised reasoning-effort and context-tier choices,
  explicit Provider defaults and visible unsupported selections. Preserve
  deliberate choices and immutable job evidence, and verify actual session
  configuration before inference. (#128)

### Changed

- Adapt repository Joe-mode orchestration to the visible CMUX Maestro cockpit.
  Retire Paseo adapter files, retain shared team and independent merge gates,
  and keep activation human-only with no unattended CMUX scheduler.

- Replace Automation overrides with repository-assignment capabilities:
  independent Publish Comment and primary Reply Comment, plus Approve or
  Approve & Merge after confirmed approval. Route eligible admitted-PR
  conversations to the primary without replaying history or changing original
  feedback ownership, and preserve actual job snapshots and uncertain-write
  recovery. (#122)
- Automatically queue and run eligible admitted reviews without manual-start
  preferences or a per-PR Start click. Preserve pause, disabled repositories,
  account/assignment availability, shared capacity, retries and independent
  comment/reply and approval capabilities; legacy start flags no longer block
  work or rewrite captured evidence. (#120)
- Update the browser-only visual prototype with account/organization repository
  browsing, URL entry, and compact drill-in rows. Newly added repositories open
  configuration with monitoring off.
- Bring the repository prototype into Settings and Genie: browse GitHub by
  connected account and personal or organization owner, add verified URLs, and
  configure compact drill-in rows. Repository Save authorizes all currently open
  and future matching PRs, persisted atomically with the configuration, while
  preserving global pause and independent action permissions. (#114)
- Use saved Agent assignments and repository configuration as ongoing review
  consent. Remove repeated revision-trust prompts from normal/retry, conversation
  and final reviews without changing read-only execution, revision validation or
  separate comment/reply and approval capabilities. Existing blocked queue records
  can proceed under the current execution gates.
- Group Accounts into AI Tooling and Git Repository provider lists before
  showing individual integration details. Preserve sign-in flows and nested
  Back focus; label unavailable providers Coming soon without setup actions.
- Use the ten shipped app doctrines consistently across Agents and reviews.
  Reconcile unversioned pre-alpha libraries once, report cleared obsolete
  selections and catalog revisions, and preserve later edits, captured job
  evidence and provider safety state. (#131)
- Preserve active PR evidence with stable, bounded native result paging. Clean
  provider-confirmed terminal detail automatically only after workers and
  uncertain writes settle; retain compact ownership/action receipts, safe
  same-head reopening and exact cleaned destinations. Reviewed renderer wiring
  remains a separate delivery.
- Re-home GitHub repository and Copilot AI connections in compact, role-labelled
  account panels with native device-code waiting, explicit identity confirmation,
  consent details and visible retry/disconnect states. Keep future direct
  providers disabled and existing account/model choices and references intact.
- Present shared-style repository editors for
  real account binding, watched identities and assignments.
  Expose the independent reviewer-request override, guarded account unbinding
  and the saved global schedule without adding scoped polling controls.
  Preserve explicit assignment capabilities, primary roles, resource
  save guards and session drafts; ambiguous binding requires explicit account choice.
- Match the approved compact Agent and doctrine libraries with shared-resource
  editors, full prompt/principles, explicit account/model choices, filter-retained
  doctrine selections and visible shared-use/deletion consequences. Save applies
  only the resource; Back/Cancel preserves earlier saves and unrelated drafts.
- Re-home global schedule, machine AI capacity and automation defaults in compact
  Preferences. Clearly separate draft-backed saves from immediate native pause,
  notification opt-in and startup; keep Status/recovery and redacted Diagnostics
  reachable through existing routes.
- Harden Windows CI with main-owned Rust dependency caching, superseded-PR
  cancellation, bounded command steps and labelled Chocolatey fault logs while
  retaining all native, installer and recovery checks.
- Apply the approved compact navy/orange/teal shell, supplied header artwork
  and persistent four-destination bottom navigation. Monitoring uses native
  pause state; unavailable snapshots never retain a successful work badge.
- Present human Queue as compact PR cards, separate from Agent work, with
  complete ordered file guides, external personal-review links and retained
  publication/provider recovery. Automatic approval remains a personal handoff,
  not a claim of human review.
- Show one continuous list of actual shared-capacity jobs: active work first,
  waiting/blocked work in arrival order, static reduced-motion states and one
  exact inspector for normal, final, reply and mention work. Preserve captured
  configuration, canonical iteration, work ordinals and separate retry counts.

### Removed

- Remove local-checkout discovery, folder scanning, and separate repository-scope
  preview/confirmation. Existing configurations and legacy admission data remain
  readable; merely browsing or adding a repository does not start monitoring.

### Fixed

- Verify pending GitHub review comments using their diff positions and frozen
  revisions when line/side are absent, while retaining identity and content
  checks. Keep publication failures on the affected PR instead of the Settings
  banner; no existing remote drafts are reset or discarded. (#154)
- Resolve GitHub pull-request URLs to the exact PR and queue it immediately
  after valid repository Save, even outside watch filters. Reuse unchanged
  iteration work while preserving pause, disabled repositories, account/Agent
  availability, capacity, revisions and independent action permissions; show
  intake failures with explicit retry. (#119)
- Infer a sole confirmed GitHub repository connection from URL intake, with a
  compact identity and Change action. Require explicit choice across accounts,
  preserve saved actors through disconnect/recovery, and never borrow Copilot
  or CLI identity for repository access. (#117)
- Restore the approved compact Queue summaries, handoff cards and designed empty
  state, with truthful ready/attention counts, exact read-only evidence navigation
  and Agent execution kept in Running. Preserve keyboard focus, accessibility
  media and small-panel layouts. (#130)
- Make repository monitoring an explicit, persistent Enabled/Disabled control
  in repository lists and Settings/Genie. Keep configuration drafts and global
  pause separate, enable valid new configurations on Save, and preserve
  deliberate disablement with guarded, actionable enablement failures. (#118)
- Distinguish hidden or absent organization repositories from read denial,
  and surface selected-account scope and organization authorization recovery
  without changing access grants. (#125)
- Fix connected corporate-account repository browsing for enterprise-managed
  usernames. Retain accessible results with explicit partial authorization,
  metadata and pagination warnings, and make Retry use the selected account
  without losing a verified connection. (#124)
- Match the approved grouped Settings overview with live resource summaries,
  separate Accounts and Repositories, retained editor drafts and Back focus,
  and continued Genie access. (#106)
- Show the running native application version beside Status and Diagnostics
  on every panel tab, with explicit unavailable metadata and readable compact
  footer layouts. (#107)
- Recover owned interrupted retention stages without deleting unowned files.
  Keep missing human-closed roots unavailable and settle full/compact replies
  consistently without hiding uncertain writes. Revalidate legacy terminal
  records explicitly and count saved mention intent once in result ordering.
- Keep save success aligned with the replacement commit point; recover ownership
  housekeeping separately without rolling back a committed login preference.
  Capture mentions from tracked iterations without requiring jobs, retain their
  cleanup deduplication keys, and prevent superseded unstarted intent from
  blocking current clearance. Expose ambiguous legacy result associations.
- Keep account keyboard focus through native state updates, reject stale GitHub
  reads after account actions, and expose retry for unavailable account reads
  and failed GitHub credential deletion. Device-code controls remain fully
  scrollable in compact windows without changing native authentication.
- Require explicit repository Save/Cancel before account unbinding, without
  consuming pending assignments, overrides or enablement. Label retained legacy
  intervals as blocked until a global cron is chosen, and expose late-preview
  cleanup failures in the active editor without replacing resource-save errors.
- Keep late monitoring-scope status reads from replacing newer preview errors,
  cancel previews after repository edits or dismissal, and lock scope selection
  while confirmation is pending.
- Keep native pending operations and rejection messages visible across unrelated
  preference saves, distinguish startup registration from the saved request, and
  label unreadable notification authorization as unknown rather than stale success.
- Synchronize the global cron helper with saved and edited expressions without
  changing time-zone semantics, legacy schedules or repository action permissions.
- Keep evidence polling alive across Status and Diagnostics, including delayed
  or failed utility reads.
- Match the approved Job details hero, facts and readable configuration cards;
  separate the selected job's state from sibling activity and PR-wide outcomes.
- Retain historical job ordinals after later same-commit work and show planned
  configuration for failures before the first execution attempt.
- Distinguish failed/retrying conversation analysis, intentional cancellation
  and pending/unknown reply publication; analysis completion is not a receipt.
- Keep selected job controls intact during unrelated polling and retain keyed
  focus and configuration disclosures through the selected job's own updates.
- Restore actual row and Settings dialog openers on WebKit pointer activation,
  including nested, replacement and asynchronously opened editors.
- Prevent delayed navigation frames from taking newer keyboard focus after
  Settings saves; retain initial route focus, exact Back positioning and
  hide/reopen drafts while rejecting stale route and focus ownership.

## 0.1.2 - 2026-10-01

### Added

- A single retained tray panel for Queue, Running, Reviewed and Settings, with
  exact work-item navigation, preserved drafts and native hide/reopen behavior.
- Shared AI capacity and durable pause/resume across normal reviews, targeted
  conversations and the primary Agent's final full review.
- Cross-iteration feedback, human-closed concern preservation and top-level
  acting-account mentions routed to the repository's primary Agent.
- Independent opt-in GitHub approval and primary-only merge after final review,
  current repository gates and durable provider-intent reconciliation.
- Resource-scoped Settings saves, multiple doctrines and immutable execution
  configuration evidence.

### Changed

- Poll enabled repositories on one global cron schedule, then reconcile each
  Agent's missing review work. Preserve sticky admission and distinct reopened
  iterations, including reopening at the same commit.
- Publish verified Homebrew cask updates through the dedicated tap-owned
  publisher and its disposable installation checks.

### Distribution

- This release publishes signed, notarized **macOS Apple Silicon** binaries
  for macOS 13.5+ through GitHub and Homebrew.
- Windows platform and unsigned installer/Chocolatey candidate work is present
  in source and CI, but this tag does **not** publish a public Windows package.
- Quit PR Sniper before upgrading. Saved settings and credentials remain
  preserved; existing inert approval flags do not become new permissions.
- The broader visual overhaul, paged history/storage cleanup and Genie UI are
  separate follow-ups, not part of this release.

## Historical development log

The entries below preserve incremental implementation history, including work
released before 0.1.2. Their stage-specific limitations are historical; the
versioned release section above defines the 0.1.2 distribution scope.

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

- Add current-user Windows installer candidates and checksum-pinned Chocolatey
  package tooling, with scoped rollback, removal checks and separate unsigned
  test packages. Public distribution remains gated on trusted signing, publisher
  setup and native lifecycle acceptance; the existing macOS release path stays
  unchanged. See [#62](https://github.com/jdylanmc/pr-sniper/issues/62) and
  [#63](https://github.com/jdylanmc/pr-sniper/issues/63).

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
