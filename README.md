# PR Sniper

A macOS menu-bar application for human-owned pull request review. Built with
Tauri 2, Rust and vanilla TypeScript. The tray exposes **Status**, **Review
Queue**, **Settings** and **Quit PR Sniper**.

Settings can explicitly verify a configured GitHub connection and read complete
pull-request metadata. It also manages independent Copilot AI accounts with
browser sign-in and per-Agent account/model selection; see
[Copilot Settings](docs/copilot-settings.md). This increment does **not** poll repositories, run agents,
publish comments or perform automated setup. Check Now is disabled. Product scope lives in the
[approved specification](docs/agent/specs/pr-sniper-mvp.nano.md), not this
implementation summary.

## Develop on macOS

Requirements: macOS 12+, Xcode Command Line Tools (or full Xcode), Node
24.20.0 and Rust 1.98.1 via rustup. `.node-version`, `rust-toolchain.toml`,
`package-lock.json` and `src-tauri/Cargo.lock` pin the baseline.
Install prerequisites yourself using their official installers; these commands
do not install global tools.

Copilot model lookup additionally requires macOS 13.5+ because of the bundled
official runtime's deployment target. Older systems retain Settings/sign-in
management but receive an explicit model-lookup compatibility error.

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
PR_SNIPER_DATA_DIR="$(mktemp -d)" \
PR_SNIPER_KEYCHAIN_SERVICE="com.jdylanmc.pr-sniper.tests.dev-$(uuidgen)" \
npm run tauri -- dev
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
`config/settings.json` stores nonsecret repository configuration and the startup
preference. Settings accepts `owner/repository` or an HTTPS `github.com` URL,
normalizes case and clone suffixes, and prevents duplicates. Rename, disable,
re-enable and confirmed removal operate on stable local repository identities;
they do not contact GitHub. There is no application-defined repository-count
limit. Failed configuration writes are visible, not reported as successful saves.
If configuration commits but recording diagnostics fails, Settings shows the
committed state with a separate warning, rather than reporting a failed save.

Settings retains the compact sidebar design, with **Integrations**, **Doctrines**,
**Agents**, and **Preferences**. In Integrations, choose a local root folder to discover GitHub remotes and select
repositories with searchable checkboxes, or add a repository manually. Discovery
reads bounded Git metadata only: no Git commands, hooks, includes or repository
code execute. It skips nested symlinks, stops descending at repositories, and
reports unreadable metadata and depth/resource limits. Linked worktrees and
ambiguous remotes can be added manually. Saved roots are not automatically
scanned. Multiple clones of the same GitHub repository share one monitoring
checkbox and saved identity; every discovered clone path remains searchable and
available in the row's local-clone details.

Changes across sections remain one draft until **Save changes**; **Reset
changes** returns to the saved state. A conflicting external update is rejected,
not overwritten. The draft remains intact until **Discard draft and reload**
explicitly replaces it with the latest saved settings. Controls are disabled
during saving and failed writes retain the draft. Startup registration is
separate and changes immediately on explicit choice in **Preferences**.
AI and repository connections also save immediately, independently of the
Settings draft. Closing a repository editor cancels its pending draft lookup;
delayed replies cannot restore dismissed edits.
Focus refreshes preserve open repository and preset forms. Dialogs retain focus,
Escape/Close and background isolation when native dialog APIs are unavailable;
viewport sizing also falls back for older WebKit versions. Production JavaScript
syntax and CSS optimization target Safari 15, preserving viewport fallbacks
through minification. Browser tests inspect the emitted stylesheet and exercise
its layout with unsupported viewport units and dialog APIs. The macOS 12 minimum
is unchanged; build targets do not polyfill runtime APIs, and these simulations
are not native acceptance evidence.

Repository **Settings** assigns reusable Agents with their own schedules and
comment preferences, and resolves watched people using the repository's
explicit GitHub account. Existing global defaults and overrides remain
preserved in storage. Configuring an assignment executes neither reviews nor
publication; approval submission remains unavailable.

