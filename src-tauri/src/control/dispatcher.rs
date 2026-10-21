use super::{
    operations::{operation, storage, value, AccountKind, Command, ResourceKind},
    revision, work, Error,
};
use crate::{
    capacity::{Batch, Coordinator},
    monitoring::Monitor,
    storage::Store,
    GithubAuth,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Mutex};
#[cfg(test)]
mod tests;

/// Platform/provider collaborators only. Routing, receipts, validation and
/// scheduling remain production code in isolated dispatcher tests.
pub(super) trait Runtime {
    fn store(&self) -> &Mutex<Store>;
    fn monitor(&self) -> &Mutex<Monitor>;
    fn ai(&self) -> &Coordinator;
    fn auth(&self) -> &Mutex<GithubAuth>;
    fn copilot_auth(&self) -> &Mutex<GithubAuth>;
    fn generations(&self) -> &Mutex<BTreeMap<String, u64>>;
    fn resolve(
        &self,
        account: &str,
        url: &str,
    ) -> Result<(crate::github::intake::ResolvedTarget, u64), Error>;
    fn launch(&self, batch: Batch);
    fn start_checks(&self) -> Result<(), Error>;
    fn login_status(&self) -> Result<Value, Error>;
    fn quitting(&self) -> bool;
}

pub(super) struct Native<'a> {
    pub app: &'a tauri::AppHandle,
    pub host: &'a crate::Host,
}
impl Runtime for Native<'_> {
    fn store(&self) -> &Mutex<Store> {
        &self.host.store
    }
    fn monitor(&self) -> &Mutex<Monitor> {
        &self.host.monitor
    }
    fn ai(&self) -> &Coordinator {
        &self.host.ai
    }
    fn auth(&self) -> &Mutex<GithubAuth> {
        &self.host.github_auth
    }
    fn copilot_auth(&self) -> &Mutex<GithubAuth> {
        &self.host.copilot.auth
    }
    fn generations(&self) -> &Mutex<BTreeMap<String, u64>> {
        &self.host.github_generations
    }
    fn resolve(
        &self,
        account: &str,
        url: &str,
    ) -> Result<(crate::github::intake::ResolvedTarget, u64), Error> {
        crate::repository_read(
            self.generations(),
            self.auth(),
            account,
            || crate::acquire_github_session(self.host, account),
            |identity, client, generation| {
                Ok((
                    client.resolve_repository_target(url, &identity.id)?,
                    generation,
                ))
            },
        )
        .map_err(operation)
    }
    fn launch(&self, batch: Batch) {
        crate::capacity::launch_batch(self.app, batch);
    }
    fn start_checks(&self) -> Result<(), Error> {
        crate::start_checks(self.app, false).map_err(operation)
    }
    fn login_status(&self) -> Result<Value, Error> {
        value(self.host.registration.status().map_err(operation)?)
    }
    fn quitting(&self) -> bool {
        self.host.quitting.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::Inspect
            | Command::Resources { .. }
            | Command::Accounts { .. }
            | Command::Queue { .. }
            | Command::Health { .. }
            | Command::Detail { .. }
            | Command::Target { .. }
            | Command::Watch { .. }
            | Command::RetryReview { .. }
            | Command::Retry { .. }
    ) || matches!(command, Command::Save { edit, .. } if !matches!(edit, crate::storage::ResourceEdit::Agent {..}))
}

fn page(values: Vec<Value>, offset: usize, limit: Option<usize>) -> Result<Value, Error> {
    let limit = limit.unwrap_or(25);
    if !(1..=100).contains(&limit) {
        return Err(Error::new("invalid_limit", ()));
    }
    let total = values.len();
    let mut selected = Vec::new();
    let mut size = 0;
    for entry in values.into_iter().skip(offset).take(limit) {
        let bytes = serde_json::to_vec(&entry).map_err(|_| storage(()))?.len();
        if bytes > super::MAX_FRAME - 4096 {
            return Err(Error::new("resource_too_large", ()));
        }
        if !selected.is_empty() && size + bytes > 384 * 1024 {
            break;
        }
        size += bytes;
        selected.push(entry);
    }
    let end = offset.saturating_add(selected.len());
    Ok(json!({"values":selected,"total":total,"next_offset":(end<total).then_some(end)}))
}

