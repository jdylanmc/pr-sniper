# Domain documentation

This repository uses a single-context layout: `CONTEXT.md` at the root and
architecture decision records in `docs/adr/`.

Before exploring an area, read the root context and relevant ADRs if present.
If those files do not exist, proceed without manufacturing them or treating
their absence as failed setup. The domain-modeling helper creates them lazily
only through an authorized owner after actual agreement.

Existing product discovery and specifications remain in `docs/agent/`.
Workflow instructions live in `docs/agents/`; these are not a competing source
of product requirements. The nano specification is the settled product
authority, and the full specification provides supporting requirements.

Use agreed glossary terms consistently. Surface conflicts with an ADR rather
than silently overriding it. Distinguish proposals, experiments and open
questions from confirmed decisions.

Discovery may propose meaningful documentation PRs for agreed knowledge,
preserving recording and exact-file approval gates. Use the dedicated
Discovery worktree and serialize branch-write custody with Shepherd. Do not
open documentation PRs merely because a heartbeat fired.
