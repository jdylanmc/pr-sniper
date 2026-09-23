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
