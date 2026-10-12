# Native host owns mutations; persist intent before effect

## Context

Approve, merge, publish, reply are irreversible provider writes. Lost responses and crashes risk duplicate or misattributed writes. Agents read untrusted PR content.

## Decision

- Only the native host mutates provider state, through one mutation owner (`actions.rs`, `publication.rs`, `follow_up.rs`). Independent of AI capacity.
- Agents get read-only host tools. Copilot credentials never publish. Approve/merge never exposed as Agent tools.
- Each effect freezes action, account, head, attribution. Intent is persisted before the request: `state/actions.json`, `state/publications.json`, `state/notifications.json`.
- Lost response or restart: reconcile the original effect by exact signed body, actor, commit, receipt. Max 3 observations in the original window.
- Missing receipt after uncertain create never authorizes another create. No resend.
- User actions: **Reconcile original action**, **Reconcile / retry publication**, **Reconcile / retry reply**. Fresh budget, original identity kept.
- Notification send intent is saved before the OS request. Lost callback = Outcome unknown, never resent.

## Consequences

- Unresolved state stays visible until proven.
- Merge seen after lost response is terminal state, not PR Sniper attribution.
- Pause/cancel cannot undo an accepted write.
- Stuck uncertain state needs a human. Avoids duplicate comments.
- Foreign pending reviews never touched.
