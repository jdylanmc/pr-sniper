# Keep the macOS tray host native and its surfaces local

The approved MVP selects Tauri instead of Electron and macOS before Windows.
Tauri 2 owns the menu bar, lazy windows, single-instance enforcement and explicit
quit lifecycle; a vanilla TypeScript/Vite frontend provides small local surfaces
without a component framework. Closing a window hides it, while quitting ends
the event loop; there is no scheduler or child process in this foundation.

Configuration and host diagnostics occupy separate `config/` and `state/`
directories under the application data root. Settings are strict typed JSON,
replaced atomically, not a database or future job schema. Diagnostics accept only
closed-schema host events, never raw errors, commands or arbitrary strings.
Credentials are deliberately absent: no secret is accepted by any foundation
command, config field or log event. Later credential work must use macOS secure
storage, not extend these files with tokens; no unused Keychain adapter is
introduced before there is an exercised credential flow.

Repository configuration extends that same typed settings file. Each configured
repository has an immutable local UUID separate from its canonical GitHub name;
editing the name or enabled state preserves identity. This does not claim a
verified GitHub repository ID. Watched accounts use decimal GitHub account IDs
as strings, with unverified login labels kept separate from their matching key.
Global defaults and sparse repository overrides resolve field by field; explicit
false, empty watchlists and the adapter-default selector are not inheritance.
Both automatic-start and publication default off and remain independent.

The storage save boundary validates complete effective policies before atomic
replacement; load rejects invalid data instead of inventing defaults. Missing
new fields in the established host-only format use defined defaults, preserving
the startup preference without a migration framework. Croner validates five-field
cron syntax and chrono-tz validates IANA zone names; neither executes schedules.
Configuration accepts no credential fields, rejects recognizable GitHub token
patterns, and never copies policy text or repository data into host diagnostics.
Arbitrary user-authored text must still be kept nonsecret; credentials belong in
the later secure-storage flow.

Configuration commit and diagnostics are distinct outcomes. Native commands and
the browser bridge share the production save-result boundary: a committed save
returns settings plus an optional safe diagnostics warning, not a false failure.
Settings refreshes preserve only edited form groups; untouched inherited fields
use current defaults. Edits invalidate pending focus refreshes, and configuration
saves hold controls stable until their response is reconciled.

GitHub connection now has separate credential and provider boundaries.
The credential source runs only bounded `gh --version` and `gh auth token
--hostname github.com` commands; captured secrets are transient and never
serialized or persisted by the application. GitHub CLI retains ownership of its
existing credential storage. No unused app Keychain store is introduced.
The provider client uses typed domain results over GET-only HTTPS, not CLI
presentation tables. Sensitive authorization headers never enter process argv,
diagnostics or frontend state; redirects are disabled.

Connections verify the stable account ID, canonical remote repository ID and
real PR-read capability. OAuth scope evidence is distinct from read access and
from item-specific publication permission; absent scope introspection remains
unknown. An explicit read exhausts REST pagination and checks changed-file
counts and revision stability. No partial metadata is returned on failure.
The local repository UUID remains separate from remote identity; native commands
recheck saved names after asynchronous reads, and UI observations are invalidated
on retarget. Connection actions preserve the existing Settings draft/save
boundary and never alter an effective policy or its automation gates.

Launch at login is an explicit Settings operation that writes the application's
own LaunchAgent plist. Settings distinguishes saved intent from absent, invalid
or structurally valid registration for the current executable; none proves
effective launchd state or overrides macOS Login Items. This replaces the
autostart plugin's existence-only Boolean, which could misreport damaged or
stale registrations. Plist encoding handles special path characters, and a
settings-write failure restores the previous registration bytes.
Saved intent is never applied at startup. An isolated data-root override disables login mutation,
allowing local acceptance runs without changing host startup. This avoids
turning a developer launch or stale preference into silent OS registration.
