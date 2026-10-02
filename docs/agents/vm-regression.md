# PR Sniper regression preparation (#91)

**Implemented: setup-independent preparation only. Native execution: BLOCKED.**
The [approved nano](../agent/specs/pr-sniper-vm-regression.nano.md) and
[supporting requirements](../agent/specs/pr-sniper-vm-regression.full.md)
remain the authority. No VM is provisioned, no guest driver is present, and no
installed-app or Windows result is claimed. This does not close #28, #33 or #91.

## Three repository-owned workflows

Use [setup-regression-suite-mac](../../.github/skills/setup-regression-suite-mac/SKILL.md)
only with interactive human consent; [regression-test](../../.github/skills/regression-test/SKILL.md)
authors common executable cases; [regression-suite-mac](../../.github/skills/regression-suite-mac/SKILL.md)
prepares a selected/full suite for a future independent worker.
Their `intent.md` files preserve exact D01-D07 and AC-003/005/006 clauses approved
in human-merged PR #94, rather than inventing another confirmation.

Each is a small original package, not an imported atomic workflow framework.
The authoring guidance's fresh-context Roast/remediation gate has **not** run:
finalization is blocked pending parent-owned independent current-head review.
Coaching/Chronicler/atomic-validator integration is not supplied by this app
repository; no such receipts or author self-signoff are claimed.

## Safe commands available now

From the selected PR Sniper worktree with Node 24.20:

```sh
node regression-suite/macos.mjs list
node regression-suite/macos.mjs prepare panel-navigation panel-drafts
node regression-suite/macos.mjs prepare full
npm run test:regression
```

`list` exits 0 and lists registered metadata, not passing tests. `prepare` emits
JSON and exits **20 (BLOCKED)**, with selected, not-selected and missing features,
zero native executions, current test revision/content hashes and missing
prerequisites. No candidate is guessed; the CLI leaves candidate identity missing.
The pure `prepareReport` API can carry an explicit declared identity for contract
checking; declaration is not artifact/signature verification.

`run` currently has exactly the same fail-closed behavior as `prepare`: it cannot
start Tart, SSH, a guest, a driver, a browser or an app. There is no fixture-to-native
fallback and no input that enables native PASS. Do not mistake readiness for
regression success. Inspection failures and invalid selection also exit 20.
Reports go to stdout; nothing writes a private runtime or evidence directory.
Keep saved reports private and ignored; do not commit machine paths or identities.

## Canonical runtime and ownership

`canonicalRuntime` resolves Git's first (primary) non-bare worktree, verifies its
common Git directory and top-level identity, then appends `.regression-suite/`.
The feature checkout selects source/tests, **not another VM**. Linked worktrees
and the primary share this one runtime even when paths contain spaces.
Separate clones have separate primary roots; bare/missing/inconsistent primaries
block. Symlink runtime homes are refused rather than following another environment.

`inspectReadiness` is read-only filesystem inspection, **not live VM probing**.
It distinguishes absent, unsafe, present-but-unverified, and busy/uncertain state.
Any `guest.lock` entry blocks, including malformed/symlink claims; its contents
are not read or deleted. No expiry, PID guess, lock stealing or automatic recovery.
There is no executable guest mutation path yet, so no lock is acquired today.
The future executor must atomically claim guest-wide ownership before its first
mutation, including across processes/worktrees, and retain it if cleanup or
termination is uncertain. A concurrent request must visibly report busy/BLOCKED.

Only the root-anchored `/.regression-suite/` is the private runtime convention.
After separately approved setup it may hold local ownership instructions, disks,
private access material and append-only run evidence. Portable cases, registry,
skills and this reconstruction recipe remain tracked outside it. This preparation
does not create its contents or access Notch's private runtime.

## Common executable cases and unproved native seam

`regression-suite/cases/panel.mjs` contains two real feature expectations from
the existing browser/native acceptance contracts:

| ID                    | Feature selection  | Assertions                                                                                        |
| --------------------- | ------------------ | ------------------------------------------------------------------------------------------------- |
| `panel-destinations`  | `panel-navigation` | Exact Queue/Running/Reviewed/Settings selection and headings, one retained native window          |
| `panel-unsaved-draft` | `panel-drafts`     | Unsaved doctrine text survives navigation, Escape and Close-button hide/reopen in the same window |

The common `run({act, observe, equal})` functions contain the assertions. A later
Mac or Windows driver implements the **same** semantic actions, not another set
of feature expectations. `openPanel`, `navigate`, `newDoctrineDraft` and
`dismissPanel` must become actual guest-native interaction. `panel` observations
need visible state, native window ID/count, selected destination and rendered
heading; `doctrineDraft` needs the actual visible title/principles controls.
Missing selectors, permissions, unsupported operations or observations block.
Do not return constants, call internal app controllers or substitute browser
Store bridge results. No SwiftUI-specific selectors or VM/account names are reused.

