# Windows foundation development

This is existing-application groundwork, not a runnable Windows release.
No provider sign-in, real user data, startup registration or notification
delivery is needed for the checks below.

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
cargo clippy --manifest-path src-tauri\foundations\Cargo.toml --locked --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri\Cargo.toml --all --check
```

`windows.yml` runs the frontend/shared tests and the two native foundation
commands. The harness compiles production source, shares the app's lockfile,
and uses disposable profiles beneath its working directory. It never opens
the real application's data or credential stores. Tests cover settings/policy
round trips, queue-selection state, invalid input, separate profiles and spaces
in paths, occupied staging files, real Windows sharing-lock replacement
failures/retry, protected current-user-only DACLs, and denied permission changes.
macOS retains its normal app tests, including the Unix mode assertions.

Keep doctrine source checkouts LF, including on Windows. If preparing a
worktree or staging with Git's CRLF conversion enabled, use per-command
`git -c core.autocrlf=false ...`; do not change the canonical doctrine files
or their verified hashes.

## Full-app proof is still partial

Always retain the result of the unmodified application check separately:

```powershell
cargo check --manifest-path src-tauri\Cargo.toml --locked
```

At this foundation boundary the actual check fails in `src-tauri/build.rs`:
Tauri requires `icons/icon.ico`, which the macOS bundle does not supply.
Windows bundle configuration/icon belongs to **#59**, not this harness.

A diagnostic-only run with a disposable icon override (not committed or used
by CI) exposed these additional errors. They are not a successful build:

| Site                                                       | Missing native boundary                                                   | Owner |
| ---------------------------------------------------------- | ------------------------------------------------------------------------- | ----- |
| `src-tauri/src/copilot/backend.rs`, `src-tauri/src/lib.rs` | Direct `MacKeychainStore` imports, fields and construction                | #58   |
| `src-tauri/src/copilot/runtime.rs`                         | `NSProcessInfo` runtime eligibility check                                 | #58   |
| `src-tauri/src/lib.rs`                                     | `startup` registration import; `ActivationPolicy`/`set_activation_policy` | #59   |
| `src-tauri/src/notifications/host.rs`                      | macOS notification type/construction                                      | #60   |

Other known adapter work (not proven by this compile probe) includes Windows
Copilot environment isolation and process cleanup, Unix-only credential/process
fixtures, login-registration fixtures and notification permission fixtures.
Windows environment-key filtering must be case-insensitive; preserving the
required Windows process environment belongs to #58. Do not silently remove
`SystemRoot` or infer authenticated inference from an offline runtime handshake.

Before native-port convergence, the unmodified full-app check and native app
tests must pass without unsupported-success stubs. #59 owns actual
tray/window/login launch acceptance, #60 owns native notification/click proof,
and #61 owns complete Windows native CI/artifact acceptance. Signed Chocolatey
distribution remains separate. Green foundation checks prove none of those.

For persistence permissions and failure semantics, see
[ADR-0004](adr/0004-windows-persistence-foundations.md).
