---
name: sequencing
description: "Order work into coherent steps that isolate risk and build a replayable argument."
scope: shared-engineering-doctrine
---

# Sequencing Doctrine

## Prime directive

Order work so each coherent step makes a claim, proves it, and leaves the next step safer.

## Position

Sequence is part of correctness. The same changes performed in a careless order can hide causes, create unsafe intermediate states, invalidate evidence, and make review harder than the work itself.

The goal is not the smallest possible step. It is the smallest coherent step: enough change to produce a meaningful state, little enough change to localize failure and understand why progress is justified.

## Principles

- **Subtract before adding.** Remove obsolete paths, duplicated choices, and accidental complexity before building new behavior on top of them. A smaller foundation makes later evidence clearer.
- **Pair claims with evidence.** Each step should say what became true, how that claim was tested, and what must be true before dependent work begins.
- **Separate execution from delivery.** Execution order helps locate failure and maintain valid states. Delivery order helps another person understand the problem, transformation, and proof. One sequence may serve both, but they are not the same concern.
- **Preserve coherent states.** Do not split generated output, migrations, protocols, or tightly coupled behavior merely to produce smaller artifacts. A step that cannot stand meaningfully is not small; it is incomplete.
- **Batch only what fails together.** Homogeneous mechanical work may move as one bounded unit when splitting adds cost without improving fault isolation. Semantic differences deserve separate proof.
- **Treat evidence as revision-bound.** Rebases, regenerated output, changed dependencies, or later edits can invalidate earlier confidence. Revalidate the state that will actually continue or land.
- **Distinguish expected evidence from unexplained failure.** A deliberate failing observation can prove an absence or defect. An unexpected red state is not progress and must not become a foundation.
- **Prove the assembled result.** Focused checks localize confidence. They do not replace validation of the complete system after the pieces meet.
- **Build an argument, not an artifact count.** More commits, phases, or change requests are useful only when they clarify dependency, risk, or proof. Fragmentation without information is ceremony.

## Boundary

Sequencing must not invent dependencies, preserve intentionally broken landing states, or demand costly validation after every trivial edit. Order work according to risk and evidence, not ritual.
