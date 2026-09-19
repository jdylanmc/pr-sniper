---
name: machine
description: "Use small rerunnable levers when they are cheaper and safer than repeated manual work."
scope: shared-engineering-doctrine
---

# Machine Doctrine

## Prime directive

Before repeating non-trivial work, compare careful manual execution with the smallest trustworthy machine that could perform or prove it.

## Position

Repetition invites inconsistency, forgotten steps, weak evidence, and expensive review. A small script, generator, query, codemod, or deterministic check can turn private effort into something replayable and inspectable.

Automation is not automatically economical. Building the lever has a cost. So do validating it, maintaining it, explaining it, and eventually removing it. Choose the lever only when its total lifecycle cost and risk are lower than doing the work carefully by hand.

## Principles

- **Build the smallest trustworthy lever.** Automate only the stable mechanical core. Leave uncertain work exploratory until its shape is understood.
- **Rerunnable is not correct.** Determinism can reproduce the same mistake perfectly. Validate the lever against independent requirements, tests, or evidence rather than trusting its first output.
- **Bound the blast radius.** A machine should know what it may touch, what it must preserve, when it should stop, and what evidence it leaves behind. Broad mutation without clear scope is merely fast damage.
- **Automate mechanics; delegate judgment.** Do not fan out humans or agents to repeat what one checked tool can do. Use independent minds for review, domain judgment, and challenges the machine cannot settle.
- **Make authority visible.** Generated and derived artifacts need one named source of truth. A machine must not create a second owner for the same fact.
- **Choose a lifecycle deliberately.** Some levers belong in the repository. Others should be temporary and discarded after verified use. Artifact creation is not evidence that permanent ownership is worthwhile.
- **Let laziness govern.** Tool-building is over-engineering when the tool costs more to construct, trust, and maintain than the work it saves.

## Boundary

Urgent, one-off, low-confidence, or genuinely judgment-heavy work may be safer by hand. Machines amplify both discipline and error; use them where replayability improves trust, not where automation merely looks systematic.
