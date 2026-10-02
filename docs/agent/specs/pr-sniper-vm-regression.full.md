# PR Sniper Non-disruptive Regression Testing - Supporting Requirements

- Spec ID: SPEC-PR-SNIPER-VM-REGRESSION
- Source: docs/agent/discovery/pr-sniper-vm-regression.md
- Source revision: 9c2f652a2bee53207581ff7f51d384e48318aab1941f387076eb8457a4242268
- Nano authority: [Product intention and acceptance criteria](./pr-sniper-vm-regression.nano.md)

## Authority

The nano sibling is the product authority for this scoped development-testing
capability under [#91](https://github.com/jdylanmc/pr-sniper/issues/91).
This document supplies context and elaboration; it cannot override the nano.
Requirement and decision bullets below name the nano criterion they elaborate.

The existing [MVP](./pr-sniper-mvp.nano.md) and
[vNext](./pr-sniper-vnext.nano.md) product specifications remain unchanged.
Development agents operating a test guest are not PR Sniper's product review
Agents. Approval of test infrastructure does not expand product permissions.

This pair is a candidate until a human merges its specification PR to the
repository's default branch. Discovery alignment, a valid pair and independent
review are not specification approval. A human merge approves these
requirements; it does not establish a provisioned VM, passing tests or native
driver compatibility.

## Problem and Users

The developer needs to keep using the host computer while agents build and
verify PR Sniper. Host-side native acceptance currently uses the real desktop,
focus, menu bar, keyboard and folder dialogs. Isolation of application data
does not isolate those interactions from the person.

The main development agent needs an executable, reusable way to verify a
feature and later rerun relevant accumulated regressions. A separate test
worker needs enough candidate, case and environment information to produce
reviewable results without relying on the implementation author's memory.
The human remains responsible for sensitive setup choices.

## Outcomes and Success

Success is the nano's observable separation: real application behavior
exercised inside the guest, shared executable cases, independent evidence and
an undisturbed host desktop. No execution-time, resource-consumption,
performance or overnight-duration threshold was settled in Discovery.

The unit of coverage is a registered feature case, not an agent's declaration
that it tested the whole app. A requested feature may lack a case or encounter
an unsupported native operation; those are visible coverage gaps rather than
successful or silently reduced scope.

## Scope and Non-goals

The initial execution environment is a dedicated macOS Tart VM. Windows has
its own later setup and runner work, but the common cases are shared from the
start. This is not a request for two independent libraries containing similar
prose or duplicated feature assertions.

The nano non-goals exclude Windows environment implementation in this slice,
host UI fallback, automatic OS consent, comprehensive retrospective coverage
and changes to product permissions or distribution.

The three initial workflow roles are:

| Role | Purpose | Boundary |
| --- | --- | --- |
| `setup-regression-suite-mac` | Guide preparation and verify usable macOS setup | Readiness is not a regression pass; user-owned setup choices remain explicit |
| `regression-test` | Author shared executable feature cases and registration | Author development runs are not independent verification |
| `regression-suite-mac` | Select cases and coordinate independent guest execution | It reports actual coverage and does not rewrite expectations or product behavior |

## Constraints and Dependencies

The stable runtime home is the canonical repository's `.regression-suite/`,
not each feature worktree's current directory. Selecting a candidate from a
worktree does not select another VM or another repository's environment.
How the implementation resolves the canonical root is a technical design
question, not a prescribed Git algorithm here.

Portable material stays tracked outside that private runtime. A fresh clone
contains reconstructible guidance and test sources, not VM disks, credentials
or a claim that the current machine is ready. Existing environments and their
ownership must be accounted for before provisioning.

## Confirmed Facts

- The confirmed foundation records Dylan's decisions D01-D07, including the dedicated VM, `.regression-suite/`, shared executable cases and macOS-first delivery.
- Notch PR #101 is merged and supplies an existing guest-native regression mechanism.
- PR Sniper has native isolation and acceptance work to assess for reuse. Its browser renderer and Store bridge do not execute the installed Tauri webview.
- Discovery did not provision, launch or operate a PR Sniper guest, modify product code, or change #28/#33.

### Source Claims

These remain claims/evidence recorded by the foundation, not PR Sniper
acceptance:

- Notch's guide describes guest-local actions and rendered-output assertions in a headless Tart VM, including bounded success while the host was locked. It explicitly does not claim a successful replacement overnight endurance run.
- Notch's local instructions record two VM directories and an existing resource/licensing limit. They are not a fresh live inventory or a legal determination of what additional installation is permitted.
- #28 describes broader regression capability; #33 records historical native observations and separate harness gaps. Their continued existence is not proof of a current application defect or permission to execute an unsafe old harness.

The foundation's evidence references preserve the cited Notch snapshot,
worker contract and current PR Sniper acceptance/browser documentation.
Notch's app selectors, account names, candidate hashes and local VM names are
reference-specific, not requirements for this product.

## Assumptions

No additional product assumptions are introduced. In particular, this pair
does not assume that a SwiftUI-specific Notch driver controls Tauri, that a
browser test is an installed-app test, or that a booted guest is logged in,
unlocked and ready for UI execution.

## Contradictions

| Earlier wording or interpretation | Confirmed direction |
| --- | --- |
| #91 suggests `.local/vm-regression/` | D02 fixes the runtime location at canonical repository-root `.regression-suite/` |
| Unqualified setup/runner skill names | D03/D05 select `setup-regression-suite-mac` and `regression-suite-mac`; test authoring stays shared |
| A separate feature test library for each platform | D04 requires the same underlying executable cases, with native interaction separated |
| Notch PR #101 was draft at filing | It is now merged; this resolves freshness, not PR Sniper compatibility |
| Browser or Mac success establishes installed-app or Windows success | Each execution environment needs its own actual evidence |
| An unavailable VM permits host UI testing | D01 excludes that fallback |

This pair does not edit or close the historical tickets, and it does not
reinterpret their old platform/version wording as newly approved requirements.
Current coverage reconciliation remains explicit work.

## Alternatives and Examples

Sharing Notch's working guest, using a VM per feature branch, separating Mac
and Windows case libraries, or falling back to host UI automation conflicts
with the confirmed direction. They are not equivalent implementations of
this specification.

The native-driver framework, common case representation, guest artifact
placement, original/test-copy identity mechanism and VM provisioning/reset
procedure were not selected. Those choices must satisfy the observable
requirements without silently turning this context into technical design.

## Product Requirements

- VR-001 [AC-001]: Exercise the built application's real guest UI and observe the resulting behavior there. Headless means no routine host viewer, host pointer/keyboard control or focus takeover; it does not mean substituting a browser page or an internal model call for the application.
- VR-002 [AC-002]: All worktrees of the canonical repository discover the same private runtime home. The selected source/build identity remains explicit even though the VM environment is shared by those worktrees.
- VR-003 [AC-002]: Keep portable skills, executable cases, registration and reconstruction guidance in Git, and keep private disks, credentials and generated evidence out of Git. An absent private directory is an expected setup condition, not permission to invent its contents.
- VR-004 [AC-003]: When setup is absent, inaccessible or unusable, route to the guided Mac setup workflow and report what requires the user. A stopped but usable environment and a missing environment are different observations. Declined or incomplete setup leaves testing unexecuted, not passed.
- VR-005 [AC-003]: Establish resource/licensing availability, ownership and effective required guest readiness before claiming setup complete. Passwords, OS authorization, guest security settings and replacement/deletion choices are not silently supplied by the agent; existing Notch resources remain protected.
- VR-006 [AC-004]: Shared cases carry their feature behavior and assertions independently of macOS-specific native operations. The later Windows runner consumes the same cases, rather than a separately authored set of Windows expectations. Until that runner exists and is exercised, report Windows execution as unavailable or unverified.
- VR-007 [AC-005]: A feature case states its supported preconditions, observable actions and expectations, and cleanup. Its executable source and registration accompany the feature coverage; narrative notes alone do not satisfy this capability.
- VR-008 [AC-006]: Resolve the requested feature selection explicitly. Reports distinguish selected subsets from the full registered suite and expose unregistered, unsupported or unexecuted requested coverage. Do not silently substitute a smaller successful suite.
- VR-009 [AC-007]: Independent verification binds an actual separate worker assignment to the submitted candidate and tests. Do not replace that evidence with different actor-name strings, a second shell in the author's context, or an author's self-signoff.
- VR-010 [AC-008]: Retain source/build identity, test-source identity, installed artifact identity, signature qualification and the actual running instance with the run evidence. Keep the test profile and credential namespace isolated; a transformed test copy has its own identity and does not prove unchanged-release behavior.
- VR-011 [AC-009]: Assess the intended observable output, not an incidental occurrence of similar text or the success of an automation command. Missing identity, stale observations, skipped execution or unavailable capability cannot be normalized into PASS.
- VR-012 [AC-009]: Separate the expected outcome of a deliberate negative control from its raw FAIL/BLOCKED result and from an unexpected product failure. Preserve earlier attempts and their failure evidence; a later pass does not explain or erase them.
- VR-013 [AC-010]: Report restoration and termination as observed outcomes, including when the primary assertion aborts. Repeated runs must be able to establish their declared starting state without inheriting unexplained prior mutations. Preserve exclusive ownership while termination or cleanup remains uncertain; do not claim a clean reset merely because a process was restarted.
- VR-014 [AC-011]: Return results to the main development agent for evidence-backed triage. The report distinguishes candidate product defects, harness faults, setup problems, expected controls and missing evidence; it does not itself establish a new confirmed bug or authorize a tracker write.
- VR-015 [AC-012]: Retain existing complementary checks and product safety boundaries. Guest execution does not grant access to real provider credentials, personal data, login-item changes, paid inference or external publication, approval or merge effects.

## Product Decisions

- PD-001 [AC-001]: D01 chooses a dedicated headless Tart VM for routine Mac application interaction, with no host-desktop fallback.
- PD-002 [AC-002]: D02/D06 choose canonical repository-root `.regression-suite/` for ignored runtime, shared across feature worktrees, while portable assets remain tracked.
- PD-003 [AC-003]: D03 chooses guided `setup-regression-suite-mac`, preserving user control of sensitive setup and resource choices.
- PD-004 [AC-004]: D04/D05 choose one shared executable case library, macOS execution first and a separate later Windows runner.
- PD-005 [AC-005]: D04 chooses the platform-neutral `regression-test` authoring role for growing feature coverage.
- PD-006 [AC-006]: D05 chooses requested-feature or full-registered-suite execution through `regression-suite-mac`; C02 keeps coverage claims bounded.
- PD-007 [AC-007]: D05 chooses independent worker verification rather than implementation-author signoff.
- PD-008 [AC-011]: D07 assigns evidence review and suspected-bug validation to the main development agent, not the test worker.

## Traceability

| Nano criterion | Source basis | Supporting requirement |
| --- | --- | --- |
| AC-001 | D01; dedicated headless testing intention | VR-001 |
| AC-002 | D02, D06 | VR-002, VR-003 |
| AC-003 | D03, C04; guided provisioning/readiness questions | VR-004, VR-005 |
| AC-004 | D04, D05, C01 | VR-006 |
| AC-005 | D04, C02; shared executable case intention | VR-007 |
| AC-006 | D05, C02, D07 | VR-008 |
| AC-007 | D05; independent worker relationship and reference contract | VR-009 |
| AC-008 | D07; candidate-contract question and identity risk | VR-010 |
| AC-009 | D07; invalid-evidence and negative-control risks | VR-011, VR-012 |
| AC-010 | D07; setup verification/restoration/reset question | VR-013 |
| AC-011 | D07; main-agent validation relationship | VR-014 |
| AC-012 | D06, C03; product-permission boundary | VR-015 |

## Open Questions

No unresolved product-direction choice is required to state the nano intent.
The following engineering, setup and later-platform questions remain open;
they prevent an operational-readiness claim, not the publication of these
requirements for human approval.

| Owner / later work | Question | Relevant authority |
| --- | --- | --- |
| Driver adaptation and bounded proof | Which guest-native Tauri interaction/observation path executes the shared cases, and what evidence establishes that it works without host input? | AC-001, AC-004 |
| Candidate contract | Which artifact is executed, how is any test-copy transformation qualified, and how are original/copy/instance identities verified? | AC-008 |
| Guided setup and human operator | What current resource/licensing budget and dedicated provisioning/reset arrangement preserve Notch's existing VMs? | AC-003, AC-010 |
| Setup verification | How are required guest permissions, unlocked/awake behavior, reset and restoration established? No overnight duration is selected, and bounded evidence does not prove overnight endurance. | AC-003, AC-009, AC-010 |
| Initial coverage reconciliation | Which implemented behaviors are initially registered, and which current native observations and historical #28/#33 gaps remain outside that coverage? | AC-005, AC-006 |
| Later Windows workflow | Which Windows setup/native runner executes the same cases and supplies independent Windows results? | AC-004 |
