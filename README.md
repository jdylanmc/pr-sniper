# PR Sniper

A macOS menu-bar foundation for human-owned pull request review. Built with
Tauri 2, Rust and vanilla TypeScript. The tray exposes **Status**, **Review
Queue**, **Settings**, **Setup Doctor** and **Quit PR Sniper**.

This increment does **not** connect to GitHub, poll repositories, run agents,
publish comments or perform setup checks. These surfaces say so explicitly;
Check Now is disabled. Product scope lives in the
[approved specification](docs/agent/specs/pr-sniper-mvp.nano.md), not this
implementation summary.

## Develop on macOS

Requirements: macOS 12+, Xcode Command Line Tools (or full Xcode), Node
24.20.0 and Rust 1.98.1 via rustup. `.node-version`, `rust-toolchain.toml`,
`package-lock.json` and `src-tauri/Cargo.lock` pin the baseline.
Install prerequisites yourself using their official installers; these commands
do not install global tools.

```sh
npm ci
npm run tauri -- dev
```

If Homebrew's rustup is present but `cargo`/`rustc` are not on PATH, add the
installed toolchain's `bin` directory to your current shell. Do not confuse
missing shell shims with a missing Rust installation.

No main window opens at startup. Click the crosshair in the macOS menu bar.
Closing any window leaves the tray running; **Quit PR Sniper** ends the process.
The local frontend server is only for development; terminate the development
command too when finished.

For isolated runs, use an absolute application-data directory:

```sh
PR_SNIPER_DATA_DIR="$(mktemp -d)" npm run tauri -- dev
```

That override never changes login items and disables the startup checkbox.
Launch-at-login registration requires an explicit Settings change in a normal
installed-app run. The checkbox shows your saved request, not effective macOS
state. The separate registration status validates the owned plist and current
executable; even a valid registration may be disabled by macOS Login Items.
Startup only inspects registration: it does not enable, disable or reapply the
saved preference.

## Check and bundle

```sh
npm run format:check
npm run lint
npm test
npm run bundle
```

`npm run format` formats owned application sources. `npm run build` checks and
builds the frontend. CI executes the checks and builds a real macOS `.app`;
native interactive proof is separate from compile and filesystem tests.
See [native acceptance](tests/macos-acceptance.md) for reproducible
install/launch/close/quit and scoped visual checks, including capability limits.

The bundle is `src-tauri/target/release/bundle/macos/PR Sniper.app`.
For a local user installation, copy it with Finder to `~/Applications` (create
that directory if needed), then open it. Quit any earlier PR Sniper instance
first. This is a local development bundle, not a signed/notarized distribution:
Gatekeeper may require explicit approval under Privacy & Security for a
downloaded CI artifact. Do not disable Gatekeeper globally. Production signing,
notarization, updates, Homebrew and Windows packages are out of scope.

## Local data and diagnostics

The default data root is
`~/Library/Application Support/com.jdylanmc.pr-sniper/`.
`config/settings.json` stores only the startup preference.
`state/diagnostics.jsonl` records timestamped, fixed-schema host events, capped at
256 KiB plus one rotated file. **Settings > Open redacted diagnostics** opens
an in-app reader, not an arbitrary filesystem or shell interface.
Invalid settings are reported rather than silently reset or overwritten.

No credential is requested or stored in this foundation. Future credentials
belong in macOS secure storage, never config, state or diagnostics.
See the [bounded architecture decision](docs/adr/0001-macos-foundation.md).

## Crosshair assets

The canonical crosshair is original vector geometry, not a private-use font.
`src/crosshair.svg` serves the local surfaces, and the native tray uses a
transparent template PNG that macOS adapts to its appearance.
Regenerate checked-in PNG/ICNS assets on macOS with
`swift scripts/generate-icons.swift`.
