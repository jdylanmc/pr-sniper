# Stored approved intent

Verbatim clauses from the human-merged specification (PR #94, merge
dbb11b6acd197fbde484090cbd4f149cf5967ab6), not a new confirmation:

- D01 - Dylan: PR Sniper gets a dedicated VM. Routine app interaction happens inside it, never on the host desktop, even when setup is unavailable.
- D02 - Dylan: Local runtime lives at the canonical PR Sniper repository root in .regression-suite/, gitignored and shared by the repository's feature worktrees.
- D03 - Dylan: setup-regression-suite-mac guides missing or incomplete setup. Passwords, OS permissions and resource/security choices stay user-controlled.
- D06 - Dylan: Skills, test sources, registry and reconstruction guide stay in Git. VM disks, credentials and generated evidence stay ignored. Existing unit, build and browser checks remain.
- AC-003: `setup-regression-suite-mac` guides the user through absent or unusable setup and reports verified readiness separately from a regression result. Passwords, OS permissions and resource/security choices remain user-controlled. It does not silently provision, delete, replace or take over existing VMs, including Notch's environment; required resource/licensing choices are resolved before provisioning.

Sources: [confirmed D01-D07](../../../docs/agent/discovery/pr-sniper-vm-regression.md),
[nano authority](../../../docs/agent/specs/pr-sniper-vm-regression.nano.md).
These clauses are stored requirements, not operational permission.
