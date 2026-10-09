# Settings behavioral tests

`repository-account-inference.spec.mjs` exercises the shared URL intake with
zero, sole and multiple compatible Git Repository connections. Sole-account
inference is compact; ambiguous selection uses a named native select with
keyboard typeahead and an accessible label. Copilot-only, unsupported,
unconfirmed and missing-scope connections cannot supply the repository actor.
Change captures the chosen ID in the actual resolution request and real Store
binding; late URL/account/disconnect/generation updates cannot commit stale
results. Account read failures, provider warnings and nested recovery retain
input, identity and focus. Unsupported URLs never borrow a GitHub actor, and
recognizing a PR URL does not implement explicit PR admission.

Transport responses in those browser tests are synthetic; persistence uses
the isolated native Store bridge. The offline native
`repository_read_tests::repository_intake_saves_the_verified_actor_and_rejects_same_login_replacement`
test crosses the selected-session/provider resolution, current-binding guard
and Store boundaries with confirmed in-memory account generations, including
same-login connection replacement. Existing session-cache, scope-loss and
precommit controls remain covering evidence. No tests require live accounts,
Keychain access, provider writes or desktop automation.

`queue-visual.spec.mjs` exercises the production retained Queue with the real
isolated Store bridge. Empty, populated and attention-only fixtures reconcile
the approved `prototypes/v2/evidence/configured-queue.png` design with human-only
handoffs: orange Ready for you and white Needs attention summaries, an actual
capacity link to Running, and the same compact card structure when empty.
Counts come from the existing native projection; no sample work, Agent lane,
manual-start gate or personal-review acknowledgment is introduced.

Captures include the actual renderer and a side-by-side comparison (approved
panel crop left, actual Queue right). Image color samples and measured summary/
card landmarks use the approved capture's 408x744 panel at desktop coordinates
(813,42). Explicit tolerances allow platform typography and truthful captions,
not the prototype's obsolete counts, Agent queue or workflow. Same-Store reloads
must preserve screenshots within a 0.1% changed-pixel allowance for isolated
rasterization differences (16 intensity levels per channel). Measurement artifacts include reference and
actual image hashes. Keyboard/Back/refresh and unavailable-destination cases
retain exact item identity and scroll/focus; media cases cover light/dark
(the panel intentionally retains its approved light content), forced colors,
reduced motion, tiny viewports, 1x/2x device scale and doubled actual text sizes.
These browser/accessibility-tree checks do not establish native outer corners,
desktop tray behavior or a live screen-reader session.

Queue's heading, summary and error banner share its retained content scroller,
including exact saved evidence and utility routes.
Only the brand header, navigation and utilities stay pinned; other destinations
keep their existing heading/summary placement. Short Queue panels lay out navigation
at each label's intrinsic width and give the complete hide/quit guidance a footer
row, rather than wrapping one label or squeezing guidance into a narrow column.
No text size, counts or errors are reduced to make the evidence fit.

The three combined regressions capture screenshots and measured content,
navigation, footer and individual-control bounds before keyboard interaction:
320x440 at 200% actual text, 320x300 with a monitoring-read error, and 320x300
with both. The scrolling viewport must remain at least 64px high, every pinned
control must fit completely, unknown counts remain unknown, and ordinary
keyboard traversal must open the exact last handoff and retain scroll/focus on
Back. Doubled card text persists across real row redraws. macOS WebKit uses
Option+Tab for all-controls traversal, matching the existing Genie seam.
Use `--browser webkit` with the same focused command to check the second engine.

After the ordinary frontend and Store-bridge build, run only this lane's checks
on an available private port, for example:

```sh
SETTINGS_TEST_PORT=1652 npm exec playwright -- test \
  --config tests/settings/playwright.config.mjs queue-visual.spec.mjs \
  --output /absolute/private/queue-visual
```

