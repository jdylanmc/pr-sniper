# Give reviews three host-owned tools, not a sandbox

## Context

Review reads untrusted PR content. Letting the model clone or execute repo code is unsafe.

## Decision

- Host reads metadata and immutable trees through the bound account.
- Copilot gets exactly 3 host-owned tools: `read_changes`, `read_source`, `search_paths` (`src-tauri/src/review/runtime.rs`). Runtime tool catalog must equal the allowlist, or the review is blocked.
- Lossless paging. The Copilot runtime redirects tool output over ~50 KB to a file the model cannot read, so a large response reached the model truncated. Every response is now ≤ 32,000 serialized bytes (`MAX_TOOL_BYTES`) and the session pins the runtime threshold to 256 KiB.
- `read_changes {path, part: patch|before|after, offset?, max_bytes?}` and `read_source {side, path, offset?, max_bytes?}` return `{content, offset, next_offset, total_bytes, complete}`; chunks end on UTF-8 boundaries. `search_paths` pages by `offset`.
- Coverage is bytes: a changed file counts only when every byte of every part (`changed_files[].parts`) was served by `read_changes`. Partial read = retryable provider failure, no result accepted. `read_source` and searches never cover.
- A response that cannot fit the limit is a retryable provider failure for the whole review, never a model decision.
- No repo code is cloned, checked out, or executed.

## Consequences

- Allowlist drift blocks reviews. Loud, not silent.
- Process is current-user constrained. Explicitly not an OS sandbox.
- Reviews cannot run tests or build.

## Proposed change ([ADR 0021](0021-review-packet.md))

The fixed three-tool allowlist becomes a per-assignment tool set plus grant-gated exits. Allowlist drift still blocks a run. Paging and byte coverage stay. Not yet implemented.
