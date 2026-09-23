# Bind every repository to an explicit provider account

PR Sniper supports concurrent provider accounts and binds each configured repository to one stable provider/account/repository identity instead of maintaining a process-wide active account. This prevents overlapping access, account removal, refresh failure, or future Azure DevOps support from silently changing the acting identity; GitHub is implemented now while provider-neutral contracts preserve the later Azure DevOps boundary. This supersedes the GitHub credential and single-account assumptions in ADR-0001.
