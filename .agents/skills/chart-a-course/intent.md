# Intent: chart-a-course

Chart-a-course exists to find a critical path to one named goal through tasks.
The existing backlog is evidence, not the boundary of the work.

It works backward from the goal through existing requirements, issues, epics,
and tasks. It identifies missing tasks and prerequisites as well as existing
work, and reveals research spikes where uncertainty prevents a credible path.
Proposed tasks and dependency relationships stay distinct from confirmed
tracker records and evidenced dependencies. Unknowns are not invented certainty.

It shows which work gates the goal, which work is dependency-unblocked, which
work is blocked by named prerequisites, which work is complete, and which work
is outside the goal's dependency chain. Being dependency-unblocked does not by
itself make a task specified, authorized, or ready for delivery.

When every remaining gating record has a reliable estimate in one unit, the
skill may calculate the longest weighted gating path. Otherwise it reports only
the longest structural dependency chain and says plainly that this is not a
calendar or time critical path. Equal longest paths must remain equal rather
than being collapsed into a preferred route. Completed records remain in path
topology but contribute zero remaining weight. Weighted values use exact
integer smallest units; fractional estimates are not used for weighted
comparison.

The skill must not present proposed dependency edges as established facts or
silently repair malformed work data. Missing or duplicate identities, absent
edge endpoints, unclear edge direction, cycles, stale or unavailable lifecycle
state, and unresolved goal coverage remain visible. Conclusions that those
defects make unsafe are refused or clearly qualified. Defects clearly outside
the named goal's possible closure remain visible and lower confidence without
suppressing otherwise supported goal conclusions.

This is a read-only planning workflow, not a delivery controller. It can propose
raising an issue and recommend Discovery against a particular issue or epic
when necessary to establish the path. It does not itself create or close work,
mutate trackers, dispatch workers, or conduct the recommended investigation.

The caller owns execution. A human can act on its recommendation; a
human-started Joe-mode caller invokes recommended Discovery within its existing
scope and authority, preserving Discovery's human-alignment and write gates.
Investigation findings feed back into the path rather than being treated as
delivery completion. Chart-a-course returns one next planning recommendation
alongside the map, gaps, research spikes, and evidence limitations.
