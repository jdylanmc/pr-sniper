# Triage labels

GitHub labels represent the five canonical triage roles.

| Canonical role | GitHub label | Meaning |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | Maintainer evaluation required |
| `needs-info` | `needs-info` | Missing information or a human decision |
| `ready-for-agent` | `ready-for-agent` | Fully specified and ready for agent work |
| `ready-for-human` | `ready-for-human` | Human implementation required |
| `wontfix` | `wontfix` | Will not be actioned |

Use these exact mappings rather than inventing another readiness vocabulary.
Only fully specified work without unresolved human-owned scope decisions
receives `ready-for-agent`. Dependencies, ownership and live blockers still
govern dispatch. Preserve unrelated labels and provider state.

For an information-blocked item, record the exact missing answer and use
`needs-info`; do not dispatch it while that blocker remains. Human publication
and workflow-specific mutation gates still apply.
