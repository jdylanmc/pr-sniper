# Windows application development

This is the existing application, not a signed Windows release.
No provider sign-in, real user data, real startup registration or notification
delivery is needed for the checks below. Startup tests own unique synthetic
registry keys outside the Windows Run key.

## Prerequisites

- Windows x64 with Microsoft Visual Studio 2022 / Build Tools and the
  **Desktop development with C++** workload, MSVC x64 tools and Windows SDK.
- Rust **1.98.1**, `x86_64-pc-windows-msvc`, through rustup; the repository
  `rust-toolchain.toml` also selects rustfmt and Clippy.
- Node **24.20.0** from `.node-version` (package compatibility permits Node
  24.20+ below 25), plus npm.
- Python **3.13** for the existing portable release tests.
- Microsoft Edge **WebView2 Evergreen Runtime** for native app launch.
- Windows PowerShell 5.1 (inbox): native ACL tests use its .NET filesystem
  access-control reader as an independent oracle, not a storage substitute.
  A default Windows client policy (`Restricted`) refuses `.ps1` files. The npm
  scripts bypass it for their own process only. Run other repository scripts
  with `powershell -NoProfile -ExecutionPolicy Bypass -File <script>`; no
  machine or user policy changes.

Use official installers; see [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows).
These commands do not install or alter global tools/settings. Run them from
the repository in PowerShell, with those executables on the process PATH.

```powershell
node --version
rustup show active-toolchain
npm ci
npm run build
npm run test:release:windows
npm run format:check
cargo check --manifest-path src-tauri\Cargo.toml --locked --all-targets
cargo test --manifest-path src-tauri\Cargo.toml --locked --all-targets -- --nocapture
cargo test --manifest-path src-tauri\Cargo.toml --locked --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture
cargo clippy --manifest-path src-tauri\Cargo.toml --locked --all-targets -- -D warnings
npm run test:settings
npm run build:windows
```

`windows.yml` runs frontend/shared checks, full native application checks,
the offline runtime handshake, browser regressions and the standalone build.
Tests use disposable profiles and exact owned native fixtures. They never open
the real application's data or credential stores. Tests cover settings/policy
round trips, queue-selection state, invalid input, separate profiles and spaces
in paths, occupied staging files, real Windows sharing-lock replacement
failures/retry, protected current-user-only DACLs, and denied permission changes.
The suite also exercises production discovery: native junction fixtures prove
that nested junctions are skipped and junction-linked Git metadata remains
unavailable. These cases pass the existing implementation; no discovery bug or
broader claim about every Windows reparse-point type is inferred.
macOS retains its normal app tests, including the Unix mode assertions.
The suite also runs the production Windows startup adapter's native tests,
including exact-value ownership, registration status, rollback and access
denial, without enabling the actual application at login.

The suite exercises the real credential codec, account/rotation contracts,
OAuth implementation, native secure store, Copilot catalog runtime and operation
fences. Windows tests create unique `com.jdylanmc.pr-sniper.tests.*` namespaces
in the current user's Credential Manager and delete their owned records,
including registries. They never enumerate/read existing user credentials.
Capacity failures are explicit: each serialized record is limited to 2,560
bytes, including codec overhead. Oversized records do not spill to files or
replace prior credentials. See [ADR-0005](adr/0005-windows-accounts-runtime.md).

The explicit ignored smoke runs the bundled SDK 1.0.14 / CLI 1.0.85 without
credentials or inference, with offline mode and a rejecting loopback provider.
It checks unauthenticated status/model rejection, no provider requests, clean
shutdown and temporary cleanup. Node is needed only for synthetic transport
fixtures; they use `node.exe` and native Windows child-exit checks. These are
not application launch, real grant acceptance or GUI console-window evidence.

The repository's `.gitattributes` keeps text sources LF even when Windows Git
uses `core.autocrlf=true`, so the same formatter checks run on both platforms.
Binary artwork remains binary. Rust embeds app defaults from `src-tauri/doctrines`;
the separate skill doctrine helper verifies its catalog's SHA-256 hashes.
The shared checkout regression uses real Git checkout with CRLF conversion
enabled and checks text, doctrine
and binary samples. No CI-only formatting bypass is needed; do not change
canonical doctrine contents or hashes to repair checkout conversion.

