# Use browser Authorization Code + PKCE for GitHub accounts

## Status

Accepted

## Decision

PR Sniper signs into the existing GitHub App as a native public client through
the default system browser using Authorization Code + S256 PKCE. Every attempt
uses fresh cryptographically random state and verifier values and binds
`http://127.0.0.1:53682/oauth/github/callback` before opening the browser.
GitHub callback matching is exact unless wildcard matching is configured, and
variable loopback ports are not documented for GitHub Apps, so the fixed high
port is an explicit registration prerequisite and port conflicts are visible.
This follows GitHub's
[user authorization callback documentation](https://docs.github.com/en/apps/creating-github-apps/setting-up-a-github-app/about-the-user-authorization-callback-url),
which requires the redirect host and port to match the registered callback
unless wildcard matching is enabled.

The loopback server accepts one correlated callback, rejects mismatch and
replay, returns only generic secret-free HTML, and stops on success,
cancellation, timeout, or application shutdown. No client secret, embedded
browser, hosted relay, device code, hand-written cryptography, or token-bearing
frontend state is permitted.

The implementation uses pinned maintained libraries at their ownership
boundaries: `oauth2` for PKCE, state, authorization URL and token exchange;
`axum` for the bounded loopback HTTP route and graceful shutdown; `webbrowser`
for the platform default browser; and `url` for structured URL handling.

After token exchange, PR Sniper validates `/user` and shows only the stable
account ID and login as a pending confirmation. Access and refresh credentials
remain in memory and are not added to the account registry or macOS Keychain
until **Confirm**. **Use a different account**, cancellation, retry, and
shutdown invalidate the prior pending attempt. Existing provider-account
isolation, repository bindings, refresh rotation, and acting-identity rules
remain unchanged.

## Consequences

The existing GitHub App must register the exact callback URL before live
acceptance. Port conflict, browser-open, cancellation, timeout, network,
provider, identity, and secure-storage failures remain distinct. Automated
tests use deterministic browser, callback, and HTTP seams and never contact
live GitHub.
