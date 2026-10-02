# Discovery Foundation

- Schema: 2
- Subject: https://github.com/jdylanmc/pr-sniper/issues/91
- Slug: pr-sniper-vm-regression
- Alignment: confirmed
- Aligned Findings Digest: 9e2f119eed0e3f49b00d840b7d0e85f9a9c65e3e952a96be6b2b33b7c0399d68
- Domain Model Basis Digest: 9e2f119eed0e3f49b00d840b7d0e85f9a9c65e3e952a96be6b2b33b7c0399d68
- Domain Model Digest: 5e0731914af894c8dfef3292baf928b7540a4e77202e29d9388f75421d3a098d
- Frontier Basis Digest: 5e0731914af894c8dfef3292baf928b7540a4e77202e29d9388f75421d3a098d
- Frontier Digest: b4062802ec08e7079fd9338d28ddc2155b73df53a1d7df987958ea84b7857882

## Confirmed Facts

- The Notch reference PR #101 is merged. It provides an existing guest-native regression mechanism, not proof of PR Sniper's Tauri driver or Windows execution.
- PR Sniper already has native test isolation and acceptance work that can be assessed for reuse; its browser tests do not execute the installed Tauri webview.
- Dylan explicitly confirmed this consolidated discovery direction after choosing a dedicated repository-local VM, shared executable cases, platform-specific setup and runners, and macOS-first delivery.

## Evidence References

- https://github.com/jdylanmc/pr-sniper/issues/91 - exact subject; full body and eleven proposed acceptance items read through the research route.
- https://github.com/jdylanmc/notch/pull/101 - merged reference; cited source head 5e5409ef41a617bfae187f8f240e2f4df5981049, merge commit 547d65916aa73fefb632e0bb58f34aa4a0d835df.
- https://github.com/jdylanmc/pr-sniper/issues/28 - broader regression capability expectations.
- https://github.com/jdylanmc/pr-sniper/issues/33 - historical native observation and harness gaps to reconcile, not presumed current application defects.
- https://github.com/jdylanmc/notch/blob/5e5409ef41a617bfae187f8f240e2f4df5981049/docs/agents/vm-regression.md - portable reconstruction and bounded proof record.
- https://github.com/jdylanmc/notch/tree/5e5409ef41a617bfae187f8f240e2f4df5981049/experiments/tart-regression - guest-native driver, executable suite, registry and evidence interpreter.
- https://github.com/jdylanmc/notch/blob/5e5409ef41a617bfae187f8f240e2f4df5981049/.github/skills/regression-suite/WORKER.md - independent worker inputs, candidate verification and outcome boundaries.
- Notch's ignored .local/vm-regression/AGENTS.md - read-only existing environment inventory and ownership instructions; no keys, VM disk contents or guest session were accessed.
- tests/macos-acceptance.md and tests/macos-native-smoke.swift - current native isolation, exact identity, lifecycle and cleanup contract.
- tests/settings/playwright.config.mjs and tests/settings/fixtures.mjs - browser renderer with Store bridge, not installed-app execution.
- docs/windows-development.md - Windows has its own native runtime, tooling and acceptance boundaries.
- docs/agent/discovery/pr-sniper-mvp.md and docs/agent/discovery/pr-sniper-vnext.md - earlier subjects read and preserved unchanged.
- Dylan McCurry's direct discovery answers and explicit consolidated confirmation in this session - authority for D01-D07.

## Decisions

