use crate::github::{oauth::TokenPair, Identity};
use github_copilot_sdk::{Client, ClientMode, ClientOptions, LogLevel, Model, Transport};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

// Shared account-isolated environment for catalog queries and restricted reviews.
pub(crate) fn options(
    program: PathBuf,
    root: &Path,
    token: &str,
    inherited_keys: impl Iterator<Item = OsString>,
) -> Result<ClientOptions, String> {
    let mut environment: Vec<(OsString, OsString)> = vec![
        ("HOME".into(), root.into()),
        ("XDG_CONFIG_HOME".into(), root.join("config").into()),
        ("XDG_CACHE_HOME".into(), root.join("cache").into()),
        ("TMPDIR".into(), root.into()),
        ("LANG".into(), "en_US.UTF-8".into()),
        ("COPILOT_AUTO_UPDATE".into(), "false".into()),
        ("COPILOT_TELEMETRY".into(), "false".into()),
    ];
    #[cfg(not(windows))]
    environment.push(("PATH".into(), "/usr/bin:/bin".into()));
    #[cfg(windows)]
    {
        let windows = windows_directory()?;
        let path = std::env::join_paths([windows.join("System32"), windows.clone()])
            .map_err(|_| "Windows runtime path is unavailable.")?;
        environment.extend([
            ("SystemRoot".into(), windows.clone().into()),
            ("WINDIR".into(), windows.into()),
            ("PATH".into(), path),
            ("USERPROFILE".into(), root.into()),
            ("APPDATA".into(), root.join("roaming").into()),
            ("LOCALAPPDATA".into(), root.join("local").into()),
            ("TEMP".into(), root.into()),
            ("TMP".into(), root.into()),
        ]);
    }
    // The SDK applies env_remove after its own injected values; retain only
    // keys we replace explicitly, never their ambient values.
    let remove: Vec<_> = inherited_keys
        .filter(|key| {
            !environment
                .iter()
                .any(|(safe, _)| environment_key_eq(safe, key))
                && ![
                    "COPILOT_SDK_AUTH_TOKEN",
                    "COPILOT_HOME",
                    "COPILOT_DISABLE_KEYTAR",
                ]
                .iter()
                .any(|safe| environment_key_eq(std::ffi::OsStr::new(safe), key))
        })
        .collect();
    Ok(ClientOptions::new()
        .with_program(program)
        .with_transport(Transport::Stdio)
        .with_cwd(root)
        .with_base_directory(root.join("copilot"))
        .with_mode(ClientMode::Empty)
        .with_github_token(token)
        .with_use_logged_in_user(false)
        .with_log_level(LogLevel::None)
        .with_env(environment)
        .with_env_remove(remove))
}

fn environment_key_eq(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> bool {
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(windows)]
fn windows_directory() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err("Windows runtime system directory is unavailable.".into());
    }
    Ok(OsString::from_wide(&buffer[..length]).into())
}

pub(crate) fn private_directory(prefix: &str) -> Result<tempfile::TempDir, String> {
    let directory = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(|_| "Cannot create private Copilot runtime state. Check local storage.")?;
    if crate::storage::private_fs::directory(directory.path()).is_err() {
        directory
            .close()
            .map_err(|_| "Cannot protect or clean up private Copilot runtime state.")?;
        return Err("Cannot protect private Copilot runtime state.".into());
    }
    Ok(directory)
}

pub(crate) async fn with_directory<T>(
    directory: tempfile::TempDir,
    work: impl std::future::Future<Output = T>,
) -> Result<T, String> {
    let result = work.await;
    #[cfg(windows)]
    {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            match std::fs::remove_dir_all(directory.path()) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                // The SDK requests asynchronous Job termination, including on
                // aborted startup. CWD/file locks can outlive that request.
                Err(error)
                    if matches!(error.raw_os_error(), Some(5 | 32 | 145))
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep_until(
                        (tokio::time::Instant::now() + Duration::from_millis(10)).min(deadline),
                    )
                    .await;
                }
                Err(_) => {
                    return Err(
                        "Copilot stopped, but private runtime state could not be cleaned up."
                            .into(),
                    )
                }
            }
        }
        // Removal succeeded; disarm TempDir rather than trying to delete twice.
        let _ = directory.keep();
    }
    #[cfg(not(windows))]
    directory
        .close()
        .map_err(|_| "Copilot stopped, but private runtime state could not be cleaned up.")?;
    Ok(result)
}

