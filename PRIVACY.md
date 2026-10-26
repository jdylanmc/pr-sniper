# Privacy statement

Effective date: 2026-10-26

## Summary

PR Sniper is a desktop app that runs on your computer. It has no server, account
system or telemetry of its own. It talks directly to the services you connect,
and keeps everything else on your device.

## What PR Sniper accesses

- Names, IDs and owners of repositories your connected account can access, so
  you can choose which ones to add. Only repositories you add are monitored.
- Pull requests in repositories you add: metadata, diffs, repository files,
  comments, review threads, CI status and CI logs.
- Your account identity (login and ID) for each account you connect.
- Sign-in tokens issued when you connect an account.

## Where data goes

- **GitHub.** Sign-in uses the GitHub device flow. PR Sniper reads pull request
  data from the GitHub API and, only when your settings allow it, posts comments
  and replies or approves and merges. CI logs are fetched from links GitHub
  provides.
- **GitHub Copilot.** Reviews run through the bundled Copilot runtime with your
  Copilot account. PR content (title, description, diffs, comments), any
  repository files, CI status and CI logs a review agent reads, and your agent
  instructions are sent to GitHub Copilot and the AI model provider
  behind it. Their terms and privacy statements apply. PR Sniper starts the
  runtime with its telemetry and auto-update turned off.
- **Azure DevOps via Azure CLI.** When you connect Azure DevOps, PR Sniper
  asks the Azure CLI on your computer for short-lived Azure DevOps access
  tokens. You sign in to Azure CLI with Microsoft Entra ID; Azure CLI keeps
  its own sign-in data in its configuration folder. PR Sniper keeps these
  tokens in memory only and stores no Azure DevOps secret. It runs Azure CLI
  with telemetry and auto-upgrade turned off and never changes your Azure CLI
  configuration. To find your organization's Microsoft Entra tenant, PR Sniper
  sends an unauthenticated request to `dev.azure.com/<organization>`. It then
  reads pull request data from Azure DevOps and, only when your settings allow
  it, writes to it, as it does for GitHub. Microsoft's terms and privacy
  statement apply.
- **Connectivity checks.** Unauthenticated requests to `api.github.com` and
  `api.githubcopilot.com` to see whether they are reachable.

## What stays on your device

| Data                                     | Where                                                                                                                |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Sign-in tokens                           | macOS Keychain; Windows Credential Manager. Never in files.                                                          |
| Azure DevOps tokens                      | Memory only. Azure CLI keeps its own sign-in data.                                                                   |
| Settings, queue, reviews, notifications  | macOS: `~/Library/Application Support/com.jdylanmc.pr-sniper/`; Windows: `%LOCALAPPDATA%\com.jdylanmc.pr-sniper\`    |
| Diagnostics logs (content-free)          | `state/` in the folder above. Shared only if you copy them yourself.                                                 |
| Copilot runtime                          | Your user cache folder, under `github-copilot-sdk`                                                                   |
| Local control server (Copilot CLI / MCP) | `127.0.0.1` only, with a per-launch token; entry in `.copilot/mcp-config.json` in your home folder if you install it |

## What the author receives

Nothing. PR Sniper contains no analytics, telemetry, crash reporting or update
checks. Updates come from Homebrew or GitHub Releases, which you run yourself.

## Removing your data

1. Disconnect each account in Settings. This deletes its stored tokens.
2. Revoke the app's access in your GitHub account settings. For Azure DevOps,
   sign out of Azure CLI (`az logout`) if you no longer use it.
3. Turn off launch at login, then quit PR Sniper.
4. Delete the data folder listed above and the `github-copilot-sdk` cache
   folder. Remove the `pr-sniper` entry from `.copilot/mcp-config.json` in
   your home folder if you installed it.

## Contact

Open an issue at
[github.com/jdylanmc/pr-sniper/issues](https://github.com/jdylanmc/pr-sniper/issues).
Issues are public; do not include tokens or private code.
