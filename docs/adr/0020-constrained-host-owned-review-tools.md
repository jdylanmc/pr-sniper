# Give reviews three host-owned tools, not a sandbox

## Context

Review reads untrusted PR content. Letting the model clone or execute repo code is unsafe.

## Decision

- Host reads metadata and immutable trees through the bound account.
- Copilot gets exactly 3 host-owned tools: `read_changes`, `read_source`, `search_paths` (`src-tauri/src/review/runtime.rs`). Runtime tool catalog must equal the allowlist, or the review is blocked.
- 1 MiB cap per tool response. Over cap = visible failure.
- No repo code is cloned, checked out, or executed.

## Consequences

- Allowlist drift blocks reviews. Loud, not silent.
- Process is current-user constrained. Explicitly not an OS sandbox.
- Reviews cannot run tests or build.