- D01 - Dylan: PR Sniper gets a dedicated VM. Routine app interaction happens inside it, never on the host desktop, even when setup is unavailable.
- D02 - Dylan: Local runtime lives at the canonical PR Sniper repository root in .regression-suite/, gitignored and shared by the repository's feature worktrees.
- D03 - Dylan: setup-regression-suite-mac guides missing or incomplete setup. Passwords, OS permissions and resource/security choices stay user-controlled.
- D04 - Dylan: regression-test adds shared executable feature cases. Platform-specific interaction is separated from shared behavior and assertions; there are no duplicated Mac/Windows case libraries.
- D05 - Dylan: regression-suite-mac runs requested features or the full registered suite using an independent worker. Build the macOS environment first; Windows setup and execution follow separately and must run the same cases.
- D06 - Dylan: Skills, test sources, registry and reconstruction guide stay in Git. VM disks, credentials and generated evidence stay ignored. Existing unit, build and browser checks remain.
- D07 - Dylan: Results identify the exact candidate, exercised cases, PASS/FAIL/BLOCKED, evidence and cleanup. Missing coverage stays visible; the main agent validates suspected bugs before routing them.

## Constraints

- C01 - Shared cases must be designed for independent execution on both platforms from the start; the initial macOS delivery does not claim a working Windows runner.
- C02 - A full suite means all registered cases, not proof that every application feature is already covered. Add executable regression coverage as features are built.
- C03 - Preserve existing product-Agent permissions, account isolation and provider-action boundaries. Development test infrastructure is separate.
- C04 - Preserve Notch's existing VMs and evidence. Dedicated PR Sniper provisioning/reset must address the documented existing VM/resource/licensing constraints before adding or replacing environments.
- C05 - No VM setup, launch, SSH access, private-key reads, host UI operation, implementation, commits or tracker changes occurred in this discovery.

## Assumptions

_None recorded._

## Contradictions

- Issue #91's suggested .local/vm-regression location and unqualified setup/runner skill names are superseded by Dylan's .regression-suite location and macOS-qualified setup/regression-suite names.
- Issue #91's draft-at-filing caveat for Notch PR #101 is historical; the reference is now merged. Its mechanism still does not establish PR Sniper compatibility.
- Treating a browser-only test or a macOS pass as proof of installed Tauri behavior or Windows execution would contradict the confirmed testing boundary.

## Open Questions

- Driver adaptation and proof owner: establish PR Sniper's guest-native Tauri control/observation path while keeping shared case behavior independent of platform-specific interaction.
- Candidate-contract owner: decide and verify the exact original-versus-test-copy identity contract before executing a transformed candidate.
- Guided setup and human operator: inspect current resource/licensing availability and agree a dedicated VM provisioning/reset arrangement without silently deleting or taking over either Notch VM.
- Setup verification owner: verify effective guest permissions, login/idle behavior, clean reset and restoration; bounded proof must not be represented as overnight endurance.
- Later Windows workflow: provide its own setup/native interaction runner and independent results for the same shared cases.
- Existing acceptance reconciliation owner: map current native coverage and remaining #28/#33 gaps without automatically closing either issue.

## Source Claims

- Issue #91 proposes development regression infrastructure modeled on Notch, not broader permissions for PR Sniper's product review Agents.
- Notch's tracked reconstruction guide describes working guest-local UI actions and output assertions in a headless Tart VM. Its corrected bounded host-locked proof is not a successful replacement overnight endurance run.
- Notch's local environment instructions record two existing VM directories and a two-instance/resource limit; they prohibit silently adding a third or taking over another environment. This is documented inventory, not a fresh live resource or licensing determination.
- PR Sniper issues #28 and #33 record broader regression expectations and historical native acceptance/harness gaps; adopting #91 does not automatically complete either issue.

## Relationship Claims

- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D04"}],"notes":["Platform-specific interaction remains separate from shared case intent and assertions."],"relationship":"adds feature behavior and assertions without separate Mac and Windows case libraries","source":"regression-test","target":"Shared executable feature cases"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D05"}],"notes":["The actual built application is operated and observed in the guest."],"relationship":"runs requested features or the full registered suite inside the dedicated VM","source":"regression-suite-mac","target":"Independent test worker"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D03"}],"notes":["An unavailable VM never permits host-desktop UI fallback."],"relationship":"routes to guided user setup","source":"Missing or unusable regression environment","target":"setup-regression-suite-mac"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D04"},{"reference":"D05"}],"notes":["Windows execution is separate follow-up scope, not established by a macOS pass."],"relationship":"must execute the same underlying cases independently","source":"Future Windows setup and runner","target":"Shared executable feature cases"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D07"}],"notes":["Reports preserve actual outcomes, missing coverage and cleanup state."],"relationship":"validates evidence before routing","source":"Main development agent","target":"Reported potential bugs"}

