# macOS foundation acceptance

This procedure covers the focused P1 contract in
[#12](https://github.com/jdylanmc/pr-sniper/issues/12#issuecomment-5746576429),
not the historical Windows/distribution scope or later monitoring features.
Automated storage tests do not establish native window or menu-bar behavior.

## Safety and evidence

- Coordinate one native app session at a time. Do not launch over another
  developer's instance. Record the candidate commit, macOS version, build
  command, installed bundle path and actual executable PID.
- Use both supported isolation variables: an absolute temporary
  `PR_SNIPER_DATA_DIR` and a unique test-owned
  `PR_SNIPER_KEYCHAIN_SERVICE` beginning with
  `com.jdylanmc.pr-sniper.tests.`. An isolated launch missing either value must
  fail rather than falling back to production data or credentials.
- Never turn on the developer's real login item to test persistence. Storage
  tests may save `launch_at_login: true` in their own temporary fixture because
  they do not call the operating-system login service.
- Capture only the app/menu region for visual evidence; avoid unrelated screen
  content. Record permission failures as unverified, not passed.
- A screenshot proves appearance, not persistence or process termination.
  Source text, successful compilation and an isolated lifecycle model are not
  substitutes for observing the installed native app.

## Behavior checks

| Requirement                | Action                                                                                           | Required observation                                                                                                                                    |
| -------------------------- | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| AC-001 / PR-001            | Build the local macOS bundle, copy into an isolated installation directory and launch that copy. | Native crosshair appears in the menu bar; no persistent main window; actual installed process remains alive.                                            |
| AC-001 / PR-001            | Open the tray menu.                                                                              | Queue, Settings, Status, Setup Doctor and Quit entry points are reachable.                                                                              |
| AC-001 / PR-001            | Open Queue, Status and Setup Doctor individually.                                                | Surfaces render; unimplemented monitoring, provider and setup operations are explicitly unavailable, not falsely reported successful.                   |
| AC-001 / PR-002            | Open Settings, close its native window, reopen it from the tray; repeat with Queue.              | Window disappears, same process and tray remain alive, and each surface opens again.                                                                    |
| AC-001 / PR-001            | Observe login preference on a fresh isolated profile, then restart without changing it.          | Preference remains off; launching, opening Settings and restarting do not enable a login item.                                                          |
| AC-018 / PR-039 host slice | Open diagnostics from Settings.                                                                  | Readable host diagnostics; no tokens, credentials or arbitrary raw error payloads. Configuration and diagnostics/state have distinct storage locations. |
| AC-015 / PR-032            | Inspect the native tray crosshair in light and dark menu-bar appearance.                         | Legible canonical crosshair, not a missing-glyph box or font-dependent text; preserve cropped evidence for both appearances if available.               |
| AC-001 / PR-002            | Choose Quit from the tray.                                                                       | Recorded process exits, tray disappears, app-owned child work terminates. Do not count force termination as a successful Quit.                          |
| AC-018                     | Relaunch the installed copy with the same isolated profile.                                      | Saved settings remain; previous diagnostics remain readable.                                                                                            |

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

## Human-authorized GitHub OAuth App acceptance

Run this only after the candidate's independent review and exact-head CI pass.
Use one preserved isolated profile for the full acceptance: a unique absolute
app-data directory and unique test-owned Keychain service. Reuse both values
across restart and refresh checks, then delete only those test-owned artifacts.
Do not create another OAuth App, expose token contents, or perform provider
mutations to probe capability. Device authorization does not use a callback;
issue #36 remains required product work outside this authentication path.

1. Record the candidate commit and build/install the candidate bundle. Choose
   an absolute temporary data directory and a unique service such as
   `com.jdylanmc.pr-sniper.tests.oauth-<uuid>`, then launch the bundle
   executable with both `PR_SNIPER_DATA_DIR` and
   `PR_SNIPER_KEYCHAIN_SERVICE` set. Confirm `gh` is absent or signed out.

   ```sh
   PR_SNIPER_DATA_DIR="/absolute/test-profile" \
   PR_SNIPER_KEYCHAIN_SERVICE="com.jdylanmc.pr-sniper.tests.oauth-<uuid>" \
   "/absolute/PR Sniper.app/Contents/MacOS/pr-sniper"
   ```
2. Confirm OAuth App application ID `3878184` has public client ID
   `Ov23lidoL3QovWyfxnA4`, expiring tokens and device flow enabled, and no
   client secret; do not mutate the registration during acceptance. Open
   Settings, choose **Add GitHub account**, confirm PR Sniper shows a one-time
   user code and opens `https://github.com/login/device` in the default system
   browser. If GitHub does not return `verification_uri_complete`, confirm PR
   Sniper does not invent a prefilled-code URL. Use the displayed code, verify
   the consent clearly requests GitHub's broad `repo` scope for public and
   private repository access, complete authorization, return to PR Sniper,
   verify only the stable account ID/login is shown, then choose **Confirm**.
   Evidence may show the user code only when required to complete this
   test-owned attempt; never capture the secret device code or any token.
   Add a second GitHub account
   through **Use a different account** and confirm both acting identities
   remain visible concurrently. Confirm browser-open, cancellation, denial,
   expiry, disabled-registration, network and provider failures remain
   distinguishable if any occur;
   never capture device codes, tokens or credentials in evidence.
3. For each account, choose **Load repositories for _login_**. Confirm owned,
   collaborator and organization repositories available to that user appear,
   including authorized private repositories, without installing PR Sniper on
   each repository. Confirm no repository is monitored merely because it was
   listed. Use a repository visible to both accounts and confirm
   no account is auto-selected: explicitly choose one acting account, save, and
   verify the stable account/repository IDs persist after reopening
   Settings. Verify the repository and read its complete pull-request metadata
   with `gh` still unavailable; every repository/action surface must show the
   acting provider account.
   Also manually enter a readable third-party public repository that does not
   appear in either account's affiliation list. Choose each acting account in
   turn, confirm direct authenticated resolution supplies the stable repository
   ID, save both bindings, and verify each independently.
4. Quit normally and relaunch. Confirm both accounts restore independently from
   Keychain, each `/user` identity is revalidated, the selected repository
   remains bound to the same account and stable IDs, and
   repository/people/pull-request reads use only that bound OAuth session.
5. For real rotation, leave the isolated profile intact until the issued access
   token expires (GitHub currently documents an eight-hour lifetime). After
   expiry, trigger **Load repositories** once. Confirm the Keychain
   item's modification time advances, identity and repository access continue,
   and no reconnect prompt appears. Do not inspect or export the Keychain secret.
6. Disconnect one account. Confirm the other account remains connected and its
   bound repository evidence remains usable, while repositories bound to the
   removed account show needs-attention and never transfer automatically.
   Reconnect the removed account and explicitly rebind one repository. Then
   disconnect both accounts, quit and relaunch, and confirm their account
   registry entries and account-addressed Keychain secrets are absent before
   deleting the isolated data directory and exact test-owned Keychain services.

Record each step as **met**, **unmet** or **unverified**. A fixture refresh,
Keychain unit test, mocked provider response, sign-in without repository
discovery, or successful `gh` read does not satisfy this live acceptance.

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
and a unique `PR_SNIPER_KEYCHAIN_SERVICE`,
checks native visible windows and menu actions, closes and reopens Settings
and Review Queue, then selects Quit PR Sniper and waits for the exact PID to
exit. The host's explicit data-root override disables operating-system
autostart mutation. Never run this harness against a version lacking that
isolation behavior.

Failed runs terminate only their own process and remove only their fresh data
fixture and exact test-owned Keychain services. Forced cleanup is not a
successful Quit assertion. This harness does
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
