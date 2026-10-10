---
name: pull-request
description: "Both. Write or update a pull request body as a terse what/why/how with a show-me style visual, before/after evidence, and blast radius. Humans review this surface; no prose padding."
disable-model-invocation: false
user-invocable: true
---

# Pull Request

**Entry:** a human request, or an agent inside an already authorized delivery (Ship, Patch, Refactor, Shepherd), under the [invocation contract](../setup/INVOCATION.md). This skill writes the PR body. A request to open or update a PR authorizes that one PR; it grants no merge, approval, or ready-for-review authority. Without publication authority, return the body only.

The body is the surface a human reviews. Give the **what**, the **why**, and the **how**. Show the how; do not narrate it.

## Voice

Write each prose line at the [Synthesize](../synthesize/SKILL.md) **Caveman** altitude: cut filler, keep every substantive fact, constraint, and uncertainty. Readable fragments are fine. This is a per-artifact style, not the session [Caveman](../caveman/SKILL.md) mode, which excludes PR text; the operator explicitly requested this exception. Do not invent abbreviations. Keep `not`, `never`, `only`, `except`. Keep code, paths, identifiers, and numbers exact.

Use the vocabulary in `CONTEXT.md` (via `CONTEXT-MAP.md` if present) when it exists. Skip preambles, recaps, and restating the diff in prose.

## Template

```markdown
## What

<1-2 lines. The change, as observed behavior.>

## Why

<1-2 lines. Goal, bug, or ticket. Closing keyword only if the work fully satisfies it.>

## How

<smallest visual that makes the point; see Visuals>

## Evidence

- **Before:** <screenshot / output / failing test>
  **After:** <screenshot / output / passing test>

## Blast Radius

**Scope:** <one word>

<optional: ramifications of merging>

**Door:** <one-way | two-way>

<optional: why>
```

Omit optional lines when empty. Do not add other sections unless the repository's own PR template requires them; if one exists, keep its required headings and place this content inside them.

## Visuals (How)

Pick the smallest view that makes the key point. Use one or several; rarely all. Show only the calls, files, props, states, and boundaries a reviewer needs. Put each visual next to the single line it supports.

| Shape | Use for | Format |
| --- | --- | --- |
| Pseudocode | Logic or algorithm | `text` fence |
| Call tree | Runtime control flow | `text` fence |
| Component tree | UI structure, state and module boundaries, with file paths where they matter | `text` fence |
| Shallow file tree | File responsibility or broad refactor, one `#` comment per node | `text` fence |
| Mermaid | Component interaction, control flow, data flow | `mermaid` fence |
| Diff sketch | What changes when surrounding shape already exists | `diff` fence |
| Whole block | Mostly new code, or a copyable target shape | language fence |

Match the diff sketch to the topic. Prefix unchanged lines with a space, added with `+`, removed with `-`:

```diff
 submitForm
   createSession
     persistPrompt
+    expandSkillMention
     launchAgent
-  navigateToSession
+  navigateToSession
+    subscribeToEvents
```

Rules:

- Sketch the shape; do not paste the real diff. The host already shows it.
- Verify every name, path, and ordering against the actual change. Never draw structure that does not exist.
- Check that the target host renders Mermaid in PR descriptions. If unsure or unsupported, use a call tree or sequence as text instead.
- The body is Markdown only. Do not embed or link generated HTML artifacts as the explanation.

## Evidence

Concrete proof the change works, as before and after. Never fabricate or imply a run that did not happen.

1. **Screenshots** (best) when the change is visual and an environment can capture them. Attach only by a supported host mechanism; do not commit images to the repository for this.
2. **Execution** (next best): exact test or command output. Name the test that failed before and passes now, and show it as pseudocode:

   ```text
   test "cached save returns same result"
     before: FAIL  expected cached, got fresh write
     after:  PASS
   ```

3. **Neither available:** write `Evidence: none` and say why. Do not substitute reasoning for proof. Name unrun checks.

For a new behavior with no meaningful before, show the failing test on the base and the passing test on the head. Report only runs on the current head; label any older run.

## Blast Radius

- **Scope.** One word for the scope of impact, then optional lines for ramifications. Consider all possibilities: consumers breaking, layout shift, mobile responsiveness, performance, permissions, rollout order, other repositories or services.
- **Door.** *Two-way*: cheap to roll back (revert restores state). *One-way*: destructive or hard to reverse, such as data migration or deletion, published API or contract change, external side effects, secrets rotation. Say which, and why in one line when not obvious.
- Say `none known` only after checking consumers and call sites. Mark inference as inference.

## Publish or update

Follow [DELIVERY](../ship/DELIVERY.md) for provider mechanics: reuse any existing PR for the branch, create as draft, query before retrying after an uncertain result, and never mark ready or merge. Use the repository's PR template headings when one exists. Confirm the PR URL and read the stored body back; it must match the intended text.

When updating after new commits, revise What, How, Evidence, and Blast Radius to the current head; do not append history.

## Check before finishing

- What and Why each fit in two lines; nothing restates the diff.
- How shows structure, with names verified against the change.
- Evidence is real, current, and labeled before/after, or explicitly `none`.
- Scope and Door are both stated.
- No required meaning was lost to compression.

## Relationship and attribution

This skill owns the PR **body format and voice**. [Create Pull Request](../create-pull-request/SKILL.md) owns generic **publication** when no other route does; when both apply, use this body inside that publication flow. Do not run two publication workflows.

Adapted from Matt Pocock's MIT-licensed [`pr`](https://github.com/mattpocock/skills/blob/main/skills/engineering/pr/SKILL.md) skill, which credits Dex Horthy and HumanLayer's MIT-licensed [`show-me`](https://github.com/humanlayer/skills/blob/main/plugins/show-me/skills/show-me/SKILL.md). See the bundled [third-party notice](../setup/NOTICE.md).