Repository-writing library/bootstrap fixtures use explicit synthetic stable
account/repository bindings through `seedBoundRepositories`; enabled legacy
unbound rows are not valid enabled-save inputs. The helper changes only the
new fixture bindings, not monitoring intent, schedules, assignments or authority.
`resources.spec.mjs` keeps an independent native-byte regression for rejecting
unbound enabled assignment saves and preserving legacy state without a grant.
Connection checks scope their status to the connection section, separately from
the repository's monitoring status. Shared Save helpers assert actual errors
stay hidden on success before asserting dismissal; they never force-close a
rejected save or repair invalid state behind the caller's back.

`repository-monitoring.spec.mjs` covers persistent listing/editor monitoring
switches through resource-scoped native Store saves, authorization and reload.
Global pause remains separate, intentionally disabled saves stay disabled, and
quick switches use saved configuration without consuming unrelated or repository
field drafts. Invalid enablement and compare-save conflicts retain drafts and
show errors without optimistic enabled state. New intake stays disabled until
configuration Save; a deliberate off choice survives assignment saves and Back.
Compact keyboard/focus, forced-colors and reduced-motion cases use isolated
browser fixtures, not the native desktop. Native resource and account-precommit
tests separately cover schedule/binding/assignment validity, account loss,
generation changes and durable authorization.
Failed new configuration Save is followed by a nested assignment Save with
independent native settings-byte and authorization assertions: only a later
valid configuration Save may enable the new row. Nested saves refresh actual
saved monitoring state/details without consuming the pending setup choice.
At 320px and 408px, panel and standalone controls also receive actual 100%/200%
computed text sizes. Tests check row, copy, state, switch and switch-text
containment inside the clipping list, visible keyboard focus and Space
activation before and after redraw; document overflow alone is not that oracle.
Short standalone 280x300 and 320x300 panes also test doubled text with the whole
computed focus outline inside every clipping ancestor, including after a
paused save or actionable account rejection. The repository heading scrolls
with content in short standalone panes instead of starving the list viewport.
Scroll padding and target margin retain the existing external focus outline.
Tab reaches the switch, ordinary scrolling reveals its full focus target, and
the same drill-in/editor and native pause/authorization semantics remain intact.

`corporate-repository-url.spec.mjs` exercises Settings and Genie URL intake
with synthetic account-keyed HTTP responses through the real native
`GithubClient` parser and isolated Store, not final-result-only UI stubs.
It covers authorized private organization metadata with nullable optional
fields, stable account mismatch, ambiguous 404, scope, organization authorization,
read denial, expired authorization, transport/provider/schema failures, retry
without losing URL/account, disabled initial persistence and late cancellation.
`corporate_repository_url` additionally compares browser and URL stable identities,
supported HTTPS/name variants, explicit denial and canonical/schema guards.
Native `repository_read_tests` and `repository_save_account_tests` protect actual
session publication, generation changes and precommit authority separately;
the process-per-command browser bridge does not simulate a live Keychain,
credential refresh, confirmation race or installed corporate entitlement.

GitHub's [troubleshooting contract](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api)
states that 404 also hides unauthorized private resources. URL verification
therefore never treats 404 as definitive absence or definite read denial.
Explicit OAuth scope evidence can distinguish a missing required `repo` scope
from an otherwise unclassified 403, without overriding single-sign-on or rate-limit
classification. Optional permissions metadata is not a default read grant:
admission still requires authenticated repository metadata and a successful
pull-request read. Required identity/private/archive/disabled fields remain
validated against GitHub's [repository contract](https://docs.github.com/en/rest/repos/repos#get-a-repository).
Live organization access or app restrictions require authorized evidence; these
fixtures cannot establish the cause of the original installed corporate failure.

Fix-wave regressions separate absent scope evidence (`scope_unverified`, no
admission and no account-wide scope-revocation claim) from an explicit scope
list missing `repo`. Unverified later catalog pages retain only preceding
verified records with a visible incomplete-read warning. HTTP rejection status
is retained separately from malformed successful JSON/schema, including 422.
Confirmed rate limits retain Retry-After/reset timestamps and relevant partial
organization visibility; a partial-results SSO marker alone does not prove
required authorization. Native rate scheduling prefers Retry-After, otherwise
uses the reported reset time.

