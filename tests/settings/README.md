# Settings behavioral tests

Run `npm ci`, then `npm run test:settings` with the repository's Rust toolchain
on `PATH`. If Playwright reports a missing browser executable, install its
matching Chromium build with `npm exec playwright install chromium`.

The tests serve the actual Vite production build and operate its four-tab
Settings controls in an isolated Chromium session. Tauri IPC is replaced:
storage requests launch the Rust example `settings_bridge`, which creates a
fresh production `Store` at a test-owned temporary root. The fixture returns
explicit empty GitHub/Copilot account lists by default; account/metadata tests
override only their provider responses. Other unknown commands still fail.
Initial host preferences
are seeded through `Store::save_settings`; assertions read through a separate
Store process and reload the UI. Persistence is not a JavaScript imitation.
The fixture removes its own temporary data even when an assertion fails.

`doctrine-seeding.spec.mjs` covers the complete 23-document canonical catalog
(exact titles and bodies, with only frontmatter/H1 removed), durable first load,
Agent choices before visiting Doctrines, and saving Integrations first. It
checks edited/custom/deleted and delete-all libraries across fresh Store
processes and UI reload, conservative handling of legacy missing fields,
visible initialization write errors, and conflicting drafts with explicit
discard/reload. `src-tauri/tests/doctrine_seeding.rs` independently checks the
same storage and canonical-content contracts. Fresh settings now persist the
starter library immediately; startup and automation remain opted out.

`copilot.spec.mjs` covers the four-tab Settings Copilot surface: multiple
confirmed identities, cancel/reconnect/disconnect, repository-role separation,
explicit dynamic model selection (including Claude through Copilot), policy and
network failures, empty catalogs, stale replies, and retained legacy/disconnected
Agents. Auth and catalog responses are synthetic; Agent/account references and
repository assignments are saved and reloaded through real Rust storage.
These tests do not prove live OAuth-to-Copilot token acceptance. See the
[native live-check procedure](../../docs/copilot-settings.md).

Deferred Copilot responses also cover cancellation before/after an unrelated
account failure, deletion retry, and stale focus reads across disconnect,
confirmation and verification, including the cached state sent to Agents.
Native fake-backend tests inject credential deletion and rotated-pair save
failures, assert no identity/SDK calls after failed persistence, and check
account-local recovery and generation fences across cancellation/deadlines.

The add-repository test checks canonical GitHub names, persistence across fresh
Store processes and UI reload, and preservation of the startup preference.
The bridge and native command call the same production Store operation.

The repository lifecycle test adds two records, then checks canonical URL
rename, duplicate add/rename rejection, disable/re-enable, and confirmed
removal. Fresh reads protect immutable identity, the independent neighboring
record, and the startup preference throughout. Both lifecycle commands call the
same production Store operations as the native app.

`policy-inheritance.spec.mjs` now exercises reusable Agents with multiple
independent per-repository assignments, interval/cron/time-zone settings,
comment permissions and removal/reset. It seeds nondefault legacy policies and
presets and proves they remain unchanged by current UI saves. The old global
defaults/override/preset editors are intentionally absent from the approved
four-tab model, not hidden test prerequisites. `sidebar.spec.mjs` covers
per-repository People, inert doctrine authoring, retained Agent references,
disabled Approve/notification controls and both desktop/mobile navigation.

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
committed. Repository add/enable/remove, Agent edits and assignment edits all
use the current UI's shared `save_preferences` operation, including the native
`SettingsSaved` diagnostics phase through `Store::finish_settings_save`.
Tests require both authoritative visible saved state and an explicit warning.
They separately preserve the no-commit contract for failed configuration writes.

Draft-lifecycle tests keep unrelated Agent/repository edits across saves and
ensure assignment timers stay independent of reusable Agent changes. A fixture can hold one
real Store reply at the IPC boundary, allowing deterministic focus/save races
without sleeps or fake persistence. The submitting Settings controls must be disabled
while its reply is pending; the implementation serializes Settings mutations by
temporarily disabling the other controls too. Unsupported Approve remains
disabled when a save fails. Corrections and production-CSS tests retain
keyboard trapping, nested-modal focus restoration, stale reply rejection and
scroll/viewport evidence using the current repository, Agent and doctrine
editors. Assignment selectors must immediately reflect advanced interval edits.

The tests do not exercise native Tauri command registration, macOS WebKit,
menu-bar behavior, or login-item integration. It never launches the native
application or changes host login settings. Tests run serially. Port 1421 must
be free, or select another port with `SETTINGS_TEST_PORT=1422 npm run test:settings`
for a separate worktree. An existing server is never reused.
