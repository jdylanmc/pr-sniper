# Windows native boundary development

This is existing-application groundwork, not a runnable Windows release.
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
- Microsoft Edge **WebView2 Evergreen Runtime** for eventual native app
  launch. It is not needed to run the persistence/policy harness.
- Windows PowerShell 5.1 (inbox): native ACL tests use its .NET filesystem
  access-control reader as an independent oracle, not a storage substitute.

Use official installers; see [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows).
These commands do not install or alter global tools/settings. Run them from
the repository in PowerShell, with those executables on the process PATH.

```powershell
node --version
rustup show active-toolchain
npm ci
npm run build
npm run test:release:windows
cargo test --manifest-path src-tauri\foundations\Cargo.toml --locked
cargo test --manifest-path src-tauri\foundations\Cargo.toml --locked --lib copilot::runtime::tests::bundled_runtime_handshakes_offline_without_credentials -- --exact --ignored --nocapture
cargo clippy --manifest-path src-tauri\foundations\Cargo.toml --locked --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri\Cargo.toml --all --check
```

`windows.yml` runs the frontend/shared tests and all four native boundary
commands. The harness compiles production source, shares the app's lockfile,
and uses disposable profiles beneath its working directory. It never opens
the real application's data or credential stores. Tests cover settings/policy
round trips, queue-selection state, invalid input, separate profiles and spaces
in paths, occupied staging files, real Windows sharing-lock replacement
failures/retry, protected current-user-only DACLs, and denied permission changes.
The harness also exercises production discovery: native junction fixtures prove
that nested junctions are skipped and junction-linked Git metadata remains
unavailable. These cases pass the existing implementation; no discovery bug or
broader claim about every Windows reparse-point type is inferred.
macOS retains its normal app tests, including the Unix mode assertions.
The harness also runs the production Windows startup adapter's native tests,
including exact-value ownership, registration status, rollback and access
denial, without enabling the actual application at login.

The harness also imports the real credential codec, account/rotation contracts,
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

The repository's `.gitattributes` keeps canonical doctrine Markdown and its
manifest LF even when Windows Git uses `core.autocrlf=true`. Rust embeds those
exact bytes and the doctrine helper verifies their SHA-256 hashes. The shared
checkout regression exercises real Git checkout with CRLF conversion enabled;
no CI-only setting or parser normalization is needed. Do not change the
canonical doctrine files or their verified hashes to repair checkout conversion.

## Full-app proof is still partial

Always retain the result of the unmodified application check separately:

```powershell
cargo check --manifest-path src-tauri\Cargo.toml --locked
```

The #57/#58 foundation check originally failed for the missing Windows icon,
startup import, macOS activation policy and native notification adapter.
#59 supplies the icon/configuration and native startup boundary. Until #60's
real notification adapter is integrated, the full-app check remains blocked:

| Site                                   | Missing native boundary                     | Owner |
| -------------------------------------- | ------------------------------------------- | ----- |
| `src-tauri/src/notifications/host.rs`  | macOS notification type/construction        | #60   |
| `src-tauri/src/notifications/tests.rs` | Unix permission assertion in full-app tests | #60   |

The account/runtime code now uses a native credential alias, keeps legacy
Keychain migration macOS-only, compares Windows environment keys
case-insensitively and supplies private profile/temp paths. Full review and
host-level regression execution still needs the remaining adapters; the focused
harness does not replace those tests. Login-registration and notification
permission fixtures also remain platform work. Do not infer authenticated
inference or actual GUI-parent console/cancellation acceptance from the offline
runtime handshake.

Before native-port convergence, the unmodified full-app check and native app
tests must pass without unsupported-success stubs. #59 owns actual
tray/window/login launch acceptance, #60 owns native notification/click proof,
and #61 owns complete Windows native CI/artifact acceptance. Signed Chocolatey
distribution remains separate. Green foundation checks prove none of those.

For persistence permissions and failure semantics, see
[ADR-0004](adr/0004-windows-persistence-foundations.md).

## Standalone executable

After native notification integration, build from PowerShell:

```powershell
cargo check --manifest-path src-tauri\Cargo.toml --locked --all-targets
cargo test --manifest-path src-tauri\Cargo.toml --locked --all-targets
cargo clippy --manifest-path src-tauri\Cargo.toml --locked --all-targets -- -D warnings
npm run test:settings
npm run build:windows
```

The last command embeds the production frontend; no Vite process is needed at
runtime. The unsigned executable is `src-tauri\target\release\pr-sniper.exe`,
or `<CARGO_TARGET_DIR>\release\pr-sniper.exe` when that variable is supplied.
The browser Store bridge likewise resolves `CARGO_TARGET_DIR` and the Windows
`.exe` suffix. Keep each worktree's build target separate. The existing
`npm run bundle` command still creates the macOS `.app`; it is not a Windows
build command. Windows icon regeneration, when artwork changes:
`powershell -NoProfile -File scripts\windows-icon.ps1`.

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
3. Open **Status**, **Review Queue** and **Settings** from that tray. Check that
   each is the actual application surface. **Check Now** on the unconfigured
   profile must not cause provider actions. Close each window; verify its
   native window hides while the owned host and tray remain.
4. Launch the exact executable again with the same profile. Verify the second
   process exits, one host/icon remains and no additional startup window
   appears. Reopen a hidden window from the tray.
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
