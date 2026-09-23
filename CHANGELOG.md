# Changelog

Notable changes are recorded here using [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
No release/versioning policy has been established yet.

## Unreleased

### Changed

- Anchor Settings to the approved compact sidebar, with repository selection,
  readable GitHub people lookup, preserved model selections, local review
  presets and separate review/publication switches. Settings drafts save together
  and detect conflicting updates; existing custom prompts, cron schedules and
  per-field overrides remain supported. Discovery reads only metadata beneath an
  explicitly chosen folder, never repository code. Agent/model health remains
  explicitly unavailable; no automation or approval is enabled by this change.
  Dialogs include an accessible older-WebKit fallback with viewport sizing
  preserved in production builds; delayed replies preserve
  open forms and cannot restore dismissed repository edits. Multiple local
  clones share one repository checkbox while retaining searchable clone paths.
  Conflicting drafts now offer explicit discard-and-reload recovery, custom
  schedule controls stay synchronized, and repeated remote URLs preserve Git's
  fetch identity while ambiguous remotes remain unavailable.
  See the [approved Settings design](docs/agent/design/settings-default.md).

### Added

- Connect through the registered PR Sniper GitHub OAuth App in the default browser
  using Authorization Code + S256 PKCE, a fixed localhost-only callback,
  single-use state correlation, bounded cancellation/timeout and explicit
  stable-identity confirmation before persistence. Multiple provider accounts retain
  account-addressed credentials with absolute expirations in macOS Keychain and
  independent serialized rotation boundaries. Restart restores and validates
  every account, retry-safely migrates the prior active-account layouts without
  orphaning secrets, recovers interrupted first-time account additions, and
  removes only the selected account on disconnect. OAuth App credentials use a
  generation-specific Keychain namespace; superseded GitHub App credentials
  require explicit reconnect and are cleaned only after the new pair is
  durable. Each OAuth session requests
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
