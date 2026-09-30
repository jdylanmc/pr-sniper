# Windows notification identity and activation

This is the existing notification workflow's Windows adapter (#60), not a new
notification category or provider. It uses the inbox
`Windows.UI.Notifications.ToastNotificationManager` through Microsoft's pinned
`windows` / `windows-core` 0.62.2 crates. No Windows App SDK redistributable or
Tauri notification plugin is required. macOS retains UserNotifications.

## Opt-in and truthful status

Notifications still default off. Normal launch only reads an existing
registration and, when it matches, registers a **process-local** COM class factory.
Only the explicit notification opt-in operation creates persistent Windows
identity. It does not change login/startup registration or any Windows setting.

The adapter's identity must be read from an existing, validated notification
ledger, never the synthetic default used by a missing-file read. If initial
ledger persistence fails, only notifications become unavailable. Explicit opt-in
can retry that persistence after the operator resolves the exact filesystem
obstruction; it rebuilds the identity from the saved ledger before any Windows
registration. Setup and the final opt-in save both recheck the persisted UUID.
A replaced saved profile cannot silently reuse the prior runtime identity.

Windows desktop notifications do not have a macOS-style authorization prompt.
Opt-in creates this profile's owned Start Menu shortcut and current-user COM
registration, then reads `ToastNotifier.Setting`. Application, user, group-policy
and manifest blocks remain explicit; errors and unknown enum values fail closed.
If Windows blocks delivery, the app preference remains off. The registration
remains available for Windows Settings and a later explicit opt-in.

`Setting` is **aggregate**, not separate banner/notification-center
authorization. Windows returns `null` for those two channel fields. The UI says
they are unknown instead of claiming enabled or disabled. Focus, Do Not Disturb,
per-channel switches and other OS behavior can suppress a visible banner even
when the aggregate setting allows submission. Inspect Windows Settings > System >
Notifications > PR Sniper for those controls.

The common ledger still persists intent before sending. A successful `Show`
means `accepted_unconfirmed`, **not observed delivery**. Failure during `Show`
is conservatively `outcome_unknown`, never an automatic retry. Preparation
failures are definite failures. Interrupted submitting entries remain unknown
after restart. Existing deduplication and queue/Settings destinations are reused.

## Owned identity

Each saved notification profile UUID determines an application-specific
Application User Model ID (AUMID) and COM class ID (CLSID). The shortcut contains
both `System.AppUserModel.ID` and `System.AppUserModel.ToastActivatorCLSID`.
The executable target, activation arguments and properties must all match.

Persistent locations:

- Current user's Programs known folder:
  `PR Sniper notifications (<profile-uuid>).lnk`.
  Isolated profiles use `PR Sniper notifications (test <profile-uuid>).lnk`.
- `HKCU\Software\Classes\CLSID\{<derived-clsid>}`:
  one `PRSniperNotificationOwner` value and one `LocalServer32` child.
- `LocalServer32`: the quoted exact executable plus the notification-server
  argument, and `ServerExecutable` naming that executable without arguments.

The owner value is a versioned base64url JSON identity: exact canonical data
root, executable, notification profile UUID and optional isolated credential
service. It contains **no credential or provider data**. The entire identity,
command, shortcut and expected registry contents must match to reuse or remove
the registration. A stale executable, incomplete setup, foreign contents or
changed profile is an error; startup does not repair it or take ownership.

Shortcut creation stages a new file beside the destination and installs it with
an exclusive hard link, not an overwriting COM `Save`. The stage is removed on
success or failure. Registry creation checks `REG_CREATED_NEW_KEY` before writes.
A partial registry failure remains an explicit error with its exact owned
evidence preserved; it is not silently deleted with a recursive registry cleanup.
Current-user/same-user ownership is not protection against a compromised user.

Turning the preference off stops new submissions. Existing notifications can
still navigate. Quitting revokes the **runtime** class object on its registering
COM apartment; it does not remove the persisted cold-start registration.

## Click and cold-start boundary

The class factory exposes `INotificationActivationCallback`. It accepts only its
own AUMID and a canonical `pr-sniper:<profile-uuid>:<notice-uuid>` argument, with no
action/input payload. The GUI callback checks the full running profile identity,
checks the saved notice, then marshals to the UI thread and uses
`notifications::host::open`. No click invokes review, publication, approval or
merge. A missing destination never substitutes a nearby queue item.

