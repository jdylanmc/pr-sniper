# Reuse account records behind native Windows credentials

## Scope

The existing-application Windows slice (#58) adds protected credentials and
ports the constrained Copilot process boundary. It does not add an account
selector, provider, model, tool, or publication capability. Full Windows app
build/GUI acceptance still depends on the tray/startup and notification slices.

## Credential records

`NativeCredentialStore` selects macOS Keychain or Windows Credential Manager.
Both use the same production binary token-pair/registry codec and
`RotationSafeStore`. The extraction preserves the established macOS record
bytes, pending-addition/deletion journal, and legacy cleanup/retry behavior.
Only the macOS backend imports legacy Keychain records; Windows has no
superseded GitHub App store and does not claim to migrate one.

Windows uses `CredWriteW`, `CredReadW`, and `CredDeleteW` with generic,
current-user credentials persisted on the local machine. App-owned target
names encode the service and record keys as hexadecimal bytes because native
target matching is case-insensitive. Case-distinct IDs must not alias. GitHub
repository and Copilot roles retain separate service namespaces and explicit
provider/account keys, with no application-wide active account.

Each native write replaces one complete serialized record. Credential
Manager's **2,560-byte record limit** includes the codec overhead, not just
token text. Oversized token pairs or registries return `StoreError::TooLarge`
before a native write, emit only a fixed capacity category, and reach the
existing secure-save failure UI. No truncation, plaintext overflow file,
partial token-pair write, or ambient credential fallback is permitted. Account
capacity depends on ID/login lengths; the application does not promise an
unbounded number of accounts. Registry-capacity and failed-rotation tests
verify preservation of prior records and absence of newly orphaned secrets.
For an existing account reconnect, the complete prospective registry is
serialized and validated against the native capacity before replacing its
token pair. A longer login cannot commit new credentials and then fail the
metadata capacity check. This preflight changes neither the established wire
format nor the pair-before-registry write order or new-account journal.

Native read/delete errors remain errors except for an actually missing item.
Returned native blobs and temporary codec buffers are zeroized. OS credential
protection is not a defense against code already running as the same user or
a compromised host. Tests use only unique app-owned synthetic namespaces and
make cleanup/deletion failures visible.
Capacity fixtures seed the native registry with disconnected identities and
only one real token pair, so byte-limit checks do not require bulk live
credentials. Each fixture records its exact native targets before writes,
cleans only those targets, and checks absence through a fresh store. Cleanup
failures are reported without a panic in `Drop` masking an original failure.
Failed native writes report only the fixed stage and numeric Win32 error code,
not credential contents or real account identifiers.

Windows serializes each native credential read, write and delete within this
process, including release of returned native allocations. Registry and
per-account semantic locks remain unchanged; the native mutex never spans
provider/AI work, a refresh exchange or an await. This orders this application's
native I/O, not other processes or Windows globally. A bounded host comparison
found two exact residuals without this ordering and none among the serialized
run's 18 targets when read by a fresh process; it does not establish OS causality
or promise universal race freedom.

A separate Windows integration test logs two exact owned targets before writes.
Its producer exercises bounded parallel operations through the production store,
then exits. Another process verifies exact-target absence with `CredReadW`
before the controller's final exact-target cleanup. Verification failure remains
a test failure even if cleanup succeeds. This is not a bulk/stress test or
credential enumeration.

The configured `cargo test` invocation
[runs test executables serially](https://doc.rust-lang.org/cargo/commands/cargo-test.html#description).
This integration executable has one ordinary controller test; its worker is
ignored unless explicitly selected in a child process. Consequently unrelated
native unit-test I/O has finished before the child scenario runs. The controller
does its initial reads before spawning the producer, waits for each child to
exit, and cleans up only after the verifier exits. No parent native critical
section spans a child wait. Parallel producer calls still exercise in-process
native ordering; the separate verifier checks only the exact fixture records'
post-exit persistence. Other harnesses must preserve this executable isolation,
not assume the process-local mutex coordinates concurrent executables.

## Windows Copilot process boundary

The pinned SDK/runtime remains 1.0.14 / 1.0.85. Windows runs the actual bundled
executable; only macOS queries `NSProcessInfo` for its existing minimum-version
guard. A failed native startup is an error, never a fabricated platform version
or ambient CLI fallback.

Windows environment-key comparisons are case-insensitive. Required
`SystemRoot`/`WINDIR` come from `GetWindowsDirectoryW`, not inherited values.
PATH contains only that directory and its System32 directory. HOME,
USERPROFILE, APPDATA, LOCALAPPDATA, TEMP/TMP and Copilot state are private to
the operation. The existing current-user-only filesystem ACL helper protects
the temporary root before the child can write. Ambient tokens, provider
configuration, Node hooks, proxies and CLI configuration remain stripped.

Explicit token/account/model selection, SDK empty mode, no automatic login,
the exact three read tools, permission denial, operation budgets, generation
fences and bounded shutdown remain unchanged. Catalog and review code share
the environment and private-directory helpers. Cleanup errors remain visible.
No speculative SDK wrapper or console-window patch is introduced.
The checksum-verified pinned SDK already uses a Windows Job Object for child
tree ownership. Its termination request is asynchronous. Catalog and review
execution share the production directory lifecycle: after cooperative/forced
shutdown or aborted startup, Windows retries only native lock/access/not-empty
cleanup errors for at most one second, in bounded 10 ms intervals. This lets
root/descendant CWD and file handles release before removal. Persistent locks
still return the explicit cleanup failure; macOS retains its single close.
The five-second graceful SDK shutdown budget is unchanged.

Native fixtures invoke that same shutdown-plus-cleanup boundary with owned
descendants, including stalled shutdown and cancelled/failed startup. Receipts
live outside the operation root so directory absence and child exit are
checked after production cleanup, without an intervening test-only wait.

## Evidence boundary

The existing native harness imports actual production credential types,
OAuth/rotation logic, binary codec, native stores, Copilot catalog runtime and
operation fences. There are no duplicate domain types or success-shaped host
stubs; the app remains Cargo's default workspace member and retains one
lockfile. Windows CI retains all foundation/frontend/shared checks, adds the
runnable native account/runtime tests, and explicitly runs the real offline
bundled-runtime handshake.

Synthetic transport tests establish isolation, rejection, deadlines,
cancellation and native child-exit checks. The real offline smoke establishes
connect/status, unauthenticated model rejection, no requests to its rejecting
loopback provider, clean shutdown and temporary cleanup. Neither proves live
sign-in/inference or GUI-parent console behavior. Full review/host regression
execution on Windows remains gated by the complete app's other native
adapters; macOS keeps its existing application and browser assertions.