Organization policy recognition at URL repository/pulls boundaries is narrow:
the known 403 OAuth App restriction message template or an explicit required
SSO marker, not any generic 403 or arbitrary OAuth-related wording. Combined
policy/scope evidence explains both administrator approval and the selected
account's missing scope; only actual scope deficiency invalidates that actor's
session. Rate-limit recovery remains primary when also evidenced, with the
other safe facts retained. Unknown wording stays a conditional denial, never
a guessed policy diagnosis. These cases use real provider parsing, native
session/precommit guards and Store-backed Settings/Genie tests in Chromium and
WebKit; no live entitlement or real Keychain/CredStore proof is implied.

The exact `actions::tests::provider_rules_` native selectors exercise rules
HTTP 403/429 through actual action observation and persisted retry state.
Plain, Retry-After and contextual reset/partial-authorization limits all remain
failures, preserving current authority and captured finals without provider
writes. Reset-only limits retain their measured retry interval. Unsupported
or denied rules remain approval-independent and merge-blocking; they are not
silently reclassified as transient failures or accepted merge policy.

`agent-intelligence.spec.mjs` exercises advertised reasoning efforts, context
tiers and token capacities through the shared Agent editor and real native
resource saves/restarts. Account/model changes retain incompatible deliberate
choices until the operator explicitly repairs them; discovery failures never
declare retained values invalid. Models with no advertised overrides explain
disabled controls and offer Provider default only. Job inspectors distinguish
captured requests, runtime-reported settings and unavailable legacy evidence,
including after a later Agent edit.

`actions.spec.mjs` also covers final-only Intelligence in both Diagnostics
routes through real resource saves, native final admission and persisted action
ledgers. Queued/captured/failed finals show bound requests without inventing an
actual report; completed, legacy and absent records stay distinct after later
Agent edits. Repeated read projections retain normal/reply/mention ordering and
deduplicate execution IDs. The primary-final inspector retains its own snapshot.

The native `review::runtime::tests` use the pinned SDK transport with synthetic
account/model catalogs to check session payloads, readback, rejected/ignored
overrides and failures before inference. Normal, primary-final, owned-reply and
mention paths share this machinery. Run the separate
`bundled_runtime_intelligence_offline` test explicitly with `--ignored
--nocapture` to exercise the actual SDK 1.0.14 / runtime 1.0.85 production
session configuration and model readback, using a rejecting loopback provider,
no credentials and no inference. It is offline configuration evidence, not
proof of a live account's advertised capabilities or a live model's reasoning
quality. The runtime revalidates each actual account/model catalog, reads back
the session configuration and blocks before inference if an override is not
honored. No SDK/runtime upgrade or credential reset is part of this delivery.

`repository-review-fixes.spec.mjs` follows the actual Manage Copilot accounts
button in retained and standalone Settings, rejects rename/rebind collisions
without leaving the originating editor, and checks incomplete enabled saves
alongside explicit disabled repair. Legacy incomplete settings remain loadable.
Enabled-save fixtures use explicit saved AI accounts/models; clearing the final
assignment requires disabling monitoring before saving. Native
`repository_read_tests` exercises all three intake commands' shared completion
boundary with held provider successes/auth failures and real confirmation
generation changes, both with and without an earlier disconnect.

`settings-overview.spec.mjs` covers the grouped retained-panel Settings home:
saved resource counts, separate Accounts/Repositories, all six destinations,
Accounts category/provider drill-down at 408x744 and 320x300, inert coming-soon
providers, per-provider summary failures, retained sign-ins and nested Back focus,
keyboard entry, Back focus/scroll, unrelated drafts and Genie reuse. Existing
shared-editor and panel suites exercise resource saves, explicit discard and
hide/tab retention through those rows. Footer cases compare the version with the
native Store bridge's compiled metadata, cover missing/rejected metadata and
Status retry, and retain utility controls and hide-only guidance on all tabs.
The footer keyboard case holds the Settings navigation reply, then waits for
the mounted overview and pending IPC before focusing Status. Persistent footer
text alone does not establish that route focus restoration has finished.
Screenshots use 408x744 logical pixels at 2x device scale; footer geometry also
covers 320x300 and 408x441, reduced motion and 2x text enlargement. These are
browser/isolated-bridge results, not acceptance of the running native tray app.