Each driver must establish the listed empty/offline baseline, discard its unsaved
draft, restore the original profile/hidden state and verify owned process/child
termination, including after a failed assertion. Implement and prove that seam
separately before any native run. The existing `settings_bridge` is useful prior
art for synthetic fixture seeding, **not a native observation channel**.

`contract.mjs` only exercises **fixture contracts**, deliberately labeled
`evidenceKind: fixture-contract` and `nativeStatus: BLOCKED`, even when the
fixture oracle says PASS. Wrong observed values yield raw FAIL; missing/stale
identity, driver failures and absent observations yield BLOCKED. Driver assertion
errors are harness errors, not automatically product regressions.
Cleanup uncertainty blocks success and retains an earlier raw FAIL.
`summarizeAttempts` keeps all supplied attempts; retrying cannot erase failure.
Negative controls are assertions in the fixture tests that expect raw FAIL or
BLOCKED, not additional passing application cases.

The structural identity input records `sourceCommit`, `testCommit`,
`testTreeSHA256`, `buildCommand`, `original` and `installed` artifact SHA-256,
executable SHA-256, bundle ID and signature qualification, `installedPath`,
`transformation` (null only for unchanged identity), `instance` (ID/PID/start),
and `isolation` (absolute data directory/test credential namespace). See the
synthetic builder in `tests/regression/preparation.test.mjs` for its shape.
Receipts bind run, attempt, case, identity digest, target, increasing sequence,
fresh observation time and payload digest. These checks do not authenticate an
untrusted native driver. Future native proof must verify actual artifact bytes,
signature, production frontend, process start identity and effective isolation,
plus evidence capture provenance. A declared JSON hash is not that proof.

Full means **all registered cases only**. Native close, Back/focus, notifications,
geometry, login/provider flows and all other gaps stay visible. No retrospective
backfill or historical issue closure is implied. Existing unit/Rust integration,
browser/release checks remain complementary; Windows fixture CI is not Windows
native execution.

## Morning human-owned reconstruction and proof

Do not execute this checklist as unattended setup. Missing/incomplete setup
first requires an interactive user's consent and separately authorized tooling.

1. Resolve host compatibility, current licenses/VM limits, storage and CPU/RAM
   budget with the owner. Preserve both existing Notch VMs; no takeover, deletion,
   third-VM assumption or private inventory/credential reads.
2. Choose a dedicated PR Sniper guest and baseline/reset ownership. Pin and
   verify official Tart, Apple image and compatible licensed Xcode provenance.
   No versions, sizing or VM names from the reference are prescribed here.
3. Let the user handle dedicated guest account/password, guest access/host-key
   trust and any OS/privacy/network/security decisions. No personal account,
   profile migration, host credential reuse or automated privacy database edits.
4. Establish effective guest graphical login, idle/lock behavior, accessibility
   and observation capability through approved guest-only checks. **Headless is
   a guest GUI without a viewer or host input**, not no graphical session.
   A stopped prepared guest and an absent guest require different actions; the
   current filesystem inspection cannot establish either live condition.
5. Agree the exact original-versus-test-copy contract, isolated
   `PR_SNIPER_DATA_DIR` and `PR_SNIPER_KEYCHAIN_SERVICE` starting with
   `com.jdylanmc.pr-sniper.tests.`. Record transformations separately. The
   verifier must not patch, rebuild, replace or re-sign the submitted candidate.
6. Prove the Tauri native seam, actual independent dispatch, atomic ownership,
   wrong-output and stale-evidence rejection, abort cleanup and fresh-baseline
   repeat. Keep earlier failures and unresolved termination visible. Bounded
   proof is not overnight endurance; reset proof is not just process restart.
7. Return evidence to the main agent for suspected-bug validation and then
   ad-hoc triage or the owning feature flow. No automatic tracker/provider writes,
   paid inference, login-item changes, review publication, approval or merge.

## Existing native acceptance disposition

`tests/macos-native-smoke.swift` and [the old acceptance procedure](../../tests/macos-acceptance.md)
remain useful compile-only/reference material. **Do not run them on the host
desktop for routine regression, even if the VM is absent.** Historical host
instructions are not a fallback grant. Their candidate isolation and cleanup
contracts inform a future guest adaptation; this change neither ports nor runs
them. The existing CI harness compilation and all prior gates stay intact.

Behavioral reference only: Notch's tracked reconstruction/worker/probe design
in PR #101. No Notch code, selectors, licensed package, private data or tool
execution was imported into this original PR Sniper preparation.
