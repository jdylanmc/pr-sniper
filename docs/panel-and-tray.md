# Panel and tray

Reference for the tray icon, the retained panel, navigation rules and the Running/Queue/Reviewed tab behavior.

## Tray

- No main window at startup. Left-click the crosshair: one retained panel. Tabs: **Queue**, **Running**, **Reviewed**, **Settings**.
- Right-click menu (`src-tauri/src/lib.rs`): **Review Queue**, **Check Now**, **Settings**, **Close Panel**, **Quit PR Sniper** (`CmdOrCtrl+Q`).
- Hide the panel: outside click, `Escape`, panel Close, tray **Close Panel**. Native window close is intercepted; webview kept ([ADR-0018](adr/0018-retained-hidden-panel.md)). Background work continues.
- **Quit PR Sniper** ends the process.
- Launching the app again (Finder, Spotlight, Dock) opens the panel.
- Placement: work area of the clicked tray monitor. Size 408×744 logical points, clamped to small or mixed-scale screens (`src-tauri/src/panel.rs`). Set `PR_SNIPER_PANEL_DEBUG=1` to log placement.
- Windows: see [windows-development.md](windows-development.md) and [ADR-0006](adr/0006-windows-tray-startup.md).

## Navigation state

- Route, drafts, scroll, focus survive panel dismissal and destination change.
- Back returns to the originating list and row.
- Opening another PR or job replaces the single detail layer.
- Route focus restored on commit, not on animation frame.
- Deferred positioning and the initial Settings load yield to newer focus, routes, dismissal.
- Native notification and legacy Settings entry use retained routes. No webview reload.
- Unavailable exact PR, iteration or job: missing-destination view. Never another item.
- Profile-scoped PR selection survives restart (`state/queue-selection.json`). Unsaved drafts live in-process only.
- Back and Settings-dialog close restore the activating control. Saves keep the logical opener through redraw by stable resource ID. Removed resources: section heading. Removed assignment: Add control. Background completion never steals focus.
- External browser sign-in may hide the panel. Reopen keeps flow and draft.

## Chrome

- Navy header band, artwork, bottom nav. Cards overlap the band. 8px corners. Page title and back arrow in the band.
- One scroll container per view, between the header and bottom nav. An open Settings editor's body is the only scroller; the list behind it keeps its position but never scrolls.
- Monitoring control = machine-wide pause gate. Unknown automation state: control disabled, active-work badge cleared.
- Version comes from native app metadata, shown in the footer. Unavailable metadata is labelled; clicking it retries. Footer version opens the in-app Changelog: an ordinary sub-page (shared band, art, overlapping card) rendering `**bold**` and `` `code` `` as text nodes, never HTML.
- Repository intake has no local-folder selection.

## Queue tab

- Two toggle cards: **Ready for you**, **Needs attention**. Active card orange.
- **Ready for you**: machine-signed-off PRs the agent did not approve (no Approve grant); the human decides. Confirmed approval of the current head moves the PR to Reviewed (**Approved**), out of both cards. One current handoff per account/repo PR. Completed reviews stay Ready despite config drift or disabled repo. Stale only on new head/base.
- **Needs attention**: every parked failure. Cause chip: Credentials, Permission, Network, Failed. Row actions **Fix** (credentials, opens the failing account) and **Retry**. **Retry all** pill: icon + count of rows it re-arms, shown from one; skips permanent, panic, withdrawn-publication and provider-action failures; single-flight. Paused: re-armed rows leave the list and the pill shows clock + count until resume.
- Queue item detail: status block (PR title, short state label, Fix / Retry / Open) plus collapsed **Details**.
- Approved (current head), merged or closed PRs move to Reviewed.
- States, evidence, links: [review-queue.md](review-queue.md).

## Running tab

- Native shared-capacity jobs: active, queued, stopping. Counts jobs, not triggers. Assigned Agents beyond capacity wait.
- Disabled-repository and superseded entries stay out of Running and waiting counts; saved evidence stays. A live worker remains visible until teardown releases its slot.
- Active rows first, then waiting in arrival order with reason. Only active rows animate; static under reduced motion.
- Row opens the exact job. No second PR drill-in.
- Job inspector: purpose (normal, primary-final, reply, mention); immutable execution config vs current planned config; canonical iteration; per-Agent work ordinals; retry attempts. Missing legacy evidence shows "unavailable"; never rebuilt from Settings.
- Job detail shows its own capacity/execution state above facts. Never a sibling's or PR-wide state. Never-started failures keep planned config, no execution claim.
- Full config and IDs sit in collapsed evidence.
- Conversation inspectors keep analysis, cancellation and reply publication separate. Failed/backoff analysis is not a user stop. Completed analysis is not reply confirmation. Unknown publication outcome keeps reconciliation controls; never shows Done.
- Unrelated-job polls keep the open inspector. Own updates keep focus and disclosures.

## Reviewed tab

- Finished and merged/closed PR results. Paging: [active-results.md](active-results.md).

## Removed screens

- No Status or Diagnostics screen (removed 2026-10-14). Queue-item failures: Queue > **Needs attention**. Parked repository polls recover at their repository's next scheduled run or tray **Check Now**. Diagnostics: Settings > Preferences **Copy diagnostics** ([diagnostics.md](diagnostics.md)).
