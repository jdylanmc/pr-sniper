---
name: automate-this
description: "Human only. Turn recurring activities into workflow specifications; do not implement or execute the automation."
argument-hint: "A workflow to design, or nothing to go find one"
disable-model-invocation: true
user-invocable: true
---

# Automate This

**Entry:** Human only. Turn recurring activities into workflow specifications; do not implement or execute the automation. Follow the [invocation contract](../setup/INVOCATION.md).

Use [doctrine selection and application](../doctrine/APPLY.md). Preserve explicit choices; otherwise, consider `machine` and `laziness` for automation design, not as requirements for every life workflow. Carry selections into any authorized delivery handoff. If a separately authorized PR will deliver workflow documents, require `worktrees` before preparing changes.

Only a human invokes this workflow. Clarify design in focused rounds, recommending answers where useful, aimed at the vocabulary and goal below. Interrogate is internal only to Discovery or Joe-mode; do not call it directly or route ordinary Automate-this design through full domain-modeling Discovery just to reach it.

This workflow owns creating, editing, and deleting the **workflow** specs and `NOTES.md` below as the human resolves the design. Persist outputs only there; no additional domain-model recording. Confirm intended files and material changes before writes; confirm deletion explicitly. Preserve unrelated human notes; do not record secrets. The command name does not authorize building/running automation, provisioning integrations, scheduling jobs, or operating accounts.

## The loop lens

A **loop** is a recurring pattern in the user's life: their career, their week, their morning, a single repeated activity. Loops within loops reveal how predictable life's activities are, making them worth **delegating**. Find loops worth specifying, and propose ones the user hasn't noticed.

A **workflow** is the spec of one loop, made real. You run a workflow on a loop: the loop is its running instantiation. Workflows live in `workflows/*.md` and are the source of truth.

## Vocabulary

Use this shared language only when a workflow calls for it, never as a checklist. **Mandate nothing structural**: a workflow needs no AI, no checkpoint, and no schedule unless the design conversation shows it does.

- **Trigger**: what fires each run, an **event** (a new email, a new issue) or a **schedule** (every morning). Event-triggering is usually the more efficient.
- **Checkpoint**: a human-in-the-loop point where the user is asked to verify or decide. Some workflows have none and run autonomously; some use no AI at all.
- **Push right**: defer discretionary checkpoints as far as useful, preparing a decision-ready result. Never defer required consent or an approval gate until after the action it controls.
- **Brief**: what a checkpoint presents, a tight, decision-ready summary (what was produced, why, and a link down to the asset itself), never the raw output. The user reads a brief, not a draft. Speed of review is imperative.

## Definition of done

A workflow spec is done when an implementer can build the agreed scope without inventing human decisions. Resolve relevant trigger, inputs/outputs, tools and access, constraints, failure/recovery behavior, human checkpoints, and verification expectations; mandate no template or extra ceremony. Keep remaining questions explicit. If the human cannot yet answer, pause with an incomplete spec rather than interrogating indefinitely or inventing certainty.

## The workspace

- `workflows/*.md`: one spec per workflow.
- `NOTES.md`: raw notes on the user's world, tools they use, channels they process, and their terminology for both. If empty or thin, interview them about their world before specifying anything. Sharpen fuzzy terms into canonical ones as they surface; record them here.

For authorized file changes, use [Changelog](../changelog/SKILL.md) for the correct repository/component: curate notable `Unreleased` entries following Keep a Changelog 1.1.0, or report no entry needed for non-notable notes/design edits. Include any entry in the approved write scope. This is the sole additional bookkeeping write, not another workflow spec destination. No changelog write during proposal/review, commit-log dump, automatic versions/releases, or recursive entry for changelog-only changes.
