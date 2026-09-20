# Changelog

Notable changes are recorded here using [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
No release/versioning policy has been established yet.

## Unreleased

### Added

- Locally buildable macOS menu-bar foundation with a font-independent crosshair,
  queue/status/setup placeholders, persistent startup preference, explicit
  opt-in launch at login, and redacted host diagnostics accessible from Settings.
  Closing windows keeps the tray alive; Quit ends the host. Includes developer
  setup, storage regression coverage and a macOS build/check pipeline. This
  foundation does not yet monitor repositories or perform reviews. See [#12](https://github.com/jdylanmc/pr-sniper/issues/12).
