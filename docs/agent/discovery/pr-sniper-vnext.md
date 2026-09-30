# Discovery Foundation

- Schema: 2
- Subject: https://github.com/jdylanmc/pr-sniper/issues/51
- Slug: pr-sniper-vnext
- Alignment: confirmed
- Aligned Findings Digest: 7e279fd9db02545fbb1ca71349dfd1f3a74ae3839217e7bd0a9e3058946ab643
- Domain Model Basis Digest: 7e279fd9db02545fbb1ca71349dfd1f3a74ae3839217e7bd0a9e3058946ab643
- Domain Model Digest: 76318a77a3430e7915477d915d0f6f031317c988a882179d0f216577915b9b41
- Frontier Basis Digest: 76318a77a3430e7915477d915d0f6f031317c988a882179d0f216577915b9b41
- Frontier Digest: 36b2a11b7c341897ae4756b330e171e7d80398d406980345c49cdbc640af44fd

## Confirmed Facts

- Read #51 and its three screen catalogues, the decision-bearing #41/#71-77 issues, and the historical discovery, Settings and specification documents; visually inspected the approved popover and continuous Agent queue captures.
- Current GitHub main was 00eaa6a3608719c329f25447eddc1481c4e96856 at inspection. Its monitoring configuration still creates assignment-level schedules, and its specification still excludes provider APPROVE submission.
- Dylan verified the consolidated vNext discovery findings on 2026-09-30. The decisions below are product direction, not shipped behavior.
- No tracker updates, production implementation, provider approvals or merges were performed by this discovery.

## Evidence References

- https://github.com/jdylanmc/pr-sniper/issues/51 - vNext Experience; current epic supersedes its historical two-lane prototype brief.
- https://github.com/jdylanmc/pr-sniper/issues/51#issuecomment-5913477478 - approved screen catalogue, part 1.
- https://github.com/jdylanmc/pr-sniper/issues/51#issuecomment-5913478209 - approved screen catalogue, part 2.
- https://github.com/jdylanmc/pr-sniper/issues/51#issuecomment-5913478837 - approved screen catalogue, part 3.
- https://github.com/jdylanmc/pr-sniper/issues/41 - Onboarding Genie over shared account and configuration editors; historical reusable-entry requirement.
- https://github.com/jdylanmc/pr-sniper/issues/71 - tracked PR lifecycle, revision passes and scheduling decisions.
- https://github.com/jdylanmc/pr-sniper/issues/72 - shared capacity and pause/resume.
- https://github.com/jdylanmc/pr-sniper/issues/73 - owned feedback and targeted replies.
- https://github.com/jdylanmc/pr-sniper/issues/74 - whole-cycle human handoff and optional provider approval.
- https://github.com/jdylanmc/pr-sniper/issues/75 - shared resource saves and captured Agent configuration.
- https://github.com/jdylanmc/pr-sniper/issues/76 - paged history, storage accounting and protected purge.
- https://github.com/jdylanmc/pr-sniper/issues/77 - visual shell and supplied artwork.
- https://github.com/user-attachments/assets/f1d3cd7b-d967-424c-826d-854e5a011eed - visually inspected approved popover capture.
- https://github.com/user-attachments/assets/b7e2d330-696d-4d21-beef-e7d019e71fe2 - visually inspected continuous Agent-job queue capture.
- docs/agent/discovery/pr-sniper-mvp.md - historical manually read revision-9 foundation; preserved unchanged.
- docs/agent/design/settings-default.md - historical sidebar Settings design, not the approved vNext layout.
- https://github.com/jdylanmc/pr-sniper/blob/00eaa6a3608719c329f25447eddc1481c4e96856/docs/agent/specs/pr-sniper-mvp.nano.md - inspected current specification, including provider approval exclusion.
- https://github.com/jdylanmc/pr-sniper/blob/00eaa6a3608719c329f25447eddc1481c4e96856/src-tauri/src/monitoring.rs - inspected assignment schedule construction and activation admission.
- https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews - GitHub self-approval and required-review behavior.
- https://learn.microsoft.com/en-us/azure/devops/repos/git/branch-policies?view=azure-devops - configurable requestor votes and review policies.
- Dylan McCurry, direct discovery answers and consolidated verified alignment on 2026-09-30 - authority for D01-D12; these are product decisions rather than provider execution grants.

## Decisions

