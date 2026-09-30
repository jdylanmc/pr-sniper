# Reuse the tray host and keep Windows startup opt-in

## Host boundary

The authorized Windows parity slice (#59) reuses the existing Tauri menu,
lazy Status/Review Queue/Settings windows, Check Now, single-instance plugin,
close-to-tray behavior and bounded cancellation/quit path. It does not add a
startup window, redesign the frontend or grant review/publication authority.
Repeated Quit requests start only one shutdown. macOS retains Accessory
activation and its template tray image; Windows uses the existing colored
application artwork. The release Windows executable uses the GUI subsystem,
not a console subsystem.

`tauri.windows.conf.json` supplies the Windows icon and platform bundle
configuration without changing the macOS app-bundle command or signing/minimum
version settings. `npm run build:windows` performs a Tauri production build with
embedded Vite assets and no installer; the executable needs WebView2, not a
development server. This is not a signed/distributed release. The checked-in
multi-resolution icon derives from the existing application artwork through
`scripts/windows-icon.ps1`.

Existing text names the system tray, Windows Credential Manager, local
application-data folder and Startup Apps on Windows. The macOS terminology and
runtime minimum remain on macOS. Browser user-agent platform detection changes
presentation only; no authorization or native platform choice trusts it.

## Registration boundary

`LoginRegistration` keeps the existing status and transactional settings
contract with native platform implementations. macOS retains its LaunchAgent.
Windows writes exactly one `REG_SZ` value, `com.jdylanmc.pr-sniper`, in the
current user's `Software\Microsoft\Windows\CurrentVersion\Run` key.
The value is the quoted absolute executable path, without arguments. Missing,
stale, malformed and wrong-type values remain distinguishable from a valid
registration. Native access errors remain errors, never absence.

The Run key is not application-owned. The adapter never removes that key,
touches other values, modifies Startup folders or changes access controls.
Disabling removes only its exact named value. A settings-save failure restores
the previous value's exact type and bytes, or absence; rollback failure remains
visible. Commands exceeding the documented 260-character Run limit fail before
changing registration.

Saved intent is not applied at startup. Only an explicit Settings request
changes registration, and the existing isolated-profile guard rejects that
request before the native adapter. The `registered` status describes the
current executable's registration, not successful launch. Windows Startup Apps
can disable a registered entry; the application neither resets that choice nor
claims to detect actual startup execution.

## Evidence boundary

Native startup tests create exclusively owned UUID registry keys outside Run,
use synthetic values/executables, independently read the native values and
verify deletion of their exact keys. They cover persistence, quoted paths,
invalid/missing/stale registration, preservation of unrelated values, native
access denial and exact rollback on settings failure.

The existing foundation harness imports this exact production adapter for
targeted checks. After notification integration, Windows CI runs the full
application suite, including these tests, rather than a replacement host.
Full app checks, browser bridge tests and the real
[Windows acceptance procedure](../windows-development.md#windows-application-acceptance)
remain convergence requirements. Unit/native tests do not prove taskbar
interaction, Windows logon, notification delivery or absence of GUI-child
console flashes.
