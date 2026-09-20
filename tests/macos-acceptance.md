# macOS foundation acceptance

This procedure covers the focused P1 contract in
[#12](https://github.com/jdylanmc/pr-sniper/issues/12#issuecomment-5746576429),
not the historical Windows/distribution scope or later monitoring features.
Automated storage tests do not establish native window or menu-bar behavior.

## Safety and evidence

- Coordinate one native app session at a time. Do not launch over another
  developer's instance. Record the candidate commit, macOS version, build
  command, installed bundle path and actual executable PID.
- Use an isolated temporary app-data location supported by the host. If that
  seam is unavailable, record the limitation before launch; do not overwrite
  existing configuration, state, credentials or login items.
- Never turn on the developer's real login item to test persistence. Storage
  tests may save `launch_at_login: true` in their own temporary fixture because
  they do not call the operating-system login service.
- Capture only the app/menu region for visual evidence; avoid unrelated screen
  content. Record permission failures as unverified, not passed.
- A screenshot proves appearance, not persistence or process termination.
  Source text, successful compilation and an isolated lifecycle model are not
  substitutes for observing the installed native app.

## Behavior checks

| Requirement | Action | Required observation |
| --- | --- | --- |
| AC-001 / PR-001 | Build the local macOS bundle, copy into an isolated installation directory and launch that copy. | Native crosshair appears in the menu bar; no persistent main window; actual installed process remains alive. |
| AC-001 / PR-001 | Open the tray menu. | Queue, Settings, Status, Setup Doctor and Quit entry points are reachable. |
| AC-001 / PR-001 | Open Queue, Status and Setup Doctor individually. | Surfaces render; unimplemented monitoring, provider and setup operations are explicitly unavailable, not falsely reported successful. |
| AC-001 / PR-002 | Open Settings, close its native window, reopen it from the tray; repeat with Queue. | Window disappears, same process and tray remain alive, and each surface opens again. |
| AC-001 / PR-001 | Observe login preference on a fresh isolated profile, then restart without changing it. | Preference remains off; launching, opening Settings and restarting do not enable a login item. |
| AC-018 / PR-039 host slice | Open diagnostics from Settings. | Readable host diagnostics; no tokens, credentials or arbitrary raw error payloads. Configuration and diagnostics/state have distinct storage locations. |
| AC-015 / PR-032 | Inspect the native tray crosshair in light and dark menu-bar appearance. | Legible canonical crosshair, not a missing-glyph box or font-dependent text; preserve cropped evidence for both appearances if available. |
| AC-001 / PR-002 | Choose Quit from the tray. | Recorded process exits, tray disappears, app-owned child work terminates. Do not count force termination as a successful Quit. |
| AC-018 | Relaunch the installed copy with the same isolated profile. | Saved settings remain; previous diagnostics remain readable. |

## Automated behavior evidence

Run from the repository root:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked \
  --test settings_persistence --test diagnostics --test startup_registration
```

If Rust is installed but not on the shell path, use the existing selected
toolchain rather than installing a global package. On a rustup host:

```sh
export PATH="$(dirname "$(rustup which cargo)"):$PATH"
```

The tests write to unique temporary directories and read through fresh storage
instances. Persistence must fail if saving becomes a no-op or loading always
returns defaults. Additional cases cover explicit opt-out, invalid settings,
visible I/O errors, independent configuration/state, append-only typed
diagnostics, safe rejection of synthetic secret payloads and rotation before a
write would exceed the 256 KiB current-log limit. They do not touch macOS
launch-at-login configuration or access credentials.

LaunchAgent tests use only fixture plist/executable paths: missing, valid,
malformed, stale and unreadable registrations, explicit request/removal, escaped
paths and failed-save rollback. A valid registration is not evidence of
effective macOS launch state. Settings must keep the saved request and validated
registration status distinct and warn that macOS Login Items can still prevent
launch.

Retain command output and distinguish a behavior assertion failure from a
missing compiler, missing package, compilation failure or unexecuted test.
The integration owner supplies the repository's build, lint and CI commands.

### Native lifecycle harness

First perform a non-launching permission check:

```sh
swift tests/macos-native-smoke.swift --preflight
```

It never requests or changes permissions. Missing Accessibility or Screen
Recording access is a failed preflight and **unverified** native acceptance.
Do not change host permissions merely to turn that result green.

After coordinating with the implementation owner, use an installed local copy:

```sh
swift tests/macos-native-smoke.swift "/absolute/installation/PR Sniper.app"
```

The harness refuses a concurrent instance with the same bundle identifier,
launches the bundle's actual executable with a fresh `PR_SNIPER_DATA_DIR`,
checks native visible windows and menu actions, closes and reopens Settings
and Review Queue, then selects Quit PR Sniper and waits for the exact PID to
exit. The host's explicit data-root override disables operating-system
autostart mutation. Never run this harness against a version lacking that
isolation behavior.

Failed runs terminate only their own process and remove only their fresh data
fixture. Forced cleanup is not a successful Quit assertion. This harness does
not prove visible icon quality, diagnostics content, login-item state or
child-process cleanup; retain the manual checks above. Runtime automation must
be exercised successfully before reporting its lifecycle checks as met.

## Completion record

Report each row as **met**, **unmet** or **unverified**, with the decisive
observation or artifact and its candidate commit. Include failed attempts,
environment/permission constraints, any skipped appearance mode and cleanup.
Stop only the recorded test-owned PID if failure leaves it running; never kill
processes by name. Remove only the exact temporary fixture/install directory
created for this run after preserving needed evidence. Do not delete the
developer's application or application data.
