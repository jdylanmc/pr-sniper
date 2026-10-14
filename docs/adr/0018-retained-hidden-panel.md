# Retain the tray panel hidden; never destroy it

## Context

The tray panel holds drafts, routes, scroll, focus. Destroying it on dismiss loses them and costs a reload. Extends ADR 0006.

## Decision

- No main window at startup. Click the tray crosshair to show one retained webview (`src-tauri/src/panel.rs`).
- Outside click, Escape, Close button, tray **Close Panel** all hide the same panel.
- Close Panel is the native window close, intercepted (`prevent_close` in `lib.rs`).
- Quit is the only exit. Repeated Quit starts one shutdown.
- Native entry points navigate retained routes. No webview reload.
- Deferred work (positioning, Settings load) yields to newer navigation.
- Every show recomputes placement from current displays, preferring the clicked
  tray display; stale/missing geometry falls back to an available display.
- The full panel fits the chosen work area, shrinking on small displays.
- macOS reads the native status-item frame and current `NSScreen.visibleFrame`
  in AppKit points and synchronously sets the native panel frame. It never
  reuses a retained window's screen or converts through a primary-display height.
- Windows uses current physical tray/display work-area coordinates.
- Background scanning continues while hidden.

## Consequences

- Drafts, scroll, focus survive dismiss.
- Drafts live in-process only. Lost on quit.
- Hidden webview keeps memory resident.
- Startup registration is inspect-only: never enable, disable, or reapply saved preference. Dev data-dir override never touches login items.
