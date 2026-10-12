# macOS development

Reference for building, signing, installing and verifying PR Sniper on macOS. Agent workflow and the full command list: [AGENTS.md](../AGENTS.md).

## Requirements

- macOS 13.5+ (`tauri.conf.json` `minimumSystemVersion`; matches the bundled Copilot runtime deployment target). JS/CSS target Safari 15; no runtime polyfills.
- Xcode Command Line Tools, Node `24.20.0` (`.node-version`), Rust `1.98.1` via rustup (`rust-toolchain.toml`). Lock files: `package-lock.json`, `src-tauri/Cargo.lock`.
- Use official installers. Repository commands install no globals.
- Homebrew rustup may leave `cargo`/`rustc` off `PATH`: add the toolchain `bin` to the shell.

## Run

```sh
npm ci
npm run tauri -- dev
```

- `npm run dev:signed`: signs the dev build with an Apple Development identity so Keychain **Always Allow** survives rebuilds. Override: `PR_SNIPER_DEV_SIGN_IDENTITY="Apple Development: ..."`. Ad-hoc builds re-prompt.
- Isolated profile (real data and login items untouched): [local-data.md](local-data.md#root).
- The local frontend server is dev-only. Terminate the dev command when done.

## Checks

```sh
npm run format:check
npm run lint
npm test
npm run bundle
```

CI (`.github/workflows/macos.yml`) runs the same. Windows: [windows-development.md](windows-development.md).

## Bundle and install

- Output: `src-tauri/target/release/bundle/macos/PR Sniper.app`.
- Local install: quit any running instance, copy the bundle in Finder to `~/Applications` (create it if needed).
- Local and CI bundles are ad-hoc signed with the bundle identity (needed for notifications). Not Developer ID, not notarized. A downloaded CI artifact may need approval in Privacy & Security; never disable Gatekeeper globally.
- Tagged releases are Developer ID signed, notarized, stapled, shipped through Homebrew: [releases.md](releases.md).

## Verification

- Bundle compile is not native acceptance. Native window boundary: [tests/macos-acceptance.md](../tests/macos-acceptance.md) and the compiled smoke harness.
- Notifications need the packaged app: [tests/macos-notifications.md](../tests/macos-notifications.md), [review-queue.md](review-queue.md#notifications).
- Settings with real Rust storage: `npm run test:settings` ([tests/settings/README.md](../tests/settings/README.md)). Browser proof is not native macOS proof.
- Icons: canonical crosshair `src/crosshair.svg`; tray uses a transparent template PNG. Regenerate: `swift scripts/generate-icons.swift`.
