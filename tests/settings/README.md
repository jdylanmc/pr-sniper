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
and 400px/small-monitor layout. Explicit pointer, keyboard and nonfocusing
activation regressions cover row redraw, nested/replacement Settings editors,
and retained drafts. `monitoring-activation.spec.mjs` also moves focus while a
scope preview is pending to prove that closing its asynchronous dialog returns
to the actual invoker. Final and conversation suites also exercise
their exact panel job routes. The bridge serializes the production native
`panel::Session` only to carry it between fixture processes; the real host
retains that session in memory. Only the Tauri event delivery and window
visibility boundary are mocked. Tests do not prove OS focus/dismissal or
notification activation; the isolated native harness and platform procedures
remain required.

`completion-focus.spec.mjs` retains the pointer/keyboard/nonfocusing completion
matrix, final-redraw identity checks, independent saved resources/drafts and
delayed account verification. Held-frame variants run each real resource save
with its navigation frame delivered both before and after the save reply;
neither may steal newer navigation focus. `panel-focus.spec.mjs` pairs those
negative cases with initial route focus, delayed Settings mounting, exact Back
row/scroll, rapid routes sharing one heading, and hide/reopen with a retained
draft. The frame helper delays application callbacks, not Playwright's
actionability checks, and attaches browser focus-call/frame ordering evidence.
It uses no sleeps, retries, extended assertion timeouts or replacement storage.

`visual.spec.mjs` covers the approved 408px shell and human cards, the complete
301-file ordered guide, exact provider links, failed snapshot recovery, seven
Agent jobs at capacity four, duplicate reservation identity, completion/refill,
tail append, retained blockers, immutable full configuration and keyboard access
at 320x300 with reduced motion. Automatic-poll regressions retain the exact
selected inspector controls for unrelated job changes and preserve keyed focus
and open configuration/provenance through its own status transition.
Screenshots live under `<target>/visual-correction-1`. The fixture-only
`fixture_capacity_snapshot` command uses the real native Coordinator reservation
and projection path with synthetic jobs. It launches no workers or provider
sessions; deterministic native capacity tests separately prove dispatch/refill.
Final and conversation regressions cover all four work purposes and exact
same-commit reopened iteration provenance. These captures are browser evidence,
not installed-app or live-provider acceptance.

`shared-editors.spec.mjs` covers compact shared libraries, saved Agent counts
above AI capacity, explicit account/model selection, full editor text and
signatures, keyboard-scrolled and filter-retained doctrine selections, resource
Save/Back/Cancel, shared-use deletion guards, empty-library restart and failed
write/retry at 320x300. Screenshots use Playwright's per-test output directory.
Run with `--browser=chromium` or `--browser=webkit`; the existing
`SETTINGS_TEST_PORT` variable selects an isolated preview port. This is headless
browser and test-owned Store evidence, not native focus, installed-app or live
authentication acceptance.

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

The default Chromium run does not exercise native Tauri command registration, macOS WebKit,
Windows WebView2, tray behavior, or login-item integration. They never launch the native
application or change host login settings. The same production build and native
bridge can be exercised in **headless Playwright WebKit**, including its
nonfocusing pointer clicks, without launching the app:

```sh
SETTINGS_TEST_PORT=1441 npm exec playwright -- test \
  --config tests/settings/playwright.config.mjs --browser=webkit \
  visual.spec.mjs panel.spec.mjs actions.spec.mjs conversations.spec.mjs \
  css-compatibility.spec.mjs corrections.spec.mjs monitoring-activation.spec.mjs
```

Build first with `npm run build` and
`cargo build --manifest-path src-tauri/Cargo.toml --locked --example settings_bridge`.
For an isolated `CARGO_TARGET_DIR`, build the bridge there or copy the compiled
bridge from the same native source revision into its `debug/examples` directory.
Install the matching WebKit build only if it is missing. Playwright WebKit is
browser regression evidence, not native WKWebView/installed-app acceptance.
Keep screenshots in the owned target and generate hash manifests only after
all capture runs finish. Tests run serially. Port 1421 must
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
presentation.
The inspector matrix retains native-shaped stopped/failed/manual-retry and
stopped/backoff operations for both owned replies and primary mentions. Completed
analysis is combined with quiet, human judgment, waiting/publishing, rejected,
uncertain and confirmed publication states; unknown outcomes retain the original
reconciliation controls, and only actual AI reservations animate.
Native conversation tests exercise production scan admission,
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
