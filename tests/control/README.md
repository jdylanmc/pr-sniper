# Native control acceptance

| Platform | Built artifact                                       | Isolation                                                   |
| -------- | ---------------------------------------------------- | ----------------------------------------------------------- |
| macOS    | Debug `.app` built with `tauri.acceptance.conf.json` | Separate compiled Tauri identifier; copied run-owned bundle |
| Windows  | Existing standalone release executable               | Fresh GitHub-hosted VM only; copied run-owned executable    |

```sh
npm run tauri -- build --debug --bundles app --config tests/control/tauri.acceptance.conf.json -- --locked
node tests/control/native-acceptance.mjs --executable "src-tauri/target/debug/bundle/macos/PR Sniper Control Acceptance.app/Contents/MacOS/pr-sniper"
```

- Both native CI workflows **run** the bundled CLI against the real app after
  building. Compilation alone cannot pass this check.
- CLI launches the closed app. Harness verifies native inspect, resource save,
  stale-save rejection, pause, malformed/unknown/login rejection and real
  process close/relaunch with persisted pause and resource.
- A deliberately fragmented authenticated request also reaches the real native
  listener and must return a typed rejection; the original CLI rejection
  assertions remain unchanged.
- Fresh run-owned profile and credential-service namespace. No Copilot install,
  sign-in, inference, provider effects, skill installation or login-item changes.
- Stop uses exact observed PID plus creation identity for the run-owned copied
  executable. No process-name kill, installed-app termination or broad cleanup.
  Process termination is restart acceptance, **not** graceful tray-Quit proof.
- Four-minute workflow budget; bounded CLI calls and cleanup polling.
- `src-tauri/target/control-native-acceptance/run-*/receipt.json` records source
  digest, checks, CLI exit codes/timing, native PIDs and cleanup. CI always retains
  these receipts, including failed attempts. Endpoint capabilities, profiles,
  raw account data and copied executables are excluded from artifact uploads.
- The real CLI intentionally ignores app console streams. Harness keeps
  content-free operation results and per-call native liveness, not fabricated
  native console diagnostics. On transport failure, the app records at most
  sixteen phase/IO-kind/elapsed-time entries; the harness retains these in its
  receipt before cleanup. Frame bodies, capabilities and credentials are absent.
- Windows compiles all test executables in a separate bounded step. The
  existing fifteen-minute execution gate and sixty-minute job bound remain.
- `dispatcher/tests.rs` separately drives authenticated frames through the
  production command router and receipt boundary; only provider resolution,
  account state and inference launch are isolated collaborators.
