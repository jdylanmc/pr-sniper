# PR Sniper vNext Experience - Supporting Requirements

- Spec ID: SPEC-PR-SNIPER-VNEXT
- Source: docs/agent/discovery/pr-sniper-vnext.md
- Source revision: fb869d7c9888691c3d3a7777220f542e82e694875c10bb1170c0336fa7b34cb7
- Nano authority: [PR Sniper vNext Experience](./pr-sniper-vnext.nano.md)

## Authority

The nano sibling is the product authority for this scoped amendment. This
document elaborates it and never overrides it. Merging the specification change
approves the requirements, not their implementation or any provider action.

The [MVP nano](./pr-sniper-mvp.nano.md) and
[supporting requirements](./pr-sniper-mvp.full.md) remain the baseline outside
the changed behavior identified below. Their historical text is preserved.
The existing Windows-parity and macOS/Windows distribution amendments remain
separate and are not undone or expanded here.

### Explicit supersession

| Historical rule | vNext replacement | Preserved boundary |
| --- | --- | --- |
| MVP AC-003/004 and PR-004 schedule clauses only: selectable repository/assignment polling schedules and the separate fixed-interval schedule alternative | AC-002: one global cron schedule and builder, default every 15 minutes | Explicit time zone, validation, scheduler health and saved preferences; non-schedule watched-author/trigger/Agent/model/prompt and start/publication overrides, explicit AI account selection and no implicit defaults remain in force |
| MVP AC-005/006/012, PR-022/025/026/027/055-057: trigger revalidation and head-bound admission | AC-003/004: scans reconcile assignments; admission is sticky; reopen is a new iteration | Scope activation, account binding, enabled state, trust and independent action gates; reviewer requests can admit older/unwatched PRs without blanket trust |
| MVP AC-013, PR-029/030, PD-007: only owned-thread replies | AC-006/007: owner-only thread replies plus primary-routed top-level account mentions | Read-only analysis, meaningful output, signed replies and durable deduplication |
| MVP PR-036, PD-006/010 and provider-approval non-goal; original #51 no-merge exclusion | AC-005/008/009/010: independent permissions, primary final review, capability- and policy-governed actions | No impersonation, policy bypass, inferred human review, unconfirmed success or blind mutation retries |
| MVP AC-017/PR-035 personal-review handoff, in the presence of newly permitted automated merge | AC-011: cleared unmerged PRs still receive the handoff, even after approval; confirmed merged PRs become terminal history | Affirmative human handoff for open work, no inferred personal review, and no extra human-acknowledgment gate on a separately permitted merge |
| Prior sidebar Settings design and whole-Settings draft workflow | AC-001/013/014: approved compact shell and shared resource editors; Genie also available after setup | Stable identities, saved settings, explicit model choice, retained libraries and visible failures |
| Original #51 two Queue lanes | AC-001/011/012: human-only Queue and one continuous Agent-job queue | Exact work identity, complete evidence and distinct human versus machine responsibilities |

This table describes only the linked nano criteria, not an additional authority
or an instruction to rewrite the historical sources.

## Problem and Users

The user needs to monitor multiple repositories and teammates without managing
a separate polling timer per Agent or confusing the machine backlog with human
attention. Several Agents can contribute different review perspectives, but
they do not become separate provider identities merely by having different
names or prompts.

The primary is a repository assignment role. It receives otherwise ambiguous
top-level mentions and provides the final full review before enabled provider
actions. Approval is the principal automation outcome. Optional merge remains
an independently configured action constrained by the repository's real rules.

## Outcomes and Success

Success is the observable behavior in the nano acceptance criteria: one
understandable queue, predictable scan-time reconciliation, explicit primary
responsibility and truthful provider outcomes. No new performance threshold,
review-duration estimate or synthetic completion percentage is asserted.

## Scope and Non-goals

This is the #51 experience amendment, not a replacement for every MVP feature.
Its nano non-goals retain GitHub-first scope, existing platform/distribution
boundaries and the exclusion of a human review editor or new policy engine.

## Constraints and Dependencies

Reuse existing identity, account, review, publication, persistence and native
platform capabilities. The existing issue graph records delivery prerequisites;
this specification neither dispatches that work nor changes its readiness.
Unimplemented provider vocabulary is product context, not integration evidence.

## Confirmed Facts

- The source foundation records human-confirmed decisions D01-D12 on 2026-09-30
  and carries the cited relationship, boundary and frontier evidence.
- Discovery inspected the implementation and specification at `00eaa6a`;
  they still used assignment schedules and the old no-approval specification.
  This is a historical observation, not a claim about every later commit.
- The reconciliation branch starts at `22a8a57`, after the Windows native-host
  follow-up. It changes documentation only and leaves that implementation intact.

### Source Claims

