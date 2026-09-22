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

## Scheduled polling acceptance (P4)

`tests/macos-polling-smoke.swift` is a separate Accessibility-only harness for
P4. It does not request Screen Recording or prove icon appearance. Window
metadata must nevertheless be available: unavailable AX/CG observations fail,
never count as zero windows. Compile its native entry point first; compilation
does not launch an app, probe permissions or establish native acceptance:

```sh
xcrun swiftc -parse-as-library -warnings-as-errors tests/macos-polling-smoke.swift \
  -o /absolute/owned-evidence/macos-polling-smoke
```

Only after receiving the native-session slot, its preflight checks existing
Accessibility access without requesting or changing permissions:

```sh
/absolute/owned-evidence/macos-polling-smoke --preflight
```

The integration owner prepares a fresh owned data directory with exactly one
enabled, permitted GitHub repository, a one-minute interval, launch-at-login
off and both automation gates off. Use the real stable watched-author ID for
an existing open non-draft fork PR; never create or modify provider data just
to make the test pass.

```sh
/absolute/owned-evidence/macos-polling-smoke \
  "/absolute/PR Sniper.app" "/absolute/owned-data-fixture" \
  "/absolute/expected-queue.json"
```

The optional expected-queue file contains public fixture facts:
`repository_id`, `repository_name`, `pull_request_id`, `number`, `title` and
`head_sha`, `account_id` and `watched_author_id`. Recheck them live immediately
before the run using only the approved bounded GETs. IDs are decimal strings.
The fixture must select that one watched author and disable the reviewer
trigger. The fixture file is private test evidence, not repository configuration.

The harness launches only its supplied bundle. It matches genuine owned
AXWindow geometry to healthy on-screen CG metadata and revalidates unique,
enabled, supported same-PID semantic menu/close actions. Ambiguity, application
aliases and unavailable observations fail rather than widening input authority.
It samples absent normal windows throughout a distinct successful scheduled
attempt, checking the host is still alive. This is bounded sampling, not a claim
that every intervening frame was observed.

Check Now must start and complete a new successful attempt before the **original**
next due time, with unchanged cadence and fresh attempt-specific success.
A scheduled crossover is an attribution failure, not a Check Now pass. Queue
text must be actual static text fully inside the window and known scroll/web
viewports; container accessible names and offscreen descendants do not count.
Status must visibly match the persisted completed health values. The current
timestamp matcher supports the frontend's en-US local-time rendering only;
another locale fails explicitly rather than changing the user's locale.
This evidence does not measure occlusion by unrelated applications.

When an expected revision is supplied, cursor account/remote/policy identity,
one exact revision tuple, watched-author eligibility and trust-confirmation
waiting are checked. Two completed reads must retain its original detection
time and trigger policy. An incremental cursor can exclude an unchanged PR on
the second provider response; native retained-state evidence does **not** prove
that response contained the same row again. Controlled Rust admission tests
remain the deterministic exact-replay proof. Without the expected input,
eligible-live queue acceptance remains explicitly unverified.

Settings/Queue reopen from the tray. Natural Quit requires the guarded action,
normal exact-process exit, a fresh `quit_requested` diagnostic and absence of
the recorded descendant lifetimes. Child snapshots interpret the native result
as a **PID count**, rejecting errors, invalid entries and a saturated buffer.
Traversal carries each parent's captured birth identity, rechecking it before
and after enumeration and after reading child identities. A changed/exited
parent fails the observation before its children acquire cleanup authority.
Already-recorded children remain owned across reparenting and exec.
These userspace checks are not atomic with enumeration or signaling; they do
not cover every transient process or already-reparented, unobserved child.
No AX message is sent to the exited process or used to infer tray disappearance. Preserve a
separate observation for that criterion. Polling, queue, cursor and configuration
bytes must stay unchanged through the captured next due time plus two seconds.
Snapshots, including diagnostics, are retained under the fresh profile's
`polling-smoke-evidence/` directory.

One 300-second monotonic budget covers the run, including a ten-second cleanup
reserve; individual waits cannot reset it. On failure, only the recorded app
and observed descendant lifetimes may be signaled, with PID-reuse checks.
Before every AX message, the timeout is set and checked on that **exact
reference**; application timeouts do not propagate to other/equal references.
The timeout is capped at two seconds and half the remaining work budget,
rounded down when necessary. Less than 100ms of work budget refuses another
call; zero is never used to reset the timeout. The budget is rechecked after
configuration and after each potentially blocking operation. Failure to set
the timeout prevents dispatch. The budget remains cooperative, not a preemptive
watchdog for a stalled OS call or filesystem.
Forced cleanup never satisfies natural Quit. Cleanup failure exits nonzero and
requires the owner to retain resource custody. No unbounded process wait,
global input, activation workaround or permission change is permitted.

Bind output to the exact commit, executable hash and isolated configuration.
The harness preserves its data directory and terminates only its own process
on failure. A timeout, forced cleanup, interruption, failed provider read or
missing Accessibility observation is not a native pass. Coordinate the run
with other agents; never launch a second instance to work around a failed test.

### Offline polling-harness guards

The separate entry point exercises the **same** pure guard predicates with
synthetic observations. It neither calls native-access APIs nor launches the
app or a provider. Run while another delivery owns the native slot:

```sh
xcrun swiftc -parse-as-library -warnings-as-errors -D POLLING_GUARD_TESTS \
  tests/macos-polling-smoke.swift tests/macos-polling-guards.swift \
  -o /absolute/owned-evidence/macos-polling-guards
/absolute/owned-evidence/macos-polling-guards
```

The selection covers stale success, scheduled crossover, duplicate/foreign/
disabled/unsupported actions, unavailable window snapshots, clipped text,
cursor identity, duplicate/replaced revision jobs, all four stable-state files,
PID-count boundaries, parent acquisition races, PID reuse versus reparenting,
per-reference timeout configuration/refusal and the monotonic cleanup reserve.
The macOS workflow compiles both polling entry points and executes only this
pure guard suite, using the runner's host architecture. It does not execute the
polling native harness or its preflight. These synthetic
checks and native compilation do not satisfy either full native acceptance
group. Retain complete output and real exit codes for each command.
