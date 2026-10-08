# Keep the macOS tray host native and its surfaces local

The approved MVP selects Tauri instead of Electron and macOS before Windows.
Tauri 2 owns the menu bar, lazy windows, single-instance enforcement and explicit
quit lifecycle; a vanilla TypeScript/Vite frontend provides small local surfaces
without a component framework. Closing a window hides it, while quitting ends
the event loop; there is no scheduler or child process in this foundation.

The retained tray panel uses the approved v2 surface's 19-point corner radius.
The window and WebView are transparent, and only the panel document's root/body
backgrounds are cleared; legacy standalone surfaces keep their existing theme.
On macOS the AppKit content-view layer clips the entire WKWebView subtree, with
the same CSS contour for headers, content, footers and in-panel overlays. The
layer follows the view's bounds, uses the actual backing scale for rasterization
and keeps its radius in points. Apply before each show/reopen and refresh on
resize, backing-scale and theme events, invalidating the native window shadow.
No delegate replacement, retained native pointer, global observer, timer or
custom shadow window is introduced; Tauri owns teardown and tray placement.
Failed handle/layer/scale or native-state checks return visible panel errors.

Each opening enumerates connected displays. Windows resolves the tray (or
available pointer) against current physical bounds and applies physical sizing.
macOS uses AppKit global screen points for NSScreen frames/visible work areas,
the actual status-button screen rectangle and screen, and NSEvent pointer
position. Flip coordinates once against the first (primary) screen's top edge;
do not compare Tao's independently scaled monitor origins or tray/pointer pixels.
The actual connected tray screen wins; a retained-window screen is used only if
still connected, otherwise the connected primary display is used. If that hint
is also unavailable, use the first usable connected display.

The complete panel is clamped to the selected display's current work area.
On macOS the intended maximum is 408 by 744 points regardless of old/target
backing scale. Apply the final point frame synchronously through NSWindow on the
UI thread, and read back its position and size before show/focus or an open
receipt. Do not pass target-scaled pixels through Tao's setters: they convert
using the retained window's old scale and queue native changes asynchronously.
Rejected frame readback is an opening error. Tray/focus-loss hit testing uses
the same native point domain; Windows retains its existing physical path.
Fallback is reported in the panel and as the closed-schema
`window_placement_recovered` diagnostic event. Failure to obtain usable connected
geometry remains an opening error, not an off-screen success.

Opening failures record `window_open_failed` without raw platform errors or
screen coordinates, and change the native tray-menu recovery action to
`Panel could not open - Retry`. This action remains reachable without the panel
and reopens the retained destination without resetting drafts or exact-item
identity. A successful open restores `Retry opening panel`; cancellation by newer
navigation, dismissal or Quit is not an opening-failure diagnostic. Native
screen-reader access and an actual post-undock reopen still require maintainer
verification; deterministic geometry and persistence tests do not prove them.

Transparent WKWebView composition requires Tauri's pinned `macos-private-api`
feature and matching `macOSPrivateApi` configuration. Wry 0.55.1 disables the
WebKit `drawsBackground` private KVC property and sets the under-page background;
Tao clears NSWindow's background and opacity. This is not an App Store
compatibility claim. Windows uses the existing Tauri/Tao transparent WebView2
and undecorated native-shadow frame path, including Windows 11 system rounding,
not an AppKit port or a custom Win32 region that would disable DWM rounding.
The compositor owns Windows' outer frame radius; exact macOS-radius parity and
older Windows frame/shadow behavior require separate native observation.

Builds, geometry tests, source/config contracts and headless browser captures
do not prove native corner pixels, capture compositing, shadow or tray anchoring.
Before native acceptance, an authorized operator must inspect the actual panel
at supported display scales, show/hide/reopen and monitor changes, scrolling,
focus/hover and editor overlays, dark/high-contrast/reduced-transparency modes
and capture. Check transparent corner pixels, rounded content containment and
the native shadow/tray anchor together. No native desktop automation,
provisioning or profile launch is authorized by this documentation.

Configuration and host diagnostics occupy separate `config/` and `state/`
directories under the application data root. Settings are strict typed JSON,
replaced atomically, not a database or future job schema. Diagnostics accept only
closed-schema host events, never raw errors, commands or arbitrary strings.
Credentials are deliberately absent: no secret is accepted by any foundation
command, config field or log event. Later credential work must use macOS secure
storage, not extend these files with tokens; no unused Keychain adapter is
introduced before there is an exercised credential flow.

Repository configuration extends that same typed settings file. Each configured
repository has an immutable local UUID separate from its canonical GitHub name;
editing the name or enabled state preserves identity. This does not claim a
verified GitHub repository ID. Watched accounts use decimal GitHub account IDs
as strings, with unverified login labels kept separate from their matching key.
Global defaults and sparse repository overrides resolve field by field; explicit
false, empty watchlists and the adapter-default selector are not inheritance.
Both automatic-start and publication default off and remain independent.

The storage save boundary validates complete effective policies before atomic
replacement; load rejects invalid data instead of inventing defaults. Missing
new fields in the established host-only format use defined defaults, preserving
the startup preference without a migration framework. Croner validates five-field
cron syntax and chrono-tz validates IANA zone names; neither executes schedules.
Configuration accepts no credential fields, rejects recognizable GitHub token
patterns, and never copies policy text or repository data into host diagnostics.
Arbitrary user-authored text must still be kept nonsecret; credentials belong in
the later secure-storage flow.

Configuration commit and diagnostics are distinct outcomes. Native commands and
the browser bridge share the production save-result boundary: a committed save
returns settings plus an optional safe diagnostics warning, not a false failure.
Settings refreshes preserve only edited form groups; untouched inherited fields
use current defaults. Edits invalidate pending focus refreshes, and configuration
saves hold controls stable until their response is reconciled.

GitHub connection now has separate credential and provider boundaries.
The credential source runs only bounded `gh --version` and `gh auth token
--hostname github.com` commands; captured secrets are transient and never
serialized or persisted by the application. GitHub CLI retains ownership of its
existing credential storage. No unused app Keychain store is introduced.
The provider client uses typed domain results over GET-only HTTPS, not CLI
presentation tables. Sensitive authorization headers never enter process argv,
diagnostics or frontend state; redirects are disabled.

Connections verify the stable account ID, canonical remote repository ID and
real PR-read capability. OAuth scope evidence is distinct from read access and
from item-specific publication permission; absent scope introspection remains
unknown. An explicit read exhausts REST pagination and checks changed-file
counts and revision stability. No partial metadata is returned on failure.
The local repository UUID remains separate from remote identity; native commands
recheck saved names after asynchronous reads, and UI observations are invalidated
on retarget. Connection actions preserve the existing Settings draft/save
boundary and never alter an effective policy or its automation gates.

Launch at login is an explicit Settings operation that writes the application's
own LaunchAgent plist. Settings distinguishes saved intent from absent, invalid
or structurally valid registration for the current executable; none proves
effective launchd state or overrides macOS Login Items. This replaces the
autostart plugin's existence-only Boolean, which could misreport damaged or
stale registrations. Plist encoding handles special path characters, and a
settings-write failure restores the previous registration bytes.
Saved intent is never applied at startup. An isolated data-root override disables login mutation,
allowing local acceptance runs without changing host startup. This avoids
turning a developer launch or stale preference into silent OS registration.
