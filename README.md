# PR Sniper

A macOS menu-bar application for human-owned pull request review. Built with
Tauri 2, Rust and vanilla TypeScript. The tray exposes **Status**, **Review
Queue**, **Settings**, **Setup Doctor** and **Quit PR Sniper**.

The host checks enabled GitHub repositories and shows eligible revisions and
schedule health in **Status** and **Review Queue**. **Check Now** requests an
immediate, non-overlapping check without changing the configured schedule.
Settings can still explicitly verify a connection and read complete PR metadata.
Agent execution, comment publication and automated setup are not implemented.
Product scope lives in the
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
`config/settings.json` stores nonsecret repository configuration and the startup
preference. Settings accepts `owner/repository` or an HTTPS `github.com` URL,
normalizes case and clone suffixes, and prevents duplicates. Rename, disable,
re-enable and confirmed removal operate on stable local repository identities;
they do not contact GitHub. There is no application-defined repository-count
limit. Failed configuration writes are visible, not reported as successful saves.
If configuration commits but recording diagnostics fails, Settings shows the
committed state with a separate warning, rather than reporting a failed save.

Settings opens the [approved A sidebar](docs/agent/design/settings-default.md):
**Repositories**, **People**, **Review defaults**, **Automation** and **Review
presets**. Choose a local root folder to discover GitHub remotes and select
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
not overwritten. Controls are disabled during saving and failed writes retain
the draft. Startup registration is separate and changes immediately on explicit
choice in **Automation > Startup and diagnostics**. Closing a repository editor
cancels its pending draft lookup; delayed replies cannot restore dismissed edits.
Focus refreshes preserve open repository and preset forms. Dialogs retain focus,
Escape/Close and background isolation when native dialog APIs are unavailable;
viewport sizing also falls back for older WebKit versions. Production JavaScript
syntax and CSS optimization target Safari 15, preserving viewport fallbacks
through minification. Browser tests inspect the emitted stylesheet and exercise
its layout with unsupported viewport units and dialog APIs. The macOS 12 minimum
is unchanged; build targets do not polyfill runtime APIs, and these simulations
are not native acceptance evidence.

Repository **Settings** offers an independent **Override** checkbox for each
visible policy field. Unchecked fields use current global defaults. Existing
reviewer-assignment and adapter values remain preserved in storage but the
reviewer-assignment control is not part of ordinary Settings. **Run reviews
automatically** and **Post review comments automatically** remain independent,
off by default; changing configuration executes neither reviews nor publication.
Monitoring performs provider reads only; neither gate starts an agent or
authorizes publication in this increment.

Scheduling starts in the system-local time zone for a new profile. Existing
intervals, five-field cron expressions and saved time zones remain unchanged;
**Advanced scheduling** exposes cron, custom interval and time-zone selection.
**Model** lists Default first and preserves saved model/named-agent selections.
Installed-model discovery and agent health are explicitly unavailable, not
simulated.

**People** resolves an exact GitHub login using the current CLI credential and
stores the provider's stable numeric identity. Names/logins are display labels;
IDs remain the matching key. Repository access and the signed-in account are
verified separately. **Review presets** creates and edits named machine-local
instructions. Import accepts inert JSON with only `name` and `body` strings
(24 KB maximum; 80-character names and 12,000-character instructions).
Select a preset globally or per repository; edits update selected prompts.
Editing a prompt directly switches that field to custom instructions without
changing other overrides. Existing custom prompts are retained.
Never put credentials in presets, prompts or other configuration fields.

Saving validates the effective policy on the Rust storage boundary, including
positive whole-minute intervals, five-field cron syntax, IANA time zones,
unique positive account IDs, nonempty prompts/selectors and supported adapter
and selector shapes. Recognized GitHub token patterns are rejected without
echoing them. This is not a general-purpose secret detector: all configuration
must remain nonsecret. Invalid input does not replace the last valid saved
configuration; malformed or unreadable files are reported rather than reset.
`state/diagnostics.jsonl` records timestamped, fixed-schema host events, capped at
256 KiB plus one rotated file. **Settings > Automation > Startup and diagnostics**
opens
an in-app reader, not an arbitrary filesystem or shell interface.
Invalid settings are reported rather than silently reset or overwritten.

