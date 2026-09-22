# Approved Settings default

Decision: **A - Sidebar** is the MVP Settings experience, approved by Dylan
McCurry on 2026-09-22:

> much better. approved. anchor onto these settings and make this the default
> experience for MVP.

This is the focused Settings UX follow-up related to #4 and #11, not a claim
that either issue's broader scope is complete. The original discovery and
specifications remain historical evidence. B and C were alternatives, not
production modes.

## Visual reference

The images in [settings-approved-a](./settings-approved-a/) preserve the approved
synthetic preview. **They are design references, not implemented behavior or
live data.** Sample repositories, people, models, connection claims, preview
storage and prototype chrome must not enter production.

- [Desktop repositories](./settings-approved-a/desktop-A.png)
- [Narrow repositories](./settings-approved-a/mobile-A.png)
- [People](./settings-approved-a/desktop-A-people.png)
- [Review defaults](./settings-approved-a/desktop-A-review-defaults.png)
- [Automation](./settings-approved-a/desktop-A-automation.png)
- [Presets](./settings-approved-a/desktop-A-presets.png)
- [Repository customization](./settings-approved-a/desktop-A-repository-customization.png)
- [Preset import](./settings-approved-a/desktop-A-import.png)

Original reference HTML SHA-256:
`4c024c0d65176132514f354825e85106a0c6892e6d87edb12a30e9f1a8ee9b94`.
The HTML is not application source; its simulated data and all-or-default
inheritance are deliberately not adopted.

## Required design and behavior

Use five sections: Repositories, People, Review defaults, Automation and Review
presets. Keep the compact sidebar, readable rows, quiet borders, system-native
type and persistent Save changes / Reset changes footer. On narrow windows,
replace the sidebar links with a labeled section picker. Keep visible keyboard
focus, scrollable content and an accessible footer.

| Token | Approved value |
| --- | --- |
| Ink | `#202b37` |
| Secondary text | `#596775` |
| Divider | `#dce2e8` |
| Accent / focus | `#225cc5` |
| Selected wash | `#edf3ff` |
| Sidebar | `#f3f5f7` |
| Canvas | `#e9edf1` |
| Type | `-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` |
| Body / secondary / heading | 14px / 12px / 26px |
| Desktop sidebar | 208px |
| Group / control radius | 9px / 7px |

Root-folder discovery scans only an explicitly chosen local root. GitHub
repositories use searchable checkboxes and stable canonical stored identities;
empty, unavailable and failed results are explicit. People use readable logins
or names resolved to stable provider identities, never ordinary numeric-ID
entry.

Use one Model dropdown with Default first. Preserve saved model and named-agent
selectors even when capability discovery is unavailable. Never claim installed
models, successful health checks or Setup Doctor execution without real evidence.

Named machine-local presets support create, edit, inert JSON import, and
global/per-repository selection. Preserve custom prompts and **per-field**
inheritance. The two independent switches are **Run reviews automatically** and
**Post review comments automatically**; both retain existing persisted values
and safe defaults.

Ordinary scheduling uses the local time zone. Existing valid cron expressions
and explicit time zones remain preserved and editable in a secondary surface.
Reviewer-assignment controls disappear from ordinary Settings only; stored
values, inheritance and eligibility semantics remain intact.

## Scope and evidence boundary

This decision authorizes Settings/discovery/preset/identity wiring in the
existing vanilla TypeScript, Vite, Tauri and local persistence architecture.
It does not authorize account-wide discovery, a new agent engine, polling or
queue implementation, approval execution, automatic enablement, or changes to
review/publication predicates. Unsupported capabilities need actionable
unavailability, not simulated success.

Browser renders and isolated persistence tests establish only their actual
coverage. Native menu, keychain, window, login-item and runtime acceptance
require separate native evidence. This requirements artifact does not assert
that implementation or native acceptance is complete.