## Native proof boundaries

The #57/#58 focused foundation harness remains available for small adapter
checks. The normal Windows workflow now targets the actual application instead
of substituting that harness. It includes review and Host regression fixtures,
native startup and notification contracts, and the browser Store bridge.
Do not run both full suites merely to repeat native credential tests.

The account/runtime code now uses a native credential alias, keeps legacy
Keychain migration macOS-only, compares Windows environment keys
case-insensitively and supplies private profile/temp paths. Full review and
host-level regressions exercise the same constrained runtime helpers.
Login-registration fixtures use native owned keys; genuine macOS-only
integration tests remain macOS-only. Do not infer authenticated
inference or actual GUI-parent console/cancellation acceptance from the offline
runtime handshake.

Before declaring convergence complete, exact-candidate checks, independent
review and native application acceptance must pass. #59 owns actual
tray/window/login launch acceptance, #60 owns native notification/click proof,
and #61 owns complete Windows native CI/artifact acceptance. Signed Chocolatey
distribution remains separate. A successful compile or test run is not GUI,
logon or notification-delivery proof.

For persistence permissions and failure semantics, see
[ADR-0004](adr/0004-windows-persistence-foundations.md).

## Standalone executable

Build from PowerShell:

```powershell
npm run build:windows
# Optional standalone debug candidate:
npm exec tauri build -- --debug --no-bundle -- --locked
```

Both commands embed the production frontend; no Vite process is needed at
runtime. The unsigned executable is `src-tauri\target\release\pr-sniper.exe`,
or `<CARGO_TARGET_DIR>\release\pr-sniper.exe` when that variable is supplied.
The browser Store bridge likewise resolves `CARGO_TARGET_DIR` and the Windows
`.exe` suffix. Keep each worktree's build target separate. The existing
`npm run bundle` command still creates the macOS `.app`; it is not a Windows
build command. Windows icon regeneration, when artwork changes:
`powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows-icon.ps1`.

## Windows CI artifacts

The existing [Windows workflow](../.github/workflows/windows.yml) runs on pull
requests and main pushes with read-only repository permission and no signing,
publication or provider credentials. The combined frontend/Store build and full
browser suite has a 20-minute step budget within the existing 60-minute job
limit. Individual tests, application deadlines and required checks are unchanged;
macOS release credentials and publication gates remain separate. Failures stop
the job; stdout/stderr remains in the Actions logs.
Native credential tests log only exact synthetic target claims and fixed
outcomes, never token contents. No application profile, native credential store,
browser state directory or entire build target is uploaded.

After the release build, `scripts/windows-artifact.ps1` verifies the actual PE
header is an x64 GUI executable, requires a clean tracked source tree and checks
`GITHUB_SHA` against Git HEAD. It stages only `pr-sniper.exe`, `build.json`
(version, commit, target, SHA-256 and unsigned status) and `SHA256SUMS`.
The artifact is named `pr-sniper-windows-x64-<full-commit>`. Locally the script
honors `CARGO_TARGET_DIR`; its `windows-artifact` destination must not already
exist, avoiding accidental replacement of a prior candidate.

In GitHub Actions, select the successful **Windows native application** run for
the exact source commit and download that artifact. Extract the three files,
compare `Get-FileHash .\pr-sniper.exe -Algorithm SHA256` with `SHA256SUMS` and the
metadata, and record the downloaded executable's hash during the interactive
acceptance below. On a pull request, `github.sha` identifies GitHub's tested
merge candidate, not necessarily the head branch commit.

An uploaded unsigned executable is not a public release, signed installer,
Chocolatey package or successful launch receipt. WebView2 remains a runtime
prerequisite. A remote green run and its downloaded artifact's interactive
launch must be observed separately before closing #61 acceptance.

