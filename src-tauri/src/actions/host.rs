use super::*;
use crate::{
    github::{actions::Action, provider::RemoteRepository},
    now_seconds, Host,
};
use std::sync::{atomic::Ordering, Mutex};
use tauri::Manager;

/// Shared by all native provider-write coordinators, not by read-only AI workers.
#[derive(Default)]
pub(crate) struct MutationOwner(Mutex<Option<String>>);
impl MutationOwner {
    pub(crate) fn acquire(&self, id: &str) -> Result<bool, String> {
        let mut active = self
            .0
            .lock()
            .map_err(|_| "Provider mutation coordination unavailable.")?;
        if active.is_some() {
            return Ok(false);
        }
        *active = Some(id.into());
        Ok(true)
    }
    pub(crate) fn release(&self, id: &str) -> Result<(), String> {
        let mut active = self
            .0
            .lock()
            .map_err(|_| "Provider mutation coordination unavailable.")?;
        if active.as_deref() != Some(id) {
            return Err("Provider mutation completion lost ownership.".into());
        }
        *active = None;
        Ok(())
    }
}

fn observation(app: &tauri::AppHandle, job: &QueueJob) -> Result<Observation, Failure> {
    let (_, client) = crate::github_session(&app.state::<Host>(), &job.account_id)?;
    let connection = client.connect(&job.repository_name, Some(&job.account_id))?;
    if connection.repository.id != job.repository_id {
        return Err(Failure::permanent("Repository binding changed."));
    }
    let mut observation = client
        .action_observation(&connection.repository, job.number)
        .map_err(Failure::from)?;
    if observation.account_id != job.account_id
        || observation.pull_request_id != job.pull_request_id
    {
        return Err(Failure::permanent(
            "Provider action observation returned a different account or PR.",
        ));
    }
    observation.write_capability =
        connection.capabilities.comment == crate::github::provider::CommentCapability::Available;
    Ok(observation)
}

pub trait ActionEnvironment {
    fn observe(&mut self, effect: &Effect) -> Result<Observation, Failure>;
    fn authorize(&mut self, effect: &Effect, current: &Observation) -> Result<(), Failure>;
    fn intent(&mut self, id: &str) -> Result<Effect, Failure>;
    fn mutate(&mut self, effect: &Effect) -> Result<Receipt, crate::publication::WriteFailure>;
    fn finish(
        &mut self,
        effect: &Effect,
        result: Result<Receipt, crate::publication::WriteFailure>,
    ) -> Result<(), Failure>;
    fn post_check(&mut self, effect: &Effect) -> Result<(), Failure>;
}

pub fn execute_action(env: &mut impl ActionEnvironment, effect: &Effect) -> Result<(), Failure> {
    if effect.state != EffectState::Prepared {
        return Err(Failure::permanent(
            "Only an unattempted intent may be executed.",
        ));
    }
    let current = env.observe(effect)?;
    env.authorize(effect, &current)?;
    let intent = env.intent(&effect.id)?;
    // Intent is durable before the final fresh read or any external request.
    let current = match env.observe(&intent).and_then(|current| {
        env.authorize(&intent, &current)?;
        Ok(current)
    }) {
        Ok(current) => current,
        Err(error) => {
            return env.finish(
                &intent,
                Err(crate::publication::WriteFailure {
                    failure: error,
                    uncertain: false,
                }),
            )
        }
    };
    if current.head != intent.observation.head
        || current.base != intent.observation.base
        || intent.action == Action::Merge && current.method != intent.observation.method
    {
        return env.finish(
            &intent,
            Err(crate::publication::WriteFailure {
                failure: Failure::permanent(
                    "Pinned action revision or provider merge method changed.",
                ),
                uncertain: false,
            }),
        );
    }
    let outcome = env.mutate(&intent);
    let confirmed = outcome.is_ok();
    env.finish(&intent, outcome)?;
    if confirmed {
        env.post_check(&intent)?;
    }
    Ok(())
}

