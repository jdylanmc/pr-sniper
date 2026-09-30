# Windows native boundary development

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

At this foundation boundary the actual check fails in `src-tauri/build.rs`:
Tauri requires `icons/icon.ico`, which the macOS bundle does not supply.
Windows bundle configuration/icon belongs to **#59**, not this harness.

A diagnostic-only run with a disposable icon override (not committed or used
by CI) exposed these additional errors. They are not a successful build:

| Site                                  | Missing native boundary                                                   | Owner |
| ------------------------------------- | ------------------------------------------------------------------------- | ----- |
| `src-tauri/src/lib.rs`                | `startup` registration import; `ActivationPolicy`/`set_activation_policy` | #59   |
| `src-tauri/src/notifications/host.rs` | macOS notification type/construction                                      | #60   |

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