- [Epic #51](https://github.com/jdylanmc/pr-sniper/issues/51) and its three
  catalogues record the approved visual POC and 85 synthetic captures. Discovery
  visually inspected the shell and continuous-job-queue captures, not all 85.
- The operator says the supplied header illustration was AI-generated and
  selected keeping it. This is not an independent rights/provenance audit.
- [GitHub documentation](https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews)
  says a PR author cannot approve their own PR. Permission to merge one's own PR
  is not permission to submit a self-approval.
- [Azure DevOps documentation](https://learn.microsoft.com/en-us/azure/devops/repos/git/branch-policies?view=azure-devops)
  makes whether the requestor's vote counts configurable. This does not establish
  a working Azure DevOps adapter in PR Sniper.

## Assumptions

No additional product assumptions are introduced. Provider-specific observations,
native behavior and persistence guarantees require implementation evidence.

## Contradictions

The source explicitly reconciles global versus assignment/repository polling,
the old approval prohibition, top-level reply targeting, Genie re-entry and
same-commit reopen behavior. The supersession table maps these accepted changes
without invalidating unrelated approved work.

Automation may contribute an approval before the repository has collected every
other required vote. Requiring the completed reviewer quorum before submitting
that contributing vote would confuse approval eligibility with merge eligibility.
Neither capability is fabricated when the provider rejects the acting identity.

## Alternatives and Examples

The operator rejected exposing separate repository/Agent polling schedules for
this MVP, a special pre-alpha upgrade interview and a fixed GitHub-merges versus
Azure-DevOps-approves split. The earlier two-lane Queue is superseded by the
approved human-only Queue and continuous Agent work list. These are context for
the corresponding nano criteria, not additional implementation requirements.

## Product Requirements

- VN-001 [AC-001]: Retain persistent Queue, Running, Reviewed and Settings navigation inside the tray panel. Preserve exact destination/back context, explicit Quit, hide-only dismissal, keyboard accessibility and reduced-motion behavior. Keep the selected artwork and crosshair identity; do not ship the mock desktop or scenario controls.
- VN-002 [AC-002]: The global schedule is a five-field cron expression with a builder and `*/15 * * * *` default. Preserve explicit time-zone semantics and the user's saved choice. Repository and Agent screens must not expose competing polling schedules in this MVP; future scoped schedules are not a second scheduler project now.
- VN-003 [AC-003]: A scan discovers work and reconciles relevant assignments against the latest iteration. Adding an Agent does not itself trigger a scan; at the next scan that Agent receives missing work while already-completed normal passes remain complete. A primary final pass is an intentional separate purpose, not an accidental duplicate normal pass or retry.
- VN-004 [AC-004]: Admission persists after a watchlist match or reviewer request disappears. Explicit reviewer assignment may admit an older/unwatched PR while the other scope/trust boundaries remain intact. Confirm remote closure/merge before stopping work. Reopening creates a new review iteration even with an unchanged commit; retaining receipts must not suppress that iteration.
- VN-005 [AC-005]: Exactly one assigned Agent is primary automatically. Multiple assignments require explicit primary selection, with at most one primary; no implicit first-Agent choice. Without a primary, normal review/comment work can continue but automatic approval/merge cannot. Effective authority changes must not be hidden by an old job snapshot.
- VN-006 [AC-006]: A real owned-thread reply identifies its owning Agent. A top-level mention of the repository's acting account selects the primary; the AI account does not supply that identity. Preserve comment/trigger identity, self-output and duplicate suppression, execution/trust gates and the option to remain quiet when there is no meaningful response. A mention is not permission to edit repository code or execute arbitrary commands.
- VN-007 [AC-007]: Preserve earlier feedback and authorship across iterations. Human closure settles that concern and later reviews must not re-raise it. Author replies or commits can prompt reassessment of open concerns. Do not manufacture author-wait from unpublished local findings, or clear unrelated human concerns to manufacture readiness.
- VN-008 [AC-008]: Comment publication, Approve and Merge are separate repository-assignment permissions. Merge is only available to the primary. Designating a primary does not enable Approve or Merge, and old inert approval fields do not silently become effective grants. Do not require both permissions when the user selected only one.
- VN-009 [AC-009]: The primary's final review sees the other assigned Agents' current results and outstanding feedback. It runs after their current passes and concerns clear, before either enabled approval or merge. Reuse the final pass for both only while the reviewed revision and relevant state are unchanged; invalidation/new findings stop action and retain honest evidence.
- VN-010 [AC-010]: Draft blocks both approval and merge. Check actual acting-account capability rather than inferring it from a toggle. Approval may contribute a single account vote while other required reviewers are still pending; all merge requirements, green CI, open/non-draft state and blocking-concern checks apply before merge. Azure DevOps Waiting on author blocks automatic merge if that provider is later implemented. No policy bypass or hardcoded reviewer count is introduced.
- VN-011 [AC-011]: Every cleared unmerged PR receives an explicit personal-review handoff in the human Queue, including after automatic approval. Confirmed merged PRs become terminal history, not actionable Ready-for-you items. This handoff does not add a human-acknowledgment prerequisite to permitted automatic merge. A normal pass, aggregate clearance, primary final review, provider approval, human review and merge remain different facts. Rejected or uncertain actions are not receipts; retain exact PR/file-guide links and never infer personal review from automation.
- VN-012 [AC-012]: One capacity budget covers normal passes, the primary final pass and reply work. Preserve the approved default four, supported positive values, oldest-eligible ordering and completion-driven refill. Intentional pause/limit-reduction cancellation queues fresh attempts without consuming failure retries or erasing completed results; already-started remote mutations still require reconciliation.
- VN-013 [AC-013]: Reuse the existing account and configuration resources, with resource-level saves rather than a wizard-only store. Preserve explicit account/model choice, multiple doctrine references and actual captured prompt/doctrine/account/policy evidence. Shared edits do not rewrite completed history; failed writes retain the last valid state and the user's draft.
- VN-014 [AC-014]: Expose Genie at first setup and in Settings later, using the same editors. Completed saves remain applied when it closes. Final activation reviews effective configuration, including primary/permissions, the one global schedule and capacity, and separately confirms monitoring. Never inject the busy fixture into a fresh configuration.
- VN-015 [AC-015]: Use meaningful review activity for stable newest-first history and incremental older results. Distinguish iterations and provider receipts. Usage and purge scope must be accurately labeled; eligible completed-history purge must preserve active references, required prior feedback and minimal operation/ownership records, without deleting configuration or enabling duplicate external effects.
- VN-016 [AC-016]: Preserve the unchanged MVP security, identity, publication, retry and recovery obligations. The review path stays read-only and is not an OS sandbox claim. Revalidate relevant revision/account/trust/permission/provider state around mutations, reconcile unknown outcomes instead of blindly replacing them, and retain pending publication recovery even when the new normal publication-off UI shows local-only evidence.

## Product Decisions

- PD-001 [AC-002]: The operator selected one global cron schedule, default every 15 minutes, with a builder; future scoped schedules are deferred and saved choices survive upgrades without a special pre-alpha migration flow.
- PD-002 [AC-003]: New assignment work begins on the next scan; completed unchanged Agent work is skipped. Queue capacity drains work independently.
- PD-003 [AC-004]: Admission is sticky; closure stops work and reopening requires a new iteration rather than reuse of an old successful same-commit pass.
- PD-004 [AC-005]: A sole Agent is primary automatically; multiple Agents require explicit selection for automatic approval/merge.
- PD-005 [AC-006]: Owned-thread replies route to the owner; top-level mentions of the repository account route to the primary, not the AI identity.
- PD-006 [AC-008]: Approve and Merge are independent opt-in operations, with Merge restricted to the primary role.
- PD-007 [AC-009]: One valid primary final full review precedes either action and may cover both only while relevant state remains unchanged.
- PD-008 [AC-010]: Non-draft is the ready-for-review signal; repository policies govern real operations, and approval eligibility is distinct from the completed merge-policy quorum.
- PD-009 [AC-007]: Humans close concerns; Agents must not resurrect them. Open concerns can be reassessed after author work.
- PD-010 [AC-010]: Do not hardcode provider-specific approve/merge modes or reviewer counts. Several Agents under one account supply only that account's vote.
- PD-011 [AC-014]: Genie remains available from Settings after onboarding; the same source decision also retains the supplied header art under AC-001.
- PD-012 [AC-001]: Preserve the approved visual direction and carry existing product capabilities forward through the explicit amendment, not through a new implementation stack.

## Traceability

| Nano criterion | Supporting requirement | Source decision/evidence |
| --- | --- | --- |
| AC-001 | VN-001, PD-012 | D11-D12; approved #51 catalogues and inspected shell |
| AC-002 | VN-002, PD-001 | D01; global schedule correction |
| AC-003 | VN-003, PD-002 | D02-D03; scan-time assignment reconciliation |
| AC-004 | VN-004, PD-003 | D03; tracked lifetime and reopen correction |
| AC-005 | VN-005, PD-004 | D04; primary designation |
| AC-006 | VN-006, PD-005 | D05; deterministic owner/mention routing |
| AC-007 | VN-007, PD-009 | D09; human-closed concerns and prior feedback |
| AC-008 | VN-008, PD-006 | D06/D10; independent granular permissions |
| AC-009 | VN-009, PD-007 | D07; final full re-review and freshness |
| AC-010 | VN-010, PD-008/PD-010 | D08/D10; provider policy and capability distinctions |
| AC-011 | VN-011 | D09/D12; read-only handoff and truthful outcomes |
| AC-012 | VN-012 | D02/D07/D12; retained #72 shared-capacity contract |
| AC-013 | VN-013 | D11-D12; retained shared-resource and snapshot contract |
| AC-014 | VN-014, PD-011 | D11; confirmed Settings re-entry and retained activation boundary |
| AC-015 | VN-015 | D03/D12; retained history/purge and iteration boundaries |
| AC-016 | VN-016 | D12; unchanged MVP boundaries and source constraints |

## Open Questions

No new product choice is asserted as settled by implementation assumptions.
The source's remaining work is specification/tracker reconciliation and
implementation evidence: provider observations, final-review race handling,
cancellation, persistence and native acceptance. Those are not evidence of
already working behavior, nor an instruction to introduce a new policy engine.

This pair requires independent review and human merge before it is an approved
specification. Source alignment is not specification approval, and neither
authorizes development agents to approve or merge a live PR.
