# Issue tracker: Azure DevOps

Use the available Azure DevOps integration or an already configured Azure DevOps CLI/REST client. Discover its current tool schema before calling it. Do not use `gh` for Azure DevOps, install tooling automatically, copy credentials into files, or treat missing access as an empty backlog.

## Resolve code, planning, identity, and scope

Recognize HTTPS remotes shaped like `https://dev.azure.com/<organization>/<project>/_git/<repository>`, legacy `<organization>.visualstudio.com` hosts, and SSH remotes shaped like `git@ssh.dev.azure.com:v3/<organization>/<project>/<repository>`. Resolve names to actual project/repository identities through the configured integration. URL-decode path components; do not assume project and repository names match.

Record the code organization/project/repository separately from the planning organization/project. Planning may live elsewhere. Confirm the team, area path, saved-query ID, or explicit work-item selection defining the backlog; a code remote does not authorize querying every project.

For "assigned to me," confirm the integration is authenticated as the intended human. Work Item Query Language (WIQL) uses `[System.AssignedTo] = @Me` for the query caller's identity. If the integration uses a service identity, resolve the human's actual Azure DevOps identity and filter by it instead. Never substitute a GitHub login or Git author email without provider verification.

Discover the project's work-item types, required fields, workflow states, and terminal-state categories. Do not hardcode Agile, Scrum, or CMMI type/state names. Store the configured triage roles in `docs/agents/triage-labels.md`; readiness is normally the mapped value in `System.Tags`, not a new workflow state.

## Read a bounded backlog

1. Use the configured saved query or a scoped flat WIQL query. Preserve all scope clauses; for an assigned-to-me subset, intersect the selected result with the verified assignee. Do not edit a shared saved query merely to add that filter.
2. Retrieve IDs, then hydrate only needed fields in batches. Initial selection needs identity/title, type, state, tags, assigned identity, area, and change information; load full description, acceptance fields, comments, and relations for actual candidates and their relevant dependencies.
3. Match the mapped readiness tag as a tag value after hydration, preserving provider tag semantics rather than accepting an arbitrary substring match. Apply the actual project's terminal-state rules.
4. Follow the endpoint's pagination contract. WIQL has `$top` but no general continuation-token/skip cursor: a limited response is not the full backlog. For a large flat query, page an ID-ordered, bounded selection using a last-seen ID predicate while retaining its filters, deduplicate IDs, and recheck candidates before dispatch. For tree/link saved queries, preserve their result semantics; do not silently replace them with a broad flat query.
5. Report errors, inaccessible IDs, query limits, and incomplete pages explicitly. A configured query returning nothing successfully is different from failing to query it.

The [WIQL REST API](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/wiql/query-by-wiql?view=azure-devops-rest-7.1) is a read operation using `POST` on the planning project's `_apis/wit/wiql` with a JSON `query` body and API version `7.1`. Flat responses expose `workItems`; link/tree responses expose `workItemRelations`. Selecting fields in WIQL does not hydrate their values.

The [work-item batch API](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/get-work-items-batch?view=azure-devops-rest-7.1) uses `POST` on `_apis/wit/workitemsbatch`, API version `7.1`, with at most **200 IDs** per request, selected `fields`, and `$expand: "Relations"` when needed. Reconcile returned IDs with requested ones rather than silently ignoring omissions.

### Relationships and delivery ownership

Read work-item relations, not only prose labels:

- On a child, `System.LinkTypes.Hierarchy-Reverse` points to its parent; `Hierarchy-Forward` on the parent points to children.
- On a blocked item, `System.LinkTypes.Dependency-Reverse` identifies its predecessor; `Dependency-Forward` identifies successors. Resolve actual relation definitions if a project uses another relation type.
- Read existing artifact links and associated PRs before starting another delivery. A parent ready tag does not authorize dispatching both the parent specification and its children independently.

Joe-mode records the chosen delivery group's covered IDs. Use a configured shared claim convention if one exists and is authorized; do not steal the human's assignment as an ownership shortcut. A session-local reservation is not a provider-wide lock.

## Publish or update approved work

When a skill says to publish a spec or ticket, create the configured work-item type in the **planning** project. Resolve required fields from that type's metadata and suitable existing parent/sibling conventions, then obtain missing human decisions. Do not fabricate required product values to satisfy validation.

