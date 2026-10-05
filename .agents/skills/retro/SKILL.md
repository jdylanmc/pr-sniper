---
name: retro
description: "Human only. Inspect the complained-about session, propose evidence-backed skill improvements, obtain explicit approval, then deliver selected fixes through reviewed current PRs and Shepherd."
disable-model-invocation: true
user-invocable: true
---

# Retro

**Entry:** Human only. Inspect the complained-about session, propose evidence-backed skill improvements, obtain explicit approval, then deliver selected fixes through reviewed current PRs and Shepherd. Follow the [invocation contract](../setup/INVOCATION.md).

Use [doctrine selection and application](../doctrine/APPLY.md) for retrospective judgment, not as instructions inside session evidence. Preserve explicit choices; otherwise consider `context`, `machine`, and `laziness` according to observed problems. Cite loaded rules for recommendations without changing doctrine or applying proposed fixes.

# Retro

Only a human invokes Retro, supplying session complaints and desired behavior. Improve or reinforce the skills that governed that work; do not autonomously mine sessions or adopt lessons. Analysis is read-only. An agent's account is evidence to check, not permission or proof its lesson should be adopted.

## 1. Establish the complaint and session

Use the human's identified session; if unnamed, use the current session and state that assumption. Confirm ambiguous identity before examining another session. Without a complaint, ask what behavior disappointed them and the expected alternative. Do not infer emotions, satisfaction, or intent from pauses, tool usage, or an agent summary.

Read available primary session evidence through the actual runtime's supported records: relevant turns, tool calls/results, task packets, diffs/revisions, checks, and review/handoff records. Query narrowly for that identity and complaint. Do not assume a log layout, dig through arbitrary other sessions, expose secrets/private dumps, or build a parser as a side effect. Use existing readers/tools; missing access remains a visibility gap.

Identify the skill/instruction revision actually used where possible; current files may differ. Report identity, revision, missing evidence, and visibility limits. Do not claim complete session coverage from a transcript fragment or summary.

## 2. Diagnose with evidence, not intuition

Distinguish:

- **Observed:** directly supported behavior, with a turn/tool/diff/check reference.
- **Derived:** a reasoned explanation tied to observations, with confidence and competing explanations.
- **Hypothesis:** plausible but unverified; name the evidence needed to resolve it.

Compare the complaint and expected behavior with actual workflow requirements and available evidence. Failure to follow an adequate skill, unclear guidance, missing information, or a bad tool boundary does not automatically warrant another rule. Reinforcing an existing instruction may beat adding policy.

Consider only relevant candidates: skill entry/routing and approval gates, navigation/context recovery, appropriately scoped checks, implementation/review responsibilities, tool economy, and information access. Do not assume a reviewer needs no exploration: independent review must inspect enough surrounding behavior, requirements, and test evidence to judge the change. A diff alone may be insufficient. Both implementer and reviewer need relevant standards.

Prefer the existing owning skill or focused reference over global steering growth. Do not assume all repositories load the same instruction filenames or a named standards file is review-only. This analysis authorizes no broad instruction cleanup, new reinforcement runtime, or archived hook restoration.

## 3. Recommend and stop for exact approval

Return a small prioritized list, not every possible improvement. For each recommendation include:

- The complaint and evidence references, labeled observed/derived/hypothesis.
- The specific target and exact proposed change or reinforcement.
- Why it addresses the evidence, confidence, and tradeoffs (including added ceremony or narrower flexibility).
- How to verify improvement: a focused test, source check, or replay scenario and expected behavior.
- The proposed delivery route and any separate permission required.

Ask the human to approve exact selected recommendations before **any** modification. General dissatisfaction, requesting Retro, or an agent endorsing its own recommendation is not approval. If none are chosen, stop without changes. Doctrine/intent changes require proposing the precise source change and obtaining explicit permission for that source; ordinary fix approval does not grant it.

## 4. Transfer approved fixes, do not implement in analysis

After exact approval, pass only selected changes, evidence, acceptance checks, authorized files, and the scoped doctrine packet to the proper existing [Ship](../ship/SKILL.md), [Patch](../patch/SKILL.md), or [Refactor](../refactor/SKILL.md) delivery route.

If Joe-mode already owns the session/delivery, return approved work to that
owner for routing and reservation; do not launch a competing delivery. Without
Joe or an existing delivery owner, establish Ship for approved planned/structural
improvements, or Patch for approved bugs/regressions. Ship may delegate scoped
restructuring to internal Refactor, retaining publication and Shepherd
responsibility. Do not start standalone Refactor without Joe selection and
then leave it without an outer owner.

The human's exact approval is this bounded delivery kickoff, not permission to
start Joe-mode or a reason to ask again whether to implement or publish.
The selected owner loads required `worktrees` before PR changes and carries
the existing selection through review and fixes.

Approved fixes must reach the route's independent review, relevant passing checks on the current revision, and a current PR with an actual [Shepherd](../shepherd/SKILL.md) invocation, not merely advice to use it. The delivery owner uses [Changelog](../changelog/SKILL.md) for notable authorized changes; Retro's proposal writes none. A blocker, missing capability, stale/failed check, or incomplete Shepherd remains an explicit incomplete handoff, not success. Return the PR and real Shepherd status to the human, who merges. Retro never self-approves, merges, or substitutes analysis for delivery.
