# Settings behavioral test

Run `npm ci`, then `npm run test:settings` with the repository's Rust toolchain
on `PATH`. If Playwright reports a missing browser executable, install its
matching Chromium build with `npm exec playwright install chromium`.

The test serves the actual Vite production build and operates its Settings
controls in an isolated Chromium session. Only Tauri IPC is replaced: each
request launches the Rust example `settings_bridge`, which creates a fresh
production `Store` at a test-owned temporary root. Initial host preferences
are seeded through `Store::save_settings`; assertions read through a separate
Store process and reload the UI. Persistence is not a JavaScript imitation.
The fixture removes its own temporary data even when an assertion fails.

This first RED slice demonstrates the missing Add repository control after
the existing Settings surface has successfully read the saved startup
preference. The bridge's `save_repository` arm is explicitly unimplemented;
GREEN must connect it to the same production Store operation as the Tauri
command before the full round trip can pass.

The test does not exercise native Tauri command registration, macOS WebKit,
menu-bar behavior, or login-item integration. It never launches the native
application or changes host login settings. Port 1421 must be free; an
existing server is never reused.
