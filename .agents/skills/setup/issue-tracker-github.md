# Issue tracker: GitHub

Issues and specs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --comments`, filtering comments by `jq` and also fetching labels.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Confirm the host and owner/repository from setup, using `git remote -v` as evidence rather than authority. Pass the configured repository explicitly (`--repo [HOST/]OWNER/REPO` on issue/PR commands); a code remote does not authorize another repository or organization-wide operations.

## Joe-mode backlog selection

Resolve the configured GitHub host, then query its authenticated user with `gh api --hostname "$HOST" user --jq .login`. Record the anchored owner/repo and optional project/milestone/label selection. For assigned-to-me, add that verified login as the assignee filter; never use Git author identity as a substitute.

Use `gh issue list` with explicit repository, open state, mapped readiness label, and selected filters, or the corresponding paginated API. The CLI's default limit is not the full backlog: obtain complete pages or report a partial selection. Hydrate requirements, comments, dependencies, and associated PRs only for relevant candidates. Preserve unrelated labels on updates; follow configured claim conventions rather than reassigning someone's work.

Reserve one spec graph or intentionally separate child deliveries, never both. Inspect linked PRs before starting another Ship. An unmerged dependency is not available on another delivery's base merely because its checks pass.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

When set to `yes`, PRs run through the same labels and states as issues, using the `gh pr` equivalents:

- **Read a PR**: `gh pr view <number> --comments` and `gh pr diff <number>` for the diff.
- **List external PRs for triage**: `gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments` then keep only `authorAssociation` of `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, or `NONE` (drop `OWNER`/`MEMBER`/`COLLABORATOR`).
- **Comment / label / close**: `gh pr comment`, `gh pr edit --add-label`/`--remove-label`, `gh pr close`.

GitHub shares one number space across issues and PRs, so a bare `#42` may be either: resolve with `gh pr view 42` and fall back to `gh issue view 42`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue only within the calling workflow's approved scope. Publish the complete specification or ticket, preserving source/revision and acceptance references. If the complete artifact exceeds provider limits, obtain approval for an accessible document/attachment and link it; never truncate or silently write a repository file. Mark only ready work with the configured readiness label. Reconcile uncertain publication before retrying.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.

## Legacy discovery maps

Older discovery maps use the layout below. Current `/discovery` reads these as evidence; it does not automatically create a map, claim or resolve tickets, or build dependency graphs. Experimental questions route to `/poc`, including items with the old `discovery:prototype` label. Do not migrate existing labels automatically.

Any discovery-tracker maintenance using these commands requires an exact proposed change and explicit approval outside the read-only cycle. Delivery ticketing belongs to `/breakdown-tickets`, not discovery.

- **Map**: a single issue labelled `discovery:map`, holding the Notes / Decisions-so-far / Fog body. `gh issue create --label discovery:map`.
- **Child ticket**: an issue linked to the map as a GitHub sub-issue (`gh api` on the sub-issues endpoint). Where sub-issues aren't enabled, add the child to a task list in the map body and put `Part of #<map>` at the top of the child body. Labels: `discovery:<type>` (`research`/`prototype`/`interrogate`/`task`). Once claimed, the ticket is assigned to the driving dev.
- **Blocking**: GitHub's **native issue dependencies**, the canonical, UI-visible representation. Add an edge with `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where `<blocker-db-id>` is the blocker's numeric **database id** (`gh api repos/<owner>/<repo>/issues/<n> --jq .id`, _not_ the `#number` or `node_id`). GitHub reports `issue_dependencies_summary.blocked_by` (open blockers only, the live gate). Where dependencies aren't available, fall back to a `Blocked by: #<n>, #<n>` line at the top of the child body. A ticket is unblocked when every blocker is closed.
- **Frontier query**: list the map's open children (`gh issue list --state open`, scoped to the map's sub-issues / task list), drop any with an open blocker (`issue_dependencies_summary.blocked_by > 0`, or an open issue in the `Blocked by` line) or an assignee; first in map order wins.
- **Legacy claim**: an assignee marks a claimed ticket. Changing it with `gh issue edit <n> --add-assignee @me` requires approval; it is not a discovery intake step.
- **Resolve**: `gh issue comment <n> --body "<answer>"`, then `gh issue close <n>`, then append a context pointer (gist + link) to the map's Decisions-so-far.
