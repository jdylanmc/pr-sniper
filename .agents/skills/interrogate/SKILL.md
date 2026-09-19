---
name: interrogate
description: "Internal to Discovery or Joe-mode only. Ask dependency-aware questions and obtain actual human decisions; preserve the parent conversation and recording gates."
disable-model-invocation: false
user-invocable: false
---

# Interrogate

**Entry:** only inside an authorized Discovery or Joe-mode context, under the
[invocation contract](../setup/INVOCATION.md). If another workflow needs one
clarification, it asks normally; material discovery goes through Discovery.
Return questions to the parent for the human, never simulate human answers.

Preserve the caller's [doctrine selection](../doctrine/APPLY.md). With none, choose guidance from catalog metadata only when it informs this decision; do not impose engineering doctrine on unrelated conversations. Pass the scoped packet to any exploration or domain-recording worker. Loading doctrine neither selects requirements for the human nor enables recording.

## Recording

Default to conversation-only interviews; a repository's presence does not authorize file writes.

When the user or calling workflow requests domain-model recording, call the Skill tool with "domain-modeling" and apply it as answers settle. Reuse existing domain documents; create them only for resolved terms or justified architectural decisions. Preserve that skill's confirmation gates and the caller's output scope. Do not invent additional documents or duplicate an active domain-modeling session.

## Interview

Interview the user relentlessly until you reach shared understanding. Map a **design tree**: each decision branches into its dependent decisions.

Work in **rounds**. The **frontier** contains every decision with settled prerequisites: questions you can ask _now_ without guessing unheard answers. Ask the whole frontier in one round; number each question and give your recommended answer. Wait for the user's answers before the next round.

Format a round like so:

```
❓ **Q1** - **<question title>**: <question body, might be multiple paragraphs, including multiple choices>

➡️ <your recommended answer>

---

❓ **Q2** - **<question title>**: <question body, might be multiple paragraphs, including multiple choices>

➡️ <your recommended answer>
```

Each answered round reshapes the tree: settled decisions advance the frontier and unblock dependent questions. Recompute the frontier and ask the next round. Questions depending on another question still open in this round belong to a _later_ round.

Finding _facts_ is your job, never the user's. Read accessible evidence for small lookups; delegate substantial independent investigation when useful and supported. Don't ask the user for inspectable facts. Running exploration is an unsettled prerequisite: only downstream questions wait; ask the rest of the frontier now. The _decisions_ are the user's: put each to them through the parent and wait.

The session ends when the frontier is empty: every design-tree branch visited, nothing silently assumed. Do not act until the user confirms shared understanding.
