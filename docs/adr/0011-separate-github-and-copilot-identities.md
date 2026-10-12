# Keep GitHub and Copilot identities separate

## Context

Repository access and publication use one GitHub grant. AI inference uses another Copilot grant. One shared sign-in blurs who acts and who infers. Builds on ADR 0002 and 0003.

## Decision

- Two independent native sign-in flows: **Add GitHub account**, **Connect Copilot account**.
- Tokens live in separate account-addressed Keychain services. Never in config, state, or diagnostics.
- Never start sign-in or choose a model automatically.
- Identity saved only on completed browser sign-in. Cancel assigns nothing.
- Connect never assigns repos, model, or automation. Assignment is an explicit step.
- No fallback account. Session or credential loss clears cache and blocks native Save until fixed.
- Green Copilot check = sign-in only. Not model access.
- Disconnect forgets the account for clean reconnect.

## Consequences

- Two account lists, one extra setup step.
- Acting identity never changes silently.
- Direct Claude/Codex/Grok stay disabled. Azure DevOps/Bitbucket "Coming soon".
