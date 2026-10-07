# Bind every repository to an explicit provider account

PR Sniper supports concurrent provider accounts and binds each configured repository to one stable provider/account/repository identity instead of maintaining a process-wide active account. This prevents overlapping access, account removal, refresh failure, or future Azure DevOps support from silently changing the acting identity; GitHub is implemented now while provider-neutral contracts preserve the later Azure DevOps boundary. This supersedes the GitHub credential and single-account assumptions in ADR-0001.

A connected account may explicitly select any repository that provider
authorizes for that account, including repositories owned by another user or
organization. Affiliation discovery is only a convenience; a user may enter a
known repository and bind its stable identity after direct authenticated
validation with the selected account. Discovery never configures every
accessible repository automatically. Azure DevOps must follow the same account-centric model when
implemented, while still surfacing organization, project, tenant and
authorization restrictions; this decision does not select its authentication
mechanism or activate Azure DevOps in the MVP.

Repository URL intake infers the GitHub provider and may select a sole
compatible, confirmed Git Repository connection. Multiple compatible
connections require an explicit choice; neither a last-used account nor the
process-wide GitHub CLI identity resolves ambiguity. A compact identity and
Change action expose the selected actor. Copilot AI access is a separate role,
even when its GitHub login matches. No usable repository connection offers
sign-in/reconnect; an account-state read failure offers Retry rather than
pretending the catalog is empty.

Inference ends at intake. The saved account/repository binding remains explicit
and is never replaced after disconnect, revocation or another account appearing.
Account or URL changes invalidate pending resolution. Native resolution verifies
the selected identity and repository access; current connection generation and
availability are checked again before committing. Numeric IDs remain available
inside connection details, not as primary configuration labels. Recognizing
GitHub from a PR URL does not itself admit or queue that PR.
