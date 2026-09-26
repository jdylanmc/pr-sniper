use super::*;
use crate::github::{
    macos_keychain::MacKeychainStore,
    token_store::{CredentialStore, RotationSafeStore},
};
use std::time::Duration;

#[test]
fn disconnected_identity_survives_native_keychain_restart_without_touching_repository_role() {
    let namespace = format!(
        "com.jdylanmc.pr-sniper.tests.copilot-{}",
        uuid::Uuid::new_v4()
    );
    let repo = RotationSafeStore::new(MacKeychainStore::with_service(format!("{namespace}.repo")));
    let ai = RotationSafeStore::new(MacKeychainStore::with_service(format!("{namespace}.ai")));
    let repo_account = ActiveAccount::new("101", "fixture-login").unwrap();
    let ai_account =
        ActiveAccount::for_provider(ProviderId::copilot(), "101", "fixture-login").unwrap();
    let pair = TokenPair::new(
        "fixture-access",
        "fixture-refresh",
        Duration::from_secs(60),
        Duration::from_secs(120),
    );
    repo.save_account(&repo_account, &pair, false).unwrap();
    ai.save_account(&ai_account, &pair, false).unwrap();
    ai.clear_account_credentials(&ai_account.provider_account_id())
        .unwrap();
    let restarted =
        RotationSafeStore::new(MacKeychainStore::with_service(format!("{namespace}.ai")));
    let state = GithubAuth::restore_accounts(&restarted, ProviderId::copilot()).unwrap();
    let view = serde_json::to_value(state.copilot_view()).unwrap();
    let repository_pair = repo
        .load(&repo_account.provider_account_id())
        .unwrap()
        .unwrap();
    repo.remove_account(&repo_account.provider_account_id())
        .unwrap();
    ai.remove_account(&ai_account.provider_account_id())
        .unwrap();
    assert!(matches!(
        state.accounts.get("101"),
        Some(GithubAccountState::ReconnectRequired { .. })
    ));
    assert_eq!(view["accounts"][0]["provider"], "copilot");
    assert_eq!(view["accounts"][0]["login"], "fixture-login");
    assert!(!view.to_string().contains("fixture-access"));
    assert_eq!(repository_pair.access_token(), pair.access_token());
    assert_eq!(repository_pair.refresh_token(), pair.refresh_token());
}

#[test]
fn independent_flows_keep_reconnect_identity_and_cancel_pending_secrets() {
    let mut repo = GithubAuth::new();
    let mut ai = GithubAuth::new();
    let (repo_cancel, _) = tokio::sync::oneshot::channel();
    let repo_attempt = repo.start_attempt(None, repo_cancel);
    let (ai_cancel, _) = tokio::sync::oneshot::channel();
    let ai_attempt = ai.start_attempt(Some("101".into()), ai_cancel);
    ai.finish_attempt(
        ai_attempt,
        Ok((
            github::Identity {
                id: "202".into(),
                login: "wrong".into(),
            },
            TokenPair::new(
                "fixture",
                "refresh-fixture",
                Duration::from_secs(60),
                Duration::from_secs(120),
            ),
        )),
    );
    assert!(ai.pending.is_none());
    assert_eq!(ai.failure, Some(GithubAuthFailure::WrongIdentity));
    ai.cancel_attempt();
    assert!(repo.is_active_attempt(repo_attempt));
    assert!(repo.failure.is_none());
}

#[derive(Default)]
struct FakeBackend {
    pairs: Mutex<BTreeMap<String, TokenPair>>,
    loads: Mutex<Vec<String>>,
    identities: Mutex<Vec<String>>,
    refreshes: Mutex<Vec<String>>,
    saves: Mutex<Vec<String>>,
    fail_save: AtomicBool,
    fail_clear: AtomicBool,
    hold_save: AtomicBool,
    hold_clear: AtomicBool,
    hold_identity: AtomicBool,
    hold_refresh: AtomicBool,
    entered_save: tokio::sync::Notify,
    entered_clear: tokio::sync::Notify,
    release_save: tokio::sync::Notify,
    release_clear: tokio::sync::Notify,
    identity_failure: Mutex<Option<GithubAuthFailure>>,
    model_calls: Mutex<Vec<String>>,
    fail_models: AtomicBool,
    entered_identity: tokio::sync::Notify,
    entered_refresh: tokio::sync::Notify,
    release_identity: tokio::sync::Notify,
    release_refresh: tokio::sync::Notify,
    dropped_identity: Arc<std::sync::atomic::AtomicUsize>,
    dropped_refresh: Arc<std::sync::atomic::AtomicUsize>,
}