- D01 - Dylan: One global cron polling schedule with an expression builder, default */15 * * * *. Preserve saved choices across upgrades; no special pre-alpha migration flow. Leave room for later per-repository/Agent schedules without exposing them now.
- D02 - Dylan: Each scan reconciles assigned Agents against the latest PR iteration. Newly assigned Agents review it; Agents that already reviewed it skip unchanged work. Capacity, not polling frequency, drains admitted jobs.
- D03 - Dylan: Admission stays active despite later watchlist/reviewer-request removal. Confirmed closure stops work. Reopening counts as a new review iteration, even at the same commit.
- D04 - Dylan: One assigned Agent is automatically primary. With multiple Agents, at most one is explicitly primary; without one, reviews/comments work but automatic approval/merge do not.
- D05 - Dylan: Owned-thread replies go to the owning Agent. Top-level @mentions of the repository's signed-in GitHub user go to the primary automatically. The separate AI identity is not the target.
- D06 - Dylan: Approve and Merge are separate opt-in permissions. Merge is available only to the primary; selecting a primary does not enable either permission. Auto-approval is the main outcome; merge is optional.
- D07 - Dylan: Before either automatic approval or merge, the primary performs a final full re-review after the other Agents have reviewed and outstanding concerns are addressed. Reuse that final pass only while the revision and relevant state remain unchanged.
- D08 - Dylan: Draft PRs cannot be approved or merged. Merge additionally requires green CI, satisfied repository policies and no unresolved blocking human/Agent concerns; Azure DevOps Waiting on author blocks it. No invented reviewer quota or policy bypass. Approval contributes the acting account's vote; it need not wait for every other required approval.
- D09 - Dylan: Humans close out discussions. Do not resurrect a human-closed concern. Author commits/replies can prompt reassessment of open concerns. Multiple Agents sharing an account do not create multiple independent approvals.
- D10 - Dylan: Repository policy and actual provider capability govern the available operations; do not hardcode a GitHub-merges/Azure-DevOps-approves split or create a separate fixed reviewer-count policy.
- D11 - Dylan: Keep the supplied AI-generated header artwork and crosshair identity. Genie remains available from Settings after onboarding and reuses the shared editors.
- D12 - Dylan: Retain the approved compact navy/orange/teal tray popover, human-only Queue, continuous Agent-job queue, Reviewed history and shared Settings/Genie editors. Existing auth, revision safety, shared capacity, truthful status, read-only review evidence and human GitHub actions carry forward.

## Constraints

- These decisions intentionally supersede conflicting historical MVP or epic directions, but do not rewrite those sources or assert the changes are implemented.
- No new providers or production implementation are authorized by this discovery.
- No tracker update was requested or approved. Exact tracker changes require a separate approval.
- Save a separate vNext discovery foundation; preserve the historical MVP discovery document.
- Evidence from mock screens is design evidence only. Native behavior, provider mutations, race handling and persistence require implementation verification.
- Agent awareness of the other assigned Agents is required for the primary's final approval/merge pass.

## Assumptions

_None recorded._

## Contradictions

- The current implementation has assignment-level timers and #71 proposes repository-level polling. D01 selects one global cron schedule for the present MVP instead.
- The current MVP specification forbids provider approval and #51 excludes automatic merge. D06-D08 establish independent opt-in approval and primary-only merge, requiring explicit downstream specification reconciliation.
- #73 leaves top-level comment targeting unresolved. D04-D05 supply the primary-Agent route for mentions of the repository account rather than guessing from prose.
- #41's newer onboarding-first mock does not expose its older reusable wizard entry. D11 confirms post-onboarding Settings entry.
- Reusing a completed head-SHA pass on reopen would conflict with D03. Reopening is a new review iteration even when the commit is unchanged.
- The example that a GitHub author can approve their own PR conflicts with GitHub's documented restriction. Reflect provider capability honestly rather than displaying a fabricated approval.

## Open Questions

- Specification workflow: reconcile the approved vNext decisions with current MVP documents and affected issue text; no tracker write is authorized yet.
- Implementation and verification owners: establish provider-capability and required-policy observations, final-review invalidation/race handling, cancellation, persistence and native UI behavior. No acceptance claim is supplied by this discovery.

## Source Claims

- #51 and its catalogues record approval of the final POC on 2026-09-29 and the screenshot-backed backlog on 2026-09-30. The 85 captures are synthetic design evidence, not native/provider acceptance. Only two captures were visually inspected in this discovery.
- Dylan states that AI made the supplied header artwork and explicitly selected keeping it. This is not an independent rights audit.
- GitHub documentation states that pull-request authors cannot approve their own pull requests, even when they can merge them.
- Azure DevOps documentation makes whether a requestor's vote counts toward minimum required reviewers configurable through branch policy.

