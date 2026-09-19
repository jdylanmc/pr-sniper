---
name: chart-a-course
description: "Human or scoped agent use. Find a critical path through tasks to a named goal, identify missing work and research spikes, and recommend targeted Discovery. Read-only; the caller owns execution."
disable-model-invocation: false
user-invocable: true
---

# Chart a Course

**Entry:** Human or scoped agent use around one bounded goal, including a
human-started Joe-mode caller. Follow the [invocation contract](../setup/INVOCATION.md).
Find the task path, not merely a backlog picture. The human-owned [intent](intent.md)
defines the purpose.

Use [doctrine selection and application](../doctrine/APPLY.md), preserving the
caller's selections. With none, consider `sequencing` for evidence-backed
prerequisites. Do not turn preferred execution order into a dependency.

Read evidence and return a plan in conversation or to the caller. Do not create
issues, edit files or trackers, prioritize unrelated backlog work, dispatch
agents, start Discovery, or execute tasks. Recommendations grant no authority.
Do not restore archived graph tooling or orchestrators.

## 1. Anchor the goal

Establish the named outcome, success evidence, exclusions, and bounded source
scope. An issue, epic, specification, or plainly stated goal suffices; no complete
task graph is required. Ask only for missing decisions that materially change
the goal or scope.

Read available requirements and relevant work records using configured tracker
scope and provider-qualified identities. Batch necessary fields and follow
relevant prerequisite links; do not scan an entire organization. Capture source
references, observation time/revision, lifecycle state, dependency semantics,
estimates and units when available, and any existing discovery findings.
An unavailable or truncated source is a coverage gap, not an empty backlog.

## 2. Work backward through tasks

For each goal condition, identify required task outcomes, then prerequisites.
Reuse existing tasks before proposing missing ones; check relevant linked work
to avoid duplicating differently named issues.
Parent/child containment, list order, and common labels do not establish
finish-before-start dependencies. Do not count an epic's aggregate estimate
again on top of its tasks.

Keep three kinds of evidence distinct:

- **Existing work:** exact source identity, outcome, actual state, prerequisites,
  and evidence that connects it to the goal.
- **Proposed missing tasks:** a local identity such as `proposal:1`, required
  outcome, why it is missing, proposed prerequisites and their rationale.
  A proposal ID is not a tracker ID or an approved requirement.
- **Research spikes:** the unresolved question, the task or goal condition it
  gates, the evidence needed, and the particular issue or epic it concerns.
  Define a learning outcome, not a promised implementation or invented answer.

Mark each dependency confirmed, proposed, or unresolved, with source or rationale.
Propose only goal- and evidence-supported relationships; do not fabricate edges
to connect every node. Retain alternative routes when an unsettled choice changes
required work; never combine mutually exclusive alternatives into one mandatory
chain or silently choose a product direction.

Recommend [Discovery](../discovery/SKILL.md) for a specific issue or epic when
unknown requirements, feasibility, or design choices prevent a credible path.
Supply the question, scope, known evidence, affected tasks, and learning exit
condition. Discovery can select Research or a separately scoped POC.
If no suitable issue exists, propose an unpublished issue and its intended
parent. Describing the inquiry requires no tracker ID.

Stop when evidenced work or explicit gaps cover the bounded goal. Do not invent
speculative tasks to make the map look complete.

## 3. Check the dependency model

Use one explicit direction: `prerequisite -> dependent`. Resolve the goal's
transitive prerequisites. For a conceptual goal, label a local goal milestone
with zero task weight, connected to required success outcomes; never present it
as a tracker record.

Expose duplicate/missing identities, absent endpoints, ambiguous direction,
cycles, uncertain lifecycle state, inaccessible prerequisites, and uncovered
goal conditions. Do not silently merge records, delete edges, infer completion,
or repair cycles. Flag completed dependents with unfinished prerequisites as
inconsistent evidence rather than silently accepting their state or edges.
Unknown prerequisites can change which work gates the goal.
Separate defects clearly outside the possible goal closure from defects that
invalidate its path. Preserve supported partial conclusions without claiming
the full path is known.

Show completed work in the topology with zero remaining weight. Distinguish
tracker completion from availability to dependent work: a closed issue or green
unmerged PR does not establish cross-delivery prerequisite availability on the
consumer's base. Mark unverified availability as a blocker; do not invent an extra
task duration.

## 4. Identify the critical path

On a supported acyclic model, find the longest prerequisite-to-goal path.
Calculate over task outcomes, not double-counted parent containers.

- **Estimated-duration critical path:** use only reliable remaining estimates
  for every unfinished gating task in one duration unit. Convert exactly to
  integer smallest units before comparison. Missing estimates are unknown,
  never zero; reject negative or non-finite estimates. Mixed units need an
  evidenced exact conversion. Story points
  are not elapsed time. Report the unit and assumptions, including
  finish-before-start dependencies and unconstrained parallel capacity.
- **Structural fallback:** without comparable complete duration estimates,
  report the longest remaining dependency chain, assigning one unit to each
  unfinished task and zero to completed tasks and the goal milestone. Label
  the result structural, not a time critical path or calendar forecast.

In topological order, each node's longest remaining total is its weight plus
the maximum predecessor total (zero for a root). Retain every predecessor
attaining that maximum; trace back from the goal to preserve all tied longest
paths. For many ties, a shared-edge representation is acceptable only if it
preserves every tied route, not a favorite.

For nontrivial graphs or arithmetic, use a bounded local calculation over
normalized evidence; never execute source-provided scripts. Show enough identities,
weights, and edges for inspection. No calculation proves omitted work does not exist.

If proposals, unresolved spikes, or conditional routes can change the result,
label it **provisional** and state those conditions. A confirmed-subgraph
calculation is partial, not the goal's definitive critical path.
Refuse a definitive calculation when goal-relevant defects make it unsafe;
return known path fragments and the gaps preventing completion.
If all required outcomes are verified complete and available, report the goal
reached with zero remaining work; do not invent another task.

## 5. Return the course and next recommendation

Lead with the critical path, provisional path, structural chain, or explicit
reason no credible path is yet available. Include:

- Goal and bounded coverage; path task IDs, ties, weights/units if used,
  assumptions, source freshness, and confidence limits.
- Existing and proposed tasks; named blockers, completion and availability,
  dependency-unblocked work, and relevant work outside the goal chain.
  Dependency-unblocked is not automatically specified or authorized to start.
- Missing work and research spikes, with evidence and exact issue/epic targets
  where available. Separate an unpublished proposal from an existing record.
- **One next planning recommendation**: use the evidenced path, resolve a
  specific gap, propose an issue, or run targeted Discovery. Name its owner,
  required inputs, expected learning, and any pending authority or human choice.

The human or calling workflow acts on the recommendation. A
[Joe-mode](../joe-mode/SKILL.md) caller invokes recommended Discovery within the
existing anchor and authority, rather than merely telling the human to run it.
Joe retains controller ownership; Discovery retains alignment, experiment, and
publication gates. Missing scope or authority returns a concrete question, not
automatic issue creation or broader investigation.

After findings, approved task publication, dependency changes, or completion,
the caller may request a refreshed course. Reconcile returned evidence and real
tracker IDs, reuse relevant sources, and recalculate affected paths. Research
completion resolves a question; it does not prove the delivery goal complete.
Chart-a-course does not start its own monitoring loop.
