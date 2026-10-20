# 0025 — App-owned local agentic control

- Scope: #103, macOS and Windows, Copilot CLI.
- Decision: authenticated loopback framing; native operations own all writes.
- Authority: human-approved non-login configuration and explicit PR watching.

## Compared routes

| Route                            | Evidence / consequence                                                                                                                                                                                          |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Unix socket + Windows named pipe | OS-native peer access; two transport implementations and two discovery/lifecycle paths. Existing app uses neither.                                                                                              |
| Private file request mailbox     | Reuses filesystem ACLs, but needs polling, claims, abandoned-request recovery and response cleanup in addition to app storage.                                                                                  |
| Authenticated loopback           | One framed client/server on both platforms; existing `storage/private_fs` provides Unix modes and protected current-user Windows DACLs. Requires a private capability descriptor and explicit bounds. Selected. |

## Contract

- Bind only `127.0.0.1`, ephemeral port. No HTTP, URL dispatcher or shell RPC.
- Private `control/endpoint.json`: protocol, port, per-launch random capability.
  Never included in diagnostics, command output or account enumeration.
- Eight-byte magic, four-byte length, at most 1 MiB per frame; two-second
  total request-read deadline; authenticated native operation budget 90 seconds.
- One control reader; unsupported methods, unknown fields, authentication and
  protocol mismatches fail before application dispatch.
- Store mutex serializes native validation/commit with the UI and scheduler.
  Configuration revision plus resource expected-value protects stale saves.
- Mutation UUID and timestamp reserve a durable receipt before execution.
  Repeated requests replay their original result; interrupted ones remain
  outcome-unknown. Five-minute acceptance window, maximum 256 active receipts.
- CLI timeout never implies rollback. No automatic mutation retry after bytes
  were sent. A failed watch configuration retains a persisted admission hold.
- Background launch preserves pause and panel focus. App owns scheduling;
  no CLI daemon, provider credentials, authentication operation or review engine.
- Same-user processes share the local-user trust boundary. This is not proof
  of human presence or protection against a compromised current-user process.

## Scope and review behavior

- `explicit` admission admits only native PR intake; subsequent PR revisions
  retain normal heartbeat tracking. A watch command never enables a repository.
- Eligible queue priority changes order only. Running, Wait, retry, teammate,
  capacity, account and provider gates remain authoritative.
- Explicit review-again creates a fresh pass identity and PR-state sequence;
  completed reviews and provider receipts remain untouched.
- Request overrides select existing Agent/doctrine references on never-started
  work only. Shared configuration and assignment grants remain unchanged.
- The additive SOLID/Security/Maintainer preset supplies actual prompts,
  doctrines, role assignments and zero grants. Customized components survive.

## Distribution

- `skills/pr-sniper` is the source package; native binary embeds matching assets.
- Settings installs only Copilot CLI at `~/.copilot/skills/pr-sniper`.
- Install receipts verify every managed file. Existing unowned, changed,
  linked or additional content produces a conflict, not overwrite/removal.
- Installer records the exact app executable; source-distributed skills also
  recognize standard per-user/system install locations.
- Node 22+ runs the small CLI. `/skills reload` discovers installation changes.

## Verification boundary

- Native tests exercise real Store, queue, preset, installer and framed CLI
  boundaries using synthetic profiles; browser tests use the native Store bridge.
- These checks do not establish a live provider review, native closed-app
  launch, OS notification permission, Copilot conversational compliance or
  Windows runtime acceptance. Those require the platform delivery checks.
