# Copilot accounts and Agent models

Settings has four tabs: **Integrations**, **Doctrines**, **Agents**, and
**Preferences**. There are two independent connection roles:

| Connection                | Purpose                                                                   |
| ------------------------- | ------------------------------------------------------------------------- |
| GitHub repository account | Repository access and the acting identity for future comments/publication |
| Copilot AI account        | AI credentials and that account's returned model catalog                  |

All accounts are user-selected. The same GitHub identity can be connected to
either or both roles; connecting one never connects the other. This build
does not run reviews, send prompts, publish comments, submit approvals, or
check subscriptions/seats.

## Connect and configure

1. In **Integrations > AI integration**, choose **Connect Copilot account**.
   Complete the one-time code flow in your default browser. Choose the
   intended GitHub account there; PR Sniper does not switch your terminal login.
2. Check the returned login and stable numeric account ID. Choose **Confirm
   Copilot account** to save its credentials in macOS Keychain, or cancel.
   A green check means **verified sign-in only**.
3. In **Agents**, create or edit an Agent. Explicitly choose its **AI account**,
   then choose a **Model** from the catalog returned for that account. No account
   or model is auto-selected. Claude models through Copilot are supported;
   direct Claude, Codex and Grok integrations remain unavailable.
4. Save the Agent, then **Save changes** to persist the Settings draft.
   Authentication changes are immediate and separate from the Settings draft.

**Verify sign-in** rechecks GitHub `/user`, not a subscription or inference
endpoint. Models are fetched only when editing an Agent with a connected
account or explicitly retrying its model list. Network, provider, access and
policy failures are shown separately from verified sign-in. An empty returned
catalog is described as empty, not replaced by built-in model choices.
Unavailable/disabled saved models remain visible until explicitly changed.

**Disconnect** deletes only the selected AI role's local token pair. Its
identity and dependent Agent references remain for reconnect; no Agent,
assignment or repository connection is removed or rebound. **Reconnect**
requires the same stable GitHub ID, even if its display login has changed.
Revoking the PR Sniper OAuth App on GitHub itself can affect both roles:
local disconnect deliberately does not revoke that upstream authorization.

Old Agents with a provider name such as `copilot` in their model field load
unchanged and are marked unconfigured. They need an explicit account and actual
model choice; opening or saving unrelated settings does not migrate their
selection. Existing doctrine, prompt, signature and repository assignments stay
intact.

## Native boundary

The application reuses its own secretless GitHub device-flow registration and
identity-confirmation/refresh machinery. AI sign-in requests `read:user` and
`offline_access`, not `repo`. GitHub may retain or present previously granted
permissions for the same OAuth App. New sign-in obtains credentials specifically
for this role; PR Sniper never copies credentials from `gh`, Copilot CLI, or the
repository connection.

AI credentials use the separate Keychain service
`com.jdylanmc.pr-sniper.copilot.oauth-app.v1`, keyed by provider and stable
account ID. No global active account is used. Settings stores only
`ai_account: { provider, account_id }` and the returned model ID.

The pinned official Rust SDK `github-copilot-sdk` **1.0.14** embeds its native
Copilot CLI payload at build time. Building therefore downloads the SDK's
checksum-verified runtime assets and increases the application size. A release
does not need the user's Node installation or global Copilot CLI. Runtime
extraction uses the SDK-managed versioned cache; the app explicitly selects
the bundled executable, not `COPILOT_CLI_PATH`.

Each lookup starts a fresh account-specific SDK client in a private temporary
HOME, working directory and Copilot state directory. It uses explicit token
authentication, `use_logged_in_user=false`, stdio transport, SDK empty mode
and disabled keytar. The child environment retains only explicitly supplied
safe values: ambient GitHub tokens, direct-provider keys, Node hooks, telemetry
configuration, plugins and MCP configuration are not inherited. No SDK session,
tool or inference request is created. This is isolation of credentials and
configuration, not an operating-system sandbox.

The credential travels in the SDK's child environment, never command arguments,
Settings, prompts or frontend payloads. Runtime logging and SDK tracing are
disabled; application errors use fixed stage-specific text. Model metadata is
provider data, displayed as text rather than HTML. Startup and catalog requests
are bounded (30 and 45 seconds), with cancellation and bounded shutdown
(5 seconds, then forced termination). Temporary runtime state is removed.
All slow native identity, Keychain and SDK work runs off the Tauri UI thread.

## Isolated native live check

The delivery owner coordinates this with the human. Automated fixtures and an
offline runtime handshake do **not** prove this OAuth grant's live Copilot
acceptance.

Build with `npm run bundle`. Quit an earlier PR Sniper instance before launching
the candidate. Use a new absolute data directory and a matching test-owned
Keychain namespace, retained across restarts of this check:

```sh
export PR_SNIPER_DATA_DIR="$(mktemp -d /tmp/pr-sniper-copilot-live.XXXXXX)"
export PR_SNIPER_KEYCHAIN_SERVICE="com.jdylanmc.pr-sniper.tests.copilot-live-$(uuidgen)"
"src-tauri/target/release/bundle/macos/PR Sniper.app/Contents/MacOS/pr-sniper"
```

No window opens automatically. Use the menu-bar crosshair to open Settings.
Complete the connection and Agent steps above with human-selected identities.
Check cancel and identity confirmation, two accounts if available, explicit
model choices, local disconnect, retained blocked Agents, reconnect, and
restart using the same two environment values. Keep any repository connection
unchanged. Record a model-access failure as a separate live result; do not
reinterpret it as a seat claim or send a prompt to investigate.

No repository actions or real review execution belong to this check. Quit
through the tray when finished. Disconnect test accounts in the candidate
before removing the explicitly recorded test data; never clear production
Keychain entries or change global CLI authentication.

## Evidence boundaries

`tests/settings/copilot.spec.mjs` drives the production UI and real Rust
Settings storage, using synthetic auth/model IPC responses. Native tests cover
device scopes, flow/identity isolation, credential deletion, Keychain restore,
SDK transport isolation, cancellation, and process cleanup. The explicit
ignored `bundled_runtime_handshakes_offline_without_credentials` test exercises
the actual embedded runtime without credentials or inference. Browser login,
real grant acceptance, enterprise policies and native WebKit remain separate
interactive acceptance.

Official integration contracts:
[application OAuth](https://github.com/github/copilot-sdk/blob/v1.0.14/docs/setup/github-oauth.md),
[Rust SDK](https://github.com/github/copilot-sdk/tree/v1.0.14/rust),
[authentication](https://github.com/github/copilot-sdk/blob/v1.0.14/docs/auth/authenticate.md).