**Doctrines** manages plain-text review principles. **Agents** selects a Copilot
account and a real model returned by that account, alongside an optional
doctrine, prompt and signature. Provider and model are distinct. Old Agents
without AI account bindings remain unconfigured until explicitly updated.
Disconnecting an AI account preserves dependent Agents and assignments.
Never put credentials in doctrines, prompts or other configuration fields.

Saving validates the effective policy on the Rust storage boundary, including
positive whole-minute intervals, five-field cron syntax, IANA time zones,
unique positive account IDs, nonempty prompts/selectors and supported adapter
and selector shapes. Recognized GitHub token patterns are rejected without
echoing them. This is not a general-purpose secret detector: all configuration
must remain nonsecret. Invalid input does not replace the last valid saved
configuration; malformed or unreadable files are reported rather than reset.
`state/diagnostics.jsonl` records timestamped, fixed-schema host events, capped at
256 KiB plus one rotated file. **Settings > Preferences**
opens
an in-app reader, not an arbitrary filesystem or shell interface.
Invalid settings are reported rather than silently reset or overwritten.

App-owned GitHub and Copilot token pairs use separate account-addressed macOS
Keychain services, never config, state or diagnostics. Neither Settings
connection copies terminal credentials. A green Copilot check verifies sign-in
only, not a subscription, seat or inference request.
See the [bounded architecture decision](docs/adr/0001-macos-foundation.md).

## Read-only GitHub connection

In **Integrations > Git repositories**, connect an account through the PR Sniper
GitHub OAuth App and confirm its stable identity. Repository OAuth requests
the broad `repo` scope for public/private access. Explicitly select the acting
account and repository, then save. In its **Settings > Repository and
connection**, verify the GitHub connection. A different account fails visibly
instead of silently switching identities. Connections use saved bindings,
not unsaved rename drafts. Copilot credentials do not determine the repository
acting identity.

The application verifies `/user`, repository metadata and an actual PR read.
Read access is separate from comment capability: classic OAuth `repo` or
`public_repo` scopes are checked against the repository visibility and archived
state. Scope availability is **not publication authorization** or proof that an
individual PR is unlocked or unrestricted. Credentials without scope evidence
(including fine-grained tokens) show comment permission **unverified**, never
fabricated write permission. No mutation is used to probe permissions.

**Read PR metadata** rechecks the verified account and remote repository ID,
then reads every PR page, each PR's full changed-file pages and requested users
and teams. It includes closed/merged PRs and explicit deleted-account/fork states.
Head changes, malformed pages, duplicate entries, incomplete file counts,
GitHub's 3,000-file cap and failed pages produce an error, not partial success.
No file is checked out or executed. This is metadata, not a guarantee of complete
diff text. Retargeting a saved repository invalidates its previous connection.

Authentication, CLI, permission, rate-limit, timeout, network and provider failures
remain visible. No automatic retry or automation enablement occurs. Diagnostics
record only fixed connection/read success or failure events, never raw provider
bodies, tokens or subprocess output. HTTPS credentials travel only in a sensitive
authorization header; redirects are disabled.

For an explicitly authorized legacy CLI-credential read-only diagnostic using
the native provider client (not the Settings OAuth connection):

```sh
cargo run --manifest-path src-tauri/Cargo.toml --locked --example github_read -- \
  jdylanmc/pr-sniper 6954990 --metadata
```

Use your own expected account ID when appropriate. This prints verified public
identity, capability and aggregate metadata counts, never a credential.

`npm run test:settings` exercises the production Settings UI against real Rust
storage with temporary data and fresh process reads. See
[Settings behavioral tests](tests/settings/README.md) for setup and the boundary
between browser proof and native macOS verification.

## Crosshair assets

The canonical crosshair is original vector geometry, not a private-use font.
`src/crosshair.svg` serves the local surfaces, and the native tray uses a
transparent template PNG that macOS adapts to its appearance.
Regenerate checked-in PNG/ICNS assets on macOS with
`swift scripts/generate-icons.swift`.
