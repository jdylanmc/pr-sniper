---
name: machine
description: "Review automation for independent proof, bounded effects, clear ownership, and lifecycle value."
scope: default-pr-sniper-doctrine
---

# Machine Doctrine

Automation earns its place when a small trustworthy mechanism costs less and risks less than careful repetition. Machines amplify discipline and mistakes alike.

Review scripts, generators, queries, codemods, workflows, and mechanical edits against their purpose and full lifecycle cost. Repeated manual work invites inconsistency and forgotten steps; unnecessary tooling creates permanent obligations.

Look for:

- **Stable scope.** Automate the understood mechanical core. Uncertain requirements or judgment-heavy decisions should not be frozen into machinery merely because execution can be repeated.
- **Independent proof.** Determinism can reproduce the same error perfectly. Requirements, tests, or independently checked outputs must establish correctness; successful execution alone does not.
- **Bounded effects.** Allowed targets, preserved state, stop conditions, and resulting evidence should be explicit. Broad mutation without a clear boundary is fast damage, not efficiency.
- **Replayable work.** Repetition should be inspectable, with assumptions and outcomes visible. Consider whether rerunning the operation can compound effects or invalidate earlier evidence.
- **Appropriate judgment.** Checked tools should handle repetitive mechanics; people or agents should contribute independent review and domain judgment, not duplicate what one reliable tool already does.
- **One authority.** Generated and derived artifacts need a named source of truth. Hand-maintained copies or competing generators can create conflicting ownership.
- **Earned permanence.** Construction, validation, explanation, maintenance, and eventual removal all count. A temporary lever may be sufficient; creating a tool does not justify keeping it forever.

Ground a finding in foreseeable inconsistency, unsafe effects, unverifiable output, or unnecessary lifecycle cost. Recommend the smallest reliable mechanism, or careful manual work when cheaper and safer. Urgent, one-off, uncertain work does not automatically need a framework.
