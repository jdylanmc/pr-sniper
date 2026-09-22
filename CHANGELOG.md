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
  See the [approved Settings design](docs/agent/design/settings-default.md).

### Added

- Verify configured GitHub repositories with the current CLI account and stable
  account ID, distinguish read access from comment scope, and read complete
  paginated PR, reviewer and changed-file metadata in Settings. CLI, identity,
  permission, rate-limit and incomplete-read failures remain visible. Credentials
  stay in memory; no sign-in, provider mutation or automation enablement occurs.
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
