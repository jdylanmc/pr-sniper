# Hand each agent one versioned review packet

Status: proposed.

## Context

Today the agent prompt is one JSON string built in `src-tauri/src/review/runtime.rs` (`prompt()`): task line, SHAs, title, review lens, changed-file list, result schema.

- No description, author, state, CI, labels, commit list, or full conversation.
- No statement of which actions are permitted.
- Decision is a result field (`machine_sign_off` or `human_input_required`), not a tool choice.
- Untrusted content is fenced only in the feedback section.
- Nothing stores what the agent saw.
- A tool-output truncation incident showed that completeness cannot be proven without a manifest.

## Decision

- Every agent run gets exactly one **review packet**: a versioned, labeled, deterministic document. See [Review packet](../review-packet.md).
- Sections in fixed order: Meta, Rules, Pull request, Conversation, Files, Directive, Output.
- Rules are fixed caveman text. Untrusted content is fenced.
- File content is paged. A manifest lists every part with bytes and hash. Host checks every byte served.
- The agent has **no shell, no `az`/`gh`, no network**. Its only hands are PR Sniper tools: thin hooks into deterministic host code.
- Two classes from one registry. **Investigation** tools are read-only, always listed, bounded (`read_changes`, `read_source`, `search_paths`, `search_code`, `list_tree`, `get_thread`, `get_checks`, `rubber_duck`). **Action** tools are grant-gated (`comment_on_pr`, `reply_to_thread`, `approve_pr`, `merge_pr`, `notify_ready`, `request_human`).
- Host validates every call, applies grants, saves intent before effect, audits it. Denied call = failed run.
- Adding a tool = adding a registry entry.
- Packet is stored with the run, minus file content, with `packet_hash`.
- Kinds: `review` and `reply`, one envelope.

## Consequences

- Reviews are reproducible and auditable. UI can show the exact packet.
- Larger context per run. CI and conversation data add cost and latency.
- Result schema moves from prompt JSON to tool arguments. Needs a `schema` version and a migration for stored results.
- Extends [ADR 0020](0020-constrained-host-owned-review-tools.md): the 3-tool allowlist becomes a registry. Allowlist drift still blocks the run.
- Providers writes stay in host code: the model can never post, approve or merge by itself.
- Keeps [ADR 0008](0008-one-review-per-head-sha.md) and [ADR 0017](0017-opt-in-provider-actions.md): grants gate which terminal tools exist.
- Delivered in phases; early phases change content, not behavior.