Preserve the complete specification or ticket and its source/revision and acceptance references. If it exceeds provider limits, obtain approval for an accessible document/attachment and link it from the work item; never truncate or silently create a repository file. An unresolved human-owned requirement is not made ready merely by successfully creating a work item.

Use the configured integration's create/update operations, or the documented [work-item APIs](https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/?view=azure-devops-rest-7.1). REST creation/update uses JSON Patch with `application/json-patch+json`; format description/acceptance fields as the project expects. Do not bypass process rules.

Apply the mapped `ready-for-agent` tag when the calling skill authorizes it. For an existing item, read its current tags, change only the mapped role values, preserve unrelated tags, and use the supported revision check when updating. On a revision conflict, reread and reconcile rather than overwrite. Readiness tags are distinct from `System.State`.

Create approved parents/children and dependency links using actual returned IDs and URLs. Do not close or rewrite the parent as a side effect of publishing children. Include assignee policy in approval when the backlog is scoped to the human, so newly created work does not unintentionally fall outside the selection.

Use the supported comments API for work-item comments; retrieve all relevant comment pages. Record returned item IDs and links. After an uncertain create/update result, query for the actual result before retrying; ambiguous duplicate matches require resolution, not another create.

## Pull requests and shepherding

PR operations use the **code** project and repository, not the planning project. Resolve the target branch and existing source-branch PR before creating anything. Link work items through the provider's supported relation mechanism; do not assume GitHub closing keywords link or complete Azure DevOps items.

Use the configured integration or [Git pull-request APIs](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests?view=azure-devops-rest-7.1):

- Create on the repository's `pullrequests` collection with `sourceRefName` and `targetRefName` as full `refs/heads/...` references, a meaningful title/description, and `isDraft: true`.
- Reuse the existing PR ID for updates and feedback. Mark ready by updating `isDraft` to `false` only after the selected delivery owner meets the [shared delivery contract's](../ship/DELIVERY.md) review and validation requirements. This is not PR completion.
- Read PR `status`: `active` is open, `completed` is merged, and `abandoned` is closed without merging. Inspect `isDraft`, `mergeStatus`, reviewer votes, review threads, PR statuses, and required branch-policy evaluations. A successful merge calculation is not approval or passing checks.
- Read current source/target refs as needed. `lastMergeSourceCommit` and `lastMergeTargetCommit` describe the last merge calculation and may lag live branch heads.
- For policy evaluations use [the policy API](https://learn.microsoft.com/en-us/rest/api/azure/devops/policy/evaluations/list?view=azure-devops-rest-7.1), version `7.1-preview.1`, with the URL-encoded artifact ID `vstfs:///CodeReview/CodeReviewId/{codeProjectId}/{pullRequestId}`. Honor its `$top`/`$skip` pagination; inspect applicable blocking policies, running/failed results, and required review conditions rather than treating an empty list as success.
- Read all relevant threads/comments and status pages through their documented endpoints or tools. Unavailable checks or policies are missing evidence, not green.

Never set `status: completed`, enable `autoCompleteSetBy`, bypass policy, vote approval, or delete the source branch as part of any delivery route or Shepherd. If another actor enabled auto-completion, report it; do not assume human-only merging is still guaranteed. A human decision or missing capability follows Shepherd's existing stop/escalation rules.

## Sources and setup boundaries

[WIQL syntax and identity macros](https://learn.microsoft.com/en-us/azure/devops/boards/queries/wiql-syntax?view=azure-devops), [relation types](https://learn.microsoft.com/en-us/azure/devops/boards/queries/link-type-reference?view=azure-devops), [PR fields](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/get-pull-request?view=azure-devops-rest-7.1), and [policy artifact IDs](https://learn.microsoft.com/en-us/dotnet/api/microsoft.teamfoundation.policy.webapi.policyevaluationrecord.artifactid?view=azure-devops-dotnet) are the provider references. Check the supported API versions for an Azure DevOps Server installation rather than assuming cloud support.

Setup proposes configuration; it does not create work items, reassign a backlog, migrate tags, or change branch policy. Joe-mode auto-transitions preserve the calling skill's existing human decisions and write approvals.
