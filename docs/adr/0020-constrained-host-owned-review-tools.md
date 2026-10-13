# Give reviews three host-owned tools, not a sandbox

## Context

Review reads untrusted PR content. Letting the model clone or execute repo code is unsafe.

## Decision

- Host reads metadata and immutable trees through the bound account.
- Copilot gets exactly 3 host-owned tools: `read_changes`, `read_source`, `search_paths` (`src-tauri/src/review/runtime.rs`). Runtime tool catalog must equal the allowlist, or the review is blocked.
- Lossless paging. The Copilot runtime redirects tool output over ~50 KB to a file the model cannot read, so a large response reached the model truncated. Every response is now ≤ 32,000 serialized bytes (`MAX_TOOL_BYTES`) and the session pins the runtime threshold to 256 KiB.
- `read_changes {path, part: patch|before|after, offset?, max_bytes?}` and `read_source {side, path, offset?, max_bytes?}` return `{content, offset, next_offset, total_bytes, complete}`; chunks end on UTF-8 boundaries. `search_paths` pages by `offset`.
- Coverage is bytes, recorded, never enforced: a changed file counts as read only when every byte of every part (`changed_files[].parts`) was served by `read_changes`. The count ("N of M files read") is stored on the result (`coverage`) and the attempt diagnostics. The host never rejects, fails or retries a review for unread files; the agent decides what to read and says what it did not. `read_source` and searches never count. Older `incomplete_coverage` failure records still load.
- A response that cannot fit the limit is a retryable provider failure for the whole review, never a model decision.
- No repo code is cloned, checked out, or executed.

## Consequences

- Allowlist drift blocks reviews. Loud, not silent.
- Process is current-user constrained. Explicitly not an OS sandbox.
- Reviews cannot run tests or build.

## Amended ([ADR 0021](0021-review-packet.md))

The fixed three-tool allowlist is now a per-run tool set: the assignment's granted investigation tools plus the exits the gates offer (the primary's; a teammate has `submit_report`). Registry: `src-tauri/src/review/tools.rs`; the three original definitions are byte-compatible. The runtime catalog must equal the run's tool set (`with_available_tools` plus `get_current_metadata` equality; the PreToolUse hook is a backstop), or the run is blocked. The legacy three-tool run (`search_paths`) is removed ([ADR 0021](0021-review-packet.md)). Paging stays. The byte-coverage rule above is informational. A response that cannot fit the limit is still a failure, never a partial answer.

## Amended ([ADR 0021](0021-review-packet.md))

Exit `reply_comments` and `resolve_own_threads` are requests like every exit. The host verifies and writes; the agent gains no provider tool. Owned-thread resolution is limited to roots proven by saved publication receipts.
