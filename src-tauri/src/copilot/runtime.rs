use crate::github::{oauth::TokenPair, Identity};
use github_copilot_sdk::{Client, ClientMode, ClientOptions, LogLevel, Model, Transport};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

// No session is ever created: these are client-scoped identity/catalog calls only.
fn options(
    program: PathBuf,
    root: &Path,
    token: &str,
    inherited_keys: impl Iterator<Item = OsString>,
) -> ClientOptions {
    let environment: Vec<(OsString, OsString)> = vec![
        ("HOME".into(), root.into()),
        ("XDG_CONFIG_HOME".into(), root.join("config").into()),
        ("XDG_CACHE_HOME".into(), root.join("cache").into()),
        ("TMPDIR".into(), root.into()),
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LANG".into(), "en_US.UTF-8".into()),
        ("COPILOT_AUTO_UPDATE".into(), "false".into()),
        ("COPILOT_TELEMETRY".into(), "false".into()),
    ];
    // The SDK applies env_remove after its own injected values; retain only
    // keys we replace explicitly, never their ambient values.
    let remove: Vec<_> = inherited_keys
        .filter(|key| {
            !environment.iter().any(|(safe, _)| safe == key)
                && key != "COPILOT_SDK_AUTH_TOKEN"
                && key != "COPILOT_HOME"
                && key != "COPILOT_DISABLE_KEYTAR"
        })
        .collect();
    ClientOptions::new()
        .with_program(program)
        .with_transport(Transport::Stdio)
        .with_cwd(root)
        .with_base_directory(root.join("copilot"))
        .with_mode(ClientMode::Empty)
        .with_github_token(token)
        .with_use_logged_in_user(false)
        .with_log_level(LogLevel::None)
        .with_env(environment)
        .with_env_remove(remove)
}

pub(super) fn models(
    identity: &Identity,
    pair: &TokenPair,
    cancelled: &AtomicBool,
) -> Result<Vec<Model>, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Copilot model lookup cancelled.".into());
    }
    let program = github_copilot_sdk::install_bundled_cli()
        .ok_or("The bundled Copilot runtime is unavailable. Reinstall PR Sniper and retry.")?;
    let directory = tempfile::Builder::new()
        .prefix("pr-sniper-copilot-")
        .tempdir()
        .map_err(|_| "Cannot create private Copilot runtime state. Check local storage.")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot start the Copilot runtime. Retry.")?;
    // SDK transport errors/stderr can contain provider data. Return fixed
    // stage-specific errors instead, with all SDK tracing disabled here.
    let dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let result = tracing::dispatcher::with_default(&dispatch, || {
        runtime.block_on(query(
            options(
                program,
                directory.path(),
                pair.access_token(),
                std::env::vars_os().map(|(k, _)| k),
            ),
            &identity.login,
            cancelled,
        ))
    });
    directory
        .close()
        .map_err(|_| "Copilot stopped, but private runtime state could not be cleaned up.")?;
    result
}

async fn cancellation(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn query(
    options: ClientOptions,
    login: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<Model>, String> {
    let client = tokio::select! {
        _ = cancellation(cancelled) => return Err("Copilot model lookup cancelled.".into()),
        result = tokio::time::timeout(Duration::from_secs(30), Client::start(options)) => {
            result.map_err(|_| "Copilot runtime startup timed out. Retry.")?
                .map_err(|_| "The Copilot runtime could not start. Retry or reinstall PR Sniper.")?
        }
    };
    let outcome = tokio::select! {
        _ = cancellation(cancelled) => Err("Copilot model lookup cancelled.".into()),
        result = tokio::time::timeout(Duration::from_secs(45), async {
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
    match tokio::time::timeout(Duration::from_secs(5), client.stop()).await {
        Ok(Ok(())) => {}
        _ => {
            client.force_stop();
            eprintln!("[copilot] stage=runtime_cleanup outcome=forced_stop");
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

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
        );
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
        );
        assert!(result.unwrap_err().contains("cancelled"));
    }

    fn fixture_options(root: &Path, token: &str) -> ClientOptions {
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join("node"))
            .find(|path| path.is_file())
            .expect("Node is a project test prerequisite");
        let mut config = options(node, root, token, std::env::vars_os().map(|(key, _)| key))
            .with_prefix_args([Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/support/copilot-runtime.mjs")
                .into_os_string()]);
        config
            .env
            .push(("TEST_RECEIPT".into(), root.join("receipt.jsonl").into()));
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
        let process = std::process::Command::new("/bin/ps")
            .args(["-p", &events[0]["pid"].to_string(), "-o", "pid="])
            .output()
            .unwrap();
        assert!(
            !process.status.success(),
            "SDK child must be reaped before returning"
        );
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
            let root = tempfile::tempdir().unwrap();
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
        let root = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let trigger = async {
            tokio::time::sleep(Duration::from_millis(250)).await;
            cancel.store(true, Ordering::SeqCst);
        };
        let (result, ()) = tokio::join!(
            query(fixture_options(root.path(), "waiting"), "waiting", &cancel),
            trigger
        );
        assert!(result.unwrap_err().contains("cancelled"));
        assert_stopped_and_catalog_only(root.path());
    }

    #[tokio::test]
    async fn cancelled_or_failed_startup_does_not_leave_a_child() {
        for token in ["waiting-start", "bad-start"] {
            let root = tempfile::tempdir().unwrap();
            let cancel = AtomicBool::new(false);
            let trigger = async {
                tokio::time::sleep(Duration::from_millis(250)).await;
                cancel.store(true, Ordering::SeqCst);
            };
            let (result, ()) = tokio::join!(
                query(fixture_options(root.path(), token), token, &cancel),
                trigger
            );
            assert!(result.is_err());
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_stopped_and_catalog_only(root.path());
        }
    }

    #[tokio::test]
    #[ignore = "explicit offline bundled-runtime smoke; no credentials or inference"]
    async fn bundled_runtime_handshakes_offline_without_credentials() {
        let root = tempfile::tempdir().unwrap();
        let reject_provider = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        reject_provider.set_nonblocking(true).unwrap();
        let mut config = options(
            github_copilot_sdk::install_bundled_cli().expect("bundled native runtime"),
            root.path(),
            "",
            std::env::vars_os().map(|(key, _)| key),
        );
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
            config.env_remove.retain(|name| name != key);
            config.env.push((key.into(), value.into()));
        }
        let client = tokio::time::timeout(Duration::from_secs(30), Client::start(config))
            .await
            .unwrap()
            .unwrap();
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
        let (status, auth, models) = outcome.unwrap();
        assert!(!auth.is_authenticated);
        assert!(models.is_err());
        assert!(
            matches!(reject_provider.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
        );
        assert!(matches!(cleanup, Ok(Ok(()))));
        eprintln!("offline bundled runtime: version={}, unauthenticated, model rejection, no provider requests, clean shutdown", status.version);
    }
}
