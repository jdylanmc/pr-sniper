# Share one AI capacity pool; keep provider writes serial

## Context

Normal reviews, owned-thread analysis, primary-final reviews, replies, and mentions all spend AI slots. Per-kind pools starve or overrun the machine.

## Decision

- One machine-wide capacity, default 4.
- All adapters merge into one canonical FIFO and reserve through `capacity::Coordinator::reserve` (`src-tauri/src/capacity.rs`).
- Persist work before it competes for a slot.
- Publication and reply-publication coordinators stay serial. Separate from AI slots.
- Backpressure: a rate limit gates a provider account (GitHub account, Copilot account) in `backpressure.json` until its reset (`src-tauri/src/backpressure.rs`). `Work.delayed_until` holds runnable work back; `reserve` skips it. Other accounts keep flowing. In-flight 429: the attempt is handed back, no budget spent. Polling skips a gated account; a rate-limited poll waits, sets no failure. A success clears the gate. Gates persist across restart. UI: quiet clock + local time, never an error.
- Pause automation (`automation.json`) signals workers. Slots stay _stopping_ until teardown completes. Capacity reduction drains the same way.

## Consequences

- Raising capacity never raises write concurrency.
- Teardown is visible and occupies its slot.
- Storage failure leaves a blocked stopping slot until repaired + restart.
- Pause cannot undo an accepted remote request.
