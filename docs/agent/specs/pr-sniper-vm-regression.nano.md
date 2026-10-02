# PR Sniper Non-disruptive Regression Testing

- Spec ID: SPEC-PR-SNIPER-VM-REGRESSION
- Source: docs/agent/discovery/pr-sniper-vm-regression.md
- Source revision: 9c2f652a2bee53207581ff7f51d384e48318aab1941f387076eb8457a4242268
- Full specification: [Supporting requirements](./pr-sniper-vm-regression.full.md)

## Intention

Let development agents verify new PR Sniper features and rerun accumulated
regressions against the actual application without commandeering the user's
desktop. Establish macOS execution in a dedicated headless Tart VM first, with
shared executable cases that a later Windows runner can execute independently.

## Acceptance Criteria

- AC-001: Routine application UI actions and output observation occur inside PR Sniper's dedicated headless macOS VM against the actual compiled Tauri application and its production frontend. They do not focus, click, type or open a test viewer on the host desktop. Missing guest capability never falls back to host UI automation.
- AC-002: The macOS regression workflows discover their private environment at `.regression-suite/` beneath the canonical PR Sniper repository root, shared by that repository's feature worktrees. VM disks, credentials and generated run evidence remain Git-ignored; portable skills, executable tests, registry and reconstruction guidance remain tracked outside that ignored runtime.
- AC-003: `setup-regression-suite-mac` guides the user through absent or unusable setup and reports verified readiness separately from a regression result. Passwords, OS permissions and resource/security choices remain user-controlled. It does not silently provision, delete, replace or take over existing VMs, including Notch's environment; required resource/licensing choices are resolved before provisioning.
- AC-004: One shared library defines executable feature cases with common behavior and assertions, separate from platform-specific setup and native interaction. A later Windows runner must execute those same cases without reauthoring their feature expectations. This delivery provides the macOS environment only and does not claim Windows execution from a macOS result.
- AC-005: `regression-test` adds or updates executable feature cases and their registration in Git, with stated preconditions, actions, expected observable behavior and cleanup. Coverage grows with implemented features; notes, mocks or successful command dispatch alone are not an installed-app regression test.
- AC-006: `regression-suite-mac` runs the requested features or the full registered suite and identifies exactly which cases were requested, executed and not exercised. Missing coverage or unsupported interaction is explicit, not silently omitted. A full registered suite is never presented as proof that every application feature is covered.
- AC-007: Verification uses a fresh independent test worker, with the selected candidate, test revision, scope and expected behavior identified before execution. Its report establishes the worker's separate assignment; the implementing agent's own run is not independent signoff, and the verifier does not change the candidate or expectations to obtain a pass.
- AC-008: Every run binds its observations to the exact source/build candidate, installed test artifact, signature qualification, running application instance and isolated test data/credential namespace. If a test copy is transformed or re-signed, original and transformed identities are recorded separately; it is not described as the unchanged release.
- AC-009: Reports preserve per-case PASS, FAIL and BLOCKED with actual executed counts and supporting observations. Wrong observable behavior produces FAIL; unavailable capability or stale, invalid or missing evidence cannot produce PASS. Expected negative controls remain distinguishable from application regressions, and earlier failures are not erased by successful retries.
- AC-010: Runs preserve declared starting state or report an unsuccessful restoration, and a fresh run can establish its declared baseline independently of previous mutations. Guest ownership prevents overlapping workers from changing the same test state; uncertain termination or cleanup remains visible and prevents an unsupported successful or clean-reset claim.
- AC-011: Results return candidate-bound evidence, selected coverage, limitations, cleanup status and potential bugs to the main development agent. That agent verifies suspected bugs before ad-hoc triage or return to the owning feature-delivery flow; setup failures, harness defects and deliberate controls are not automatically application bugs.
- AC-012: Existing unit, Rust integration, browser and release-policy checks remain complementary obligations. Regression infrastructure preserves product-Agent execution, trust, account and provider-action boundaries and does not incidentally access real provider credentials, migrate personal profiles, enable the user's login item, run paid inference or publish reviews, approvals or merges.

## Non-goals

- Implementing a Windows environment or runner in this macOS-first delivery, duplicating platform-specific feature case libraries, or claiming unexecuted Windows compatibility.
- Host-desktop computer-use fallback, reuse or deletion of Notch's VMs without explicit ownership decisions, or automatic OS/privacy consent.
- Comprehensive retrospective feature coverage, a replacement for existing checks, an overnight-reliability claim from bounded evidence, or automatic closure of #28/#33.
- Selecting a driver framework, test language, VM sizing or image/toolchain versions in this specification; changing product behavior, permissions, distribution or merge authority.
