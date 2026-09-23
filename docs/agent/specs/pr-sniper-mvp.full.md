# PR Sniper MVP: Supporting Requirements

- Spec ID: SPEC-PR-SNIPER-MVP
- Source: [`../discovery/pr-sniper-mvp.md`](../discovery/pr-sniper-mvp.md)
- Source revision: 8
- Source SHA-256: `b0386c81299d36e44fa9efc0ab6938272b8aa9e1ed4872d291cb6ea236a459da`
- Nano authority: [PR Sniper MVP](./pr-sniper-mvp.nano.md)

## Authority

The nano sibling is the settled product authority. This document explains and traces that intent; it does not override it.

The human-approved [Settings A - Sidebar follow-up](../design/settings-default.md)
anchors the MVP's ordinary Settings experience. It refines presentation and
local configuration workflows without changing stored reviewer eligibility,
per-field inheritance, existing cron support, or independent automation gates.
Its screenshots contain synthetic design data, not implementation evidence.

## Problem and Users

The primary user is a human reviewer whose pull-request workload spans repositories and teammates. Teammate authors receive machine-authored feedback through GitHub. PR Sniper detects eligible work, invokes the configured local agent, publishes validated comments, follows its own unresolved conversations, and prepares the final human handoff.

## Outcomes and Success

The MVP succeeds when every nano acceptance criterion is demonstrated in a reproducible macOS application flow. Quantitative adoption, latency, defect-detection, and cost targets remain unknown and are not invented here.

## Scope and Non-goals

Scope includes a Tauri menu-bar application, local settings and state, concurrent first-party GitHub App device-flow accounts, explicit repository-to-account binding, scheduled GitHub polling, watched-author and reviewer-assignment triggers, GitHub Copilot CLI review, comment publication, follow-up replies, queueing, notifications, and Setup Doctor. GitHub CLI remains only optional development or migration context. The domain and secure-storage contract uses provider-neutral stable identifiers so another provider can implement it later; only GitHub is implemented in the MVP. The nano non-goals exclude Azure DevOps implementation, automatic resolution of ambiguous repository-account migration, Windows release, embedded human diff review, leaderboards, hosted team state, hosted authentication brokerage, production updater feeds, and provider approval submission.

## Constraints and Dependencies

The application runs monitoring only while its tray process is active, with opt-in launch at login. GitHub and the selected local agent remain external dependencies with independent authentication and limits. The registered GitHub App uses public client ID `Iv23li1HXvoQVkSzV2l5`, device flow, no client secret or private key, and rotating credentials in account-addressed native secure storage. Each repository configuration names one provider and stable provider-account identity. When multiple accounts can access the same repository, the user explicitly chooses the binding; account removal leaves affected repositories visibly unbound or disabled rather than transferring them. Pull-request content is untrusted. The configured agent runs with the current user's access, so PR Sniper does not claim sandbox isolation and must constrain the review workflow without implying a stronger operating-system boundary.

## Confirmed Facts

- Both Electron and Tauri passed the disposable macOS tray, hidden-window, timer, notification, subprocess, cancellation, and packaging probe; Dylan selected Tauri.
- Read-only GitHub polling retrieved identity, pull requests, changed files with pagination, review state, reviewer assignments, and revision SHA; normal repository APIs have materially more rate-limit capacity than global search.
- A real GitHub Copilot CLI review produced normalized JSON and independently identified a defect also found by a human reviewer and official Go documentation.
- GitHub CLI credentials act as the signed-in human, so an `APPROVE` event cannot honestly represent a distinct automation identity.
- The PR Sniper GitHub App is registered as App ID `5048251`, with device flow enabled, webhooks disabled, and permissions for checks read, contents write, metadata read, pull requests write, and statuses read. Registration and installation are external prerequisites, not application-owned mutations.
- GitHub App device flow supports a distributed desktop client without an embedded client secret. User access tokens rotate with refresh tokens, so persistence must replace the pair together and serialize competing refresh attempts.
- GitHub pending reviews support batching inline comments before one visible review submission.
- GitHub numeric user IDs provide the stable account component for the MVP; mutable login names are display metadata, not credential or repository-binding keys.

