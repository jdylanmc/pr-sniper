# Diagnostics

Reference ([ADR-0019](adr/0019-bounded-content-free-diagnostics.md)) for retained host logs, attempt records, caps and what is never recorded.

## Files and limits

- `state/diagnostics.jsonl`: timestamped, fixed-schema host events. 256 KiB plus one rotated file (`diagnostics.previous.jsonl`). Settings > Preferences **Copy diagnostics** puts the content-free export on the clipboard. No filesystem or shell access.
- `state/attempts.jsonl`, rotated to `attempts.1.jsonl` .. `attempts.3.jsonl`. 1 MiB each (`src-tauri/src/storage/diagnostics.rs`). The export reads retained generations.
- Record cap 128 KiB. 128 paths per collection, with full and omitted counts. Oldest generations expire. Not an archive.
- Metadata over 256 serialized bytes, control characters or credential patterns become opaque SHA-256 labels. Session summaries keep the first 128 events plus errors, abort, idle.

## Record fields

- Attempt UUID, operation/work correlation, PR `number`, immutable head, model, runtime/session identity when available.
- Runtime records: monotonic elapsed ms + sequence. Host retry decisions: wall-clock timestamp; runtime sequence and elapsed explicitly absent.
- Follow the operation ID across fresh attempts. Follow the work ID across manual replacements.
- Native attempt starts link the previous retained attempt and operation for the same work. No link is invented after retention expires.
- Conversation retry decisions record acknowledged durable outcomes, failed analyses included, independent of the caller's error. After teardown only the same operation's current or retained state is used, never a replacement's counters.

## Tool evidence

- Records requested known paths, unknown-path count, returned paths, and files whose every part was fully served by `read_changes` (byte coverage). `read_source` and searches do not satisfy coverage.
- Byte counts = serialized JSON payload, not RPC framing. Failed or rejected reads = unavailable.
- Response-limit flag = app limit (32,000 bytes per tool response). Separate from runtime tool failure and conversation truncation. Per-tool runtime truncation is not exposed by the pinned SDK. Stop-hook reasons are allowlisted, else `not_exposed`.

## Never recorded

Raw arguments, search queries, prompts, source or comment bodies, transcripts, provider errors, credentials. Logging failure surfaces separately; it never turns a failed review into success or alters a publication decision.

## Publication and Rubber Duck

- Publication records correlate to the originating review attempt. `publication_attempt` and `publication_retry` carry the separate publication operation; they never masquerade as an AI retry.
- GitHub producers record actual ownership lookup, create, pending/submitted verification, submit and discard results, including uncertain mutations. Mismatches identify only the field; ownership is verified separately from comment equality. Provider errors retain typed connection categories, status and retry hints where supplied, never provider text.
- Publication durations measure the observed local call, including transport/verification, not inferred remote processing time. Host timestamps order retry decisions; absent runtime sequence/timing remains unavailable.
- Each nested Duck event carries its ordinal, model, session/runtime identity when observed and the primary attempt correlation. Nested read/exit tools use the same response-limit, runtime-error and bounded session summaries as the primary.
- Duck summaries retain only verdict/failure enums, session identity and measured session-call duration. Refused calls have no session duration. `duck_teardown` distinguishes abort from session disconnect; neither claims client-process shutdown.
- `duck_client_teardown` separately records the Duck engine's five-second serial-drain observation and actual client shutdown outcome under the primary attempt. `drain: failed` means the wait expired, not that the call settled; `forced_stop_unverified` records the existing force-stop fallback, not proof of a leaked process. Primary `teardown` and per-session `duck_teardown` remain separate.
- Duck prompts, claims, proposed/revised bodies and verdict rationale never enter diagnostics. A second nested Duck layer is rejected from retained evidence. Existing file, record, path and metadata caps still apply.
- Runtime per-tool truncation and stop reasons remain explicitly unavailable where not exposed. This producer integration does not implement connectivity or inactivity policy: #149 and #152 own their decisions and require combined integration checks.

## Code seam

- `storage::diagnostics::{Attempt, Record, Event}`, `Store::record_attempt`, `Store::record_attempt_decision` (correlates with the latest retained attempt; reports missing evidence).
- `Event::Publication`: stage, ownership outcome, mismatch field, typed failure category and observed duration. `PublicationProviderError` preserves the closed `ConnectionError` type.
- `Watchdog`, `Connectivity`, `Retry` record producer decisions, not policy. Seam tests do not prove policy.
- Review watchdog producers distinguish `inactivity_timeout` (idle and total milliseconds) from `total_duration_timeout` (total milliseconds). Saved inactivity defaults to 15 minutes; it does not replace the total budget. Expiry retires the attempt before bounded abort/shutdown; teardown failures remain explicit. Late callbacks cannot refresh a replacement attempt.
