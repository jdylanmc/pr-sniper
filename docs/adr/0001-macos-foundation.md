# Keep the macOS tray host native and its surfaces local

The approved MVP selects Tauri instead of Electron and macOS before Windows.
Tauri 2 owns the menu bar, lazy windows, single-instance enforcement and explicit
quit lifecycle; a vanilla TypeScript/Vite frontend provides small local surfaces
without a component framework. Closing a window hides it, while quitting ends
the event loop; there is no scheduler or child process in this foundation.

Configuration and host diagnostics occupy separate `config/` and `state/`
directories under the application data root. Settings are strict typed JSON,
replaced atomically, not a database or future job schema. Diagnostics accept only
closed-schema host events, never raw errors, commands or arbitrary strings.
Credentials are deliberately absent: no secret is accepted by any foundation
command, config field or log event. Later credential work must use macOS secure
storage, not extend these files with tokens; no unused Keychain adapter is
introduced before there is an exercised credential flow.

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
