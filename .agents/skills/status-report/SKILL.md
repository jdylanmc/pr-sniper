---
name: status-report
description: "Human-directed snapshot; Joe-mode may invoke after a full cycle or confirmed major-feature merge only. Report objective progress, elapsed time, own tool calls, and running descendants read-only, with explicit visibility limits."
disable-model-invocation: false
user-invocable: true
---

# Status Report

**Entry:** a human request or Joe-mode's full-cycle/major-feature-merge event,
under the [invocation contract](../setup/INVOCATION.md). No other automatic
invocation. Preserve the original [intent](intent.md).

Produce one read-only snapshot. Do not change tickets, files, assignments,
agents, or task state, or advance the work. The caller may display it and resume
its workflow; the reporter never starts a loop. Preserve relevant
[doctrine context](../doctrine/APPLY.md) without adding recording or
source-loading ceremony merely to report status.

## Capture the objective and cutoff

Before inspecting activity, identify the current objective, responsible agent,
and recorded start from actual task context; set a snapshot cutoff with time
zone. Joe-mode supplies its controller ID, anchored objective, known descendant
ownership, start evidence, cycle/event identity, and evidence pointers. Report
that objective, not the reporter worker's short lifetime.

Use existing runtime/session records. Do not install hooks, resurrect an
archived recorder, invent event schemas, or reset the objective clock. A new
cycle, merge, report, or context compaction is not a new objective.

The start requires an actual task-bound timestamp. Use session creation time
only with evidence the objective began then. Label an unestablished start or
cutoff unavailable. A known later observation may yield a labeled lower bound,
never an invented exact duration.

## Inspect once, within scope

Read existing progress records and minimum live evidence to distinguish
complete, remaining, blocked, and in-progress work. A worker's completion
message does not prove its PR merged. Resolve ticket number **and title**
together; explicitly mark unavailable titles.

Count the objective-owning agent's tool calls from actual objective-scoped
events through the cutoff. Exclude descendants, unrelated same-session work,
and this snapshot's post-cutoff inspection. Never substitute token totals,
turn counts, session-wide tool totals, or guesses. For missing events, return
**unavailable**, or **at least N, partial coverage** with the known range.

Inspect the known objective-owned agent tree, including nested descendants.
Follow parent/ownership relationships, not matching repository names. Count
distinct running agents and give each one's current assignment. Exclude
idle/completed agents and unrelated siblings or repository agents. Use a
complete bounded tree query when available; otherwise follow known IDs and
state unseen descendants or runtime visibility limits. Do not repeatedly poll,
wait for completion, or message workers.

Bind observations to the cutoff where runtime history permits. Otherwise give
the live observation window and label the snapshot non-atomic. Unknown is not
zero; a visible subset is not a complete fleet count.

## Return a compact snapshot

Use the human's vocabulary; expand unfamiliar acronyms on first use. Start with
the objective in no more than three sentences, then short completed/remaining
bullets. Include:

- **Snapshot / elapsed:** timestamp and objective duration, or explicit limits.
- **Completed / remaining:** concrete outcomes, blockers, and pending decisions.
- **Tool calls:** this agent's objective-only count and coverage.
- **Running descendants:** total or partial/unavailable status, with each known
  running agent and its assignment.

Every ticket mention, including worker assignments, includes number and title.
Qualify by provider/repository when needed; do not chain bare IDs. Group common
assignment context for a readable long fleet, retaining each known running
agent's identity and task. If evidence cannot reconstruct progress, say so
rather than inventing plausible history.

Return the snapshot to the human/caller. Joe-mode owns event deduplication and
continuation; reporting neither mutates its board nor resets its objective.
