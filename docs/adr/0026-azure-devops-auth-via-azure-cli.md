# 0026 — Azure DevOps auth delegated to Azure CLI token source

- Scope: #243 (FR-10–14), #246.
- Decision: PR Sniper gets Azure DevOps bearer tokens from the user's Azure CLI.
  It registers no app, stores no Azure DevOps secret and never holds a
  refresh token.
- Supersedes: the own Microsoft Entra app direction explored in #6.

## Compared routes

| Route                       | Evidence / consequence                                                                                                          |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Own Entra app (PKCE)        | Works technically; organizations that require admin consent block an unverified multitenant app. Admin consent is out of scope. |
| Legacy Azure DevOps OAuth   | Closed to new registrations; retiring.                                                                                          |
| Git Credential Manager      | First-party token limited to code and packaging scopes; no identity search. Dropped.                                            |
| Personal access token (PAT) | User-managed long-lived secret PR Sniper would have to store. Not in 0.6.0.                                                     |
| Azure CLI                   | Pre-authorized first-party client; delegated token with full Azure DevOps capability; silent refresh by Azure CLI. Selected.    |

## Contract

- `TokenSource` per Microsoft Entra tenant: `token(tenant)`, `invalidate(tenant, rejected)`.
  Several organizations can share one tenant.
- `AzCliTokenSource` runs
  `az account get-access-token --resource 499b84ac-1321-427f-aa17-267ca6975798 --tenant <tenant> --output json`.
  Expiry uses POSIX `expires_on` only; minimum Azure CLI 2.54.0 (`az version`), else `AzTooOld`.
- Tokens stay in memory, zeroized on drop, redacted in `Debug`. Refresh at
  `expires_on − 5 min`. One Azure CLI run in flight per tenant. Failures are
  not cached.
- A 401 invalidates only the rejected token, then retries once (`with_token`).
- Organization → tenant: unauthenticated `HEAD https://dev.azure.com/{org}`,
  no redirects, `X-VSS-ResourceTenant`. Found, 404, missing and all-zero
  tenant are distinct; the organization name is validated before any request.
- Shared bounded CLI runner (`cli_process`): absolute program, closed stdin,
  per-stream output cap, timeout that kills only its own child, per-tool
  environment policy. Never logs output.
- Azure CLI environment, per child only: remove `COPILOT_AGENT_SESSION_ID`
  (agent mode bypasses the token cache and falls back to an interactive
  browser sign-in); `AZURE_CORE_COLLECT_TELEMETRY=false` (no detached
  uploader); `AZURE_CORE_ONLY_SHOW_ERRORS=true`; `AZURE_AUTO-UPGRADE_ENABLE=no`;
  `AZURE_CORE_NO_COLOR=true`. The user's Azure CLI configuration is never written.
- Classification happens once, at the process boundary, into `TokenError`:
  `AzMissing`, `AzTooOld`, `NotLoggedIn`, `TenantNotFound`, `NoAccountInTenant`,
  `TenantAccessBlocked`, `InteractionRequired { aadsts }`, `Network`, `Timeout`,
  `MalformedOutput`, `Other { exit_code, aadsts }`. Azure CLI exits 1 for all
  authentication failures and reports some as tracebacks, so only stable
  markers are read: Microsoft Entra `AADSTS` codes and fixed sign-in/network
  hints. Unknown output is `Other`. No Azure CLI text leaves the classifier.

## Consequences

- Azure CLI owns the refresh token and its own cache in its configuration
  folder; sign-out and revocation follow Azure CLI and Microsoft Entra.
- Azure CLI must be installed and signed in. Background acquisition is never
  interactive; sign-in and install are user-initiated (#247).
- Windows: Azure CLI is `az.cmd`, found in absolute PATH entries, then
  `%ProgramFiles%\Microsoft SDKs\Azure\CLI2\wbin` and its `%ProgramFiles(x86)%`
  twin, so a winget install from Settings is found although the running app's
  PATH is stale. cmd.exe re-parses batch command lines, so `cli_process` only
  passes a `.cmd`/`.bat` program arguments of `[A-Za-z0-9._/-]` (fixed literals
  and validated GUIDs), on top of Rust's batch argument escaping.
