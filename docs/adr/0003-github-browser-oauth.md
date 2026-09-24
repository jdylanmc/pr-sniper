# Use GitHub device authorization for OAuth accounts

## Status

Accepted for #35. This supersedes this ADR's earlier authorization-code +
PKCE localhost proof after live provider evidence disproved its secretless
token-exchange assumption.

## Decision

PR Sniper signs into the separately registered GitHub OAuth App through
GitHub's RFC 8628 device authorization flow. The native host requests a device
grant with public client ID `Ov23lidoL3QovWyfxnA4`, `repo` and
`offline_access`, opens the provider-returned GitHub verification URI in the
default system browser, shows the provider-issued one-time user code with copy
and manual-entry guidance, and polls within the provider's interval and expiry.
There is no client secret, callback listener, embedded browser or hosted token
exchange.

The secret device code remains only inside the native OAuth boundary. It never
enters frontend state, persistence, logs, clipboard data or diagnostics. The
verification URI must be HTTPS on `github.com`. PR Sniper uses
`verification_uri_complete` only when GitHub actually supplies it and the same
destination validation succeeds; it never invents a query parameter to prefill
the user code.

The implementation uses pinned maintained libraries at their ownership
boundaries: `oauth2` 5.0.0 for device grant construction, RFC polling,
provider interval and slow-down behavior, token parsing and refresh;
`reqwest` for bounded native HTTPS transport; `webbrowser` for the platform
default browser; and `url` for destination validation.

GitHub can return OAuth error envelopes with HTTP 200. The pinned `oauth2`
endpoint parser treats HTTP 200 as a success response before deserialization,
so PR Sniper narrowly recognizes a JSON `error` envelope at the GitHub
transport boundary, changes only its status classification for the SDK, and
retains the original body for the SDK's typed error parser. This applies to
device initiation, polling and refresh. Malformed bodies remain invalid
responses. Diagnostics emit only allow-listed stage/reason categories and
field-presence booleans, never raw bodies, URLs, codes or tokens.

Authorization requests the standard broad `repo` scope. GitHub does not offer
a read-only OAuth scope for arbitrary private repositories, so the consent UI
states that this grants broad access to public and private repositories.
Repository discovery remains a convenience and repository selection remains
explicit.

After device polling returns a token pair, PR Sniper validates `/user` and
shows only the stable account ID and login as a pending confirmation. Access
and refresh credentials remain in memory and are not added to the account
registry or macOS Keychain until **Confirm**. Cancellation, replacement,
shutdown, denial and expiry clear the attempt's transient user/device values;
stale completions cannot update a replacement attempt.

## Evidence and superseded assumption

The earlier #35 proof used Authorization Code + S256 PKCE with a fixed loopback
callback and no client secret. Live attempts proved callback and state
correlation worked, then GitHub returned HTTP 200 with
`incorrect_client_credentials` and no token. GitHub's authorization-code token
contract requires `client_secret`; PKCE does not remove that requirement.
Green fixture tests that returned a successful token without exercising the
live provider contract were insufficient evidence.

GitHub documents that device authorization requests and refreshes of
device-issued tokens do not require a client secret. Live preflight also
confirmed device initiation returns a five-second interval and that an
unauthorized poll returns an HTTP-200 `authorization_pending` envelope.

## Consequences

The OAuth App is owned by `jdylanmc`, application ID `3878184`, public client
ID `Ov23lidoL3QovWyfxnA4`, expiring tokens and device flow enabled, and no
client secret. Browser-open, cancellation, denial, expiry,
disabled-registration, network, provider,
identity and secure-storage failures remain distinct. Automated tests use
deterministic HTTP, browser, time/sleep and persistence seams and never contact
live GitHub.

Issue #36 remains open and MVP-required product work, but device authorization
does not consume an application callback. Its earlier GitHub-authentication
rationale is superseded rather than silently treated as delivered.
