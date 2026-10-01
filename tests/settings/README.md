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
The fixture drains accepted Store operations before removing its own temporary
data, even when an assertion fails. Native bridge lookup honors `CARGO_TARGET_DIR`
and the Windows `.exe` suffix; temporary profiles and browser output remain under
that target. The default target is `src-tauri/target`.

`panel.spec.mjs` exercises the production root panel (not the explicit
`?view=settings` / `?view=queue` component harnesses): all four destinations,
one detail layer, exact Back row/scroll/focus, retained unsaved editors,
Escape/close/hide/reopen, native route revisions, notification destinations,
missing/escaped identities, external-auth and folder-dialog fixture returns,
and 400px/small-monitor layout. Final and conversation suites also exercise
their exact panel job routes. The bridge serializes the production native
`panel::Session` only to carry it between fixture processes; the real host
retains that session in memory. Only the Tauri event delivery and window
visibility boundary are mocked. Tests do not prove OS focus/dismissal or
notification activation; the isolated native harness and platform procedures
remain required.

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

`policy-inheritance.spec.mjs` exercises reusable Agents with multiple
independent per-repository assignments, comment permissions and removal/reset.
It seeds nondefault legacy schedules, policies and presets and proves they
remain unchanged by resource saves, without exposing scoped polling editors.
`resources.spec.mjs` covers resource-scoped persistence, conflicting and failed
writes with retained drafts, ordered doctrine selection, rename/deletion guards,
global cron/time-zone/capacity controls, shared saved-resource readiness, primary
selection and independent opt-in permissions. New assignments retain the saved
global schedule even with invalid or different unsaved Preferences; existing
assignment schedules and unrelated drafts remain untouched. `sidebar.spec.mjs` covers
per-repository People, inert doctrine authoring, retained Agent references,
opted-out Approve/notification controls and both desktop/mobile navigation.

The error tests require visible rejection of invalid time zones with the
previous configuration bytes intact, preserve unsaved edits across focus,
and characterize malformed/unreadable data and failed atomic replacement.
Filesystem faults affect only each test's temporary root; unreadable fixture
permissions are restored before cleanup. Windows uses a real fixture-only DACL
read denial with exact DACL restoration; Unix retains its mode-based fixture.
The bridge mirrors the native
snapshot's `settings: null` plus safe error on failed reads. A forty-repository
fixture checks rendering and reload, not unlimited physical resource capacity.

`src-tauri/tests/policy_validation.rs` covers semantic validation at both
global and override save boundaries, supported schedules, strict unsupported
shapes, synthetic credential rejection, sparse overrides and fresh effective
policy reads. Startup regression tests use only fixture-owned plist/executable paths on macOS
and unique test-owned registry keys on Windows, never actual login registrations.

Review regressions exercise diagnostics failure after configuration has already
committed. Repository add/enable/remove, Agent edits and assignment edits all
use the current UI's shared `save_resource` operation, including the native
`SettingsSaved` diagnostics phase through `Store::finish_settings_save`.
Tests require both authoritative visible saved state and an explicit warning.
They separately preserve the no-commit contract for failed configuration writes.

Draft-lifecycle tests keep unrelated Agent/repository edits across saves and
ensure repository policy stays independent of reusable Agent changes. A fixture can hold one
real Store reply at the IPC boundary, allowing deterministic focus/save races
without sleeps or fake persistence. The submitting Settings controls must be disabled
while its reply is pending; the implementation serializes Settings mutations by
temporarily disabling the other controls too. Independent permissions retain
their prior values when a save fails. Corrections and production-CSS tests retain
keyboard trapping, nested-modal focus restoration, stale reply rejection and
scroll/viewport evidence using the current repository, Agent and doctrine
editors. The global schedule helper must match the saved cron expression.
Review fixtures distinguish captured, planned, interrupted and legacy missing
configuration without using today's library as historical evidence.

`iterations.spec.mjs` passes the new queue envelope, tracking and global-scan
fixtures through the native Store bridge. It verifies seven independent ordered
jobs, immutable admission versus current matching, separate pass/attempt counts,
provider-confirmed terminal history, same-head reopened iterations, retained
destination aliases and tracking with no assignments. These are presentation
fixtures, not live provider claims. `src-tauri/tests/iterations.rs` exercises
the production Monitor, provider metadata reader and review/publication gates:
scan-time assignment capture, sticky admission, explicit missing-PR reads,
404/unavailable/incomplete responses, restart and write-failure recovery,
legacy receipt deduplication and continued owned-thread polling. No capacity
engine, new mention routing or provider action is exercised.
Normal-review execution and resume checks ignore sibling assignment archives
while preserving the original snapshot and rejecting changes to the selected
Agent's inputs, effective authority and repository gates.

`platform.spec.mjs` checks the existing host and Settings wording for Windows
and macOS without changing authorization or native policy. Browser presentation
checks do not select a native credential backend.

The tests do not exercise native Tauri command registration, macOS WebKit,
Windows WebView2, tray behavior, or login-item integration. They never launch the native
application or changes host login settings. Tests run serially. Port 1421 must
be free, or select another port with `SETTINGS_TEST_PORT=1422 npm run test:settings`
for a separate worktree. An existing server is never reused.

`capacity.spec.mjs` covers durable pause across queue/Settings, resource-isolated
capacity saves, failed writes, honest stopping counts and manual admission while
another job runs. Occupancy rendering uses explicit synthetic snapshots, not
live inference. Native `capacity::tests` exercises the production reservation,
candidate, preparation and completion paths with controlled worker completions:
limits 1/4/20, mixed FIFO, concurrent manual requests, long initial waits, restart,
pause/reduction, rapid resume and real failures racing cancellation. Existing
restricted-runtime tests still verify owned process teardown. Provider tests
inject pause during pending creation and a lost submit response, preserving
original receipts and proving no duplicate batch is posted.

`conversations.spec.mjs` exercises typed conversation targets/current contexts
through the Store bridge, explicit versus absent feedback assessments, retained
provider-closed tombstones, unavailable observations and missing-primary
presentation. Native conversation tests exercise production scan admission,
normal/reply/mention FIFO dispatch, provider comment pagination, scoped mention
matching, current-head observation with immutable root provenance, exact
reconciliation after lost responses/primary changes and human-judgment gates.
Owned and mention runtime fixtures share the same restricted tool contract.
Provider responses are deterministic fixtures, never live action acceptance.

`actions.spec.mjs` drives native Store-backed final-observation fixtures, durable
manual final requests, approval-versus-personal-review handoff, terminal merge
receipts, unknown original-effect reconciliation and local-only publication.
Native `actions::tests` exercises real aggregate/basis capture, shared-capacity
dispatch, preparation/intent/receipt/reconciliation helpers and the GitHub
transport adapter with deterministic responses. The restricted-runtime fixture
answers an actual external-tool request for a changed file before emitting a
final full-review result. No test approves or merges a live PR.
