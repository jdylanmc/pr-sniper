use super::*;
use crate::{
    github::{
        http::HttpTransport,
        provider::{CommentCapability, GithubClient, RemoteRepository},
    },
    now_seconds, Host,
};
use std::sync::{atomic::Ordering, Arc, Mutex};
use tauri::Manager;

#[derive(Default)]
pub(crate) struct Coordinator {
    active: Mutex<Option<String>>,
}

#[derive(Serialize)]
pub(crate) struct Candidate {
    pub(crate) review_operation_id: String,
    pub(crate) automatic: bool,
    pub(crate) blocked: Option<String>,
    pub(crate) publication: Option<Publication>,
}

pub(crate) fn candidates(store: &Store) -> Result<Vec<Candidate>, String> {
    let settings = store.load_settings()?;
    let jobs = store.load_queue()?;
    let publications = store.load_publications()?;
    let reviews = store.review_evidence()?;
    let latest: std::collections::BTreeMap<_, _> = reviews
        .iter()
        .map(|review| (&review.key, &review.operation.id))
        .collect();
    Ok(reviews.iter()
        .filter(|review| review.operation.state == OperationState::Completed && review.result.is_some())
        .map(|review| {
            let mut policy = jobs.iter().find(|job| review.matches_job(job))
                .ok_or("Review detection is no longer available.".to_string())
                .and_then(|job| automatic_policy(&settings, review, job))
                .and_then(|automatic| {
                    if review.result.as_ref().and_then(|r| r.reviewed_base_sha.as_ref()).is_none() {
                        Err("This older result has no reviewed base. Run a new review before publication.".into())
                    } else { Ok(automatic) }
                });
            if conflicting_publication(&publications, review) {
                policy = Err("Another review attempt owns this revision's publication; reconcile it instead of creating another batch.".into());
            }
            if latest.get(&review.key).copied() != Some(&review.operation.id) {
                policy = Err("A newer local review attempt superseded this result.".into());
            }
            Candidate {
                automatic: policy.as_ref().copied().unwrap_or(false),
                blocked: policy.err(),
                publication: publications.iter().find(|p| p.review.operation.id == review.operation.id).cloned(),
                review_operation_id: review.operation.id.clone(),
            }
        }).collect())
}

fn next_candidate(store: &Store, now: i64) -> Result<Option<Candidate>, String> {
    if store.load_automation()?.paused {
        return Ok(None);
    }
    Ok(candidates(store)?
        .into_iter()
        .find(|candidate| match &candidate.publication {
            Some(run) => {
                matches!(
                    run.operation.state,
                    OperationState::Queued | OperationState::Interrupted
                ) && run
                    .operation
                    .next_attempt_at
                    .is_some_and(|next| now >= next)
            }
            None => candidate.blocked.is_none() && candidate.automatic,
        }))
}

fn prepare_launch(
    store: &Store,
    review_id: &str,
    manual: bool,
    now: i64,
) -> Result<Publication, String> {
    if store.load_automation()?.paused {
        return Err("Automation paused. Resume to publish or reconcile existing receipts.".into());
    }
    let candidate = candidates(store)?
        .into_iter()
        .find(|c| c.review_operation_id == review_id)
        .ok_or("Completed review is no longer available.")?;
    let mut runs = store.load_publications()?;
    let review = match &candidate.publication {
        Some(run) => run.review.clone(),
        None => store
            .review_evidence()?
            .into_iter()
            .find(|r| r.operation.id == review_id)
            .ok_or("Completed review disappeared.")?,
    };
    if conflicting_publication(&runs, &review) {
        return Err(
            "Another review attempt owns this revision's publication; do not create another batch."
                .into(),
        );
    }
    let run = if let Some(mut run) = candidate.publication {
        if manual {
            // Reconciliation remains available even after permission/configuration loss.
            run.retry(candidate.automatic, now)?;
        }
        run
    } else {
        if let Some(error) = candidate.blocked {
            return Err(error);
        }
        Publication::new(review, candidate.automatic, manual, now)?
    };
    replace(&mut runs, &run);
    store.save_publications(&runs)?;
    Ok(run)
}