`copilot.spec.mjs` also checks retained account re-entry in the panel and standalone
Settings after model lookup invalidates an identity, without a window-focus event.
Failed re-entry reads remove stale verified claims and expose an explicit retry;
the mounted card and persisted resources remain intact.

`genie.spec.mjs` covers R41's fresh/partial/existing flows through the production
Welcome, Genie and shared Settings editors. Repository and AI sign-in/model
transport is explicitly synthetic; saved resources, readiness, authorization
and expected-state checks use the isolated native Store bridge. It covers
explicit account/model/Agent choices and safe-off new permissions, authorization
at repository Save with no second consent, stale configuration,
model/account loss, failed saves, unrelated drafts, cancellation, restart,
late replies, mounted account ownership and already-authorized re-entry.
Re-entry holds the saved-setup refresh and waits for the explicit Review setup
action afterward, rather than clicking a next button with cached step state.
The historical native preview tests remain as compatibility coverage for old
persisted admission modes; no production command exposes separate activation.
Remediation cases compare displayed authors with native effective filters;
hold catalog/read replies across Back;
replace an AI connection after its catalog completes while another waits; and
preserve deliberately inactive installs and newer/manual Settings destinations.
The scoped Copilot unit case exercises the existing generation-before-auth
boundary and rejects a reconnect racing native commit, without provider or SDK
runtime changes. Completion-focus fixtures explicitly opt in to Comment for
their comment-authorized assignment scenario and dispatch the real account
refresh event rather than assuming a resource redraw remounts account widgets.

The initial-setup race cases hold Settings' first snapshot independently of the
setup reply. They finish the normal Settings mount/navigation focus before
establishing newer keyboard focus, then release the old setup success or failure.
Visible section controls alone do not mean that initial navigation is complete.

Each fixture root serializes native Store commands, matching the production
`Host.store` mutex. Original results and errors reach their callers, teardown
drains accepted commands, and independent roots remain independent. Held external
IPC replies do not hold the Store queue. Lifecycle regressions cover queued
rejections, first notification writes and delayed replies; this does not relax
production filesystem checks or browser assertions.

Hosted CI reserves 15 minutes for the Windows frontend/Store build and full
browser suite. The macOS foundation job has 45 minutes for its cold native build,
all checks and bundle/archive steps. The expanded suite reached the previous
10-minute Windows step limit, and macOS reached its 30-minute job limit during
bundling after its checks passed. These are bounded infrastructure budgets, not
changes to individual test timeouts, application deadlines, assertions or
fail-fast behavior. WebKit's cold Store-bridge build and full browser suite share
the existing 30-minute step budget inside its 60-minute job limit. Hosted WebKit
uses two isolated file workers to keep that bounded gate from serializing the
entire growing suite behind the cold build. Cases within each file remain
sequential; each test keeps its private native Store root and FIFO operations.
Local defaults remain one worker, zero retries and unchanged test deadlines.
No case, assertion, build, credential restriction or CI deadline is removed.

The four readiness rows are saved-configuration evidence, not an inference or
subscription test. No fixture starts reviews, uses human credentials, calls a
provider or automates the native desktop. Screenshots from the real renderer
are written to each test's Playwright `outputPath("screenshots", browser)`,
including Welcome,
Genie, effective review and compact final controls at 320x300, 408x441 and
408x744. Sequential Tab traversal checks all four checklist rows, their complete
focus outlines and every clipping ancestor at these sizes, with reduced motion
both off and on. Each capture has source, built-renderer, synthetic-fixture,
browser/version, viewport, timestamp and image hashes. Use a unique `--output`
directory per run and archive immutable copies after that run; later full-suite
runs must not write into archived delivery screenshots.

