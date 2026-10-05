# PR Sniper vNext Experience

- Spec ID: SPEC-PR-SNIPER-VNEXT
- Source: docs/agent/discovery/pr-sniper-vnext.md
- Source revision: 6092326a4f7718384942bf6d72a573a780b3c7a6417cab46fa1a0b8e14460950
- Full specification: [Supporting requirements](./pr-sniper-vnext.full.md)

## Intention

Give PR Sniper users one compact tray experience that discovers work on a global
schedule, coordinates independent Agent reviews, and lets a repository's primary
Agent provide a final review before separately permitted approval or merge.
This amendment governs vNext Experience #51; unchanged MVP requirements and
separately approved platform/distribution work remain in force.

## October 5, 2026 consent amendment

Dylan explicitly authorized removing per-revision trust approval throughout the
app. Saving an Agent assignment and confirming the repository's monitoring
scope authorize ongoing read-only reviews under the saved start policy,
including forks, unwatched/reviewer-requested authors, later revisions, retries,
owned-thread replies, mentions and primary final reviews. No additional
revision-trust checkbox or trust flag gates execution or provider actions.
Existing confirmed setups do not require reapproval on upgrade.

This supersedes the trust-confirmation clauses of MVP AC-011, PR-021/022/025,
the "without establishing trust" clauses of AC-005/PR-056, and vNext references
to execution/revision trust gates. Explicit manual-start preferences,
cancellation/retry behavior, account/model selection, read-only tool enforcement,
scope activation, current-revision validation, independent Comment/Approve/Merge
permissions, provider policy, human-closed concerns and uncertain-write recovery
remain effective. PR content remains untrusted input, not executable instructions.

## Acceptance Criteria