pub(super) fn dispatch(runtime: &impl Runtime, command: Command) -> Result<Value, Error> {
    match command {
        Command::Watch {
            expected,
            url,
            account_generation,
            review_again,
            overrides,
        } => watch(
            runtime,
            expected,
            url,
            account_generation,
            review_again,
            overrides,
        ),
        Command::RetryReview { candidate_key } => {
            if runtime.quitting() {
                return Err(Error::new("quitting", ()));
            }
            let batch = {
                let store = runtime.store().lock().map_err(|_| storage(()))?;
                let candidates = crate::review::host::candidates(&store).map_err(storage)?;
                let candidate = candidates
                    .iter()
                    .find(|candidate| candidate.key == candidate_key)
                    .ok_or_else(|| Error::new("missing_destination", ()))?;
                match candidate.run.as_ref().map(|run| &run.operation.state) {
                    Some(crate::monitoring::OperationState::Completed) => {
                        return Err(Error::new(
                            "review_again_required",
                            "Use explicit watch review_again for completed work.",
                        ))
                    }
                    Some(
                        crate::monitoring::OperationState::Failed
                        | crate::monitoring::OperationState::ManualRetry,
                    ) => {}
                    _ => {
                        return Err(Error::new(
                            "review_not_retryable",
                            "Only failed or cancelled AI work can be retried.",
                        ))
                    }
                }
                crate::review::host::request_batch(
                    &store,
                    runtime.ai(),
                    &candidate_key,
                    crate::now_seconds().map_err(storage)?,
                )
                .map_err(operation)?
            };
            runtime.launch(batch);
            Ok(json!({"state":"requested","candidate_key":candidate_key}))
        }
        Command::Retry { operation_id } => {
            {
                let store = runtime.store().lock().map_err(|_| storage(()))?;
                runtime
                    .monitor()
                    .lock()
                    .map_err(|_| storage(()))?
                    .manual_retry_operation(
                        &store,
                        &operation_id,
                        crate::now_seconds().map_err(storage)?,
                    )
                    .map_err(operation)?;
            }
            runtime.start_checks()?;
            Ok(json!({"state":"requested"}))
        }
        Command::Inspect => {
            let accounts = runtime
                .auth()
                .lock()
                .map_err(|_| storage(()))?
                .accounts
                .len();
            let copilot = runtime
                .copilot_auth()
                .lock()
                .map_err(|_| storage(()))?
                .accounts
                .len();
            let store = runtime.store().lock().map_err(|_| storage(()))?;
            let settings = store.load_settings().map_err(storage)?;
            let notifications = store.load_notifications().map_err(storage)?;
            Ok(json!({
                "revision":revision(&settings),
                "counts":{"agents":settings.agents.len(),"doctrines":settings.doctrines.len(),
                    "voices":settings.voices.len(),"repositories":settings.repositories.len(),
                    "provider_accounts":accounts,"copilot_accounts":copilot,
                    "queue_jobs":store.load_queue().map_err(storage)?.len(),
                    "repository_health":store.load_monitoring_state().map_err(storage)?.health.len(),
                    "pending_watch_configuration":work::holds(&store).map_err(storage)?.len()},
                "automation":{"paused":store.load_automation().map_err(storage)?.paused,"capacity":settings.capacity},
                "theme":settings.theme,"launch_at_login":settings.launch_at_login,
                "notifications":{"enabled":notifications.enabled,"muted":notifications.muted},
                "login_registration":runtime.login_status()?
            }))
        }
        Command::Accounts {
            kind,
            offset,
            limit,
        } => {
            let generations = runtime.generations().lock().map_err(|_| storage(()))?;
            let accounts = match kind {
                AccountKind::Repository => {
                    runtime
                        .auth()
                        .lock()
                        .map_err(|_| storage(()))?
                        .repository_view(&generations)
                        .accounts
                }
                AccountKind::Copilot => {
                    runtime
                        .copilot_auth()
                        .lock()
                        .map_err(|_| storage(()))?
                        .copilot_view()
                        .accounts
                }
            };
            page(
                accounts.into_iter().map(|a| value(a).unwrap()).collect(),
                offset,
                limit,
            )
        }
        Command::Resources {
            kind,
            offset,
            limit,
        } => {
            let store = runtime.store().lock().map_err(|_| storage(()))?;
            let settings = store.load_settings().map_err(storage)?;
            let values = match kind {
                ResourceKind::Agents => value(&settings.agents)?,
                ResourceKind::Doctrines => value(&settings.doctrines)?,
                ResourceKind::Voices => value(&settings.voices)?,
                ResourceKind::Repositories => value(&settings.repositories)?,
                ResourceKind::Preferences => json!([settings.global_preferences()]),
            };
            let mut result = page(values.as_array().unwrap().clone(), offset, limit)?;
            result["revision"] = json!(revision(&settings));
            Ok(result)
        }
        Command::Target { url } => {
            let target = crate::github::intake::repository_target(&url).map_err(operation)?;
            let store = runtime.store().lock().map_err(|_| storage(()))?;
            let settings = store.load_settings().map_err(storage)?;
            Ok(
                json!({"target":target,"repository_ids":settings.repositories.iter()
                .filter(|r|r.name==target.name).map(|r|&r.id).take(100).collect::<Vec<_>>(),
                "revision":revision(&settings)}),
            )
        }
        Command::Queue { offset, limit } => {
            let store = runtime.store().lock().map_err(|_| storage(()))?;
            let snapshot = crate::queue::snapshot(&store, vec![]).map_err(storage)?;
            let summaries=snapshot.items.iter().map(|item|json!({
                "id":item.id,"configuration_id":item.job.configuration_id,"number":item.job.number,
                "title":item.job.title.chars().take(256).collect::<String>(), "state":item.state,
                "failure_class":item.failure_class,"failure_cause":item.failure_cause,
                "dismiss_token":item.dismiss_token,
                "reviews":snapshot.reviews.iter().filter(|r|item.review_keys.contains(&r.key))
                    .take(25)
                    .map(|r|json!({"candidate_key":r.key,"operation_id":r.run.as_ref().map(|r|&r.operation.id),
                        "state":r.run.as_ref().map(|r|&r.operation.state)})).collect::<Vec<_>>(),
                "review_count":item.review_keys.len()
            })).collect();
            page(summaries, offset, limit)
        }
        Command::Health { offset, limit } => {
            let _generations = runtime.generations().lock().map_err(|_| storage(()))?;
            let accounts = runtime
                .auth()
                .lock()
                .map_err(|_| storage(()))?
                .monitoring_accounts();
            let store = runtime.store().lock().map_err(|_| storage(()))?;
            let mut monitor = runtime.monitor().lock().map_err(|_| storage(()))?;
            monitor
                .synchronize_configuration(
                    &store,
                    &accounts,
                    crate::now_seconds().map_err(storage)?,
                )
                .map_err(storage)?;
            let values = monitor.snapshot().into_iter().map(|health| {
                let op = health.operation.as_ref();
                json!({
                    "configuration_id":health.repository_id,"repository_name":health.name,
                    "provider_account_id":health.provider_account_id,"account_login":health.account_login,
                    "provider_repository_id":health.provider_repository_id,"enabled":health.enabled,
                    "schedule_available":health.schedule_available,"in_flight":health.in_flight,
                    "operation_id":op.map(|o|&o.id),"state":op.map(|o|&o.state),
                    "failure":op.and_then(|o|o.failure.as_ref()),
                    "failure_class":op.and_then(|o|o.failure_class()),
                    "retryable":op.is_some_and(|o|matches!(o.state,
                        crate::monitoring::OperationState::Failed | crate::monitoring::OperationState::ManualRetry)),
                    "last_failure":health.last_failure.map(|s|s.chars().take(256).collect::<String>()),
                    "last_attempt":health.last_attempt,"last_success":health.last_success,
                    "next_run":health.next_run,"next_attempt_at":op.and_then(|o|o.next_attempt_at)
                })
            }).collect();
            page(values, offset, limit)
        }
        Command::Detail {
            item_id,
            offset,
            limit,
        } => {
            let store = runtime.store().lock().map_err(|_| storage(()))?;
            let snapshot = crate::queue::snapshot(&store, vec![]).map_err(storage)?;
            let item = snapshot
                .items
                .iter()
                .find(|i| i.id == item_id || i.aliases.contains(&item_id))
                .ok_or_else(|| Error::new("missing_destination", ()))?;
            let reviews: Vec<_> = snapshot
                .reviews
                .iter()
                .filter(|r| item.review_keys.contains(&r.key))
                .collect();
            let operation_ids: Vec<_> = reviews
                .iter()
                .filter_map(|r| r.run.as_ref().map(|r| &r.operation.id))
                .collect();
            let detail = json!({"item":item,"reviews":reviews,
                "publications":snapshot.publications.iter().filter(|p|operation_ids.contains(&&p.review_operation_id)).collect::<Vec<_>>(),
                "follow_ups":snapshot.follow_ups.iter().filter(|f|item.follow_up_ids.contains(&f.run.id)).collect::<Vec<_>>()});
            let text = serde_json::to_string(&detail).map_err(|_| storage(()))?;
            let limit = limit.unwrap_or(32 * 1024);
            if !(1..=64 * 1024).contains(&limit) {
                return Err(Error::new("invalid_limit", ()));
            }
            let total = text.chars().count();
            let chunk: String = text.chars().skip(offset).take(limit).collect();
            let end = offset.saturating_add(chunk.chars().count());
            Ok(
                json!({"encoding":"json-text","revision":revision(&detail),"text":chunk,
                "offset":offset,"next_offset":(end<total).then_some(end),"total":total}),
            )
        }
        Command::Save {
            revision: expected,
            edit,
            account_generation,
            allow_grants,
        } => {
            let settings = {
                let generations = runtime.generations().lock().map_err(|_| storage(()))?;
                let auth = runtime.auth().lock().map_err(|_| storage(()))?;
                crate::validate_repository_save_account(
                    &auth,
                    &generations,
                    &edit,
                    account_generation,
                )
                .map_err(operation)?;
                let store = runtime.store().lock().map_err(|_| storage(()))?;
                super::operations::save_checked(&store, &expected, edit, allow_grants)?
            };
            let saved = crate::finish_committed_settings(
                runtime.generations(),
                runtime.auth(),
                runtime.store(),
                runtime.monitor(),
                settings,
            );
            Ok(
                json!({"state":"saved","revision":revision(&saved.settings),"warning":saved.warning}),
            )
        }
        _ => Err(Error::new("invalid_request", ())),
    }
}