For a cold COM launch, preflight runs **before** plugins, accounts or default data
root reads. A bounded, credential-free activation process receives the COM
callback before starting the same executable with the exact profile and notice.
This prevents the single-instance plugin from exiting the COM process before it
receives the callback. The relay admits one activation and closes admission
before beginning its handoff. A second callback is rejected for retry, not
acknowledged into an undrained queue. The first callback returns success only
after its exact-profile GUI invocation was started successfully.

On success, handoff failure or the 60-second handoff deadline, the relay closes
new activation/object/server-lock admission and revokes its class object on the
owning COM thread. It then keeps that apartment alive for at most five seconds
while existing callback objects and server locks release. Release timeout is an
explicit error, including alongside a handoff error. Only then does it finish
the COM thread and exit. It does not own a tray or provider operation.

The GUI invocation validates the unchanged executable, saved profile UUID, owned
registration and saved notice before restoring the isolated environment. Isolated
launches retain both the exact data root and test-owned credential service;
they do not rely on inherited environment. A production descriptor must resolve
to the actual current-user production data directory. Stale, missing or foreign
profiles fail with a native error dialog rather than opening production state.

The single-instance callback queues bounded activation requests received before
`Host` is managed. Readiness and queue mutation share one lock: setup atomically
marks the queue ready and takes pending requests; later producers dispatch
directly. Navigation and error UI run outside that lock. Shutdown closes
admission and explicitly reports any cancelled startup navigation. If a
different profile is already running, the app reports that
conflict and asks the operator to quit it and retry; it does not open that
profile's Settings or an unrelated pull request.

## Explicit removal and isolated test cleanup

Quit the application first. Read **the exact owned** CLSID's
`PRSniperNotificationOwner` value, and pass that token to the same application:

```powershell
& 'D:\path\pr-sniper.exe' --pr-sniper-notification-unregister '<owner-token>'
```

This is an explicit removal request, not normal startup behavior. It checks the
entire registration and shortcut before removing only their known values and
empty keys. It never deletes a registry tree, enumerates credential targets, or
changes startup entries. Foreign or incomplete contents cause an error requiring
manual inspection of the exact reported paths. A missing old profile does not
prevent removal of an otherwise exactly matching owned registration.

For a failed partial install, compare its UUID-derived paths, owner descriptor,
command and shortcut against the installation attempt's receipt before any manual
recovery. Do not remove another installation's entries or a broad product prefix.

## Verification boundary

The existing foundation test package imports the **actual native implementation**
and runs identity/argument, native XML, aggregate permission mapping, real
in-process COM callback/rejection and class-revocation tests, deterministic startup
queue interleavings, held-handoff second activation, and retained callback/server
lock handling on success, handoff error and timeout. No replacement host,
notification domain or permission-success stub is introduced:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --locked `
  -p pr-sniper-foundation-tests --test windows_notifications
```

One deliberately ignored native ownership test creates a unique test-only
shortcut and CLSID, logs the exact paths before writing, verifies foreign-install
rejection, removes its exact owned registration and verifies absence:

```powershell
cargo test --manifest-path src-tauri\Cargo.toml --locked `
  -p pr-sniper-foundation-tests --test windows_notifications `
  owned_registration_preserves_foreign_install_and_cleans_exact_identity `
  -- --ignored --nocapture
```

That test does not send a toast, modify production notification identities,
touch credentials/startup entries or launch the GUI. It is not interactive
delivery proof.

Full application convergence with #59 must separately prove native test delivery,
running click, quit/cold click, exact queue destination, stale-profile rejection,
permission-block guidance and restart deduplication. GUI screenshots/activation
receipts and actual Windows/macOS CI belong to that integrated candidate, not
this source-level adapter validation.

## Primary API references

- [Desktop shortcut identity](https://learn.microsoft.com/en-us/windows/win32/shell/enable-desktop-toast-with-appusermodelid)
- [Microsoft DesktopToasts COM activation sample](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/DesktopToasts/CPP/DesktopToastsSample.cpp)
- [INotificationActivationCallback::Activate](https://learn.microsoft.com/en-us/windows/win32/api/notificationactivationcallback/nf-notificationactivationcallback-inotificationactivationcallback-activate)
- [ToastNotifier.Setting and block precedence](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotifier.setting)
- [CoRegisterClassObject](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coregisterclassobject)