## Assumptions

- GitHub's current pending-review and comment APIs remain available for the MVP.
- The selected agent adapter can enforce a read-only review tool policy; an untrusted pull request is blocked when the adapter cannot enforce it.
- macOS secure storage and application-signing behavior can support local credential handling without placing tokens in ordinary files.

## Contradictions

None remain in the aligned source. Earlier plans for automated provider approval were removed from the MVP because GitHub CLI would attribute approval to the signed-in human.

## Alternatives and Examples

Electron remains a fallback only if later Windows or process-control evidence exposes material Tauri friction. Hosted webhooks are deferred because scheduled polling satisfies the MVP. A reviewer-assignment trigger may select an author outside the trusted watchlist; that pull request remains eligible but requires confirmation before agent execution. A head change before visible submission discards the pending review; a head change after submission records a stale-after-publication state, stops further comments, and queues the new revision only if it remains eligible. Eligibility loss stops without requeue.

## Product Requirements

- PR-001 [AC-001]: The application shall use Tauri, present as a macOS menu-bar utility without a persistent main window, expose status, queue, settings, Setup Doctor, and quit actions, and support opt-in launch at login.
- PR-002 [AC-001]: Closing settings or queue windows shall not stop monitoring; quitting the tray process shall stop monitoring.
- PR-003 [AC-002]: Repository settings shall support add, edit, disable, re-enable, and remove without a hard-coded product maximum, while surfacing provider and local-resource limits.
- PR-004 [AC-003]: Global defaults and per-repository overrides shall cover fixed interval or five-field cron schedule with explicit time zone, watched GitHub identities, reviewer-assignment trigger, agent adapter, model or named-agent selector, prompt, automatic agent start, and automatic comment publication.
- PR-005 [AC-003]: Settings shall show each effective automation gate and reject unsupported combinations before saving.
- PR-006 [AC-004]: The scheduler shall run only while PR Sniper is active, prevent concurrent checks for the same repository, tolerate sleep and missed intervals, and record last attempt, last success, next run, and last failure.
- PR-007 [AC-005]: The GitHub provider shall poll configured repositories incrementally and filter by stable author identity or requested-reviewer identity before checkout or agent invocation.
- PR-008 [AC-006]: A review-job identity shall include provider, repository, pull request, head revision, and trigger-policy identity; successful work shall not run or publish twice, and a new head revision shall be independently eligible.
- PR-009 [AC-006]: Persisted jobs shall recover after restart into an honest queued, interrupted, waiting, completed, stale-after-publication, or failed state.
- PR-010 [AC-007]: The MVP shall connect through the registered PR Sniper GitHub App's OAuth device flow without requiring GitHub CLI, honor provider poll intervals and `slow_down`, and expose pending, denied, expired, cancelled, network/provider-failed and connected outcomes honestly.
- PR-011 [AC-007]: Credential providers shall remain separate from pull-request domain operations; any GitHub CLI path shall be labeled development or migration context and shall not be represented as product sign-in.
- PR-012 [AC-008]: Setup Doctor shall distinguish missing executable, broken executable or shim, signed-out state, wrong identity, missing permission, and ready state.
- PR-013 [AC-008]: Setup Doctor shall show each declared command, purpose, source, expected effects, and possible administrator prompt before explicit confirmation, execute only structured declared commands, stream redacted output, rerun health checks, and make repeated successful setup a no-op.
- PR-014 [AC-009]: The MVP shall provide a GitHub Copilot CLI adapter and a registry that probes candidate path, identity, version, health, and capabilities before presenting an adapter as available.
- PR-015 [AC-009]: Agent authentication shall remain owned by the agent tool, and the adapter contract shall not change provider or monitoring domain models when another local agent is added.
- PR-016 [AC-010]: The normalized result shall contain a one-sentence synopsis, every changed file in a validated order, findings with path, location, severity, title, explanation, and confidence, a machine decision, session and usage metadata, or explicit failure.
- PR-017 [AC-010]: The agent shall receive the complete changed-file set and sufficient repository context for pull requests containing hundreds of files; the interface may collapse or virtualize but shall not omit files.
- PR-018 [AC-010]: The queue shall preserve validated agent order and use bytewise ascending changed-file path order as the deterministic fallback.
- PR-019 [AC-011]: Agent execution shall be cancellable, time-bounded, observable, and separate from the validated provider-publication operation; invalid or partial output shall fail schema validation and never publish.
- PR-020 [AC-011]: The review workflow shall expose pull-request metadata, diffs, repository reads, and search, and shall not execute repository code, builds, tests, hooks, or installation commands.
- PR-021 [AC-011]: Fork pull requests and authors outside the configured trusted watchlist shall require confirmation while enforceable tool restrictions remain active; when an adapter cannot enforce those restrictions, the untrusted review shall be blocked and confirmation shall not override the block.
- PR-022 [AC-003]: Immediately before agent invocation, PR Sniper shall revalidate head revision, open and non-draft lifecycle state, trigger eligibility, repository enabled state, trust confirmation, and the effective agent-start gate; disabled automatic start requires explicit human start.
- PR-023 [AC-012]: Inline findings shall bind to the reviewed head revision and valid diff locations; unmappable findings shall remain visible locally rather than being dropped or posted incorrectly.
- PR-024 [AC-012]: Inline findings shall be assembled as a pending GitHub review and become visible through one `COMMENT` submission only after final revalidation.
- PR-025 [AC-012]: Before and after every provider mutation, PR Sniper shall revalidate head revision, open and non-draft lifecycle state, trigger eligibility, repository enabled state, trust confirmation, and the effective publication gate.
- PR-026 [AC-012]: A head change before visible submission shall discard the pending review and queue the new revision only if it is currently eligible; eligibility loss shall stop publication without requeueing ineligible work.
- PR-027 [AC-012]: A head change after visible submission shall stop further publication, record stale-after-publication, and queue the new revision only if currently eligible; eligibility loss without a new eligible head shall stop without requeue.
- PR-028 [AC-012]: Provider failures shall preserve pending output and mutation identity, reconcile remote state, and permit idempotent retry without claiming unconfirmed success.
- PR-029 [AC-013]: PR Sniper shall monitor unresolved threads created by its own reviews and create a follow-up job keyed by owned-thread ID, triggering external-comment ID, and reviewed head revision.
- PR-030 [AC-013]: Each follow-up key may publish at most one concise signed reply across polling and restart when the agent produces evidence-backed new information; unchanged answers, acknowledgment-only filler, speculation, and duplicates shall not publish, while a later external comment creates a new key and human-judgment questions move the job to waiting for human input.
- PR-031 [AC-014]: Every machine-authored review summary, reply, and machine sign-off shall end with the exact signature ` PR Sniper`.
- PR-032 [AC-015]: The application shall use `` as its canonical crosshair brand mark and render a legible platform-appropriate menu-bar icon.
- PR-033 [AC-016]: The queue shall distinguish queued, reviewing, waiting for author, waiting for human input, machine-signed-off, stale-after-publication, and failed states; machine-signed-off and human-input-required items shall sort ahead of routine states.
- PR-034 [AC-016]: Native notifications shall cover confirmation required, human input required, ready for human review, and failure, deduplicate repeated state transitions, and open the exact queue item or GitHub pull request.
- PR-035 [AC-017]: Machine-sign-off handoff shall state that automated review completed, request final human review, and never represent machine sign-off as human approval.
- PR-036 [AC-017]: The MVP shall never submit a provider `APPROVE` event.
- PR-037 [AC-018]: Local persistence shall contain repository configuration, schedules, poll cursors, jobs, state transitions, pending-review identities, publication receipts, retry budgets, and notification deduplication records.
- PR-038 [AC-018]: Provider/account-scoped access and refresh credentials shall use separate account-addressed operating-system secure-storage records plus a non-secret account registry; prompts, child-process arguments, ordinary configuration, logs and UI payloads shall not contain provider secrets.
- PR-039 [AC-018]: Structured redacted logs shall expose provider, repository, agent, scheduler, persistence, and publication health from Settings.
- PR-040 [AC-019]: Automatic retry shall apply only to provider rate limits, transient network or transport failures, provider server failures, transient checkout fetch or input/output failures, and agent launch, timeout, or temporary-exit failures before any provider mutation.
- PR-041 [AC-019]: Authentication, permission, configuration, cancellation, schema-validation, stale-revision, ineligibility, and explicit-provider-rejection failures shall require correction or human action and shall not consume automatic retries.
- PR-042 [AC-019]: Each persisted job-operation identity shall receive one budget whose initial attempt starts a 15-minute deadline and permits at most three retries; provider timing takes precedence and otherwise exponential backoff with jitter applies.
- PR-043 [AC-019]: Restart shall preserve attempt count and deadline; an expired deadline shall enter visible manual-retry state without another automatic attempt, and explicit manual retry shall create a new budget.
- PR-044 [AC-019]: Every externally visible operation shall retain repository, pull request, head revision, trigger policy, operation type, attempted mutation, pending-review identity, owned-thread ID, triggering external-comment ID when applicable, confirmed receipt, retry count, and retry deadline for reconciliation.
- PR-045 [AC-018]: Competing refreshes for one provider/account credential shall serialize against the latest stored pair; successful rotation shall replace access and refresh credentials together, while persistence failure shall retain the previously confirmed pair and report failure.
- PR-046 [AC-007, AC-018]: Settings shall distinguish disconnected, connecting, connected stable identity and reconnect-required states without implying repository access or enabling review execution, publication, notifications or merging.
- PR-047 [AC-002, AC-018]: Each repository configuration shall bind to exactly one provider ID and stable provider-account ID. When multiple retained accounts can access the same provider repository, creation or rebind shall require an explicit account choice rather than inferred priority, last sign-in, or mutable login.
- PR-048 [AC-002, AC-007]: The MVP shall retain multiple GitHub accounts concurrently in one provider-neutral account registry keyed by provider ID and stable provider-account ID; adding or refreshing one account shall not replace another account.
- PR-049 [AC-002, AC-003, AC-018]: Persisted repository configuration and every repository-scoped job, confirmation, mutation attempt, and receipt shall carry its provider and stable provider-account binding. Settings and queue surfaces shall show the acting login and stable account context before provider work or publication.
- PR-050 [AC-018]: Credential storage, refresh coordination, reconnect state, and removal shall be isolated by provider ID plus stable provider-account ID; work for different accounts shall not share an account refresh lock or overwrite another account's pair.
- PR-051 [AC-002, AC-018]: Removing an account shall remove only that account's credentials and registry entry, clear its active selection if applicable, retain every other account, and leave repositories bound to the removed account visibly blocked until an explicit rebind. PR Sniper shall not silently select another account.
- PR-052 [AC-018]: Migration from the combined active-account secure record and the earlier active-marker-plus-credential layout shall write and confirm the account-addressed credential and registry before deleting legacy records. Every interruption shall preserve at least one confirmed credential copy and converge safely on retry.
- PR-053 [AC-002, AC-018]: Migration of repository records that predate explicit account binding shall assign an account only when the binding is unambiguous. Multiple plausible accounts require a visible user choice; implementing that repository-schema migration is separate from the secure-account foundation.
- PR-054 [AC-007, AC-018]: `ProviderId` and `ProviderAccountId` shall be stable provider-neutral concepts. GitHub supplies the only MVP implementation; Azure DevOps may implement the same contract later but shall not appear connected, selectable, or functional before its provider and Microsoft Entra authentication exist.