The workflow also bundles an unsigned current-user NSIS candidate and performs
installer/Chocolatey acceptance in a separate fresh hosted VM. The bundler patches bundle-type metadata for the installer, then restores the
standalone binary. Installer provenance hashes the extracted NSIS payload
separately from that restored standalone artifact.
This retains all native/browser checks and the existing executable artifact.
See [Windows packaging](windows-packaging.md) for build/pack-only commands,
per-user ownership, exact artifact provenance, hosted-only install/upgrade/
uninstall checks and still-blocked trusted signing/publication. Never execute
an installer on a shared developer machine as an incidental packaging test.

## Windows application acceptance

These are required native observations, not assertions that an untested
candidate works. Record exact commit, executable hash, current-user identity,
Windows/WebView2 versions, process IDs and outcomes. Do not authenticate with
real providers, enable review/publication automation or mutate existing user
credentials for these checks.

1. Ensure no existing PR Sniper host is running before the isolated test.
   Ask its owner to Quit through the real menu; never kill processes by name.
   Prepare a fresh absolute profile beneath this checkout, set
   `PR_SNIPER_DATA_DIR` to it and set `PR_SNIPER_KEYCHAIN_SERVICE` to a unique
   `com.jdylanmc.pr-sniper.tests.<uuid>` namespace. Do not use another profile's
   namespace or enumerate credentials.
2. Launch the production executable without Vite. Verify exactly one tray icon,
   no startup main window and no console. Use the actual taskbar/overflow area,
   including native UI Automation if appropriate, not a replacement test menu.
3. Left-click the tray crosshair: exactly one compact panel opens. Toggle it
   closed/reopen and check no focus-loss race immediately reopens it. Navigate
   **Queue**, **Running**, **Reviewed**, **Settings** and one exact saved job
   detail; Back restores its row/scroll/focus. Retain an unsaved Settings draft
   across tabs, Escape, outside click, custom Close and native Alt+F4; each hides
   the same HWND while the host/tray remain. Right-click retains **Status**,
   **Review Queue**, **Check Now**, **Settings**, **Diagnostics** and **Quit**.
   Status/Diagnostics are routes within that panel, not new webviews.
   **Check Now** on the unconfigured profile must not cause provider actions.
   Check top/bottom/side taskbar and overflow placement at 100/125/150/200%
   scaling and a negative-coordinate secondary display: the panel fits the
   actual monitor work area without double-scaling or horizontal clipping.
   A native folder dialog and a synthetic external-auth flow must not discard
   the draft or strand the panel. Observe native picker focus restoration.
4. Launch the exact executable again with the same profile. Verify the second
   process exits, one host/icon remains and no additional startup window
   appears. Reopen the retained panel from the tray. Exact notification routes
   preserve the registered profile and requested iteration/job, or show an
   explicit unavailable destination; never substitute another item.
5. In isolated Settings, verify Windows wording and disabled login mutation.
   Record the actual Run value before/after if it already exists, without
   replacing it. Read-only inspection is not proof of Windows startup launch.
   Native automated startup fixtures cover enable/disable, reopen/status and
   rollback on an exclusively owned test key; never point them at Run.
6. Choose **Quit PR Sniper**. Observe the exact host PID exit, its tray icon
   removal and owned work cancellation. No test Vite/fixture processes should
   remain. GUI-parent Copilot checks stay synthetic/offline; do not infer the
   absence of child console flashes from a terminal-run SDK test.
7. Remove only this run's profile and exact synthetic credentials, if any
   were created. Existing accounts/namespaces are not test cleanup targets.
   The integration owner separately launches the normal app after isolated
   proof and owns final readiness/leave-running.

Actual launch-at-login acceptance requires an explicit normal-profile opt-in:
compare the saved request with the one owned Run value, restart the app, then
opt out and verify removal. Do not toggle a user's existing preference merely
to gather evidence. `registered` does not mean effective: Windows **Startup
Apps** can disable it independently. A fresh logon with that OS setting enabled
is separate human-authorized proof; these automated tests neither log out the
user nor change StartupApproved policy.

See [ADR-0006](adr/0006-windows-tray-startup.md) for the host and registration
contract. Notifications have their own #60 acceptance evidence.
