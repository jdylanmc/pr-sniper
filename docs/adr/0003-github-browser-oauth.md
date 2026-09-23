# Use browser Authorization Code + PKCE for GitHub accounts

## Status

Accepted

## Decision

PR Sniper signs into the separately registered GitHub OAuth App as a native public client through
the default system browser using Authorization Code + S256 PKCE. Every attempt
uses fresh cryptographically random state and verifier values and binds
`http://127.0.0.1:53682/oauth/github/callback` before opening the browser.
GitHub documents that OAuth Apps using a loopback callback may vary the
`redirect_uri` port while retaining the registered scheme, loopback host and
path. This candidate nevertheless uses the human-configured fixed high port so
the callback prerequisite is exact and port conflicts remain visible. See
GitHub's official
[OAuth App authorization documentation](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps).

The loopback server accepts one correlated callback, rejects mismatch and
replay, returns only generic secret-free HTML, and stops on success,
cancellation, timeout, or application shutdown. No client secret, embedded
browser, hosted relay, device code, hand-written cryptography, or token-bearing
frontend state is permitted.

The implementation uses pinned maintained libraries at their ownership
boundaries: `oauth2` for PKCE, state, authorization URL and token exchange;
`axum` for the bounded loopback HTTP route and graceful shutdown; `webbrowser`
for the platform default browser; and `url` for structured URL handling.

Authorization requests the standard broad `repo` scope. GitHub does not offer a
read-only OAuth scope for arbitrary private repositories, so the consent UI
must state that this scope grants broad access to public and private
repositories. PR Sniper uses paginated `/user/repos` discovery with owner,
collaborator and organization-member affiliations and all visibility, but
configures nothing until the user explicitly selects a repository.

After token exchange, PR Sniper validates `/user` and shows only the stable
account ID and login as a pending confirmation. Access and refresh credentials
remain in memory and are not added to the account registry or macOS Keychain
until **Confirm**. **Use a different account**, cancellation, retry, and
shutdown invalidate the prior pending attempt. Repository bindings contain
provider, account and stable repository identity only; the superseded GitHub
App installation identity is migrated to an unbound repository requiring
explicit reselection. Existing provider-account isolation, refresh rotation,
and acting-identity rules remain unchanged.

## Consequences

The OAuth App is owned by `jdylanmc`, application ID `3878184`, public client
ID `Ov23lidoL3QovWyfxnA4`, callback
`http://127.0.0.1:53682/oauth/github/callback`, expiring tokens enabled, device
flow disabled and no client secret. Port conflict, browser-open, cancellation,
timeout, missing/revoked scope, organization policy, network, provider,
identity and secure-storage failures remain distinct. Automated tests use
deterministic browser, callback, HTTP and persistence seams and never contact
live GitHub.