## Product Decisions

- PD-001 [AC-001]: Tauri is the selected desktop stack; Electron is a fallback only if later cross-platform evidence establishes a blocker.
- PD-002 [AC-001]: macOS is the first release platform, monitoring runs in the tray process, and launch at login is opt-in.
- PD-003 [AC-007]: GitHub is the first provider and the registered PR Sniper GitHub App's OAuth device flow is the product sign-in. GitHub CLI is optional development or migration context only.
- PD-004 [AC-009]: GitHub Copilot CLI is the first review agent behind a pluggable local-agent contract.
- PD-005 [AC-011]: The current-user agent is trusted and not described as sandboxed, while pull-request content remains untrusted input and the product constrains the review workflow.
- PD-006 [AC-012]: Initial-review publication uses one pending-review `COMMENT` submission after revalidation; the only other GitHub mutation is an idempotent reply to a PR Sniper-owned thread under PR-025 and PR-028–PR-030, and the MVP has no `APPROVE` path.
- PD-007 [AC-013]: PR Sniper follows only unresolved threads it created and replies only with concise meaningful additions.
- PD-008 [AC-014]: The canonical machine signature is ` PR Sniper`.
- PD-009 [AC-015]: The canonical application mark is ``.
- PD-010 [AC-017]: Final review remains human-owned; automated provider approval is deferred until a distinct automation identity exists, with future policy modeled as `off`, `manual`, or `automatic`.
- PD-011 [AC-019]: Each persisted job-operation retry budget starts with the initial attempt, permits three retries within 15 minutes, persists across restart, and resets only after explicit manual retry.
- PD-012 [AC-002, AC-007, AC-018]: Concurrent account retention and repository binding use provider ID plus stable provider-account ID. GitHub numeric user ID is the MVP account key; login is visible mutable metadata.
- PD-013 [AC-002, AC-018]: Overlapping repository access never establishes precedence implicitly. The selected binding is explicit, and account removal or credential failure never transfers work to another account.
- PD-014 [AC-007]: Provider-neutral account and storage identifiers are an extension contract, not evidence of Azure DevOps support in the MVP.