pub(crate) async fn shutdown(client: &Client) {
    if !matches!(
        tokio::time::timeout(Duration::from_secs(5), client.stop()).await,
        Ok(Ok(()))
    ) {
        client.force_stop();
        eprintln!("[copilot] stage=runtime_cleanup outcome=forced_stop");
    }
}

pub(crate) fn runtime_program() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let os = objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersion();
        if !supports_runtime(os.majorVersion, os.minorVersion) {
            return Err("The bundled Copilot runtime requires macOS 13.5 or later. Your sign-in and saved Agent selections are unchanged.".into());
        }
    }
    // Windows eligibility is established by the actual pinned native runtime,
    // not a fabricated macOS version or an ambient CLI fallback.
    github_copilot_sdk::install_bundled_cli().ok_or_else(|| {
        "The bundled Copilot runtime is unavailable. Reinstall PR Sniper and retry.".into()
    })
}

pub(crate) fn models(
    identity: &Identity,
    pair: &TokenPair,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<Model>, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Copilot model lookup cancelled.".into());
    }
    if Instant::now() >= deadline {
        return Err("Copilot operation timed out. Retry.".into());
    }
    let program = runtime_program()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot start the Copilot runtime. Retry.")?;
    let directory = private_directory("pr-sniper-copilot-")?;
    // SDK transport errors/stderr can contain provider data. Return fixed
    // stage-specific errors instead, with all SDK tracing disabled here.
    let dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let result = tracing::dispatcher::with_default(&dispatch, || {
        let options = options(
            program,
            directory.path(),
            pair.access_token(),
            std::env::vars_os().map(|(k, _)| k),
        );
        runtime.block_on(with_directory(directory, async {
            query_with_deadline(options?, &identity.login, cancelled, deadline).await
        }))?
    });
    result
}

#[cfg(target_os = "macos")]
fn supports_runtime(major: isize, minor: isize) -> bool {
    // Verified LC_BUILD_VERSION of the pinned 1.0.85 native runtime.
    major > 13 || (major == 13 && minor >= 5)
}