impl Coordinator {
    pub(crate) fn finished(&self) -> bool {
        self.active.lock().is_ok_and(|active| active.is_none())
    }

    pub(crate) fn pump(app: &tauri::AppHandle) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) || !host.publications.finished() {
            return Ok(());
        }
        let candidate = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Publication storage unavailable.")?;
            next_candidate(&store, now_seconds()?)?
        };
        if let Some(candidate) = candidate {
            Self::launch(app, &candidate.review_operation_id, false)?;
        }
        Ok(())
    }

    fn launch(app: &tauri::AppHandle, review_id: &str, manual: bool) -> Result<(), String> {
        let host = app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Err("PR Sniper is quitting.".into());
        }
        let mut active = host
            .publications
            .active
            .lock()
            .map_err(|_| "Publication coordination unavailable.")?;
        if active.is_some() {
            return Err("Another publication is running; wait before retrying.".into());
        }
        let generation = host
            .github_generations
            .lock()
            .map_err(|_| "GitHub coordination unavailable.")?;
        let mut run = {
            let store = host
                .store
                .lock()
                .map_err(|_| "Publication storage unavailable.")?;
            prepare_launch(&store, review_id, manual, now_seconds()?)?
        };
        let generation = generation
            .get(&run.review.job.account_id)
            .copied()
            .unwrap_or(0);
        *active = Some(run.id.clone());
        drop(active);
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut environment = Native {
                app: app.clone(),
                generation,
            };
            if let Err(error) = execute(&mut environment, &mut run) {
                crate::report(&app, error.message);
            }
            match app.state::<Host>().publications.active.lock() {
                Ok(mut active) => *active = None,
                Err(_) => crate::report(&app, "Publication coordination unavailable.".into()),
            };
        });
        Ok(())
    }
}

fn replace(runs: &mut Vec<Publication>, run: &Publication) {
    if let Some(previous) = runs.iter_mut().find(|p| p.id == run.id) {
        *previous = run.clone();
    } else {
        runs.push(run.clone());
    }
}

fn conflicting_publication(
    publications: &[Publication],
    review: &crate::review::ReviewRun,
) -> bool {
    publications.iter().any(|p| p.conflicts_with(review))
}

struct Native {
    app: tauri::AppHandle,
    generation: u64,
}

impl Native {
    fn read_client(
        &self,
        run: &Publication,
        check_policy: bool,
    ) -> Result<GithubClient<crate::github::review::GuardedTransport<HttpTransport>>, Failure> {
        let client = self.session(run)?;
        let guard = Native {
            app: self.app.clone(),
            generation: self.generation,
        };
        let run = run.clone();
        Ok(client.guarded(Arc::new(move || {
            if now_seconds().map_err(|_| crate::github::ConnectionError::Configuration)?
                >= run.operation.retry_deadline
            {
                return Err(crate::github::ConnectionError::Timeout);
            }
            let result = if check_policy {
                guard.local_gate(&run)
            } else {
                guard.connection_gate(&run)
            };
            result.map_err(|_| crate::github::ConnectionError::Configuration)
        })))
    }

    fn session(&self, run: &Publication) -> Result<GithubClient<HttpTransport>, Failure> {
        self.connection_gate(run)?;
        let host = self.app.state::<Host>();
        let (identity, client) = crate::github_session(&host, &run.review.job.account_id)?;
        if identity.id != run.review.job.account_id {
            return Err(Failure::permanent("The acting GitHub account changed."));
        }
        self.connection_gate(run)?;
        Ok(client)
    }