- AC-001: Queue, Running, Reviewed and Settings remain inside one compact tray-anchored experience with the approved navy/orange/teal, system-sans visual language, supplied header artwork and crosshair identity. Closing hides the panel without stopping background work; explicit Quit stops it. Back navigation, exact notification destinations, drafts, keyboard focus and reduced-motion behavior remain usable.
- AC-002: One user-configurable global five-field cron schedule scans enabled repositories, defaults to `*/15 * * * *`, and has an expression-builder helper. The saved schedule and explicit time-zone semantics persist; this MVP exposes no per-repository or per-Agent polling schedule and adds no special upgrade-confirmation workflow.
- AC-003: Each global scan reconciles current repository assignments against the latest PR iteration. A newly assigned Agent receives a normal review pass on that iteration at the next scan; an Agent that already completed it does not repeat unchanged work. New revisions and reopening are reviewable iterations; retries, targeted replies and the primary's final review remain distinguishable from normal passes.
- AC-004: Once admitted, a PR remains tracked despite removal of its watchlist match or reviewer request. Verified closure or merge stops its work; absence from a poll, repository disablement, account loss or completion of an Agent pass is not proof of either and does not authorize deletion. Reopening requires review as a new iteration even at the same commit. Repository disablement, account loss, trust and action permissions remain effective gates.
- AC-005: A repository with exactly one assigned Agent treats it as primary automatically. With multiple assigned Agents, at most one is explicitly primary. Without a primary, normal review/comment work remains available but automatic approval and merge are unavailable. Primary designation is repository-scoped and does not enable either permission.
- AC-006: Replies in PR Sniper-owned review threads route only to their owning Agent. Top-level comments mentioning the repository's acting GitHub account route to its primary automatically, not to every Agent or the separate AI identity. These triggers retain execution/trust gates, meaningful-response filtering and duplicate/loop prevention; unrelated conversation is not an automatic reply trigger.
- AC-007: Earlier owned feedback and review iterations remain available while a PR is open, including after its current Agent passes finish. Author commits or replies can prompt reassessment of open concerns, but Agents must not resurrect the same concern after a human closes it, including after reopening. Published feedback, local-only findings and concerns awaiting human judgment remain distinct.
- AC-008: Users configure Approve and Merge independently from comment publication on repository assignments. Merge is available only to the primary. Auto-approval is the main outcome; merge is optional. No permission, primary selection or provider choice grants a policy bypass or silently changes another permission.
- AC-009: Before either automatic approval or merge, the primary performs a final full re-review with awareness of the other assigned Agents, after their current passes and outstanding concerns are clear. A final review may serve both enabled actions only while its revision and relevant review, discussion, assignment and permission state remain valid; new findings or invalidated evidence prevent action.
- AC-010: Draft PRs cannot be approved or merged. Provider capabilities and actual repository policies govern operations, with no invented reviewer quota or fixed provider-specific approve/merge split. Approval supplies the acting account's vote and need not wait for every other required approval; merge additionally requires an open, non-draft, merge-ready PR, green CI, satisfied policies and no unresolved blocking concerns. Azure DevOps Waiting on author blocks automatic merge when that provider is implemented.
- AC-011: Every cleared, still-open PR receives an explicit personal-review handoff in the human Queue, including PRs with recorded automatic approval; provider-confirmed closed or merged PRs leave actionable views and proceed to automatic cleanup rather than a terminal archive. Human acknowledgment is not an extra gate on an explicitly permitted merge. Queue, job details, Reviewed and notifications distinguish machine clearance, final review, confirmed approval/merge, author-wait, human input, stale evidence and failures. A destination whose detail has been cleaned up explains its unavailability without selecting a different PR. Multiple Agents sharing an account are not independent provider votes, and automated actions never imply personal review. Read-only evidence and the complete file guide link to the provider, not an embedded human review editor.
- AC-012: Full reviews, the primary's final review and targeted reply work share one machine-wide AI capacity limit, default four, independently of saved Agent/repository counts. New jobs join the tail; available capacity starts the oldest eligible work without waiting for another poll. Pause/resume, limit changes, cancellation and retry preserve truthful state, completed work and provider receipts.
- AC-013: Settings and Genie use the same durable account, Agent, doctrine and repository resources with resource-scoped saves. Preserve explicit account/model selection, independent repository and AI identities, zero/one/many doctrines, edited libraries, unrelated drafts and safe references. Planned jobs show planned configuration; running/completed evidence retains its actual configuration rather than reconstructing it from later edits.
- AC-014: Genie is available during onboarding and from Settings afterward. Completed shared-resource saves survive closing the flow; unsaved fields are handled explicitly. Final activation shows effective identities, assignments/permissions, scope, the global schedule and capacity and requires confirmation before monitoring starts. Fresh setup never fabricates accounts, repositories or PR work.
- AC-015: Reviewed contains results for still-open PRs, including completed passes and prior iterations, ordered newest meaningful activity first with stable incremental access to older results and no fixed count cap. Unchanged scans do not reshuffle results or duplicate pages. Outcomes remain distinct without inferring external human review. There is no permanent completed-PR archive, History & storage screen, byte/purge control or restore-history feature.
- AC-016: Existing account isolation, secure storage, explicit scope activation, read-only Agent execution, current-revision validation, signed machine output, bounded retries and uncertain-mutation reconciliation survive this amendment. Settings, active PR state and required safety receipts persist across scans, restarts and upgrades without reset or silent permission expansion. Unavailable or rejected capabilities and cleanup failures remain visible; no automated provider action is reported as confirmed without its receipt.
- AC-017: After provider-confirmed closure or merge, bulky local PR detail is cleaned up automatically without a manual purge or confirmation step, but only after running work and uncertain provider writes are safely settled. A PR that reopens before cleanup completes remains protected as active work. Minimal ownership, duplicate-prevention, operation and human-closed-concern receipts survive so cleanup cannot duplicate external actions or suppress a new review iteration. Cross-file failure, interruption or restart leaves recoverable consistent state and does not report cleanup success prematurely. Settings, accounts, Agents and doctrines are untouched. Published discussion remains on the provider; unpublished local terminal detail may be discarded and has no restore guarantee.

## Non-goals

- New repository or AI providers, an Azure DevOps implementation, a competing Windows port, a new database/policy engine, an embedded human review editor, fabricated progress, or changes to independent distribution work.
- Per-repository/Agent polling controls in this MVP, a fixed peer-approval count, impersonating independent reviewers, bypassing branch policies, or treating prototype state as native/provider acceptance.
- A new permission grant to the development agents editing this repository. Specification approval, implementation dispatch and live provider actions remain separate.
- A permanent terminal archive, manual history-retention management, zero-disk-usage promise, or prescribed one-file-per-PR storage layout.
