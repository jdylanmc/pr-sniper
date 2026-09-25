# Settings behavioral tests

Run `npm ci`, then `npm run test:settings` with the repository's Rust toolchain
on `PATH`. If Playwright reports a missing browser executable, install its
matching Chromium build with `npm exec playwright install chromium`.

The tests serve the actual Vite production build and operate its Settings
controls in an isolated Chromium session. Only Tauri IPC is replaced: each
request launches the Rust example `settings_bridge`, which creates a fresh
production `Store` at a test-owned temporary root. Initial host preferences
are seeded through `Store::save_settings`; assertions read through a separate
Store process and reload the UI. Persistence is not a JavaScript imitation.
The fixture removes its own temporary data even when an assertion fails.

`copilot.spec.mjs` covers the four-tab Settings Copilot surface: multiple
confirmed identities, cancel/reconnect/disconnect, repository-role separation,
explicit dynamic model selection (including Claude through Copilot), policy and
network failures, empty catalogs, stale replies, and retained legacy/disconnected
Agents. Auth and catalog responses are synthetic; Agent/account references and
repository assignments are saved and reloaded through real Rust storage.
These tests do not prove live OAuth-to-Copilot token acceptance. See the
[native live-check procedure](../../docs/copilot-settings.md).

The add-repository test checks canonical GitHub names, persistence across fresh
Store processes and UI reload, and preservation of the startup preference.
The bridge and native command call the same production Store operation.

The repository lifecycle test adds two records, then checks canonical URL
rename, duplicate add/rename rejection, disable/re-enable, and confirmed
removal. Fresh reads protect immutable identity, the independent neighboring
record, and the startup preference throughout. Both lifecycle commands call the
same production Store operations as the native app.

The policy test exercises nondefault global settings, complete per-repository
overrides, an independently inheriting neighbor, and reset to changed defaults.
Fresh Store reads and reloaded forms protect policy persistence, effective
values and provenance, and independent agent-start/publication gates. Both
policy commands use production Store methods.

The error tests require visible rejection of invalid time zones with the
previous configuration bytes intact, preserve unsaved edits across focus,
and characterize malformed/unreadable data and failed atomic replacement.
Filesystem faults affect only each test's temporary root; unreadable fixture
permissions are restored before cleanup. The bridge mirrors the native
snapshot's `settings: null` plus safe error on failed reads. A forty-repository
fixture checks rendering and reload, not unlimited physical resource capacity.

`src-tauri/tests/policy_validation.rs` covers semantic validation at both
global and override save boundaries, supported schedules, strict unsupported
shapes, synthetic credential rejection, sparse overrides and fresh effective
policy reads. Startup regression tests use only fixture-owned plist and
executable paths, never the user's actual login-item locations.

Review regressions exercise diagnostics failure after configuration has already
committed. All five mutation commands in the bridge include the native
`SettingsSaved` diagnostics phase through the shared production
`Store::finish_settings_save` result boundary; tests require both authoritative visible
saved state and an explicit warning, without fixing the mutation response DTO.
They separately preserve the no-commit contract for failed configuration writes.

Draft-lifecycle tests keep unrelated global/repository edits across saves and
ensure inherited fields still follow changed defaults. A fixture can hold one
real Store reply at the IPC boundary, allowing deterministic focus/save races
without sleeps or fake persistence. The submitting policy form must be disabled
while its reply is pending; the implementation serializes Settings mutations by
temporarily disabling the other controls too. Inherited disabled controls remain
disabled when a save fails.

The tests do not exercise native Tauri command registration, macOS WebKit,
menu-bar behavior, or login-item integration. It never launches the native
application or changes host login settings. Tests run serially. Port 1421 must
be free, or select another port with `SETTINGS_TEST_PORT=1422 npm run test:settings`
for a separate worktree. An existing server is never reused.