    fn connection_gate(&self, run: &Publication) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Err(Failure::permanent(
                "Publication interrupted by application shutdown.",
            ));
        }
        if host
            .github_generations
            .lock()
            .map_err(|_| Failure::permanent("GitHub coordination unavailable."))?
            .get(&run.review.job.account_id)
            .copied()
            .unwrap_or(0)
            != self.generation
        {
            return Err(Failure::permanent(
                "The GitHub connection changed; reconcile before retrying.",
            ));
        }
        host.github_auth
            .lock()
            .map_err(|_| Failure::permanent("GitHub account state unavailable."))?
            .account_session_allowed(&run.review.job.account_id)?;
        Ok(())
    }

    fn local_gate(&self, run: &Publication) -> Result<(), Failure> {
        self.connection_gate(run)?;
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Publication storage unavailable."))?;
        crate::capacity::publication_gate(&store).map_err(Failure::permanent)?;
        let current = store
            .load_publications()
            .map_err(Failure::permanent)?
            .into_iter()
            .find(|p| p.id == run.id)
            .ok_or_else(|| Failure::permanent("Publication disappeared."))?;
        if current.cancelled || (!current.confirmed && !current.automatic) {
            return Err(Failure::permanent(
                "Publication confirmation was withdrawn.",
            ));
        }
        let settings = store.load_settings().map_err(Failure::permanent)?;
        let jobs = store.load_queue().map_err(Failure::permanent)?;
        let job = jobs
            .iter()
            .find(|job| run.review.matches_job(job))
            .ok_or_else(|| Failure::permanent("Review detection is no longer available."))?;
        let automatic =
            automatic_policy(&settings, &run.review, job).map_err(Failure::permanent)?;
        if automatic != run.automatic {
            return Err(Failure::permanent(
                "The publication gate changed; confirm again before retrying.",
            ));
        }
        if !host
            .monitor
            .lock()
            .map_err(|_| Failure::permanent("Monitoring state unavailable."))?
            .activation_status(&settings, &job.configuration_id)
            .active
        {
            return Err(Failure::permanent("Monitoring scope is no longer active."));
        }
        Ok(())
    }
}

impl Environment for Native {
    fn paused(&self) -> Result<bool, Failure> {
        self.app
            .state::<Host>()
            .store
            .lock()
            .map_err(|_| Failure::permanent("Publication storage unavailable."))?
            .load_automation()
            .map(|s| s.paused)
            .map_err(Failure::permanent)
    }
    fn now(&self) -> Result<i64, Failure> {
        now_seconds().map_err(Failure::permanent)
    }

    fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Publication storage unavailable."))?;
        let mut runs = store.load_publications().map_err(Failure::permanent)?;
        if runs
            .iter()
            .find(|p| p.id == run.id)
            .is_some_and(|p| p.cancelled)
        {
            run.cancelled = true;
            run.confirmed = false;
        }
        replace(&mut runs, run);
        store.save_publications(&runs).map_err(Failure::permanent)
    }

    fn inspect(&mut self, run: &Publication) -> Result<Gate, Failure> {
        let client = self.read_client(run, false)?;
        let repo = RemoteRepository {
            id: run.review.job.repository_id.clone(),
            name: run.review.job.repository_name.clone(),
        };
        let connection = client.connect(&repo.name, Some(&run.review.job.account_id))?;
        if connection.repository.id != repo.id {
            return Err(Failure::permanent("Repository identity changed."));
        }
        let pull = client.review_pull(&repo, run.review.job.number)?;
        self.connection_gate(run)?;
        let local = self.local_gate(run);
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Publication storage unavailable."))?;
        let settings = store.load_settings().map_err(Failure::permanent)?;
        let active = host
            .monitor
            .lock()
            .map_err(|_| Failure::permanent("Monitoring state unavailable."))?
            .activation_status(&settings, &run.review.job.configuration_id)
            .active;
        let jobs = store.load_queue().map_err(Failure::permanent)?;
        let current = store
            .load_publications()
            .map_err(Failure::permanent)?
            .into_iter()
            .find(|p| p.id == run.id)
            .ok_or_else(|| Failure::permanent("Publication disappeared."))?;
        let mut gate = evaluate_gate(
            &settings,
            run,
            jobs.iter().find(|job| run.review.matches_job(job)),
            &pull,
            active,
            connection.capabilities.comment == CommentCapability::Available,
            current.cancelled,
        );
        if let Err(error) = local {
            gate.stop = Some(error.message);
        }
        Ok(gate)
    }

    fn prepare(&mut self, run: &Publication) -> Result<Batch, Failure> {
        self.local_gate(run)?;
        let context = self.read_client(run, true)?.review_context(
            &RemoteRepository {
                id: run.review.job.repository_id.clone(),
                name: run.review.job.repository_name.clone(),
            },
            run.review.job.number,
            &run.review.job.head_sha,
        )?;
        self.local_gate(run)?;
        Batch::prepare(run, &context)
    }

    fn reconcile(&mut self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        let result = self.read_client(run, false)?.reconcile_publication(run)?;
        self.connection_gate(run)?;
        Ok(result)
    }

    fn mutate(&mut self, run: &Publication, mutation: Mutation) -> Result<Receipt, WriteFailure> {
        let preparation = (|| {
            let client = self.session(run)?;
            if mutation != Mutation::Create {
                let receipt = self.reconcile(run)?.ok_or_else(|| {
                    Failure::permanent("The original pending review cannot be found.")
                })?;
                if receipt.state == RemoteState::Commented {
                    return Ok((client, Some(receipt)));
                }
                if receipt.state != RemoteState::Pending {
                    return Err(Failure::permanent(
                        "The review is no longer pending; reconcile its receipt.",
                    ));
                }
            }
            let gate = self.inspect(run)?;
            if mutation != Mutation::Discard {
                if let Some(reason) = gate.stop {
                    return Err(Failure::permanent(reason));
                }
                self.local_gate(run)?;
            } else {
                // Revocation prevents publication, not removal of our exact owned pending batch.
                self.connection_gate(run)?;
            }
            if self.now()? >= run.operation.retry_deadline {
                return Err(Failure::permanent(
                    "Publication deadline expired; reconcile with a new retry budget.",
                ));
            }
            Ok((client, None))
        })();
        let (client, receipt) = preparation.map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        if let Some(receipt) = receipt {
            return Ok(receipt);
        }
        {
            let host = self.app.state::<Host>();
            let store = host.store.lock().map_err(|_| WriteFailure {
                failure: Failure::permanent("Publication storage unavailable."),
                uncertain: false,
            })?;
            crate::capacity::publication_gate(&store).map_err(|error| WriteFailure {
                failure: Failure::permanent(error),
                uncertain: false,
            })?;
        }
        client.mutate_publication(run, mutation)
    }

    fn requeue(&mut self, run: &Publication) -> Result<(), Failure> {
        if !self.inspect(run)?.requeue {
            return Ok(());
        }
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| Failure::permanent("Publication storage unavailable."))?;
        let result = host
            .monitor
            .lock()
            .map_err(|_| Failure::permanent("Monitoring state unavailable."))?
            .request_revision_check(&store, &run.review.job, self.now()?)
            .map_err(Failure::permanent);
        result
    }
}

#[tauri::command]
pub(crate) async fn publish_review(
    app: tauri::AppHandle,
    review_operation_id: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        Coordinator::launch(&app, &review_operation_id, true)
    })
    .await
    .map_err(|_| "Publication start could not finish.".to_string())?
}

#[tauri::command]
pub(crate) async fn cancel_publication(
    app: tauri::AppHandle,
    publication_id: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Host>();
        let active = host
            .publications
            .active
            .lock()
            .map_err(|_| "Publication coordination unavailable.")?;
        let store = host
            .store
            .lock()
            .map_err(|_| "Publication storage unavailable.")?;
        let mut runs = store.load_publications()?;
        let run = runs
            .iter_mut()
            .find(|p| p.id == publication_id)
            .ok_or("Publication is unavailable.")?;
        if matches!(run.phase, Phase::Published | Phase::StaleAfterPublication) {
            return Err("This batch is already published; cancellation cannot undo it.".into());
        }
        run.cancelled = true;
        run.confirmed = false;
        run.error = Some(
            "Publication confirmation was withdrawn; any pending outcome must be reconciled."
                .into(),
        );
        if active.as_deref() != Some(&publication_id) {
            // The scheduler will only reconcile/clean up, because the persisted grant is withdrawn.
            run.operation.state = OperationState::Interrupted;
            run.operation.next_attempt_at = Some(now_seconds()?);
        }
        store.save_publications(&runs)
    })
    .await
    .map_err(|_| "Publication cancellation could not finish.".to_string())?
}

#[cfg(test)]
mod tests;
