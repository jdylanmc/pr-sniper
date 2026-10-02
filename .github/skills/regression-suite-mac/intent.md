# Stored approved intent

Verbatim clauses from the human-merged specification (PR #94, merge
dbb11b6acd197fbde484090cbd4f149cf5967ab6), not a new confirmation:

- D05 - Dylan: regression-suite-mac runs requested features or the full registered suite using an independent worker. Build the macOS environment first; Windows setup and execution follow separately and must run the same cases.
- D07 - Dylan: Results identify the exact candidate, exercised cases, PASS/FAIL/BLOCKED, evidence and cleanup. Missing coverage stays visible; the main agent validates suspected bugs before routing them.
- AC-006: `regression-suite-mac` runs the requested features or the full registered suite and identifies exactly which cases were requested, executed and not exercised. Missing coverage or unsupported interaction is explicit, not silently omitted. A full registered suite is never presented as proof that every application feature is covered.

Sources: [confirmed D01-D07](../../../docs/agent/discovery/pr-sniper-vm-regression.md),
[nano authority](../../../docs/agent/specs/pr-sniper-vm-regression.nano.md).
These clauses are stored requirements, not operational permission.
