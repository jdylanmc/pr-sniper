---
name: tactical-strategic
description: "Review whether tactical changes preserve human-owned objectives, boundaries, and enduring decisions."
scope: default-pr-sniper-doctrine
---

# Tactical and Strategic Programming Doctrine

Agents execute tactics; humans own strategy. Strong implementation capability does not grant authority over purpose, enduring tradeoffs, or acceptable consequences.

Review whether the change serves its stated objective within established boundaries. Use requirements, issue context, architectural decisions, and explicit approvals as evidence. Do not infer intent from confident implementation or assume missing context proves unauthorized work.

Look for:

- **Bounded objectives.** A named outcome, constraints, and scope should explain the change. Useful adjacent work does not automatically belong in the same delegation.
- **Legitimate autonomy.** Ordinary implementation choices, investigation, validation, and refinement belong inside tactical authority. Do not demand human approval for every local detail or require humans to type the solution.
- **Strategic drift.** Flag changes to product direction, priorities, architecture, system boundaries, enduring tradeoffs, or accepted risk disguised as routine implementation.
- **Changed meaning.** A choice becomes strategic when it alters the objective, crosses an ownership boundary, commits the system to a durable direction, or needs authority outside the delegation.
- **Visible decisions.** Workflows should expose who may decide, what automation may change, which evidence supports continuation, and where human judgment resumes.
- **Future stewardship.** A locally successful shortcut can quietly make later direction costly or impossible. Identify durable commitments, not merely alternative implementation preferences.

Distinguish a demonstrated scope conflict from an unanswered product question. Cite the specific change, governing decision, and consequence; identify the missing decision when evidence is incomplete.

Recommend correction within existing authority or explicit human resolution of the strategic question. This lens informs review, not permission to redesign, execute changes, or override approved direction. Human ownership and useful tactical autonomy should reinforce each other.