async fn cancellation(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
async fn query(
    options: ClientOptions,
    login: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<Model>, String> {
    query_with_deadline(
        options,
        login,
        cancelled,
        Instant::now() + super::operation::OPERATION_LIMIT,
    )
    .await
}

async fn query_with_deadline(
    options: ClientOptions,
    login: &str,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<Model>, String> {
    if Instant::now() >= deadline {
        return Err("Copilot operation timed out. Retry.".into());
    }
    let client = tokio::select! {
        _ = cancellation(cancelled) => return Err("Copilot model lookup cancelled.".into()),
        result = tokio::time::timeout_at((Instant::now() + Duration::from_secs(30)).min(deadline).into(), Client::start(options)) => {
            result.map_err(|_| "Copilot runtime startup timed out. Retry.")?
                .map_err(|_| "The Copilot runtime could not start. Retry or reinstall PR Sniper.")?
        }
    };
    let outcome = tokio::select! {
        _ = cancellation(cancelled) => Err("Copilot model lookup cancelled.".into()),
        result = tokio::time::timeout_at((Instant::now() + Duration::from_secs(45)).min(deadline).into(), async {
            let auth = client.get_auth_status().await
                .map_err(|_| "Copilot runtime authentication status is unavailable. Retry.")?;
            if !auth.is_authenticated {
                return Err("Copilot did not accept this credential. GitHub sign-in alone does not establish Copilot access. Retry or reconnect.".into());
            }
            if auth.login.as_ref().is_some_and(|actual| !actual.eq_ignore_ascii_case(login))
                || matches!(auth.auth_type.as_deref(), Some("gh-cli" | "api-key" | "hmac"))
            {
                return Err("Copilot reported a different authentication context. No models were accepted.".into());
            }
            let models = client.list_models().await
                .map_err(|_| "Copilot model listing failed. Check network, account access or organization policy, then retry. Your verified GitHub sign-in is unchanged.")?;
            let mut ids = std::collections::HashSet::new();
            if models.iter().any(|model| model.id.trim().is_empty() || model.name.trim().is_empty() || !ids.insert(&model.id)) {
                return Err("Copilot returned an invalid model catalog. Retry.".into());
            }
            Ok(models)
        }) => result.unwrap_or_else(|_| Err("Copilot model listing timed out. Retry.".into())),
    };
    shutdown(&client).await;
    outcome
}

#[cfg(test)]
pub(crate) fn fixture_program(name: &str) -> (PathBuf, PathBuf) {
    let node = crate::process_path::executable(
        "node",
        &std::env::var_os("PATH").expect("Node test prerequisite"),
    )
    .expect("native Node test prerequisite");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = if manifest
        .file_name()
        .is_some_and(|name| name == "foundations")
    {
        manifest.parent().unwrap()
    } else {
        manifest
    };
    (node, sources.join("tests").join("support").join(name))
}

#[cfg(test)]
pub(crate) async fn wait_for_fixture_method(root: &Path, method: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(root.join("receipt.jsonl")) {
                if text
                    .lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .any(|event| event["method"] == method)
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("fixture did not reach {method}"));
}

#[cfg(test)]
pub(crate) fn assert_process_stopped(pid: u32) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            assert_eq!(
                GetLastError(),
                ERROR_INVALID_PARAMETER,
                "cannot inspect owned child"
            );
        } else {
            // TerminateJobObject requests termination asynchronously. Observe
            // the native exit signal with a bound, not an immediate PID poll.
            let status = WaitForSingleObject(process, 1000);
            assert_ne!(CloseHandle(process), 0);
            assert_eq!(
                status, WAIT_OBJECT_0,
                "owned SDK process must exit within one second of shutdown"
            );
        }
    }
    #[cfg(not(windows))]
    assert!(
        !std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success(),
        "SDK child must be reaped before returning"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct StartupDiagnostics<'a> {
        root: &'a Path,
        case: &'static str,
        started: Instant,
        phase: Cell<&'static str>,
        query_outcome: Cell<&'static str>,
        query_finished_ms: Cell<Option<u128>>,
        connect_ms: Cell<Option<u128>>,
        cancellation_ms: Cell<Option<u128>>,
    }

    fn startup_fixture_evidence(root: &Path) -> serde_json::Value {
        let text = match std::fs::read_to_string(root.join("receipt.jsonl.startup.jsonl")) {
            Ok(text) => text,
            Err(error) => return serde_json::json!({ "read_error": format!("{:?}", error.kind()) }),
        };
        let events: Vec<_> = text.lines().map(|line| {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                return serde_json::json!({ "invalid_event": true });
            };
            let allowed = |key: &str, values: &[&str]| {
                event[key].as_str().filter(|value| values.contains(value)).unwrap_or("other").to_owned()
            };
            serde_json::json!({
                "case": allowed("case", &["waiting-start", "bad-start"]),
                "phase": allowed("phase", &["entered", "delay-finished", "transport-ready", "request", "response-withheld", "response-error", "stdin-ended", "exit"]),
                "method": allowed("method", &["none", "connect", "ping", "auth.getStatus", "models.list", "runtime.shutdown"]),
                "elapsedMs": event["elapsedMs"].as_u64(),
            })
        }).collect();
        serde_json::json!(events)
    }

    impl Drop for StartupDiagnostics<'_> {
        fn drop(&mut self) {
            // Emit only after the immediate reaping assertion, including on panic.
            // Diagnostic I/O must not give asynchronous teardown extra time first.
            eprintln!(
                "[synthetic-startup] {}",
                serde_json::json!({
                    "case": self.case,
                    "phase": self.phase.get(),
                    "panicking": std::thread::panicking(),
                    "elapsedMs": self.started.elapsed().as_millis(),
                    "queryOutcome": self.query_outcome.get(),
                    "queryFinishedMs": self.query_finished_ms.get(),
                    "connectObservedMs": self.connect_ms.get(),
                    "cancellationRequestedMs": self.cancellation_ms.get(),
                    "fixture": startup_fixture_evidence(self.root),
                })
            );
        }
    }

    #[test]
    fn startup_diagnostics_emit_only_allowlisted_fixture_evidence() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("receipt.jsonl.startup.jsonl"),
            "{\"case\":\"waiting-start\",\"phase\":\"request\",\"method\":\"connect\",\"elapsedMs\":750,\"argv\":\"private-marker\"}\n{\"case\":\"private-marker\",\"phase\":\"private-marker\",\"method\":\"private-marker\",\"elapsedMs\":\"private-marker\"}\ninvalid\n").unwrap();
        let evidence = startup_fixture_evidence(root.path());
        assert_eq!(
            evidence[0],
            serde_json::json!({
                "case": "waiting-start", "phase": "request", "method": "connect", "elapsedMs": 750,
            })
        );
        assert_eq!(
            evidence[1],
            serde_json::json!({
                "case": "other", "phase": "other", "method": "other", "elapsedMs": null,
            })
        );
        assert_eq!(evidence[2], serde_json::json!({ "invalid_event": true }));
        assert!(!evidence.to_string().contains("private-marker"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn runtime_minimum_is_explicit_without_changing_app_minimum() {
        assert!(!supports_runtime(12, 7));
        assert!(!supports_runtime(13, 4));
        assert!(supports_runtime(13, 5));
        assert!(supports_runtime(14, 0));
    }

    #[test]
    fn only_explicit_environment_can_reach_account_runtime() {
        let keys = [
            "GH_TOKEN",
            "GITHUB_TOKEN",
            "COPILOT_GITHUB_TOKEN",
            "ANTHROPIC_API_KEY",
            "COPILOT_PROVIDER_BASE_URL",
            "NODE_OPTIONS",
            "COPILOT_HOME",
            "HOME",
            "COPILOT_SDK_AUTH_TOKEN",
            "COPILOT_DISABLE_KEYTAR",
            "OTEL_EXPORTER_OTLP_ENDPOINT",
        ];
        let config = options(
            PathBuf::from("/explicit/copilot"),
            Path::new("/private/account-a"),
            "fixture-token",
            keys.into_iter().map(OsString::from),
        )
        .unwrap();
        assert_eq!(config.mode, ClientMode::Empty);
        assert_eq!(config.use_logged_in_user, Some(false));
        assert_eq!(config.github_token.as_deref(), Some("fixture-token"));
        for key in &keys[..6] {
            assert!(config.env_remove.contains(&OsString::from(key)));
        }
        assert!(config
            .env
            .iter()
            .any(|(key, value)| key == "HOME" && value == "/private/account-a"));
        assert!(config
            .env_remove
            .contains(&OsString::from("OTEL_EXPORTER_OTLP_ENDPOINT")));
        assert!(config.builtin_plugin_directories.is_empty());
        assert!(!config.enable_remote_sessions);
        assert!(config.extra_args.is_empty());
    }

    #[test]
    #[cfg(windows)]
    fn windows_environment_replaces_mixed_case_system_and_private_paths() {
        let root = Path::new(r"C:\private fixture\account");
        let replaced = [
            "SYSTEMROOT",
            "windir",
            "Home",
            "userprofile",
            "AppData",
            "LocalAppData",
            "temp",
            "Tmp",
            "Path",
            "copilot_sdk_auth_token",
            "copilot_home",
            "copilot_disable_keytar",
        ];
        let denied = [
            "gh_token",
            "GitHub_Token",
            "Node_Options",
            "NODE_PATH",
            "copilot_provider_base_url",
            "openai_api_key",
            "HTTP_PROXY",
            "SSL_CERT_FILE",
            "GIT_CONFIG_GLOBAL",
            "COMSPEC",
        ];
        let config = options(
            PathBuf::from(r"C:\explicit\node.exe"),
            root,
            "fixture",
            replaced.into_iter().chain(denied).map(OsString::from),
        )
        .unwrap();
        assert_eq!(config.env_remove, denied.map(OsString::from));
        let env = |name: &str| {
            config
                .env
                .iter()
                .find(|(key, _)| key == name)
                .unwrap()
                .1
                .clone()
        };
        assert_eq!(env("SystemRoot"), windows_directory().unwrap());
        assert_eq!(env("WINDIR"), env("SystemRoot"));
        for key in ["HOME", "USERPROFILE", "TEMP", "TMP"] {
            assert_eq!(env(key), root);
        }
        assert_eq!(env("APPDATA"), root.join("roaming"));
        assert_eq!(env("LOCALAPPDATA"), root.join("local"));
        assert_eq!(config.github_token.as_deref(), Some("fixture"));
        assert_eq!(config.use_logged_in_user, Some(false));
    }

    #[test]
    fn cancellation_never_extracts_or_starts_a_runtime() {
        let pair = TokenPair::new(
            "fixture",
            "refresh-fixture",
            Duration::from_secs(60),
            Duration::from_secs(120),
        );
        let result = models(
            &Identity {
                id: "10".into(),
                login: "fixture".into(),
            },
            &pair,
            &AtomicBool::new(true),
            Instant::now() + super::super::operation::OPERATION_LIMIT,
        );
        assert!(result.unwrap_err().contains("cancelled"));
    }

    fn fixture_options(root: &Path, token: &str) -> ClientOptions {
        let (node, script) = fixture_program("copilot-runtime.mjs");
        let mut config = options(node, root, token, std::env::vars_os().map(|(key, _)| key))
            .unwrap()
            .with_prefix_args([script.into_os_string()]);
        config
            .env
            .push(("TEST_RECEIPT".into(), root.join("receipt.jsonl").into()));
        if matches!(token, "waiting" | "waiting-start" | "bad-start") {
            // A cold fixture must not be mistaken for a started SDK stage.
            config
                .env
                .push(("TEST_STARTUP_DELAY_MS".into(), "750".into()));
        }
        if matches!(token, "waiting-start" | "bad-start") {
            config
                .env
                .push(("TEST_STARTUP_DIAGNOSTICS".into(), "1".into()));
        }
        config
    }

    fn receipt(root: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(root.join("receipt.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn assert_stopped_and_catalog_only(root: &Path) {
        let events = receipt(root);
        assert_eq!(events[0]["ambient"], serde_json::json!([]));
        assert_eq!(events[0]["keytarDisabled"], "1");
        assert_eq!(events[0]["home"], root.to_str().unwrap());
        let args = events[0]["args"].as_array().unwrap();
        assert!(args.contains(&serde_json::json!("--no-auto-login")));
        assert!(!args
            .iter()
            .any(|arg| arg.as_str().is_some_and(|arg| arg.starts_with("account-"))));
        for event in events.iter().skip(1) {
            assert!([
                "connect",
                "ping",
                "auth.getStatus",
                "models.list",
                "runtime.shutdown"
            ]
            .contains(&event["method"].as_str().unwrap()));
        }
        assert_process_stopped(events[0]["pid"].as_u64().unwrap() as u32);
        if let Some(pid) = events[0]["childPid"].as_u64() {
            assert_process_stopped(pid as u32);
        }
    }

    #[tokio::test]
    async fn sdk_clients_pin_distinct_catalogs_and_reap_each_child() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let (a, b) = tokio::join!(
            query(
                fixture_options(first.path(), "account-a"),
                "account-a",
                &cancel
            ),
            query(
                fixture_options(second.path(), "account-b"),
                "account-b",
                &cancel
            ),
        );
        assert!(
            a.is_ok(),
            "{a:?}; fixture receipt: {:?}",
            receipt(first.path())
        );
        assert_eq!(a.unwrap()[0].id, "model-account-a");
        assert_eq!(b.unwrap()[0].id, "model-account-b");
        assert_stopped_and_catalog_only(first.path());
        assert_stopped_and_catalog_only(second.path());
    }

    #[tokio::test]
    async fn sdk_rejection_is_not_empty_success_and_does_not_expose_raw_errors() {
        for (token, login, message) in [
            ("missing", "missing", "did not accept"),
            (
                "account-a",
                "wrong-account",
                "different authentication context",
            ),
            ("blocked", "blocked", "model listing failed"),
        ] {
            let root = private_directory("pr-sniper-catalog-test-").unwrap();
            let error = query(
                fixture_options(root.path(), token),
                login,
                &AtomicBool::new(false),
            )
            .await
            .unwrap_err();
            assert!(error.contains(message), "{error}");
            assert!(!error.contains("secret-provider"));
            assert_stopped_and_catalog_only(root.path());
        }
    }

    #[tokio::test]
    async fn cancelling_a_waiting_catalog_shuts_down_its_client() {
        let root = private_directory("pr-sniper-cancel-test-").unwrap();
        let cancel = AtomicBool::new(false);
        let trigger = async {
            wait_for_fixture_method(root.path(), "models.list").await;
            cancel.store(true, Ordering::SeqCst);
        };
        let (result, ()) = tokio::join!(
            query(fixture_options(root.path(), "waiting"), "waiting", &cancel),
            trigger
        );
        assert!(result.unwrap_err().contains("cancelled"));
        assert_stopped_and_catalog_only(root.path());
        let path = root.path().to_owned();
        root.close().expect("cleanup cancelled runtime");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn cancelled_or_failed_startup_does_not_leave_a_child() {
        for token in ["waiting-start", "bad-start"] {
            let root = tempfile::tempdir().unwrap();
            let cancel = AtomicBool::new(false);
            let diagnostics = StartupDiagnostics {
                root: root.path(),
                case: token,
                started: Instant::now(),
                phase: Cell::new("query-and-connect"),
                query_outcome: Cell::new("pending"),
                query_finished_ms: Cell::new(None),
                connect_ms: Cell::new(None),
                cancellation_ms: Cell::new(None),
            };
            let trigger = async {
                wait_for_fixture_method(root.path(), "connect").await;
                diagnostics
                    .connect_ms
                    .set(Some(diagnostics.started.elapsed().as_millis()));
                if token == "waiting-start" {
                    cancel.store(true, Ordering::SeqCst);
                    diagnostics
                        .cancellation_ms
                        .set(Some(diagnostics.started.elapsed().as_millis()));
                }
            };
            let observed_query = async {
                let result = query(fixture_options(root.path(), token), token, &cancel).await;
                diagnostics
                    .query_finished_ms
                    .set(Some(diagnostics.started.elapsed().as_millis()));
                diagnostics.query_outcome.set(match &result {
                    Ok(_) => "success",
                    Err(error) => match error.as_str() {
                        "Copilot model lookup cancelled." => "cancelled",
                        "The Copilot runtime could not start. Retry or reinstall PR Sniper." => {
                            "startup-failed"
                        }
                        "Copilot runtime startup timed out. Retry." => "startup-timeout",
                        "Copilot operation timed out. Retry." => "operation-timeout",
                        _ => "other-error",
                    },
                });
                result
            };
            let (result, ()) = tokio::join!(observed_query, trigger);
            diagnostics.phase.set("asserting-outcome");
            let error = result.unwrap_err();
            assert!(
                error.contains(if token == "waiting-start" {
                    "cancelled"
                } else {
                    "could not start"
                }),
                "{error}"
            );
            diagnostics.phase.set("asserting-stopped-and-catalog-only");
            assert_stopped_and_catalog_only(root.path());
            diagnostics.phase.set("assertions-complete");
        }
    }

    #[tokio::test]
    #[cfg(windows)]
    async fn windows_cancellation_terminates_owned_descendants_even_if_shutdown_stalls() {
        for (token, stall) in [
            ("waiting", true),
            ("waiting", false),
            ("waiting-start", false),
            ("bad-start", false),
        ] {
            let root = private_directory("pr-sniper-process-tree-test-").unwrap();
            let path = root.path().to_path_buf();
            let receipts = tempfile::tempdir().unwrap();
            let mut options = fixture_options(root.path(), token);
            options
                .env
                .iter_mut()
                .find(|(key, _)| key == "TEST_RECEIPT")
                .unwrap()
                .1 = receipts.path().join("receipt.jsonl").into_os_string();
            options.env.push(("TEST_SPAWN_CHILD".into(), "1".into()));
            if stall {
                options.env.push(("TEST_HANG_SHUTDOWN".into(), "1".into()));
            }
            let cancel = AtomicBool::new(false);
            let trigger = async {
                let method = if token == "waiting" {
                    "models.list"
                } else {
                    "connect"
                };
                wait_for_fixture_method(receipts.path(), method).await;
                if token != "bad-start" {
                    cancel.store(true, Ordering::SeqCst);
                }
            };
            let started = Instant::now();
            let (result, ()) = tokio::join!(
                with_directory(root, query(options, token, &cancel)),
                trigger
            );
            let outcome = result.expect("production lifecycle cleaned its private CWD");
            let error = outcome.unwrap_err();
            assert!(
                error.contains(if token == "bad-start" {
                    "could not start"
                } else {
                    "cancelled"
                }),
                "{error}"
            );
            assert!(!path.exists());
            assert!(receipt(receipts.path())[0]["childPid"].as_u64().is_some());
            // Process observations happen only after production cleanup, never
            // between shutdown and directory removal.
            let events = receipt(receipts.path());
            assert_process_stopped(events[0]["pid"].as_u64().unwrap() as u32);
            assert_process_stopped(events[0]["childPid"].as_u64().unwrap() as u32);
            assert!(started.elapsed() < Duration::from_secs(8));
        }
    }

    #[tokio::test]
    #[cfg(windows)]
    async fn production_cleanup_deadline_keeps_a_persistent_lock_failure_visible() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = private_directory("pr-sniper-locked-cleanup-test-").unwrap();
        let path = root.path().to_path_buf();
        let held = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(path.join("held"))
            .unwrap();
        let started = Instant::now();

        let result = with_directory(root, async { "completed operation" }).await;

        let elapsed = started.elapsed();
        let retained = path.exists();
        drop(held);
        std::fs::remove_dir_all(&path).expect("remove only the owned lock fixture");
        assert!(result.unwrap_err().contains("could not be cleaned up"));
        assert!(retained);
        assert!(elapsed >= Duration::from_secs(1));
        assert!(elapsed < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn inherited_whole_lookup_deadline_limits_the_sdk_stage() {
        let root = tempfile::tempdir().unwrap();
        let started = Instant::now();
        // The inherited budget includes cold startup. Observe pending catalog
        // work before expiry; neither SDK stage may reset this deadline.
        let deadline = started + Duration::from_secs(8);
        let cancel = AtomicBool::new(false);
        let pending = async {
            wait_for_fixture_method(root.path(), "models.list").await;
            assert!(Instant::now() < deadline);
        };
        let (result, ()) =
            tokio::time::timeout_at((deadline + Duration::from_secs(2)).into(), async {
                tokio::join!(
                    query_with_deadline(
                        fixture_options(root.path(), "waiting"),
                        "waiting",
                        &cancel,
                        deadline,
                    ),
                    pending
                )
            })
            .await
            .expect("inherited deadline must bound the SDK and cleanup");
        assert_eq!(
            result.unwrap_err(),
            "Copilot model listing timed out. Retry."
        );
        assert!(Instant::now() >= deadline);
        assert_stopped_and_catalog_only(root.path());
        assert!(receipt(root.path())
            .iter()
            .any(|event| event["method"] == "models.list"));
    }

    #[tokio::test]
    #[ignore = "explicit offline bundled-runtime smoke; no credentials or inference"]
    async fn bundled_runtime_handshakes_offline_without_credentials() {
        let root = private_directory("pr-sniper-offline-").unwrap();
        let reject_provider = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        reject_provider.set_nonblocking(true).unwrap();
        let mut config = options(
            runtime_program().expect("bundled native runtime"),
            root.path(),
            "",
            std::env::vars_os().map(|(key, _)| key),
        )
        .unwrap();
        config.github_token = None;
        config.env_remove.push("COPILOT_SDK_AUTH_TOKEN".into());
        for (key, value) in [
            ("COPILOT_OFFLINE", "true".to_string()),
            (
                "COPILOT_PROVIDER_BASE_URL",
                format!("http://{}/v1", reject_provider.local_addr().unwrap()),
            ),
            ("COPILOT_PROVIDER_TYPE", "openai".to_string()),
            ("COPILOT_MODEL", "synthetic-no-inference".to_string()),
        ] {
            config
                .env_remove
                .retain(|name| !environment_key_eq(name, std::ffi::OsStr::new(key)));
            config.env.push((key.into(), value.into()));
        }
        let path = root.path().to_path_buf();
        let (outcome, cleanup, pid) = with_directory(root, async {
            let client = tokio::time::timeout(Duration::from_secs(30), Client::start(config))
                .await
                .unwrap()
                .unwrap();
            let pid = client.pid().expect("owned bundled runtime process");
            let outcome = tokio::time::timeout(Duration::from_secs(10), async {
                let status = client.get_status().await.unwrap();
                let auth = client.get_auth_status().await.unwrap();
                let models = client.list_models().await;
                (status, auth, models)
            })
            .await;
            let cleanup = tokio::time::timeout(Duration::from_secs(5), client.stop()).await;
            if !matches!(&cleanup, Ok(Ok(()))) {
                client.force_stop();
            }
            (outcome, cleanup, pid)
        })
        .await
        .expect("production cleanup of owned offline runtime");
        let (status, auth, models) = outcome.unwrap();
        assert!(!auth.is_authenticated);
        assert!(models.is_err());
        assert!(
            matches!(reject_provider.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
        );
        assert!(matches!(cleanup, Ok(Ok(()))));
        assert_process_stopped(pid);
        assert!(!path.exists());
        eprintln!("offline bundled runtime: version={}, unauthenticated, model rejection, no provider requests, clean shutdown", status.version);
    }
}
