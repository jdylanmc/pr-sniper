# Issue tracker: Local Markdown

Issues and specs for this repo live as Markdown files at the paths confirmed in setup. The defaults below use `.scratch/`; substitute the agreed paths consistently and preserve existing human files.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The spec is `.scratch/<feature-slug>/spec.md`
- Implementation issues are one file per ticket at `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01`, never a single combined tickets file
- Triage state is recorded as a `Status:` line near the top of each issue file (see `triage-labels.md` for the role strings)
- Comments and conversation history append to the bottom of the file under a `## Comments` heading

## When a skill says "publish to the issue tracker"

Write the complete artifact at its configured path only after the calling workflow's write approval (creating the directory if approved). Do not overwrite an existing spec or ticket without approval. Preserve source/revision and acceptance references; use the mapped readiness value only for ready work.

Local publication is a repository write, not implicit permission to create a PR. A separately authorized PR requires `worktrees` and the delivery workflow's workspace procedure. For authorized writes use the library's `/changelog` helper for this repository/component: curated notable `Unreleased` entries under Keep a Changelog 1.1.0, or report no entry needed. Do not write during proposal/review, create versions/releases, dump commits, or add a recursive entry for changelog-only edits.

## When a skill says "fetch the relevant ticket"

Read the file at the referenced path. The user will normally pass the path or the issue number directly.

## Legacy discovery maps

Older discovery maps use the layout below. Current `/discovery` reads these files as evidence; it does not automatically create, claim, or resolve tickets or build dependency graphs. Experimental questions route to `/poc`, including items with the old `prototype` type. Do not migrate existing types automatically.

Local tracker files are repository writes. Any discovery-tracker maintenance requires an exact proposed change and explicit approval outside the read-only cycle. Delivery ticketing belongs to `/breakdown-tickets`, not discovery.

- **Map**: `.scratch/<effort>/map.md` (the Notes / Decisions-so-far / Fog body).
- **Child ticket**: `.scratch/<effort>/issues/NN-<slug>.md`, numbered from `01`, with the question in the body. A `Type:` line records the ticket type (`research`/`prototype`/`interrogate`/`task`); a `Status:` line records `claimed`/`resolved`.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it lists is `resolved`.
- **Frontier**: scan `.scratch/<effort>/issues/` for files that are open, unblocked, and unclaimed; first by number wins.
- **Legacy claim**: `Status: claimed` records ownership. Changing it requires approval; it is not a discovery intake step.
- **Resolve**: append the answer under an `## Answer` heading, set `Status: resolved`, then append a context pointer (gist + link) to the map's Decisions-so-far in `map.md`.
