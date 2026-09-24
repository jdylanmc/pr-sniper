# PR Sniper MVP

- Spec ID: SPEC-PR-SNIPER-MVP
- Source: [`../discovery/pr-sniper-mvp.md`](../discovery/pr-sniper-mvp.md)
- Source revision: 8
- Source SHA-256: `b0386c81299d36e44fa9efc0ab6938272b8aa9e1ed4872d291cb6ea236a459da`
- Full specification: [Supporting requirements](./pr-sniper-mvp.full.md)
- Approved Settings UX: [A - Sidebar default](../design/settings-default.md)
  (2026-09-22 follow-up; requirements, not implementation evidence)

## Intention

PR Sniper is a macOS-first menu-bar utility that watches configured GitHub repositories for selected teammates' pull requests or pull requests requesting the signed-in user as a reviewer, runs a configured local review agent against each eligible revision, publishes clearly machine-authored review comments, and puts machine-cleared pull requests in front of the human for final review.

## Acceptance Criteria

- AC-001: PR Sniper installs as a macOS application, runs without a persistent main window, exposes its primary actions from the menu bar, and can be enabled as an opt-in login item.
- AC-002: The user can concurrently retain multiple provider accounts and explicitly select, edit, disable, re-enable, and remove any repository accessible to the chosen account without an application-defined maximum; each repository is bound to one provider, stable provider-account identity and stable provider-repository identity, overlapping access requires an explicit account choice, and provider, organization, project, tenant or local-resource limits are surfaced explicitly. GitHub implements this model in the MVP; Azure DevOps retains the same contract without a live implementation.
- AC-003: The user can configure global defaults and per-repository overrides for schedule, watched authors, reviewer-assignment trigger, local agent, model or agent selector, prompt, automatic agent start, and automatic comment publication; Settings, queue work, confirmations, and publication state show the acting account.
- AC-004: Scheduled monitoring runs while PR Sniper is active, supports fixed intervals and five-field cron schedules in an explicit time zone, prevents overlapping checks, and records last attempt, last success, next run, and failures.
- AC-005: Monitoring filters pull requests by stable GitHub author identity and reviewer assignment before scheduling expensive review work.
- AC-006: PR Sniper processes each eligible pull-request head revision at most once per trigger policy, survives restart without duplicate publication, and can re-review a new head revision.
- AC-007: The user connects one or more accounts through the PR Sniper GitHub OAuth App using GitHub's secretless device authorization flow in the default system browser, public client ID `Ov23lidoL3QovWyfxnA4`, no client secret, and the broad `repo` scope required for arbitrary private repositories. PR Sniper shows the provider-issued one-time user code, keeps the secret device code only in transient native memory, follows provider polling and expiry rules, makes the consent tradeoff explicit, validates `/user`, and requires explicit identity confirmation before persisting account-addressed credentials; browser-open, cancellation, denial, expiry, disabled-registration, missing/revoked scope, organization policy, provider, network, wrong-identity and secure-storage failures remain explicit. Device flow needs no callback; #36 remains required work outside this authentication path before claiming the complete MVP.
- AC-008: Settings includes a Setup Doctor that detects and health-checks prerequisites, previews exact install or sign-in commands, obtains confirmation, executes controlled terminal setup, verifies identity and capabilities, redacts secrets, and is safe to rerun.
- AC-009: The MVP includes a GitHub Copilot CLI review-agent adapter and an adapter contract that supports later local agents without changing the monitoring or provider domain model.
- AC-010: An agent review produces a validated normalized result containing a one-sentence synopsis, an ordered guide covering every changed file, structured findings, and either machine sign-off or human-input-required state.
- AC-011: Agent execution is cancellable, bounded, observable, and separate from publication; the trusted current-user agent is not represented as sandboxed, the review workflow does not execute repository code, builds, tests, hooks, or installation commands, forks or authors outside the trusted watchlist require confirmation, and untrusted work is blocked when the adapter cannot enforce those restrictions.
- AC-012: PR Sniper publishes comments only after the configured automatic or human-confirmed gate, submits inline comments as one pending-review batch, and revalidates revision and eligibility before and after every provider mutation; a changed head requeues only when the new revision is eligible, while eligibility loss stops publication without requeue.
- AC-013: PR Sniper monitors unresolved review threads it created and posts a concise reply only when the agent has meaningful new information; each owned-thread, triggering-external-comment, and reviewed-head key receives at most one reply across polling and restart, filler and duplicates are suppressed, and human judgment stops automation.
- AC-014: Every machine-authored review summary, reply, and machine sign-off ends with the canonical signature ` PR Sniper`.
- AC-015: The application uses `` as its canonical crosshair icon and brand mark, including a legible platform-appropriate menu-bar icon.
- AC-016: A machine-cleared or human-input-required pull request is sorted ahead of routine queue states and produces a native notification that opens that exact queue item or GitHub pull request.
- AC-017: The human handoff explicitly states that automated review completed and requests final human review; machine sign-off is never represented as human approval.
- AC-018: Settings, account registry, repository-account bindings, cursors, jobs, and idempotency state persist locally; provider/account-scoped access and refresh credentials use account-addressed operating-system secure storage, rotate as one serialized pair independently per stable provider account, and never appear in ordinary configuration, logs, prompts, child-process arguments or UI payloads. Removing or losing an account never silently transfers its repositories to another account; explicit rebind or reconnect is required, and unusable or unconfirmed credentials never receive a success-shaped fallback.
- AC-019: Authentication, permission, polling, rate-limit, checkout, agent, schema, and publication failures remain visible without duplicate comments; each retryable job operation gets at most three retries within 15 minutes from its initial attempt, preserves its count and deadline across restart, and then requires manual retry.

## Non-goals

- Azure DevOps implementation (including Microsoft Entra sign-in), automatic assignment of an ambiguous pre-binding repository to one of multiple accounts, Windows release, embedded diff review, team leaderboard, hosted team service, hosted authentication brokerage, production update feeds, provider `APPROVE` submission, and replacement of the human's final review responsibility are outside the MVP. The provider-neutral identifiers and binding contract intentionally reserve Azure DevOps without claiming that provider works.