GitHub CLI credentials are acquired in memory, never saved by PR Sniper.
Any future app-owned persisted credentials must use macOS secure storage,
never config, state or diagnostics.
See the [bounded architecture decision](docs/adr/0001-macos-foundation.md).

## Read-only GitHub connection

Monitoring uses a separate lightweight open-PR reader, never the manual
historical/changed-file reader below. Open, non-draft revisions qualify when
their author's stable ID is watched **or** the signed-in account is individually
requested and the reviewer trigger is enabled. Requested teams do not imply
individual membership. Empty watchlists are valid and do not match all authors.

The basic queue persists in `state/queue.json`; identity includes GitHub, remote
repository ID, PR ID, head SHA and a deterministic trigger-policy key. Login
label changes and matching both triggers do not create duplicate jobs. A new
eligible head is distinct. Reviewer-only work and fork/deleted-head-repository
work wait for trust confirmation, never automatic execution; trusted work
still waits for human start or an unimplemented agent. No queue item represents
a completed review or publication.

Successful incremental boundaries and verified account/remote identities persist
in `state/poll-cursors.json`. Polling includes updates equal to the saved timestamp
and never advances on failed reads. GitHub can return timestamps out of order,
so every open-PR page is enumerated and validated before older revisions are
excluded from admission. The cursor reduces returned candidates, not list GETs.
Eligibility-policy changes, disabling/removal and retargeting invalidate the
relevant cursor; queue
history still deduplicates already detected remote revisions.
Multi-page polling re-reads the visited lightweight pages in reverse order before
accepting the result. Detected page or pagination changes fail as an incomplete
read without advancing the cursor; there is no automatic retry of that attempt.
A single-page sweep needs one list GET; an N-page sweep needs 2N list GETs,
including reconciliation, in addition to connection/access checks. This costs
more reads than a timestamp-based early exit but avoids missing newer PRs behind
older rows or pages. Polling still never hydrates historical PRs or changed files.
Both polling and manual metadata pagination accept GitHub's numeric-repository
links only for the verified immutable repository and exact endpoint, with the
same origin and unchanged pagination filters.

`state/polling.json` contains safe schedule-health observations (last attempt,
success, next run, in-flight state and classified failure). A failed read never
becomes an empty successful check. Settings are reloaded after network work;
disabled, removed, retargeted or changed-policy attempts cannot admit old results.
After enumeration, polling reacquires the current CLI credential and rechecks
account identity, remote identity and read access before admission. A local
sign-out or account switch during a delayed read rejects the captured result.
If persisting dispatch state fails, all undispatched checks are released with a
visible failure. Correcting storage allows Check Now and the next scheduled run;
the failure does not leave nonexistent work marked in flight.
Closing a window does not stop checks; Quit ends the host. Full interrupted-job
recovery and retry budgets belong to a later delivery, not this basic queue.

Intervals use elapsed UTC seconds. A fresh host schedules the first interval
after the configured duration; Check Now can run sooner. A missed cadence is
coalesced into one check, never a burst of overlapping catch-up work. Active
repository exclusion survives removal/readdition and retargeting. Cron uses
Croner 4 with chrono-tz: fixed wall times skipped by a spring daylight transition
run at the first valid instant after the gap; a fixed time repeated in autumn
runs only in the first occurrence. Wildcard cron follows chronological minutes
through the repeated hour. Calendar occurrences use the configured IANA zone,
not the host's local-zone setting.
An impossible calendar remains a visible per-repository configuration failure
without blocking other repositories. Correct its schedule in Settings.

Install and authenticate the official GitHub CLI yourself. PR Sniper never runs
login, logout, installation or credential-configuration commands. It uses the
trusted current user's `gh` from an absolute PATH directory, with Homebrew's
usual directories as Finder-launch fallbacks. A bounded version/health probe
rejects missing or broken executables and shims; this is not a cryptographic
provenance check or a sandbox for an untrusted executable.

In **Settings**, save a repository, then open its **Settings > Repository and
connection > Verify GitHub connection**. Optionally enter the expected stable decimal GitHub account ID; the
verified ID is filled in for subsequent checks. This pin is window-session state,
not a persisted account selection. A different account fails visibly instead
of silently switching identities. Connections use saved repository names,
not unsaved rename drafts.

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

For an explicit real read-only smoke using the same native client:

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
