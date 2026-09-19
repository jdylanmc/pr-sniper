# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Canonical role            | Value in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

Joe-mode selects work using the mapped `ready-for-agent` role; it does not substitute a new readiness checklist. Only fully specified work without unresolved human-owned scope decisions receives that role; dependency availability and ownership still govern dispatch. Record the provider representation: GitHub labels, Azure DevOps tags (normally `System.Tags`), or the agreed local status value. Role changes preserve unrelated labels/tags and do not silently change provider workflow states.

Edit the right-hand column to match whatever vocabulary you actually use.
