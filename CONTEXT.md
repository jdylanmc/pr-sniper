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

**Primary Agent**:
The repository assignment responsible for eligible conversation assessments and
the final full review before permitted provider actions. A sole assignment is
primary automatically; multiple assignments have at most one explicit primary.
Selecting primary grants no capabilities and does not change original feedback
ownership.
_Avoid_: Global primary, thread owner as responder

**Assignment Capability**:
An explicit repository-scoped grant: Publish Comment for any assignment;
independent Reply Comment and Approve or Approve & Merge for the primary only.
Merge includes approval and follows confirmed current-head approval; an existing
provider vote is not fabricated as a new PR Sniper vote. These product grants
never authorize development agents to publish, approve or merge.
_Avoid_: Automation override, reusable Agent permission, merge-only grant

**Provider Repository Identity**:
The provider-issued stable repository ID bound to one explicit Provider Account.
_Avoid_: Repository name

**Accessible Repository**:
A repository returned for a connected Provider Account through its granted authorization, including repositories owned by another user or organization. Accessibility does not configure or monitor the repository; organization, project, tenant and provider policy can further restrict it.
_Avoid_: Installed repository

**Needs Attention**:
A repository binding or provider account that cannot currently authorize reads and requires an explicit reconnect or rebind.
_Avoid_: Disconnected repository

**OAuth Device Authorization Attempt**:
A single bounded GitHub device-flow operation containing a provider-issued one-time user code, a secret device code retained only in the native host, browser launch, polling, cancellation, and expiry.
_Avoid_: Login session, callback flow

**One-Time User Code**:
The short provider-issued code shown to the user for manual entry at GitHub's verified device authorization page. It is transient UI data, not the secret device code used for token polling.
_Avoid_: Device token, access code

**Pending Account Confirmation**:
A validated stable GitHub account ID and login whose credentials remain only in memory until the user explicitly confirms.
_Avoid_: Connected account