## Boundary Claims

- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D01"}],"notes":["Non-interactive unit, build and browser checks remain separate obligations."],"relationship":"must not focus, click, type or fall back to routine host application automation","source":"Regression UI execution","target":"Host desktop"}
- JSON: {"confidence":"confirmed","direction":"bidirectional","evidence":[{"reference":"D02"},{"reference":"D06"}],"notes":["The canonical repository root is shared by its feature worktrees; no VM per branch."],"relationship":"separates portable skills, tests, registry and reconstruction guidance from VM disks, credentials and generated evidence","source":"Git-tracked regression assets","target":"Ignored .regression-suite runtime"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D03"}],"notes":["No silent deletion, replacement or takeover of existing Notch VMs."],"relationship":"leaves passwords, OS permissions and resource/security choices with the user","source":"Guided setup","target":"Human operator"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"https://github.com/jdylanmc/pr-sniper/issues/91"},{"reference":"C03"}],"notes":["Guest verification is not incidental authority for real provider mutations or paid inference."],"relationship":"does not expand existing execution, account, trust, publication or provider-action permissions","source":"Development regression infrastructure","target":"Product review Agents"}

## Risks

- A nominally shared test library could hide divergent per-platform expectations or missing Windows support; platform execution and missing coverage must remain explicit.
- A guest can be booted but locked, unprepared or unable to run the native driver; setup readiness cannot be treated as a passing application test.
- Re-signing or changing a test copy's identity can invalidate a claim that the unchanged release was tested; original and transformed artifact identities must be recorded separately.
- Adding a dedicated VM without resolving existing resource/licensing limits could disturb Notch's working environment.
- Harness failures, stale evidence and deliberate negative controls must not be laundered into application success or automatically filed as product bugs.

## Scope

- PR Sniper #91: establish the direction for non-disruptive installed-app feature verification and reusable regressions using a dedicated macOS Tart VM.
- Three initial roles: platform-neutral regression-test, setup-regression-suite-mac, and regression-suite-mac; portable executable cases from the start, Windows runner later.
- This aligned discovery foundation informs specification and subsequent explicitly authorized setup/implementation.

## Exclusions

- Host-desktop application automation as a routine test path or missing-VM fallback.
- Windows environment implementation in the initial macOS delivery, or inferring Windows success from macOS results.
- Duplicated platform-specific feature case libraries, fabricated whole-app coverage, or replacement of existing unit/build/browser checks.
- Implicit Notch VM takeover/deletion, automatic OS-consent changes, real account/credential migration, paid inference or provider mutations.
- Implementation, provisioning, tracker edits, commits, pushes, merges and releases during this discovery.

## Domain Model

- JSON: {"actors":[{"aliases":["Dylan"],"confidence":"confirmed","evidence":[{"reference":"D01"},{"reference":"D03"}],"kind":"actor","name":"Human operator","notes":["Keeps control of the host desktop and user-owned setup choices."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D04"},{"reference":"D07"}],"kind":"actor","name":"Main development agent","notes":["Adds feature coverage and validates reported potential bugs before routing."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D05"},{"reference":"D07"}],"kind":"actor","name":"Independent test worker","notes":["Operates the candidate inside the guest and returns evidence, not self-authored success claims."]}],"boundaries":[{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D01"}],"notes":["Non-interactive unit, build and browser checks remain separate obligations."],"relationship":"must not focus, click, type or fall back to routine host application automation","source":"Regression UI execution","target":"Host desktop"},{"confidence":"confirmed","direction":"bidirectional","evidence":[{"reference":"D02"},{"reference":"D06"}],"notes":["The canonical repository root is shared by its feature worktrees; no VM per branch."],"relationship":"separates portable skills, tests, registry and reconstruction guidance from VM disks, credentials and generated evidence","source":"Git-tracked regression assets","target":"Ignored .regression-suite runtime"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D03"}],"notes":["No silent deletion, replacement or takeover of existing Notch VMs."],"relationship":"leaves passwords, OS permissions and resource/security choices with the user","source":"Guided setup","target":"Human operator"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"https://github.com/jdylanmc/pr-sniper/issues/91"},{"reference":"C03"}],"notes":["Guest verification is not incidental authority for real provider mutations or paid inference."],"relationship":"does not expand existing execution, account, trust, publication or provider-action permissions","source":"Development regression infrastructure","target":"Product review Agents"}],"concepts":[{"aliases":["Shared regression tests"],"confidence":"confirmed","evidence":[{"reference":"D04"},{"reference":"D05"}],"kind":"concept","name":"Shared executable feature cases","notes":["Shared behavior and assertions; platform-specific interaction is separate."]},{"aliases":[".regression-suite/"],"confidence":"confirmed","evidence":[{"reference":"D02"},{"reference":"D06"}],"kind":"concept","name":"Canonical ignored runtime directory","notes":["Located at the canonical PR Sniper repository root and shared by feature worktrees."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D06"}],"kind":"concept","name":"Tracked regression assets","notes":["Skills, executable tests, registry and reconstruction guidance."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D07"}],"kind":"concept","name":"Candidate-bound regression report","notes":["Preserves exercised scope, raw outcomes, evidence, missing coverage and cleanup."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D03"}],"kind":"concept","name":"Missing or unusable regression environment","notes":["Invokes guided setup rather than host UI fallback."]}],"confidence":"confirmed","events":[],"relationships":[{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D04"}],"notes":["Platform-specific interaction remains separate from shared case intent and assertions."],"relationship":"adds feature behavior and assertions without separate Mac and Windows case libraries","source":"regression-test","target":"Shared executable feature cases"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D05"}],"notes":["The actual built application is operated and observed in the guest."],"relationship":"runs requested features or the full registered suite inside the dedicated VM","source":"regression-suite-mac","target":"Independent test worker"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D03"}],"notes":["An unavailable VM never permits host-desktop UI fallback."],"relationship":"routes to guided user setup","source":"Missing or unusable regression environment","target":"setup-regression-suite-mac"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D04"},{"reference":"D05"}],"notes":["Windows execution is separate follow-up scope, not established by a macOS pass."],"relationship":"must execute the same underlying cases independently","source":"Future Windows setup and runner","target":"Shared executable feature cases"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D07"}],"notes":["Reports preserve actual outcomes, missing coverage and cleanup state."],"relationship":"validates evidence before routing","source":"Main development agent","target":"Reported potential bugs"}],"states":[],"systems":[{"aliases":["Actual compiled Tauri application"],"confidence":"confirmed","evidence":[{"reference":"D01"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/91"}],"kind":"system","name":"PR Sniper candidate","notes":["Guest-native interaction is distinct from browser-only regression tests."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D01"},{"reference":"D02"}],"kind":"system","name":"Dedicated macOS Tart VM","notes":["Chosen initial environment; PR Sniper-specific provisioning and driver proof are not yet performed."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D04"}],"kind":"system","name":"regression-test","notes":["Platform-neutral test-authoring role."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D03"}],"kind":"system","name":"setup-regression-suite-mac","notes":["Guided macOS environment preparation, not an automatic permission grant."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D05"}],"kind":"system","name":"regression-suite-mac","notes":["Runs selected features or the full registered suite through an independent worker."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D05"},{"reference":"C01"}],"kind":"system","name":"Future Windows setup and runner","notes":["Separate later execution environment for the same shared cases; no Windows acceptance is claimed."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D01"}],"kind":"system","name":"Host desktop","notes":["Must remain undisturbed by routine application computer-use verification."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"C04"},{"reference":"https://github.com/jdylanmc/notch/pull/101"}],"kind":"system","name":"Existing Notch regression environment","notes":["Reference mechanism and existing VM ownership to preserve; not the selected PR Sniper VM."]}],"terms":[{"aliases":["Full regression suite"],"confidence":"confirmed","contested":false,"evidence":[{"reference":"C02"}],"kind":"term","name":"Full registered suite","notes":["All registered cases; does not silently imply every application feature is covered."]},{"aliases":[],"confidence":"confirmed","contested":false,"evidence":[{"reference":"D07"}],"kind":"term","name":"PASS/FAIL/BLOCKED","notes":["Actual results remain distinct from missing prerequisites, stale evidence and deliberate controls."]}],"unsettledSeams":[{"confidence":"unknown","evidence":[{"reference":"D04"},{"reference":"Driver adaptation and proof owner"}],"kind":"unsettled-seam","notes":["Needs bounded native proof, not another debate over the already demonstrated Tart mechanism."],"question":"How will the guest-native Tauri driver execute the shared cases without placing platform-specific feature expectations in separate libraries?"},{"confidence":"unknown","evidence":[{"reference":"Candidate-contract owner"}],"kind":"unsettled-seam","notes":["Retain existing test-isolation safeguards and report artifact transformation honestly."],"question":"How are the original candidate and any explicitly transformed test copy identified and verified?"},{"confidence":"unknown","evidence":[{"reference":"C04"},{"reference":"D03"}],"kind":"unsettled-seam","notes":["Guided setup and the human operator own this decision; no environment operation has occurred."],"question":"Which safe provisioning/reset arrangement fits the existing VM/resource/licensing limits without disturbing Notch?"},{"confidence":"unknown","evidence":[{"reference":"C02"},{"reference":"Existing acceptance reconciliation owner"}],"kind":"unsettled-seam","notes":["Do not close historical coverage issues or claim whole-app completeness from the initial suite."],"question":"Which current native behaviors are initially registered, and which #28/#33 observations remain outside that coverage?"},{"confidence":"unknown","evidence":[{"reference":"D05"},{"reference":"C01"}],"kind":"unsettled-seam","notes":["Follow-up scope; sharing cases is not evidence of successful Windows execution."],"question":"What independent Windows environment and native interaction support will execute the same cases later?"}]}

## Frontier

- ready | origin: loop | Human-aligned direction is sufficient for specification: dedicated canonical repository-local runtime, shared executable cases, platform-specific setup/execution skills and macOS-first scope are settled.
- needs-proof-of-concept | origin: loop | A bounded guest-native PR Sniper/Tauri driver and original-versus-test-copy identity proof are required before claiming the regression environment works; the existing Notch mechanism alone is not that proof.
- setup decision | origin: loop | Guided setup and the human operator must inspect resource/licensing availability and agree dedicated VM provisioning/reset without silently altering Notch's existing VMs.
- deferred | origin: loop | Windows setup/native runner follows separately and must execute the same shared cases with its own evidence.
- coverage boundary | origin: loop | Reconcile initial registered scenarios with #28/#33 and current native acceptance; full registered coverage is not universal application coverage.
- tracker boundary | origin: loop | No tracker update requested; issue edits, assignments and dependency changes require their own approval.

## Next Action

Use specification to formalize the aligned macOS-first environment and shared cross-platform test direction. Preserve bounded Tauri-driver/artifact proof and human-guided provisioning/resource decisions as explicit prerequisites to operational readiness. Do not provision VMs, implement, update trackers, or claim Windows/native compatibility in this Discovery run.

## Resolved

_None recorded._

## History

- vm-regression-human-alignment-2026-10-01 | 2026-10-02T01:26:13.540Z | verified | succeeds none