## Traceability

| Nano authority | Supporting requirements | Discovery basis |
| --- | --- | --- |
| AC-001 | PR-001–PR-002, PD-001–PD-002 | Tray POCs and Tauri selection |
| AC-002 | PR-003, PR-047–PR-049, PR-051, PR-053, PD-012–PD-013 | Repository and concurrent-account product context |
| AC-003 | PR-004–PR-005, PR-022, PR-049 | Monitoring, settings, acting-identity, and pre-invocation gate decisions |
| AC-004 | PR-006 | Scheduled polling decision |
| AC-005 | PR-007 | GitHub filtering POC |
| AC-006 | PR-008–PR-009 | Head-SHA deduplication evidence |
| AC-007 | PR-010–PR-011, PR-046, PR-048, PR-054, PD-003, PD-012, PD-014 | GitHub App registration, device flow, and provider extension contract |
| AC-008 | PR-012–PR-013 | Setup Doctor decision |
| AC-009 | PR-014–PR-015, PD-004 | Local agent inventory and Copilot POC |
| AC-010 | PR-016–PR-018 | Real agent-review POC and complete-file decision |
| AC-011 | PR-019–PR-021, PD-005 | Cancellation evidence and trust decision |
| AC-012 | PR-023–PR-028, PD-006 | GitHub review API evidence and race decision |
| AC-013 | PR-029–PR-030, PD-007 | Conversation-loop decision |
| AC-014 | PR-031, PD-008 | Signature decision |
| AC-015 | PR-032, PD-009 | Icon decision |
| AC-016 | PR-033–PR-034 | Queue and notification issues |
| AC-017 | PR-035–PR-036, PD-010 | Human-handoff and approval-identity decisions |
| AC-018 | PR-037–PR-039, PR-045–PR-054, PD-012–PD-014 | Account-addressed secure storage, migration, binding, and rotating-token evidence |
| AC-019 | PR-040–PR-044, PD-011 | Provider and agent failure evidence |

## Open Questions

None block the MVP product requirements. Database choice, GitHub API client and REST/GraphQL split, checkout strategy, and internal concurrency remain implementation design questions rather than product decisions. Repository-schema migration must preserve an unambiguous existing binding or ask the user; this specification does not authorize guessing among multiple retained accounts.
