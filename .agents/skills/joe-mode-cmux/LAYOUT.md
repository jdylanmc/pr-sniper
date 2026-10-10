# Joe-mode CMUX layout

Use one existing repository workspace. Never create a window or a second
workspace for the same repository controller.

Git placement is separate: the Project Manager stays on `main`, Discovery
uses `discovery/<feat>`, and an explicitly authorized PR/auto-merge coordinator
uses `pr-sniper`, under [Joe's worktree contract](../joe-mode/WORKTREES.md).
Pass the role's exact worktree to native `maestro_spawn` as `cwd`. Shared CMUX pane
placement never permits a shared writing checkout.

```text
┌─────────────────────────────┬─────────────────────────────┐
│ Project Manager             │ Discovery                   │
│ managed human conversation  │ one interactive owner       │
├─────────────────────────────┼─────────────────────────────┤
│ Developers                  │ Support                     │
│ delivery surfaces as tabs   │ Shepherd/review/tests/logs  │
└─────────────────────────────┴─────────────────────────────┘
```

## Stable role presentation

| Role | Tab label | Maestro icon | Color |
| --- | --- | --- | --- |
| Project Manager | `PM · Joe Mode` | `md-meditation` (`󱅻`) | teal |
| Discovery | `Discovery` | `md-compass_outline` | blue |
| Developer | `Developer · <delivery>` | `seti-bicep` (``) | purple |
| Shepherd | `Shepherd` | `md-shield_check_outline` | green |

The glyph and color communicate role only. They grant no permissions and prove
no execution state.

## Reconciliation rules

1. Start from the exact caller workspace and surface supplied by
   `CMUX_WORKSPACE_ID` and `CMUX_SURFACE_ID`.
2. Inspect the workspace tree before changing it. Persist the exact workspace,
   pane, surface, worker, worktree, and delivery identities on Joe-mode's
   private owner board.
3. Keep the caller surface as Project Manager. Never infer ownership from a
   title, current focus, working directory, screen text, or tab order.
4. Create or move only surfaces created during this activation or already
   recorded as owned by this controller.
5. Record the focused pane before mutation and use `--focus false` for layout
   changes. Restore the Project Manager pane only when it was focused before
   reconciliation; otherwise leave the human's current focus untouched.
6. Put every developer surface in one developer pane as a tab. Separate writing
   deliveries still use separate Git worktrees. This placement, like every
   role-pane placement, requires the runtime capability below.
7. A named `Ready` terminal placeholder is optional and only for an explicit
   demonstration. Create it with
   `cmux new-surface --type terminal --workspace <workspace> --pane <pane>`
   then name its exact returned surface with
   `cmux rename-tab --surface <returned-surface> "Ready"`. Record both operations
   as placeholder ownership; it is not an agent or reserved delivery. Close only
   that recorded surface when replacing it.
8. Do not close, move, rename, or reuse an unowned surface. A conflicting
   surface blocks that part of the layout and is reported to the human.

Maestro's no-window/no-split rule governs the `maestro_spawn` call itself: it creates
the worker beside its actor. Only after [RUNTIME](RUNTIME.md)'s installed
placement capability passes may this adapter use the returned exact `surfaceId`
with CMUX `split-off` or `move-surface` inside the same workspace; otherwise
leave the surface beside its actor. Never find the worker by its display name
or move a Maestro-owned surface to another workspace/window. A new developer
joins the existing developer pane with `move-surface`, preserving its tab
stack.

The current established main lifecycle guide does not advertise that placement
extension. Same-pane tabs are therefore a valid degraded cockpit; do not patch
the installed guide or bypass its rule merely to draw the four-area layout.
Peer messaging needs no pane move or focus change.

## Workspace metadata

Preserve any existing human-set custom title and color. When neither is set, or
the human approves replacing them, use:

```sh
cmux workspace-action --workspace "$CMUX_WORKSPACE_ID" \
  --action rename --title "Joe Mode · <repository>"
cmux workspace-action --workspace "$CMUX_WORKSPACE_ID" \
  --action set-color --color Teal
```

Keep metadata concise:

- `cmux set-status joe active` for the session controller;
- separate `discovery`, `delivery`, and `review` keys only when those paths
  have a meaningful current state;
- `cmux log` for dispatches, handoffs, blockers, and verified outcomes;
- `cmux notify` only for a human decision, a material blocker, or a pull request
  ready for human review;
- `cmux set-progress` only when a real finite cohort supplies a defensible
  numerator and denominator. Never invent percentage progress from agent state.

Always pass `--workspace "$CMUX_WORKSPACE_ID"` or the recorded workspace UUID.
Workspace metadata describes observed coordination state, not successful work.