The original R41 review recorded `shared-ai-408x744.png` changing from
`14429f81de64447bea3783404c9729827a724dc03b327aafdf168de598ecd9e0` to
`706c9fccc850a1e7aa7314f824cc97bcd79a063514f862336f02699b76d01c25`
when the parent full suite reused the fixed delivery path. That mismatch remains
historical evidence; later captures do not reconstruct or replace the lost
original bytes. These are candidate evidence, not independent or release acceptance.
After building the frontend and `settings_bridge`, a focused run is:

```sh
SETTINGS_TEST_PORT=1481 npm exec playwright -- test \
  --config tests/settings/playwright.config.mjs genie.spec.mjs \
  shared-editors.spec.mjs resources.spec.mjs monitoring-activation.spec.mjs \
  panel.spec.mjs panel-focus.spec.mjs
cargo test --manifest-path src-tauri/Cargo.toml --locked \
  --test genie --test resources --test monitoring
```

`accounts-visual.spec.mjs` covers the V5 account panels with explicitly synthetic
authentication transport responses using the existing native `GithubAuthView`
and `copilot_view` shapes. It asserts exact command payloads (including the Tauri
API's empty `{}` payload), explicit role/provider entry, confirmation before
connection, cancellation, expiry/permission/wrong-identity failures, reconnect
identity pins, read/deletion retries and genuinely disabled future providers.
Both Windows and macOS consent wording are exercised. Account actions leave
production-Store settings, stable Agent/repository references and models
unchanged across fresh Store processes and reload. The existing Copilot,
resource and focus suites retain the unrelated-draft, reference-guard and
no-model-substitution assertions.

Keyboard checks retain focus through unchanged polling, account reorder,
confirmation and cancellation, while delayed reads cannot revive cancelled
sign-in or steal newer focus. Screenshots use each test's output directory at
408x744 and 408x500. The account geometry matrix additionally crosses widths
320, 400 and 408 with heights 300, 400, 439, 440, 441, 460, 499, 500, 501 and 744. Both providers' saved-account actions, requesting, device waiting,
confirmation and failure/retry states use real sequential keyboard navigation,
including opening and closing consent with Enter. On a native macOS host,
Playwright WebKit uses Option+Tab / Option+Shift+Tab to include buttons; other
browser/host combinations retain Tab / Shift+Tab. The local helper checks
`browserName` and Node's `process.platform`, not the deliberately spoofed user
agents used for consent wording. A neutral HTML test checks buttons and a summary
in both directions using the same selected chords. No host keyboard preference
is changed, and no per-control focus replaces sequential traversal.

All 30 dimensions and 4,200 geometry observations remain: every focused control
must fit completely inside the actual scroll viewport and its clipping ancestors,
retain a visible solid 3px focus outline and avoid horizontal clipping. Geometry
JSON is retained for every dimension, including observations collected before an
assertion failure. Routine matrix screenshots cover 320x300 (smallest viewport),
408x441 (just above the outer chrome breakpoint) and 408x744 (full-height panel):
22 forward/reverse and confirmation-focus captures each, 66 rather than 660.
Every failed test still captures a failure screenshot, including dimensions
outside that representative set. Existing compact-flow screenshots, native
command/effect checks and persisted-settings assertions remain unchanged.
Screenshot reduction is not proof of a hosted timing budget; complete hosted
gates remain required.

The account-scoped `:has(.account-connection)` chrome stays compact through 500px,
independently of the outer panel's 440px treatment. The 439/440/441 and 499/500/501
neighbors guard both transitions; consent uses the same scroll margin as account
buttons to prevent fractional edge clipping. For #83 integration, this selector
applies to the shared embedded Integrations chrome while account cards are
mounted, not to repository content/order or Preferences' separate
`[data-preferences]` rules. Preserve that scope and recheck the matrix after
integration rather than copying the account breakpoint into unrelated surfaces.

Failed attempts retain a screenshot and Playwright error context. These are
production-renderer/browser and Store checks, not live sign-in, native credential
storage, installed-app focus or provider acceptance. No fictional-handle form,
real clipboard operation or system-browser handoff is used.

After building the frontend and unchanged `settings_bridge`, a focused run is:

```sh
SETTINGS_TEST_PORT=1457 npm exec playwright -- test \
  --config tests/settings/playwright.config.mjs --browser=chromium \
  accounts-visual.spec.mjs github-auth.spec.mjs github-auth-layout.spec.mjs \
  copilot.spec.mjs resources.spec.mjs completion-focus.spec.mjs \
  panel-focus.spec.mjs platform.spec.mjs
```

Native fake-transport device OAuth and fake-backend confirmation/isolation tests
separately cover the existing authentication boundary. Credential-store tests
require separate native authorization; headless fixtures do not establish those
results. WebKit must be validated separately on a functioning host, not inferred
from Chromium or worked around with skipped assertions.

`preferences.spec.mjs` covers the compact V8 Preferences surface with the existing
test-owned Store bridge and explicit synthetic native registration/authorization
responses. It checks exact global cron/time-zone persistence, the full positive
u32 capacity range, invalid and failed writes, legacy intervals, independent
automatic-start/comment gates, immediate startup/notification/pause boundaries,
pending/rejected operations across unrelated saves, missing settings, exact
notification recovery routes, redacted diagnostics, drafts, Back focus and
hide/reopen. At 320x300, the focused input must fit fully within the scroll
viewport, not merely intersect it. No native application, login registration,
OS notification, provider action or desktop browser is launched by this suite.

After building the frontend and `settings_bridge` as described below, run the
same tests in headless Chromium and WebKit:

```sh
SETTINGS_TEST_PORT=1450 npm exec playwright -- test \
  --config tests/settings/playwright.config.mjs preferences.spec.mjs
SETTINGS_TEST_PORT=1450 npm exec playwright -- test \
  --config tests/settings/playwright.config.mjs --browser=webkit preferences.spec.mjs
```

Captures are under `<target>/visual-84-screenshots/{chromium,webkit}/`.
They are browser/presentation evidence, not installed-app native acceptance.

Run `npm run test:settings` with the repository's Rust toolchain on `PATH`.
Restore dependencies with `npm ci` only after a missing-dependency failure.
If Playwright reports a missing browser executable, install its matching build.

The tests serve the actual Vite production build and operate its grouped
Settings controls in an isolated Chromium session. Tauri IPC is replaced:
storage requests launch the Rust example `settings_bridge`, which creates a
fresh production `Store` at a test-owned temporary root. The fixture returns
explicit empty GitHub/Copilot account lists by default; account/metadata tests
override only their provider responses. Other unknown commands still fail.
Initial host preferences
are seeded through `Store::save_settings`; assertions read through a separate
Store process and reload the UI. Persistence is not a JavaScript imitation.
The fixture serializes native Store operations per test-owned root, matching
the production `Host.store` mutex. Rejections reach their original callers
without blocking later accepted operations; closing rejects new work and drains
all accepted queued operations before removing temporary data, even when an
assertion fails. `fixture-lifecycle.spec.mjs` covers FIFO order, errors, closing,
independent roots and held IPC replies. Reply holds begin after Store completion
and do not retain the transaction queue. Native bridge lookup honors `CARGO_TARGET_DIR`
and the Windows `.exe` suffix; temporary profiles and browser output remain under
that target. The default target is `src-tauri/target`.

`panel.spec.mjs` exercises the production root panel (not the explicit
`?view=settings` / `?view=queue` component harnesses): all four destinations,
one detail layer, exact Back row/scroll/focus, retained unsaved editors,
Escape/close/hide/reopen, native route revisions, notification destinations,
missing/escaped identities, external-auth and retained URL-editor returns,
and 400px/small-monitor layout. Explicit pointer, keyboard and nonfocusing
activation regressions cover row redraw, nested/replacement Settings editors,
and retained drafts. `monitoring-activation.spec.mjs` exercises atomic Save
authorization, write/conflict recovery, explicit disabled saves, unrelated
Preferences, commit-time dismissal locks and Genie without second consent.
Final and conversation suites also exercise
their exact panel job routes. The bridge serializes the production native
`panel::Session` only to carry it between fixture processes; the real host
retains that session in memory. Only the Tauri event delivery and window
visibility boundary are mocked. Tests do not prove OS focus/dismissal or
notification activation; the isolated native harness and platform procedures
remain required.

`fixture-coordination.spec.mjs` checks the process-per-command panel fixture,
not application persistence. A stable, per-root OS file lock owns the entire
panel read/modify/write operation (including snapshots); acquisition fails
explicitly after five seconds rather than stealing ownership. Only panel
commands take this cross-process lock. Direct probe processes remain concurrent
to exercise native contention, while ordinary Store calls share the per-root
fixture queue. Separate fixture roots and already-held IPC replies remain
independent. Never remove the lock sidecar
while its root is live. The normal fixture teardown drains operations before
removing the root.

Panel JSON is written to an existing-dependency `tempfile` in the same root,
flushed with `sync_all`, then atomically replaced while ownership is held.
Malformed JSON, read failures, invalid routes and write failures remain errors;
missing destinations still persist their exact route before returning an error.
Ordinary errors remove temporary files and close the lock handle. OS termination
releases the lock and leaves the last complete JSON intact; abrupt death can
leave an unreferenced temporary file for the root owner's teardown to remove.
This is not a power-loss/directory-durability guarantee or production storage
change. Rust standard file locking and `tempfile` replacement are cross-platform;
hosted Windows and macOS browser checks remain required.

Two opt-in bridge arguments, `--panel-probe=loaded` and
`--panel-probe=staged`, pause after reading the session or halfway through writing
the temporary file. Probe input is one JSON request line followed by
`continue`; stderr reports `fixture-panel:loaded`, `fixture-panel:staged`, or
`fixture-panel:waiting`. EOF without release is an error. Normal command input,
output and error envelopes are unchanged. These test-only barriers exercise
lost-update and torn-read prevention, process termination, replacement failure,
cleanup and bounded contention deterministically, without timing sleeps as
assertions. The test fixture reaps only the children it created, even on failure.
The ordinary-command burst also requires every exact revision to survive.

Each named conversation state-presentation case completes the initial native Job view
and its reads, then completes the Queue UI navigation and its reads before
directly requesting the exact Job again. IPC idle alone is not a route-completion
contract: a queued UI command may not have dispatched yet, and the initial shell
already says "Your queue". Controlled regressions hold the initial native reply
and Queue either before dispatch or after its real Store response, requiring the
persisted route/revision and response order to remain initial Job, Queue, Job.
Their attached JSON is browser/test-bridge evidence, not native-app acceptance or
proof of a particular hosted CI interleaving. Genuine concurrency tests and
lower-revision rejection coverage remain unchanged.

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

The exact two-destination visual assertion explicitly waits for both asynchronous
receipts before reading the full array; identities, order and full URLs remain
strictly equal. Its controlled-completion variant holds the real second
`queue_destination` reply across the assertion's browser turn before releasing
it. No application link behavior, timeout, retry or global IPC helper is changed.

`watch-choices.spec.mjs` covers direct combined/subset/empty selections through
real Store saves and reloads, effective inherited users, account-pinned user
search, keyboard selection, retained selections across loading/empty/error
results, and small-viewport focus. Provider responses are synthetic, not a live
account or native-GUI acceptance claim. Native `monitoring` tests cover all 32
watch/user-presence combinations, stable reviewer identity, draft exclusion,
captured policy, stale read fencing and sticky tracking; primary conversation
tests preserve reply-permission and general-conversation boundaries with watches
off. `github_people` and `repository_read_tests` exercise the production search
parser and generation-fenced selected-account read path.

`shared-editors.spec.mjs` covers compact shared libraries, saved Agent counts
above AI capacity, explicit account/model selection, full editor text and
signatures, keyboard-scrolled and filter-retained doctrine selections, resource
Save/Back/Cancel, shared-use deletion guards, empty-library restart and failed
write/retry at 320x300. Screenshots use Playwright's per-test output directory.
Run with `--browser=chromium` or `--browser=webkit`; the existing
`SETTINGS_TEST_PORT` variable selects an isolated preview port. This is headless
browser and test-owned Store evidence, not native focus, installed-app or live
authentication acceptance.

`doctrine-seeding.spec.mjs` covers the ten app defaults in `src-tauri/doctrines`,
not the skill catalog: exact titles and bodies with only frontmatter/H1 removed,
durable first load, Agent choices before visiting Doctrines, and saving
Integrations first. It
checks edited/custom/deleted and delete-all libraries across fresh Store
processes and UI reload, conservative handling of legacy missing fields,
visible initialization write errors, and conflicting drafts with explicit
discard/reload. `src-tauri/tests/doctrine_seeding.rs` independently checks the
same storage and canonical-content contracts in the application and foundation
harnesses, including the app-specific scope and 500-word body ceiling.
Fresh settings persist the starter library immediately; startup and automation
remain opted out.

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
reopening, stable-binding deduplication, disable/re-enable, and confirmed
removal. Fresh reads protect immutable identity, the independent neighboring
record, and the startup preference throughout. Both lifecycle commands call the
same production Store operations as the native app.

`provider-repository-setup.spec.mjs` covers personal/organization owner browsing,
complete-result search, errors/retry, account and owner races, URL validation,
stable per-account deduplication, disabled durable intake, immediate configuration,
Save authorization and absence of local-discovery/scope controls. Its normal
and compact screenshots use the actual app renderer and production Store bridge.
`repositories-compact.spec.mjs` covers the production repository rows
and editors, saved global schedule
versus unsaved Preferences, every Comment/Approve/Merge combination, automatic
and explicit primary roles, seven separate assignments and retained normal-job
identities. It also checks guarded unbind/write/conflict recovery,
watched-person identity, exact Back focus/scroll, hide/reopen, restart and
320x300 keyboard access. Tests initialize their unique Store/panel fixture
before concurrent readers and use explicit readiness rather than sleeps.
Corrective regressions require explicit Save/Cancel before dirty unbind,
preserving assignment permissions, reviewer overrides, enablement and unrelated
Preferences drafts. They retain clean unbind cancellation/write/conflict checks,
and label saved legacy intervals as polling-blocked without changing their bytes.
Obsolete clone-discovery and scope-selection cases were replaced by the new
provider intake and Save-authorization regressions, not retained as hidden routes.

Run the suite headlessly with either `--browser=chromium` or `--browser=webkit`;
screenshots use Playwright's per-test output directory. Repository, assignment,
permission and preference assertions read fresh production Store processes.
Provider identity and repository lookup transports are
explicit synthetic boundaries, not live-provider proof.
Native `github_connection`, `monitoring`, `iterations` and `resources` test targets
exercise owner metadata/pagination, atomic authorization persistence, failed
cross-store synchronization/recovery, legacy revocation receipts, first-upgrade
folder-field CAS compatibility, all old/future matching PR admission, independent
older reviewer admission, sticky tracking, seven-way scan fan-out/deduplication,
next-scan additions, primary authority and resource guards. The panel-fixture
coordination exception above is test-only; no native implementation is changed.
Browser captures are not installed-app, native focus, credential or
provider-action acceptance.

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
The additive [`macos-webkit.yml`](../../.github/workflows/macos-webkit.yml) gate
runs on fresh hosted `macos-15` runners for every pull request and main push.
It installs the locked npm dependencies and their matching WebKit build, then
uses `npm run test:settings -- --browser=webkit` to build the frontend and
Store bridge and run the **full unchanged browser suite**. It does not filter
tests, relax retries/timeouts, override the user agent or launch a native app.
Existing macOS and Windows gates remain separate and unchanged. Hosted browser
proof neither establishes native Tart readiness nor replaces isolated guest
acceptance for installed-app windows, focus, credentials or live providers.
No developer-host unlock, wake, permission change or local VM setup is required.
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
All 19 states for each kind are independently named tests (38 total), with fresh
real Store fixtures and the unchanged 30-second per-test budget. State settings,
snapshot/status/receipt/count oracles, Job -> Queue -> Job ordering and inspector
captures are unchanged. Later cases explicitly retain the compact viewport and
reduced motion previously inherited from the running-state capture; no cumulative
19-case timeout, retry or skipped state is introduced.
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
