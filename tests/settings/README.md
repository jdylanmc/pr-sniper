# Settings behavioral tests

Run `npm ci`, then `npm run test:settings` with the repository's Rust toolchain
on `PATH`. If Playwright reports a missing browser executable, install its
matching Chromium build with `npm exec playwright install chromium`.

The tests serve the actual Vite production build and operate its Settings
controls in an isolated Chromium session. Only Tauri IPC is replaced: each
request launches the Rust example `settings_bridge`, which creates a fresh
production `Store` at a test-owned temporary root. Initial host preferences
are seeded through `Store::save_settings`; assertions read through a separate
Store process and reload the UI. Persistence is not a JavaScript imitation.
The fixture removes its own temporary data even when an assertion fails.

The add-repository test checks canonical GitHub names, persistence across fresh
Store processes and UI reload, and preservation of the startup preference.
The bridge and native command call the same production Store operation.

The repository lifecycle test adds two records, then checks canonical URL
rename, duplicate add/rename rejection, disable/re-enable, and confirmed
removal. Fresh reads protect immutable identity, the independent neighboring
record, and the startup preference throughout. Its current RED is the missing
Edit control; GREEN must connect the bridge's explicit unimplemented
`update_repository` and `remove_repository` arms to the matching Store methods.

The tests do not exercise native Tauri command registration, macOS WebKit,
menu-bar behavior, or login-item integration. It never launches the native
application or changes host login settings. Tests run serially. Port 1421 must
be free, or select another port with `SETTINGS_TEST_PORT=1422 npm run test:settings`
for a separate worktree. An existing server is never reused.
