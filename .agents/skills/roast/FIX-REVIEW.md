# Scoped fix review

Use when Roast is asked to verify a particular fix wave. This supplements the single Roast findings contract, not another review/approval protocol.

## Inputs and scope

Receive the original findings, relevant requirements/standards, the previously reviewed revision, the fix-base/result or artifact snapshots, and the implementer's report. Identify missing inputs rather than assuming `HEAD~1` captures the fix.

Read the actual fix and enough context to check its mechanism. Verify each original finding and inspect for new breakage. A supplied diff/report is evidence, not a ban on checking source when it is incomplete or stale.

Do not repeat unchanged whole-artifact review merely because a fix exists. If it changes the contract or invalidates earlier coverage, identify affected scope and request the needed broader review. The owner still needs full integrated-deliverable review; a scoped fix check does not replace it.

## Evidence and output

For every original finding, report **ADDRESSED**, **NOT ADDRESSED**, or **UNVERIFIED**, with a locator and decisive evidence. Attempted remediation is not resolution. Mark what could only be inspected and what was actually executed.

Reuse applicable, current validation evidence after checking it covers changed behavior. Run permitted focused checks for remaining material doubt, within Roast's execution boundaries. Neither rerun every suite automatically nor forbid necessary checks because the implementer reported success.

Lead with one prioritized list of remaining flaws and new breakage, using Roast's complete finding fields. Append the per-finding verification results and coverage limits. Do not produce a second Standards/Spec approval rubric or a "safe to merge" verdict.

Keep incidental out-of-scope observations separate; route them to the owner. Do not silently fix them or expand implementation scope. If one invalidates the fix's claimed correctness, state that consequence rather than automatically declaring it non-blocking.

No source mutations, nested reviewers, automatic repairs, accepted-risk rulings, or human sign-off. Return to Ship or the requesting owner for the next authorized step.
