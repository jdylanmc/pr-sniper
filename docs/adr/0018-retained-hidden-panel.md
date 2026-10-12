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
- Background scanning continues while hidden.

## Consequences

- Drafts, scroll, focus survive dismiss.
- Drafts live in-process only. Lost on quit.
- Hidden webview keeps memory resident.
- Startup registration is inspect-only: never enable, disable, or reapply saved preference. Dev data-dir override never touches login items.