struct Native {
    app: tauri::AppHandle,
    generation: u64,
}
impl Native {
    fn gate(&self, effect: &Effect) -> Result<(), Failure> {
        if now_seconds().map_err(Failure::permanent)? >= effect.operation.retry_deadline
            && effect.state != EffectState::Confirmed
        {
            return Err(Failure::timeout());
        }
        let host = self.app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Err(Failure::permanent(
                "Application is quitting; reconcile the action after restart.",
            ));
        }
        if host
            .github_generations
            .lock()
            .map_err(|_| Failure::permanent("Account coordination unavailable."))?
            .get(&effect.observation.account_id)
            .copied()
            .unwrap_or(0)
            != self.generation
        {
            return Err(Failure::permanent("Action account changed."));
        }
        host.github_auth
            .lock()
            .map_err(|_| Failure::permanent("Account state unavailable."))?
            .account_session_allowed(&effect.observation.account_id)?;
        Ok(())
    }
}
impl ActionEnvironment for Native {
    fn observe(&mut self, effect: &Effect) -> Result<Observation, Failure> {
        self.gate(effect)?;
        let job = {
            let host = self.app.state::<Host>();
            let store = host
                .store
                .lock()
                .map_err(|_| Failure::permanent("Action storage unavailable."))?;
            store
                .load_actions()
                .map_err(Failure::permanent)?
                .finals
                .into_iter()
                .find(|f| f.id == effect.final_id)
                .ok_or_else(|| Failure::permanent("Original final review unavailable."))?
                .basis
                .job
        };
        let current = observation(&self.app, &job)?;
        self.gate(effect)?;
        Ok(current)
    }
    fn authorize(&mut self, effect: &Effect, current: &Observation) -> Result<(), Failure> {
        self.gate(effect)?;
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Action storage unavailable."))?;
        crate::capacity::publication_gate(&store).map_err(Failure::permanent)?;
        let ledger = store.load_actions().map_err(Failure::permanent)?;
        if ledger
            .effects
            .iter()
            .find(|e| e.id == effect.id)
            .is_none_or(|e| {
                e.cancelled
                    || e.operation.id != effect.operation.id
                    || !matches!(e.state, EffectState::Prepared | EffectState::Uncertain)
            })
        {
            return Err(Failure::permanent(
                "Action intent was cancelled or no longer belongs to this worker.",
            ));
        }
        let run = ledger
            .finals
            .iter()
            .find(|f| f.id == effect.final_id)
            .ok_or_else(|| Failure::permanent("Final review unavailable."))?;
        if effect.action == Action::Merge && current.method != effect.observation.method {
            return Err(Failure::permanent(
                "Provider-selected merge method changed before the request.",
            ));
        }
        ready(&store, run, current, effect.action).map_err(Failure::permanent)
    }
    fn intent(&mut self, id: &str) -> Result<Effect, Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Action storage unavailable."))?;
        mark_intent(&store, id, now_seconds().map_err(Failure::permanent)?)
            .map_err(Failure::permanent)
    }
    fn mutate(&mut self, effect: &Effect) -> Result<Receipt, crate::publication::WriteFailure> {
        self.authorize(effect, &effect.observation)
            .map_err(|failure| crate::publication::WriteFailure {
                failure,
                uncertain: false,
            })?;
        let (_, client) =
            crate::github_session(&self.app.state::<Host>(), &effect.observation.account_id)
                .map_err(|e| crate::publication::WriteFailure {
                    failure: e.into(),
                    uncertain: false,
                })?;
        let job = {
            let host = self.app.state::<Host>();
            let store = host
                .store
                .lock()
                .map_err(|_| crate::publication::WriteFailure {
                    failure: Failure::permanent("Action storage unavailable."),
                    uncertain: false,
                })?;
            store
                .load_actions()
                .map_err(|e| crate::publication::WriteFailure {
                    failure: Failure::permanent(e),
                    uncertain: false,
                })?
                .finals
                .into_iter()
                .find(|f| f.id == effect.final_id)
                .ok_or_else(|| crate::publication::WriteFailure {
                    failure: Failure::permanent("Final review unavailable."),
                    uncertain: false,
                })?
                .basis
                .job
        };
        match effect.action {
            Action::Approve => client.approve_exact(
                &RemoteRepository {
                    id: job.repository_id,
                    name: job.repository_name,
                },
                job.number,
                &effect.observation.account_id,
                &effect.observation.head,
                &effect.body,
            ),
            Action::Merge => client.merge_exact(&effect.observation, &effect.id),
        }
    }
    fn finish(
        &mut self,
        effect: &Effect,
        result: Result<Receipt, crate::publication::WriteFailure>,
    ) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Action storage unavailable."))?;
        finish_effect(&store, &effect.id, &effect.operation.id, result).map_err(Failure::permanent)
    }
    fn post_check(&mut self, effect: &Effect) -> Result<(), Failure> {
        let observed = self.observe(effect);
        let checked = {
            let host = self.app.state::<Host>();
            let store = host
                .store
                .lock()
                .map_err(|_| Failure::permanent("Action storage unavailable."))?;
            verify_after_effect(&store, effect, observed.clone())
        };
        if checked.is_ok() {
            if let Ok(current) = observed {
                settle_native(&self.app, &effect.item_id, Some(current))
                    .map_err(Failure::permanent)?;
            }
        }
        checked
    }
}

