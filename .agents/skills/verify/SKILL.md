---
name: verify
description: "Internal helper for evidence before completion claims. Check real acceptance conditions in task scope; reuse evidence only while relevant state and inputs remain unchanged."
disable-model-invocation: false
user-invocable: false
---

# Verify

**Entry:** Internal helper for evidence before completion claims. Check real acceptance conditions in task scope; reuse evidence only while relevant state and inputs remain unchanged. Follow the [invocation contract](../setup/INVOCATION.md).

Use [doctrine selection and application](../doctrine/APPLY.md), preserving the work packet's standards. With none, consider `testing` for behavioral proof and `integration-testing` when real boundaries matter. Verify claims about doctrine too: selection is not loading, loading is not application, and recommendations are not approval. Report missing standards without broadening validation authority.

Evidence before claims. Identify the smallest sufficient proof for the actual acceptance conditions, inspect it, report the result, and stop.

## Scope

Verification does not authorize product changes. Make fixes only when the task includes them; otherwise report failures for the user to act on. Do not add polish, cleanup, new features, or unrelated tests after the criteria pass.

An authorized modifying owner uses [Changelog](../changelog/SKILL.md) for the
resulting outcome and revalidates it; a verification-only invocation does not
edit a changelog or start delivery.

## 1. Define the claims

List the acceptance conditions and the evidence each requires. Use repository-standard commands or an appropriate inspection or manual check; do not substitute an easy proxy for the requested outcome.

| Claim | Required evidence | Not sufficient |
| --- | --- | --- |
| Tests pass | Completed run for the stated selection, with result and exit status | A run starting, an old unbound result, or an agent saying "passed" |
| Build succeeds | Successful build for the relevant target and configuration | A passing linter or unrelated tests |
| Bug fixed | Original symptom checked against the final state | Code changed or a nearby test passing |
| Regression covered | A test that fails for the original defect and passes with the fix | A newly written test that only passed |
| Requirements met | Evidence for every in-scope acceptance condition | Some tests passing |
| Agent work delivered | Actual resulting artifacts and evidence tied to their state | A success message alone |

Structural checks of skill names, metadata, or links prove those contracts, not the behavior of a consuming agent. Keep the claim as narrow as the evidence.

## 2. Check whether evidence is still current

Reuse a completed result only when it covers the same claim and you can establish that its relevant state and inputs remain unchanged:

- Code and artifacts, including relevant uncommitted changes and fixtures. The same commit ID alone is not enough.
- Command, arguments, test selection, target, and configuration.
- Dependencies, toolchain, environment, and any external data or service state the result depends on.

Keep enough context to identify the result: the command or method, completed outcome, and relevant state. Do not create a new tracking framework or claim that a reused result was freshly executed.

Rerun when relevant inputs changed, freshness is uncertain (including unverified external state or expired evidence), the result is incomplete, or the user explicitly asks for a fresh check. A later message alone does not invalidate otherwise-current evidence.

## 3. Run and inspect missing proof

Run focused checks before wider gates. Execute each selected check completely, wait for it to finish, and inspect its exit status and complete result, not a reassuring log fragment.

If output is unavailable or a process is still running, the check is not complete. Read the existing run's result rather than launching a duplicate just to obtain output.

Respect the task's environment and mutation boundaries. A need to verify does not authorize production writes, destructive reproduction, or rolling back someone else's changes.

## 4. Report the actual result

- **Pass:** the check completed and proves its stated condition.
- **Fail:** completed evidence shows the condition is not met.
- **Unavailable:** the required check cannot run with the available tools, access, or environment.
- **Blocked:** a prerequisite or complete result is still missing.

Required failed, unavailable, or blocked checks prevent an overall completion claim. Partial proof supports only a partial claim. Report warnings and limitations without turning them into either a false clean result or an unrelated repair task.

Report the claim, decisive evidence, whether it was newly run or reused, result, and unresolved risk. Keep raw logs out of the response unless they are needed to explain the result.

Once all required proof is complete, stop.
