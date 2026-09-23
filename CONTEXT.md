# PR Sniper

PR Sniper monitors configured provider repositories and prepares review work while keeping provider identity, repository access, review execution, and publication authority explicit.

## Language

**Provider Account**:
A stable user identity within one source-control provider. Provider name and provider-issued account ID together form its identity.
_Avoid_: Active account, current account

**Repository Account Binding**:
The explicit association between one configured repository and the provider account used for every provider action on that repository.
_Avoid_: Default account, inferred account

**Acting Identity**:
The provider account visibly named at the point where PR Sniper performs or proposes a repository action.
_Avoid_: Logged-in user

**Provider Repository Identity**:
The provider-issued stable repository ID, optionally qualified by a provider-specific installation identity.
_Avoid_: Repository name

**Needs Attention**:
A repository binding or provider account that cannot currently authorize reads and requires an explicit reconnect or rebind.
_Avoid_: Disconnected repository

**OAuth Login Attempt**:
A single bounded browser authorization operation with fresh PKCE verifier and state, one localhost callback, cancellation, and timeout.
_Avoid_: Login session

**Loopback Callback**:
The fixed localhost-only HTTP endpoint that receives one correlated GitHub authorization response and returns no provider secrets.
_Avoid_: Local redirect server

**Pending Account Confirmation**:
A validated stable GitHub account ID and login whose credentials remain only in memory until the user explicitly confirms.
_Avoid_: Connected account
