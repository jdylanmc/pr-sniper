---
name: changelog
description: "Internal helper for every modifying agent. Curate notable human-facing changes using Keep a Changelog; respect write scope and one integration owner. No direct human command or automatic release."
disable-model-invocation: false
user-invocable: false
---

# Changelog

**Entry:** internal to authorized modifying work under the
[invocation contract](../setup/INVOCATION.md). Inherit the caller's scope and
permissions; this helper never authorizes edits or publication independently.
Preserve applicable [doctrine selections](../doctrine/APPLY.md); `documentation`
may inform curation. PR-bound edits inherit the delivery's required `worktrees`.

Every modifying agent uses this helper, including for tests, documentation,
configuration, and skills. Use once per meaningful change; revisit when the
outcome changes, not after every edit or tool call.

## Resolve the record and owner

Read repository changelog/release guidance and the actual scoped diff or
artifacts. Resolve the relevant project, package, component, or skill changelog,
not necessarily the root file. Clarify ambiguous destinations; never create a
second history beside an established one.

Follow [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/):
curate notable changes for people by release and change type, not copied commit
messages. Its [source project](https://github.com/olivierlacan/keep-a-changelog)
owns the convention; this skill owns integration.

For a new authorized file, use `# Changelog`, a short convention statement
and the project's actual versioning policy, then `## [Unreleased]` with a
resolvable link when a meaningful comparison exists (otherwise `## Unreleased`).
Use only applicable `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, and
`Security` subsections. Put newest releases first; use real versions and
`YYYY-MM-DD` dates in release headings. Keep sections linkable; use verified
comparison/tag links when available. Never invent versions, dates, tags, or a
Semantic Versioning policy.

For an incompatible existing format, propose bounded adoption to the owner;
never silently rewrite history. Pending that decision, preserve the file and
return categorized entry candidates. Maintain established release-note
automation input, not a conflicting second source. Keep released entries
unchanged unless a specific correction is authorized.

## Curate and consolidate

Identify the human-visible outcome, affected people, and required action.
Use evidence from the final change, issue, or PR; labels and commit titles are
leads, not proof. Preserve deprecation versus removal, breaking behavior,
migration requirements, and scope limits. Never publish secrets or private
incident details in a public changelog.

Add or revise one concise `Unreleased` entry per notable outcome. Prefer
outcome-level wording across related commits. Check existing entries for
duplicates, contradictions, or intermediate designs misrepresented as shipped
behavior. Include useful traceable references without overwhelming prose.

Whitespace-only changes, temporary probes, unchanged generated output, and
incidental scratch artifacts normally need no entry. Return **no notable entry
needed** with a short reason to the owner; consultation does not require noise.
A changelog-only edit does not create an entry about itself.

Only the authorized owner edits a shared changelog. Isolated workers may update
their own copies only when assigned ownership; otherwise return proposed
category, wording, target, and evidence with the change. The integration owner
consolidates proposals against the integrated diff before final review.
Re-read current content before applying; preserve concurrent human edits.
If writing fails or ownership is unresolved, report the unapplied proposal,
not success.

## Return to delivery

Read back resulting entries and compare with the current change.
Check category, accuracy, destination, links, duplicate outcomes, and breaking
change visibility. Include applied entries in the same reviewed delivery.
Return target, applied/proposed/no-entry status, and evidence basis.

Do not independently commit, push, release, tag, choose versions, or move
`Unreleased` into a release. The delivery owner handles already-authorized
publication; humans retain release and final approval decisions.
