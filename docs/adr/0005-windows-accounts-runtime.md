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

Native read/delete errors remain errors except for an actually missing item.
Returned native blobs and temporary codec buffers are zeroized. OS credential
protection is not a defense against code already running as the same user or
a compromised host. Tests use only unique app-owned synthetic namespaces and
make cleanup/deletion failures visible.

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
tree ownership. Native fixtures verify both cooperative and forced shutdown of
a spawned descendant, using bounded native exit-signal waits because Windows
job termination is asynchronous.

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