fn watch(
    runtime: &impl Runtime,
    expected: crate::storage::Repository,
    url: String,
    account_generation: u64,
    review_again: bool,
    overrides: BTreeMap<String, super::RequestOverride>,
) -> Result<Value, Error> {
    let target = crate::github::intake::repository_target(&url).map_err(operation)?;
    let number = target
        .pull_request_number
        .ok_or_else(|| Error::new("pr_url_required", ()))?;
    if expected.name != target.name {
        return Err(Error::new("target_mismatch", ()));
    }
    if !expected.enabled {
        return Err(Error::new("repository_disabled", ()));
    }
    let account = expected
        .provider_account_id
        .as_deref()
        .ok_or_else(|| Error::new("account_required", ()))?;
    let read_at = crate::now_seconds().map_err(storage)?;
    let (resolved, observed) = runtime.resolve(account, &url)?;
    let generations = runtime.generations().lock().map_err(|_| storage(()))?;
    let auth = runtime.auth().lock().map_err(|_| storage(()))?;
    crate::validate_explicit_intake_generation(
        &auth,
        &generations,
        account,
        account_generation,
        observed,
    )
    .map_err(operation)?;
    let store = runtime.store().lock().map_err(|_| storage(()))?;
    let now = crate::now_seconds().map_err(storage)?;
    if !store
        .load_settings()
        .map_err(storage)?
        .repositories
        .iter()
        .any(|r| r == &expected)
    {
        return Err(Error::new("conflict", "Repository configuration changed."));
    }
    let prior_hold = work::holds(&store)
        .map_err(storage)?
        .iter()
        .any(|(id, n)| id == &expected.id && *n == number);
    let before = revision(&(
        store.load_queue_state().map_err(storage)?,
        store.load_pr_states().map_err(storage)?,
    ));
    if !overrides.is_empty() && !prior_hold {
        work::hold(&store, &expected.id, number, true).map_err(storage)?;
    }
    let configured = (|| -> Result<usize, Error> {
        runtime
            .monitor()
            .lock()
            .map_err(|_| storage(()))?
            .admit_explicit_pull_request(
                &store,
                &auth.monitoring_accounts(),
                &expected,
                resolved,
                read_at,
                now,
            )
            .map_err(|e| Error::new("intake_failed", e))?;
        if review_again {
            work::request_again(&store, &expected.id, number, now).map_err(|e| {
                Error::new("watch_configuration", json!({"admitted":true,"reason":e}))
            })?;
        }
        work::apply_overrides(&store, &expected.id, number, &overrides)
            .map_err(|e| Error::new("watch_configuration", json!({"admitted":true,"reason":e})))?;
        // An ordinary watch cannot erase someone else's interrupted override.
        if !overrides.is_empty() {
            work::hold(&store, &expected.id, number, false).map_err(storage)?;
        }
        work::prioritize(&store, &expected.id, number, now).map_err(storage)
    })();
    let prioritized = match configured {
        Ok(value) => value,
        Err(error) => {
            if !prior_hold
                && !overrides.is_empty()
                && before
                    == revision(&(
                        store.load_queue_state().map_err(storage)?,
                        store.load_pr_states().map_err(storage)?,
                    ))
            {
                work::hold(&store, &expected.id, number, false).map_err(storage)?;
            }
            return Err(error);
        }
    };
    let batch = runtime
        .ai()
        .dispatch(&store, now)
        .map_err(|e| Error::new("dispatch_failed", json!({"admitted":true,"reason":e})))?;
    let result = value(work::result(&store, &expected.id, number, prioritized).map_err(storage)?);
    drop(store);
    drop(auth);
    drop(generations);
    runtime.launch(batch);
    result
}
