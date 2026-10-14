# Bound diagnostics and keep them content-free

## Context

Diagnostics help operators but can leak PR content, tokens, or grow forever.

## Decision

- Fixed-schema JSONL under `state/` (`src-tauri/src/storage/diagnostics.rs`). `attempts.jsonl` + 3 rotated generations (`attempts.1..3.jsonl`).
- Caps: 1 MiB per file (`LOG_BYTES`), 128 KiB per record, 128 paths/events, 256-byte text/metadata.
- Over-limit, control-character, or credential-like metadata becomes a SHA-256 opaque label.
- Fixed events only. No prompt, body, token, or output fields. Privacy by schema.
- HTTPS credentials go in a sensitive header. Redirects disabled.
- Logging failure is isolated. Never alters review or publication.
- Typed seam: `storage::diagnostics::{Attempt,Record,Event}`.
- Read surface (2026-10-14): Settings > Preferences **Copy diagnostics** copies host log, doctrine catalog and recorded job Intelligence as text. No Diagnostics screen.

## Consequences

- Not an archive. Old attempts rotate out.
- Cannot debug content issues from logs. Reproduce instead.
- Older docs cite 256 KiB + 1 rotation. Code wins.
