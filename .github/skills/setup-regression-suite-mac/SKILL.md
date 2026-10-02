---
name: setup-regression-suite-mac
description: "Guide explicitly requested PR Sniper Mac regression setup. Use only with an interactive user's consent; missing setup alone is not activation. Not a regression run."
user-invocable: true
disable-model-invocation: true
---

# Prepare the dedicated Mac environment

Read [stored intent](intent.md), root [AGENTS.md](../../../AGENTS.md) and the
[preparation and reconstruction guide](../../../docs/agents/vm-regression.md).
This is setup-independent preparation, not a finalized or proven native package.

1. Establish an interactive user's explicit setup consent. If unavailable,
   declined or unattended, return **BLOCKED / unexecuted** with pending choices.
   Do not prompt through an unattended worker or silently provision.
2. Resolve the canonical primary repository with the read-only preparation
   command. An absent, inaccessible, linked or incomplete `.regression-suite/`
   is not ready. Never choose the feature worktree as a new VM home.
3. Use the guide's human-owned decision checklist: resource/licensing capacity,
   pinned tool/image provenance, dedicated guest identity and ownership, accounts,
   effective GUI/idle/privacy readiness, candidate isolation and reset. No VM
   operations are implemented by this preparation package. Any future operations
   need separately approved tooling and user choices.
4. Preserve both existing Notch environments. Do not inspect their private
   directories or credentials, delete them or claim them for PR Sniper. Passwords,
   permission/security/network choices and resource budgets belong to the user.
5. Return observed readiness separately from planned steps. Directory presence,
   an installed VM tool or a booted guest is not evidence of an unlocked GUI or
   working Tauri driver. Today's native driver remains **BLOCKED** even if local
   instructions exist. Do not run regressions implicitly.

Return canonical location, observed missing/incomplete/busy state, decisions
needed, verified versus unverified prerequisites, and ownership/cleanup limits.
Never automate OS consent, alter the host desktop, migrate personal profiles,
change login items, access real providers or take a host-UI fallback.