struct PendingIdentity(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for PendingIdentity {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl Backend for FakeBackend {
    async fn accounts(&self) -> Result<Vec<ActiveAccount>, String> {
        Ok(self
            .pairs
            .lock()
            .unwrap()
            .keys()
            .map(|id| {
                ActiveAccount::for_provider(ProviderId::copilot(), id, format!("fixture-{id}"))
                    .unwrap()
            })
            .collect())
    }
    async fn load(&self, key: ProviderAccountId) -> Result<Option<TokenPair>, String> {
        self.loads.lock().unwrap().push(key.account_id().into());
        Ok(self.pairs.lock().unwrap().get(key.account_id()).cloned())
    }
    async fn save_pair(
        &self,
        key: ProviderAccountId,
        pair: TokenPair,
    ) -> Result<(), GithubAuthFailure> {
        self.saves.lock().unwrap().push(key.account_id().into());
        if key.account_id() == "101" && self.hold_save.load(Ordering::SeqCst) {
            self.entered_save.notify_one();
            self.release_save.notified().await;
        }
        if key.account_id() == "101" && self.fail_save.load(Ordering::SeqCst) {
            return Err(GithubAuthFailure::CredentialsUnavailable);
        }
        self.pairs
            .lock()
            .unwrap()
            .insert(key.account_id().into(), pair);
        Ok(())
    }
    async fn clear(&self, key: ProviderAccountId) -> Result<(), String> {
        if key.account_id() == "101" && self.hold_clear.load(Ordering::SeqCst) {
            self.entered_clear.notify_one();
            self.release_clear.notified().await;
        }
        if key.account_id() == "101" && self.fail_clear.load(Ordering::SeqCst) {
            return Err("Fixture credential deletion failed.".into());
        }
        self.pairs.lock().unwrap().remove(key.account_id());
        Ok(())
    }
    async fn refresh(&self, pair: &TokenPair) -> Result<TokenPair, GithubAuthFailure> {
        let id = pair.access_token();
        self.refreshes.lock().unwrap().push(id.into());
        if id == "101" && self.hold_refresh.load(Ordering::SeqCst) {
            let _pending = PendingIdentity(self.dropped_refresh.clone());
            self.entered_refresh.notify_one();
            self.release_refresh.notified().await;
        }
        Ok(pair_for(id, false))
    }
    async fn identity(&self, pair: &TokenPair) -> Result<github::Identity, GithubAuthFailure> {
        let id = pair.access_token();
        self.identities.lock().unwrap().push(id.into());
        if let Some(failure) = *self.identity_failure.lock().unwrap() {
            return Err(failure);
        }
        if id == "101" && self.hold_identity.load(Ordering::SeqCst) {
            let _pending = PendingIdentity(self.dropped_identity.clone());
            self.entered_identity.notify_one();
            self.release_identity.notified().await;
        }
        Ok(github::Identity {
            id: id.into(),
            login: format!("fixture-{id}"),
        })
    }

    async fn models(
        &self,
        identity: github::Identity,
        _pair: TokenPair,
        _operation: Operation,
    ) -> Result<Vec<github_copilot_sdk::Model>, String> {
        self.model_calls.lock().unwrap().push(identity.id);
        if self.fail_models.load(Ordering::SeqCst) {
            Err("Fixture catalog lookup failed.".into())
        } else {
            Ok(Vec::new())
        }
    }
}

fn account_view(integration: &Integration<FakeBackend>, id: &str) -> serde_json::Value {
    serde_json::to_value(integration.view().unwrap()).unwrap()["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|account| account["account_id"] == id)
        .unwrap()
        .clone()
}

#[tokio::test]
async fn failed_disconnect_retains_secret_and_reports_pending_then_failed_until_retry() {
    let integration = fake_integration();
    let other = account_view(&integration, "202");
    integration.backend.fail_clear.store(true, Ordering::SeqCst);
    integration.backend.hold_clear.store(true, Ordering::SeqCst);
    let disconnect = {
        let integration = integration.clone();
        tokio::spawn(async move { integration.disconnect("101").await })
    };
    integration.backend.entered_clear.notified().await;
    assert_eq!(
        account_view(&integration, "101")["reason"],
        "disconnect_pending"
    );
    integration.backend.release_clear.notify_one();
    assert_eq!(
        disconnect.await.unwrap().err().unwrap(),
        "Copilot credentials could not be deleted securely. Retry disconnect."
    );
    let failed = account_view(&integration, "101");
    assert_eq!(failed["state"], "reconnect_required");
    assert_eq!(failed["reason"], "disconnect_failed");
    assert!(integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .contains_key("101"));
    let operation = integration
        .operation("101", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    assert!(integration.credential("101", &operation).await.is_err());
    assert!(integration.backend.loads.lock().unwrap().is_empty());
    assert!(integration.backend.identities.lock().unwrap().is_empty());
    assert_eq!(account_view(&integration, "202"), other);
    integration
        .backend
        .hold_clear
        .store(false, Ordering::SeqCst);
    integration
        .backend
        .fail_clear
        .store(false, Ordering::SeqCst);
    integration.disconnect("101").await.unwrap();
    assert_eq!(account_view(&integration, "101")["reason"], "disconnected");
    assert!(!integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .contains_key("101"));
    assert_eq!(account_view(&integration, "202"), other);
}

#[tokio::test]
async fn rotated_pair_save_failure_blocks_only_affected_account_and_explicit_retry_recovers() {
    for model_lookup in [false, true] {
        let integration = fake_integration();
        let other = account_view(&integration, "202");
        let expired = pair_for("101", true);
        integration
            .backend
            .pairs
            .lock()
            .unwrap()
            .insert("101".into(), expired.clone());
        integration.backend.fail_save.store(true, Ordering::SeqCst);
        let operation = integration
            .operation("101", Instant::now() + OPERATION_LIMIT)
            .unwrap();
        let error = if model_lookup {
            integration.models("101", &operation).await.unwrap_err()
        } else {
            integration.credential("101", &operation).await.unwrap_err()
        };
        assert_eq!(
            error,
            verification_error(GithubAuthFailure::CredentialsUnavailable)
        );
        assert_eq!(
            account_view(&integration, "101")["state"],
            "reconnect_required"
        );
        assert_eq!(
            account_view(&integration, "101")["reason"],
            "credentials_unavailable"
        );
        assert_eq!(account_view(&integration, "202"), other);
        assert_eq!(
            integration.backend.refreshes.lock().unwrap().as_slice(),
            ["101"]
        );
        assert_eq!(
            integration.backend.saves.lock().unwrap().as_slice(),
            ["101"]
        );
        assert_eq!(integration.backend.pairs.lock().unwrap()["101"], expired);
        assert!(integration.backend.identities.lock().unwrap().is_empty());
        assert!(integration.backend.model_calls.lock().unwrap().is_empty());

        integration.backend.fail_save.store(false, Ordering::SeqCst);
        let retry = integration
            .operation("101", Instant::now() + OPERATION_LIMIT)
            .unwrap();
        integration.credential("101", &retry).await.unwrap();
        assert_eq!(account_view(&integration, "101")["state"], "connected");
        assert_eq!(account_view(&integration, "202"), other);
        assert!(
            !integration.backend.pairs.lock().unwrap()["101"].access_is_expired(SystemTime::now())
        );
        assert_eq!(
            integration.backend.identities.lock().unwrap().as_slice(),
            ["101"]
        );
    }
}

#[tokio::test]
async fn cancelled_rotation_still_reports_failed_persistence_but_stale_generation_cannot() {
    for cancelled in [false, true] {
        for replacement in [false, true] {
            let integration = fake_integration();
            let other = account_view(&integration, "202");
            integration
                .backend
                .pairs
                .lock()
                .unwrap()
                .insert("101".into(), pair_for("101", true));
            integration.backend.fail_save.store(true, Ordering::SeqCst);
            integration.backend.hold_save.store(true, Ordering::SeqCst);
            let operation = integration
                .operation("101", Instant::now() + OPERATION_LIMIT)
                .unwrap();
            let active = {
                let integration = integration.clone();
                let operation = operation.clone();
                tokio::spawn(async move { integration.credential("101", &operation).await })
            };
            integration.backend.entered_save.notified().await;
            operation.cancelled.store(cancelled, Ordering::SeqCst);
            if replacement {
                integration
                    .account("101")
                    .unwrap()
                    .invalidate(|| {
                        integration.auth.lock().unwrap().accounts.insert(
                            "101".into(),
                            GithubAccountState::Connected(github::Identity {
                                id: "101".into(),
                                login: "replacement-generation".into(),
                            }),
                        );
                        Ok(())
                    })
                    .unwrap();
            }
            integration.backend.release_save.notify_one();
            assert!(active.await.unwrap().is_err());
            assert!(integration.backend.identities.lock().unwrap().is_empty());
            let view = account_view(&integration, "101");
            if replacement {
                assert_eq!(view["state"], "connected");
                assert_eq!(view["login"], "replacement-generation");
            } else {
                assert_eq!(view["state"], "reconnect_required");
                assert_eq!(view["reason"], "credentials_unavailable");
            }
            assert_eq!(account_view(&integration, "202"), other);
        }
    }
}

#[tokio::test]
async fn transient_identity_failures_do_not_erase_verified_sign_in() {
    for failure in [GithubAuthFailure::Network, GithubAuthFailure::Provider] {
        let integration = fake_integration();
        let original = account_view(&integration, "101");
        *integration.backend.identity_failure.lock().unwrap() = Some(failure);
        let operation = integration
            .operation("101", Instant::now() + OPERATION_LIMIT)
            .unwrap();
        assert!(integration.credential("101", &operation).await.is_err());
        assert_eq!(account_view(&integration, "101"), original);
    }
    let integration = fake_integration();
    let original = account_view(&integration, "101");
    integration
        .backend
        .fail_models
        .store(true, Ordering::SeqCst);
    let operation = integration
        .operation("101", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    assert_eq!(
        integration.models("101", &operation).await.unwrap_err(),
        "Fixture catalog lookup failed."
    );
    assert_eq!(
        integration.backend.model_calls.lock().unwrap().as_slice(),
        ["101"]
    );
    assert_eq!(account_view(&integration, "101"), original);
}

#[tokio::test]
async fn persistence_failure_after_deadline_is_still_published_and_stale_delete_failure_is_not() {
    let integration = fake_integration();
    integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .insert("101".into(), pair_for("101", true));
    integration.backend.hold_save.store(true, Ordering::SeqCst);
    integration.backend.fail_save.store(true, Ordering::SeqCst);
    let operation = integration
        .operation("101", Instant::now() + Duration::from_millis(100))
        .unwrap();
    let deadline = operation.deadline;
    let active = {
        let integration = integration.clone();
        tokio::spawn(async move { integration.credential("101", &operation).await })
    };
    integration.backend.entered_save.notified().await;
    tokio::time::sleep_until(deadline.into()).await;
    integration.backend.release_save.notify_one();
    assert_eq!(
        active.await.unwrap().unwrap_err(),
        verification_error(GithubAuthFailure::CredentialsUnavailable)
    );
    assert_eq!(
        account_view(&integration, "101")["reason"],
        "credentials_unavailable"
    );
    assert!(integration.backend.identities.lock().unwrap().is_empty());
    assert!(integration.backend.model_calls.lock().unwrap().is_empty());

    integration.backend.hold_clear.store(true, Ordering::SeqCst);
    integration.backend.fail_clear.store(true, Ordering::SeqCst);
    let deleting = {
        let integration = integration.clone();
        tokio::spawn(async move { integration.disconnect("101").await })
    };
    integration.backend.entered_clear.notified().await;
    integration.invalidate("101").unwrap();
    let replacement = account_view(&integration, "101");
    integration.backend.release_clear.notify_one();
    assert!(deleting.await.unwrap().is_err());
    assert_eq!(account_view(&integration, "101"), replacement);
}

fn pair_for(id: &str, expired: bool) -> TokenPair {
    TokenPair::new(
        id,
        format!("refresh-{id}"),
        if expired {
            Duration::ZERO
        } else {
            Duration::from_secs(60)
        },
        Duration::from_secs(120),
    )
}

fn fake_integration() -> Arc<Integration<FakeBackend>> {
    let integration = Arc::new(Integration::with_backend(FakeBackend::default()));
    integration.restored.set(()).unwrap();
    for id in ["101", "202"] {
        integration
            .backend
            .pairs
            .lock()
            .unwrap()
            .insert(id.into(), pair_for(id, false));
        integration.auth.lock().unwrap().accounts.insert(
            id.into(),
            GithubAccountState::Connected(github::Identity {
                id: id.into(),
                login: format!("fixture-{id}"),
            }),
        );
    }
    integration
}

#[tokio::test]
async fn cancelled_preflights_skip_provider_work_and_other_accounts_do_not_wait() {
    let integration = fake_integration();
    integration
        .backend
        .hold_identity
        .store(true, Ordering::SeqCst);
    let first = integration
        .operation("101", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    let active = {
        let integration = integration.clone();
        let operation = first.clone();
        tokio::spawn(async move { integration.credential("101", &operation).await })
    };
    integration.backend.entered_identity.notified().await;
    let mut queued = Vec::new();
    for _ in 0..3 {
        let operation = integration
            .operation("101", Instant::now() + OPERATION_LIMIT)
            .unwrap();
        let cancelled = operation.cancelled.clone();
        let integration = integration.clone();
        queued.push(tokio::spawn(async move {
            integration.credential("101", &operation).await
        }));
        tokio::task::yield_now().await;
        cancelled.store(true, Ordering::SeqCst);
    }
    let second = integration
        .operation("202", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        integration.credential("202", &second).await.unwrap();
        integration.view().unwrap();
        integration.disconnect("202").await.unwrap();
        for task in queued {
            assert!(task.await.unwrap().is_err());
        }
    })
    .await
    .expect("account B and cancelled queue must not wait on account A");
    assert_eq!(
        integration
            .backend
            .loads
            .lock()
            .unwrap()
            .iter()
            .filter(|id| *id == "101")
            .count(),
        1
    );
    assert_eq!(
        integration
            .backend
            .identities
            .lock()
            .unwrap()
            .iter()
            .filter(|id| *id == "101")
            .count(),
        1
    );
    assert!(integration.backend.refreshes.lock().unwrap().is_empty());
    first.cancelled.store(true, Ordering::SeqCst);
    assert!(tokio::time::timeout(Duration::from_secs(1), active)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(
        integration.backend.dropped_identity.load(Ordering::SeqCst),
        1,
        "the in-flight identity future was actually dropped"
    );
}

#[tokio::test]
async fn disconnect_fences_late_identity_publication_and_keeps_other_accounts() {
    let integration = fake_integration();
    integration
        .backend
        .hold_identity
        .store(true, Ordering::SeqCst);
    let old = integration
        .operation("101", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    let active = {
        let integration = integration.clone();
        let operation = old.clone();
        tokio::spawn(async move { integration.credential("101", &operation).await })
    };
    integration.backend.entered_identity.notified().await;
    tokio::time::timeout(Duration::from_secs(1), integration.disconnect("101"))
        .await
        .unwrap()
        .unwrap();
    assert!(active.await.unwrap().is_err());
    integration.backend.release_identity.notify_one();
    assert!(integration
        .publish_verification(
            "101",
            &old,
            &Ok((
                github::Identity {
                    id: "101".into(),
                    login: "late".into()
                },
                pair_for("101", false)
            ))
        )
        .is_err());
    assert!(!integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .contains_key("101"));
    assert!(integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .contains_key("202"));
    assert!(matches!(
        integration.auth.lock().unwrap().accounts.get("101"),
        Some(GithubAccountState::ReconnectRequired {
            reason: GithubAuthFailure::Disconnected,
            ..
        })
    ));
}

#[tokio::test]
async fn one_deadline_includes_account_queue_and_identity_preflight() {
    let integration = fake_integration();
    integration
        .backend
        .hold_identity
        .store(true, Ordering::SeqCst);
    let account = integration.account("101").unwrap();
    let guard = account.gate.clone().lock_owned().await;
    let started = Instant::now();
    let operation = integration
        .operation("101", started + Duration::from_millis(200))
        .unwrap();
    let release = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(guard);
    });
    let error = integration.credential("101", &operation).await.unwrap_err();
    release.await.unwrap();
    assert!(error.contains("timed out"));
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the preflight must not receive a fresh budget after queueing"
    );
    assert_eq!(
        integration.backend.dropped_identity.load(Ordering::SeqCst),
        1
    );
    let held = account.gate.lock().await;
    integration
        .auth
        .lock()
        .unwrap()
        .set_failure("101", GithubAuthFailure::VerificationPending);
    let queued = integration
        .operation("101", Instant::now() + Duration::from_millis(30))
        .unwrap();
    assert!(integration
        .credential("101", &queued)
        .await
        .unwrap_err()
        .contains("timed out"));
    drop(held);
    assert!(matches!(
        integration.auth.lock().unwrap().accounts.get("101"),
        Some(GithubAccountState::ReconnectRequired {
            reason: GithubAuthFailure::Timeout,
            ..
        })
    ));
    assert_eq!(
        integration.backend.loads.lock().unwrap().len(),
        1,
        "expired queue entries must never start credential work"
    );
}

#[tokio::test]
async fn cancellation_finishes_inflight_rotation_but_never_starts_identity_or_revives_disconnected_account(
) {
    let integration = fake_integration();
    integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .insert("101".into(), pair_for("101", true));
    integration
        .backend
        .hold_refresh
        .store(true, Ordering::SeqCst);
    let operation = integration
        .operation("101", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    let active = {
        let integration = integration.clone();
        let operation = operation.clone();
        tokio::spawn(async move { integration.credential("101", &operation).await })
    };
    integration.backend.entered_refresh.notified().await;
    operation.cancelled.store(true, Ordering::SeqCst);
    let other = integration
        .operation("202", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    integration.credential("202", &other).await.unwrap();
    let disconnect = {
        let integration = integration.clone();
        tokio::spawn(async move { integration.disconnect("101").await })
    };
    tokio::task::yield_now().await;
    integration.backend.release_refresh.notify_one();
    assert!(active.await.unwrap().is_err());
    disconnect.await.unwrap().unwrap();
    assert_eq!(
        integration.backend.saves.lock().unwrap().as_slice(),
        ["101"]
    );
    assert_eq!(
        integration.backend.identities.lock().unwrap().as_slice(),
        ["202"]
    );
    assert!(!integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .contains_key("101"));
}

#[tokio::test]
async fn whole_operation_deadline_also_bounds_inflight_refresh() {
    let integration = fake_integration();
    integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .insert("101".into(), pair_for("101", true));
    integration
        .backend
        .hold_refresh
        .store(true, Ordering::SeqCst);
    let operation = integration
        .operation("101", Instant::now() + Duration::from_millis(30))
        .unwrap();
    let error = integration.credential("101", &operation).await.unwrap_err();
    assert!(error.contains("timed out during credential refresh"));
    assert_eq!(
        integration.backend.dropped_refresh.load(Ordering::SeqCst),
        1
    );
    assert!(integration.backend.identities.lock().unwrap().is_empty());
    assert!(integration.backend.saves.lock().unwrap().is_empty());
}

#[tokio::test]
async fn newly_requested_work_cannot_revive_an_account_while_disconnect_waits_for_its_lease() {
    let integration = fake_integration();
    let account = integration.account("101").unwrap();
    let held = account.gate.lock().await;
    let disconnect = {
        let integration = integration.clone();
        tokio::spawn(async move { integration.disconnect("101").await })
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(
                integration.auth.lock().unwrap().accounts.get("101"),
                Some(GithubAccountState::ReconnectRequired {
                    reason: GithubAuthFailure::DisconnectPending,
                    ..
                })
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let new = integration
        .operation("101", Instant::now() + OPERATION_LIMIT)
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), integration.credential("101", &new))
            .await
            .unwrap()
            .is_err()
    );
    assert!(integration.backend.loads.lock().unwrap().is_empty());
    assert!(integration
        .publish_verification(
            "101",
            &new,
            &Ok((
                github::Identity {
                    id: "101".into(),
                    login: "late".into()
                },
                pair_for("101", false)
            ))
        )
        .is_err());
    drop(held);
    disconnect.await.unwrap().unwrap();
    assert!(!integration
        .backend
        .pairs
        .lock()
        .unwrap()
        .contains_key("101"));
    assert!(matches!(
        integration.auth.lock().unwrap().accounts.get("101"),
        Some(GithubAccountState::ReconnectRequired {
            reason: GithubAuthFailure::Disconnected,
            ..
        })
    ));
}

#[tokio::test]
async fn registry_restoration_publishes_accounts_without_waiting_for_a_slow_identity() {
    let integration = Arc::new(Integration::with_backend(FakeBackend::default()));
    for id in ["101", "202"] {
        integration
            .backend
            .pairs
            .lock()
            .unwrap()
            .insert(id.into(), pair_for(id, false));
    }
    integration
        .backend
        .hold_identity
        .store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(1), integration.restore())
        .await
        .unwrap()
        .unwrap();
    integration.backend.entered_identity.notified().await;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(
                integration.auth.lock().unwrap().accounts.get("202"),
                Some(GithubAccountState::Connected(_))
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(integration.view().unwrap().accounts.len(), 2);
        integration.disconnect("202").await.unwrap();
    })
    .await
    .expect("initial restoration cannot hold other accounts behind account A's provider request");
    integration.shutdown();
    tokio::time::timeout(Duration::from_secs(1), async {
        while integration.backend.dropped_identity.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("quit cancels an in-flight restored identity");
}