/// Consume the state of packet finals whose writes are confirmed: the post-write
/// snapshot becomes the baseline, so the host's own approval never retriggers.
fn settle_native(
    app: &tauri::AppHandle,
    item_id: &str,
    observation: Option<Observation>,
) -> Result<(), String> {
    let host = app.state::<Host>();
    let settles = {
        let store = host
            .store
            .lock()
            .map_err(|_| "Action storage unavailable.")?;
        let observation = match observation {
            Some(observation) => observation,
            None => match store
                .load_actions()?
                .observations
                .into_iter()
                .find(|o| o.item_id == item_id)
                .and_then(|o| o.observation)
            {
                Some(observation) => observation,
                None => return Ok(()),
            },
        };
        packet::settle(&store, item_id, &observation, now_seconds()?)?
    };
    for settle in settles {
        let (_, client) = crate::github_session(&host, &settle.job.account_id)
            .map_err(|_| "GitHub session unavailable.".to_string())?;
        packet::consume_settled(&host.store, &client, &settle)?;
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct Coordinator {
    pub(crate) active: Mutex<bool>,
}
pub(crate) fn prepare_next_effect(
    store: &Store,
    item_id: &str,
    now: i64,
) -> Result<Option<Effect>, String> {
    let ledger = store.load_actions()?;
    let Some(observation) = ledger
        .observations
        .iter()
        .find(|observed| observed.item_id == item_id)
        .and_then(|observed| observed.observation.as_ref())
    else {
        return Ok(None);
    };
    if let Some(effect) = ledger
        .effects
        .iter()
        .find(|effect| effect.item_id == item_id && effect.state == EffectState::Prepared)
    {
        return Ok(Some(effect.clone()));
    }
    for action in [Action::Approve, Action::Merge] {
        if action == Action::Approve
            && crate::actions::provider_approval(observation).is_some()
            && ledger.finals.iter().any(|run| {
                run.basis.item_id == item_id
                    && run.basis.permissions.merge
                    && validate_local(store, run).is_ok()
            })
        {
            continue;
        }
        if ledger
            .effects
            .iter()
            .any(|effect| effect.item_id == item_id && effect.action == action)
        {
            continue;
        }
        if let Some(run) = ledger.finals.iter().rev().find(|run| {
            run.basis.item_id == item_id && ready(store, run, observation, action).is_ok()
        }) {
            return prepare_effect(store, &run.id, action, observation, now).map(Some);
        }
    }
    Ok(None)
}

/// The item whose provider state should be observed next, if any.
pub(crate) fn next_target(store: &Store, now: i64) -> Result<Option<queue::Item>, String> {
    if store.load_automation()?.paused {
        return Ok(None);
    }
    expire_prepared(store, now)?;
    let snapshot = queue::normal_snapshot(store, vec![])?;
    let ledger = store.load_actions()?;
    let unadopted = packet::unadopted(store, &ledger)?;
    let settings = store.load_settings()?;
    Ok(snapshot
        .items
        .into_iter()
        .filter(|item| {
            ledger
                .observations
                .iter()
                .find(|o| o.item_id == item.id)
                .is_none_or(|o| o.error.is_none() || o.retry_at.is_some_and(|at| at <= now))
        })
        .filter(|i| observation_pending(i, &settings, &ledger, now) || unadopted.contains(&i.id))
        .min_by_key(|i| {
            ledger
                .observations
                .iter()
                .find(|o| o.item_id == i.id)
                .map(|o| o.at)
                .unwrap_or(0)
        }))
}

impl Coordinator {
    pub(crate) fn finished(&self) -> bool {
        self.active.lock().is_ok_and(|v| !*v)
    }
    pub(crate) fn pump(app: &tauri::AppHandle) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut active = host
            .actions
            .active
            .lock()
            .map_err(|_| "Action coordination unavailable.")?;
        if *active {
            return Ok(());
        }
        let target = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Action storage unavailable.")?;
            next_target(&store, now_seconds()?)?
        };
        let Some(target) = target else {
            return Ok(());
        };
        *active = true;
        drop(active);
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let work = (|| -> Result<(), String> {
                let generation = app
                    .state::<Host>()
                    .github_generations
                    .lock()
                    .map_err(|_| "Account coordination unavailable.")?
                    .get(&target.job.account_id)
                    .copied()
                    .unwrap_or(0);
                {
                    let host = app.state::<Host>();
                    let store = host
                        .store
                        .lock()
                        .map_err(|_| "Action storage unavailable.")?;
                    let mut ledger = store.load_actions()?;
                    let mut reconciling = false;
                    for effect in ledger
                        .effects
                        .iter_mut()
                        .filter(|e| e.item_id == target.id && e.needs_reconciliation())
                    {
                        effect.reconcile_attempts = effect.reconcile_attempts.saturating_add(1);
                        effect.reconcile_requested = false;
                        reconciling = true;
                    }
                    // An unchanged ledger is not rewritten (or re-indexed).
                    if reconciling {
                        store.save_actions(&ledger)?;
                    }
                }
                let observed = observation(&app, &target.job);
                let effect = {
                    let host = app.state::<Host>();
                    let generations = host
                        .github_generations
                        .lock()
                        .map_err(|_| "Account coordination unavailable.")?;
                    if generations
                        .get(&target.job.account_id)
                        .copied()
                        .unwrap_or(0)
                        != generation
                    {
                        return Err("Observation account changed.".into());
                    }
                    let store = host
                        .store
                        .lock()
                        .map_err(|_| "Action storage unavailable.")?;
                    synchronize(&store, &target.id, observed, now_seconds()?)?;
                    packet::adopt(&store, &target.id)?;
                    prepare_next_effect(&store, &target.id, now_seconds()?)?
                };
                if effect.is_none() {
                    settle_native(&app, &target.id, None)?;
                }
                if let Some(effect) = effect {
                    let owner = format!("action:{}", effect.id);
                    if app.state::<Host>().mutations.acquire(&owner)? {
                        let outcome = execute_action(
                            &mut Native {
                                app: app.clone(),
                                generation,
                            },
                            &effect,
                        );
                        app.state::<Host>().mutations.release(&owner)?;
                        if let Err(error) = outcome {
                            let host = app.state::<Host>();
                            let store = host
                                .store
                                .lock()
                                .map_err(|_| "Action storage unavailable.")?;
                            if store
                                .load_actions()?
                                .effects
                                .iter()
                                .any(|e| e.id == effect.id && e.state == EffectState::Prepared)
                            {
                                synchronize(
                                    &store,
                                    &effect.item_id,
                                    Err(error.clone()),
                                    now_seconds()?,
                                )?;
                                let mut ledger = store.load_actions()?;
                                if let Some(current) =
                                    ledger.effects.iter_mut().find(|e| e.id == effect.id)
                                {
                                    current.error = Some(error.message.clone());
                                }
                                store.save_actions(&ledger)?;
                            }
                            return Err(error.message);
                        }
                    }
                }
                crate::capacity::Coordinator::pump(&app)
            })();
            if let Err(error) = work {
                crate::report(&app, error);
            }
            match app.state::<Host>().actions.active.lock() {
                Ok(mut active) => *active = false,
                Err(_) => crate::report(&app, "Action coordination unavailable.".into()),
            }
        });
        Ok(())
    }
}

#[tauri::command]
pub(crate) fn cancel_provider_action(
    host: tauri::State<'_, Host>,
    id: String,
) -> Result<(), String> {
    let store = host
        .store
        .lock()
        .map_err(|_| "Action storage unavailable.")?;
    cancel_effect(&store, &id)
}

#[tauri::command]
pub(crate) fn reconcile_provider_action(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let host = app.state::<Host>();
    {
        let store = host
            .store
            .lock()
            .map_err(|_| "Action storage unavailable.")?;
        request_reconciliation(&store, &id, now_seconds()?)?;
    }

    Coordinator::pump(&app)
}

#[tauri::command]
pub(crate) fn retry_action_observation(
    app: tauri::AppHandle,
    item_id: String,
) -> Result<(), String> {
    let host = app.state::<Host>();
    {
        let store = host
            .store
            .lock()
            .map_err(|_| "Action storage unavailable.")?;
        request_observation_retry(&store, &item_id, now_seconds()?)?;
    }
    Coordinator::pump(&app)
}