## Relationship Claims

- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D01"},{"reference":"D02"}],"notes":["One global cron schedule is exposed now; per-repository or per-Agent schedules are future scope."],"relationship":"scans for eligible work and reconciles assignments against the latest PR iteration","source":"Global polling schedule","target":"Enabled repositories"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D02"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/72"}],"notes":["Preserve the approved continuous job queue rather than duplicating it in the human Queue."],"relationship":"drains eligible work independently of polling frequency","source":"Shared AI capacity","target":"Admitted Agent jobs"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D04"}],"notes":["Selecting a primary does not enable approval or merge."],"relationship":"designate one primary automatically for a sole assignment or explicitly when multiple Agents are assigned","source":"Repository assignments","target":"Primary Agent"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D05"}],"notes":["Top-level account mentions follow the separate primary-Agent route."],"relationship":"routes follow-up work to the owner","source":"Owned-thread reply","target":"Owning Agent"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D05"}],"notes":["The target is the repository's signed-in GitHub user, not the separate AI identity."],"relationship":"automatically routes a response assessment","source":"Top-level mention of repository account","target":"Primary Agent"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D06"},{"reference":"D07"}],"notes":["Approve and Merge remain independent opt-in permissions; a final pass is reusable only while relevant state remains unchanged."],"relationship":"performs the final full re-review after other Agents and outstanding concerns are clear","source":"Primary Agent","target":"Automatic approval or merge"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D09"}],"notes":["Author commits or replies can prompt reassessment of still-open concerns."],"relationship":"must not be resurrected","source":"Human-closed concern","target":"Later Agent review"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D11"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/41"}],"notes":["No separate wizard-owned account or configuration store."],"relationship":"reuses editors during onboarding and through later Settings entry","source":"Genie","target":"Shared Settings resources"}

## Boundary Claims

- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D03"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/51"}],"notes":["Sticky tracking does not bypass account, repository or permission boundaries."],"relationship":"does not grant trust or action permission","source":"PR admission","target":"Execution and provider mutations"}
- JSON: {"confidence":"confirmed","direction":"bidirectional","evidence":[{"reference":"D06"}],"notes":["Approval is the primary outcome. Merge is optional and available only to the primary Agent."],"relationship":"is independent of","source":"Approve permission","target":"Merge permission"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D08"},{"reference":"D10"}],"notes":["One account's approval is not multiple peer votes; approval can contribute before other required reviewers approve."],"relationship":"constrain actual operations without a hardcoded reviewer quota or bypass","source":"Repository provider policies","target":"Permitted approval and merge"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D08"}],"notes":["Ready for review means non-draft, not a special label."],"relationship":"blocks","source":"Draft PR","target":"Approval and merge"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D08"}],"notes":["This provider-neutral product direction does not authorize or claim an Azure DevOps implementation."],"relationship":"blocks","source":"Azure DevOps Waiting on author","target":"Automatic merge"}
- JSON: {"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D12"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/51"}],"notes":["Do not equate automated review, a provider approval, human review and merge."],"relationship":"supports personal review without asserting that it occurred","source":"Reviewed evidence","target":"Human action on the provider"}

## Risks

- Treating multiple configured Agents as independent provider reviewers would misrepresent one account's vote and repository review requirements.
- Reusing a final review after relevant revision, discussion or gate state changes could permit an action unsupported by current evidence.
- Confusing human-closed concerns with newly open feedback could resurrect dismissed concerns or erase unresolved ones.
- Carrying prototype behavior directly into production could introduce fictional identities, status, permissions, storage accounting or setup data.
- Retaining the supplied AI-generated artwork reflects the operator's choice, not independently established licensing or provenance.

## Scope

- PR Sniper #51 vNext Experience: preserve the approved visual direction and settle its outstanding polling, Agent participation, primary-role, conversation, approval/merge, artwork and Genie product decisions.
- Human-aligned discovery foundation and continuation only; ready to inform specification and later approved tracker reconciliation.

## Exclusions

- Production source changes, specification edits, tracker mutations, ticket creation, implementation dispatch, commits, pushes, approvals, merges and releases.
- New provider integration, a competing Windows port, new credentials or changes to repository branch policies.
- A redesign of the approved visual language, a second settings store, a fixed reviewer quota or a new general policy engine.
- A special pre-alpha upgrade migration workflow or production use of prototype fixtures and simulation controls.

## Domain Model

- JSON: {"actors":[{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D01"},{"reference":"D04"},{"reference":"D06"}],"kind":"actor","name":"Human operator","notes":["Configures schedules, assignments and independent permissions."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D09"}],"kind":"actor","name":"PR author and human reviewers","notes":["Address and close review discussions."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D02"},{"reference":"D05"}],"kind":"actor","name":"Assigned Agent","notes":["Reviews PR iterations and handles replies to its owned threads."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D04"},{"reference":"D05"},{"reference":"D07"}],"kind":"actor","name":"Primary Agent","notes":["Handles account mentions and the final full review before enabled provider actions."]}],"boundaries":[{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D03"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/51"}],"notes":["Sticky tracking does not bypass account, repository or permission boundaries."],"relationship":"does not grant trust or action permission","source":"PR admission","target":"Execution and provider mutations"},{"confidence":"confirmed","direction":"bidirectional","evidence":[{"reference":"D06"}],"notes":["Approval is the primary outcome. Merge is optional and available only to the primary Agent."],"relationship":"is independent of","source":"Approve permission","target":"Merge permission"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D08"},{"reference":"D10"}],"notes":["One account's approval is not multiple peer votes; approval can contribute before other required reviewers approve."],"relationship":"constrain actual operations without a hardcoded reviewer quota or bypass","source":"Repository provider policies","target":"Permitted approval and merge"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D08"}],"notes":["Ready for review means non-draft, not a special label."],"relationship":"blocks","source":"Draft PR","target":"Approval and merge"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D08"}],"notes":["This provider-neutral product direction does not authorize or claim an Azure DevOps implementation."],"relationship":"blocks","source":"Azure DevOps Waiting on author","target":"Automatic merge"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D12"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/51"}],"notes":["Do not equate automated review, a provider approval, human review and merge."],"relationship":"supports personal review without asserting that it occurred","source":"Reviewed evidence","target":"Human action on the provider"}],"concepts":[{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D01"},{"reference":"D02"}],"kind":"concept","name":"Global polling schedule","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D03"}],"kind":"concept","name":"Tracked PR","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D02"},{"reference":"D03"}],"kind":"concept","name":"Review iteration","notes":["Reopening creates a new iteration even at the same commit."]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D02"},{"reference":"D05"}],"kind":"concept","name":"Agent job","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D02"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/72"}],"kind":"concept","name":"Shared AI capacity","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D06"},{"reference":"D08"}],"kind":"concept","name":"Approve permission","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D06"},{"reference":"D08"}],"kind":"concept","name":"Merge permission","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D07"}],"kind":"concept","name":"Final full re-review","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D11"},{"reference":"D12"}],"kind":"concept","name":"Shared Settings resources","notes":[]}],"confidence":"confirmed","events":[{"aliases":[],"confidence":"confirmed","emittedBy":"Global polling schedule","evidence":[{"reference":"D01"},{"reference":"D02"}],"kind":"event","name":"Global scan","notes":[]},{"aliases":[],"confidence":"confirmed","emittedBy":"PR conversation participant","evidence":[{"reference":"D05"},{"reference":"D09"}],"kind":"event","name":"Owned-thread reply","notes":[]},{"aliases":[],"confidence":"confirmed","emittedBy":"PR conversation participant","evidence":[{"reference":"D05"}],"kind":"event","name":"Top-level account mention","notes":[]},{"aliases":[],"confidence":"confirmed","emittedBy":"Repository provider","evidence":[{"reference":"D03"}],"kind":"event","name":"PR reopened","notes":[]},{"aliases":[],"confidence":"confirmed","emittedBy":"Primary Agent","evidence":[{"reference":"D07"}],"kind":"event","name":"Final review completed","notes":[]}],"relationships":[{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D01"},{"reference":"D02"}],"notes":["One global cron schedule is exposed now; per-repository or per-Agent schedules are future scope."],"relationship":"scans for eligible work and reconciles assignments against the latest PR iteration","source":"Global polling schedule","target":"Enabled repositories"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D02"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/72"}],"notes":["Preserve the approved continuous job queue rather than duplicating it in the human Queue."],"relationship":"drains eligible work independently of polling frequency","source":"Shared AI capacity","target":"Admitted Agent jobs"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D04"}],"notes":["Selecting a primary does not enable approval or merge."],"relationship":"designate one primary automatically for a sole assignment or explicitly when multiple Agents are assigned","source":"Repository assignments","target":"Primary Agent"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D05"}],"notes":["Top-level account mentions follow the separate primary-Agent route."],"relationship":"routes follow-up work to the owner","source":"Owned-thread reply","target":"Owning Agent"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D05"}],"notes":["The target is the repository's signed-in GitHub user, not the separate AI identity."],"relationship":"automatically routes a response assessment","source":"Top-level mention of repository account","target":"Primary Agent"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D06"},{"reference":"D07"}],"notes":["Approve and Merge remain independent opt-in permissions; a final pass is reusable only while relevant state remains unchanged."],"relationship":"performs the final full re-review after other Agents and outstanding concerns are clear","source":"Primary Agent","target":"Automatic approval or merge"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D09"}],"notes":["Author commits or replies can prompt reassessment of still-open concerns."],"relationship":"must not be resurrected","source":"Human-closed concern","target":"Later Agent review"},{"confidence":"confirmed","direction":"directed","evidence":[{"reference":"D11"},{"reference":"https://github.com/jdylanmc/pr-sniper/issues/41"}],"notes":["No separate wizard-owned account or configuration store."],"relationship":"reuses editors during onboarding and through later Settings entry","source":"Genie","target":"Shared Settings resources"}],"states":[{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D03"}],"kind":"state","name":"Admitted","notes":[],"transitionsTo":["Closed"]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D03"}],"kind":"state","name":"Closed","notes":[],"transitionsTo":["Reopened"]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D03"}],"kind":"state","name":"Reopened","notes":["Requires review as another iteration."],"transitionsTo":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D08"}],"kind":"state","name":"Draft","notes":["Blocks approval and merge."],"transitionsTo":["Ready for review"]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D09"}],"kind":"state","name":"Human-closed concern","notes":["Must not be resurrected."],"transitionsTo":[]}],"systems":[{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D12"}],"kind":"system","name":"PR Sniper","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D05"},{"reference":"D08"},{"reference":"D10"}],"kind":"system","name":"GitHub","notes":[]},{"aliases":[],"confidence":"confirmed","evidence":[{"reference":"D08"},{"reference":"D10"}],"kind":"system","name":"Azure DevOps","notes":["Provider-neutral product rules only; no new integration is authorized."]}],"terms":[{"aliases":[],"confidence":"confirmed","contested":false,"evidence":[{"reference":"D08"}],"kind":"term","name":"Ready for review","notes":["Means non-draft, not a special label."]},{"aliases":[],"confidence":"confirmed","contested":false,"evidence":[{"reference":"D04"}],"kind":"term","name":"Primary","notes":["Automatic with one assignment; explicit with multiple assignments."]},{"aliases":[],"confidence":"confirmed","contested":false,"evidence":[{"reference":"D06"},{"reference":"D09"},{"reference":"D10"}],"kind":"term","name":"Approval","notes":["One acting account vote, distinct from human review and merge."]}],"unsettledSeams":[{"confidence":"unknown","evidence":[{"reference":"Human-verified remaining downstream work, 2026-09-30"}],"kind":"unsettled-seam","notes":["Downstream specification or implementation evidence; not a new product decision or delivery grant."],"question":"Specification workflow: reconcile the approved vNext decisions with current MVP documents and affected issue text; no tracker write is authorized yet."},{"confidence":"unknown","evidence":[{"reference":"Human-verified remaining downstream work, 2026-09-30"}],"kind":"unsettled-seam","notes":["Downstream specification or implementation evidence; not a new product decision or delivery grant."],"question":"Implementation and verification owners: establish provider-capability and required-policy observations, final-review invalidation/race handling, cancellation, persistence and native UI behavior. No acceptance claim is supplied by this discovery."}]}

## Frontier

- ready | origin: loop | Human verified the product findings; preserve the approved visual design and proceed to specification reconciliation, not implementation dispatch.
- ready | origin: loop | Polling, primary designation, iteration/reopen behavior, mention routing, separate approval/merge permissions, final review, human-closed concerns, artwork and Genie entry have human decisions.
- deferred to specification and implementation | origin: loop | Provider policy observations, final-review race handling, cancellation, persistence and native behavior still require their own evidence; no native or provider acceptance is claimed.
- tracker boundary | origin: loop | No tracker update requested; exact proposed updates need human approval.

## Next Action

Hand the verified vNext foundation to specification reconciliation; then propose exact updates to the affected existing issues for separate approval. Do not implement, dispatch work, mutate the tracker or perform provider actions in this discovery.

## Resolved

_None recorded._

## History

- vnext-human-alignment-2026-09-30 | 2026-09-30T20:57:54.610Z | verified | succeeds none
