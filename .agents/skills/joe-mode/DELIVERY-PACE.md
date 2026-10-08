# Delivery pace

## Target

**Two hours: delivery start to first qualified final-signoff candidate**, as
defined by [DELIVERY](../ship/DELIVERY.md#pr-states). Planning goal, not deadline.
Never skip proof, force scope cuts or add timers to hit it.

Record start and qualification times in the existing packet. Headline elapsed
includes blocked time; list known blocked intervals separately. Report later
human/merge wait separately, plus actual merges. No clock reset at handoffs,
invented active time, or tool/agent counts as output.

## Finish first

Name the outcome, owner, non-goals, acceptance seam and checks before dispatch.
Prefer a useful vertical slice. Changed scope or PR grouping needs human agreement.
Finish actionable candidates before opening lanes; independent work may continue
when one lane is blocked. Reserve support capacity within the authorized pool.

## Batch proof

- Work by end-to-end behavior, not helper, assertion or translation.
- Where TDD applies, record behavioral RED before corresponding GREEN.
  Compile errors are not RED. Tests written afterward are regression coverage,
  not retroactive TDD.
- Use focused checks during work; required integrated/current-head checks at
  the candidate boundary. Respect runner limits; no weaker oracles or timeout hacks.
- Serialize shared fixtures. Reuse evidence only while source, environment and
  claim still match; distinguish reused from rerun. Preserve unexplained failures.
- One whole independent review; same-reviewer fix/impact passes with retained
  coverage. New scope needs new coverage. Keep separate required duck/acceptance
  gates. Use [doctrine selection](../doctrine/APPLY.md#joe-review-operations).

## Act or ask

Act when outcome, owned files/resources, permission class and risk remain within
the recorded grant. Coupled test/API repairs fit; weakening acceptance does not.
Named-candidate restrictions still bind. Unknown custody is not permission.

Escalate early when the target becomes implausible: obstacle, evidence, owner,
next action. Resolve routine engineering problems within scope. Route actual
scope/risk/authority questions through the one Discovery conversation.
Present once per blocker/decision revision; repeat only for changed evidence or
a human-requested reminder. Continue independent work; silence grants nothing.

## Reconcile, don't replay

Match delayed messages to actual head/base and findings before acting.
Preserve unresolved evidence, not just the latest message. A delayed RED report
supports TDD only if its recorded execution preceded GREEN; a post-GREEN replay
is audit/regression evidence. Do not repeat already-resolved work.
Report milestones, not ACK loops. Refresh facts needed for the next decision.

| Boundary | Action |
| --- | --- |
| Test constructor changed within owned scope | Adapt it; preserve behavioral assertions. |
| New interactive test venue needs permission | Ask Discovery; no substitute green claim. |
| Only backend done; editor still missing | Implementation incomplete; no silent rescope. |
| Candidate/base changed during human review | Follow DELIVERY's revision and notice rules. |
