use super::*;
use crate::{
    github::{
        http::HttpTransport,
        provider::{GithubClient, RemoteRepository},
    },
    monitoring::OperationFailure,
    now_seconds, Host,
};
use std::sync::atomic::Ordering;
use tauri::Manager;

/// Mutation-owner id; shares the provider mutation lock with every other writer.
const OWNER_PREFIX: &str = "exit-writes:";

/// Scheduler tick: add writes for newly completed exits and start the next due one.
/// Integration point: call beside `publication::host::Coordinator::pump`.
pub(crate) fn pump(app: &tauri::AppHandle) -> Result<(), String> {
    let host = app.state::<Host>();
    if host.quitting.load(Ordering::SeqCst) {
        return Ok(());
    }
    let next = {
        let store = host
            .store
            .lock()
            .map_err(|_| "Exit write storage unavailable.")?;
        if store.load_automation()?.paused {
            return Ok(());
        }
        let now = now_seconds()?;
        admit(&store, now)?;
        match next_due(&store, now)? {
            Some(id) => store
                .load_feedback()?
                .exit_writes
                .into_iter()
                .find(|w| w.id == id),
            None => None,
        }
    };
    let Some(mut write) = next else {
        return Ok(());
    };
    let owner = format!("{OWNER_PREFIX}{}", write.id);
    if !host.mutations.acquire(&owner)? {
        return Ok(());
    }
    let generation = host
        .github_generations
        .lock()
        .map_err(|_| "GitHub coordination unavailable.")?
        .get(&write.job.account_id)
        .copied()
        .unwrap_or(0);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut env = Native {
            app: app.clone(),
            generation,
        };
        let tossed_before = write.phase == Phase::Tossed;
        if let Err(error) = run(&mut env, &mut write) {
            if !error.rate_limited() && error.kind == OperationFailure::Permanent {
                crate::report(
                    &app,
                    "An exit's replies or resolutions were not written. Open the PR's review evidence."
                        .into(),
                );
            }
        }
        if !tossed_before && write.phase == Phase::Tossed {
            if let Err(error) = requeue_tossed(&app, &write) {
                crate::report(&app, error);
            }
        }
        if let Err(error) = app.state::<Host>().mutations.release(&owner) {
            crate::report(&app, error);
        }
    });
    Ok(())
}

struct Native {
    app: tauri::AppHandle,
    generation: u64,
}

/// A tossed exit counts against the PR's toss budget and re-queues its review (AC-022).
fn requeue_tossed(app: &tauri::AppHandle, write: &ExitWrite) -> Result<(), String> {
    let host = app.state::<Host>();
    let store = host
        .store
        .lock()
        .map_err(|_| "Exit write storage unavailable.")?;
    requeue(&store, write, now_seconds()?)
}

fn waiting(message: &str) -> Failure {
    Failure {
        cancelled: false,
        kind: OperationFailure::Network,
        message: message.into(),
        retry_after_seconds: None,
        origin: crate::backpressure::Origin::Github,
        duck_setup: false,
        resume: Default::default(),
    }
}

fn storage(message: impl Into<String>) -> Failure {
    Failure::permanent(message)
}

impl Native {
    fn connection(&self, write: &ExitWrite) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Err(waiting("Interrupted by application shutdown."));
        }
        let current = host
            .github_generations
            .lock()
            .map_err(|_| storage("GitHub coordination unavailable."))?
            .get(&write.job.account_id)
            .copied()
            .unwrap_or(0);
        if current != self.generation {
            return Err(waiting("The GitHub connection changed; retrying."));
        }
        host.github_auth
            .lock()
            .map_err(|_| storage("GitHub account state unavailable."))?
            .account_session_allowed(&write.job.account_id)?;
        Ok(())
    }

    fn client(&self, write: &ExitWrite) -> Result<GithubClient<HttpTransport>, Failure> {
        self.connection(write)?;
        let host = self.app.state::<Host>();
        let (identity, client) =
            crate::github_session(&host, &write.job.account_id).map_err(Failure::from)?;
        if identity.id != write.job.account_id {
            return Err(Failure::permanent("The acting GitHub account changed."));
        }
        Ok(client)
    }

    fn repository(write: &ExitWrite) -> RemoteRepository {
        RemoteRepository {
            id: write.job.repository_id.clone(),
            name: write.job.repository_name.clone(),
        }
    }

    fn pace<T>(&self, write: &ExitWrite, result: &mut Result<T, WriteFailure>) {
        if let Err(error) = result {
            if let (Ok(now), Ok(store)) = (
                now_seconds(),
                self.app.state::<Host>().store.lock().map_err(|_| ()),
            ) {
                crate::backpressure::pace_write(
                    &store,
                    &mut error.failure,
                    error.uncertain,
                    &write.job.account_id,
                    &write.operation.id,
                    now,
                );
            }
        }
    }
}

impl Environment for Native {
    fn now(&self) -> Result<i64, Failure> {
        now_seconds().map_err(storage)
    }

    fn save(&mut self, write: &mut ExitWrite) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| storage("Exit write storage unavailable."))?;
        let mut ledger = store.load_feedback().map_err(storage)?;
        match ledger.exit_writes.iter_mut().find(|w| w.id == write.id) {
            Some(saved) => *saved = write.clone(),
            None => ledger.exit_writes.push(write.clone()),
        }
        store.save_feedback(&ledger).map_err(storage)
    }

    fn gate(&mut self, write: &ExitWrite) -> Result<(), Failure> {
        self.connection(write)?;
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| storage("Exit write storage unavailable."))?;
        if store.load_automation().map_err(storage)?.paused {
            return Err(waiting("Automation paused."));
        }
        let settings = store.load_settings().map_err(storage)?;
        let repository = settings
            .repositories
            .iter()
            .find(|r| r.id == write.job.configuration_id)
            .ok_or_else(|| Failure::permanent("Repository removed."))?;
        let allowed = repository
            .assignments
            .iter()
            .find(|a| a.id == write.assignment_id)
            .is_some_and(|a| repository.assignment_authority(a).reply);
        if !allowed {
            return Err(Failure::permanent(
                "Reply Comment permission is not granted; nothing was written.",
            ));
        }
        let active = host
            .monitor
            .lock()
            .map_err(|_| storage("Monitoring state unavailable."))?
            .activation_status(&settings, &write.job.configuration_id)
            .active;
        if !active {
            return Err(Failure::permanent("Monitoring scope is no longer active."));
        }
        Ok(())
    }

    fn dependency(&mut self, write: &ExitWrite) -> Result<Dependency, Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| storage("Exit write storage unavailable."))?;
        let publications = store.load_publications().map_err(storage)?;
        Ok(publication_dependency(
            &publications,
            &write.review_operation_id,
        ))
    }

    fn head(&mut self, write: &ExitWrite) -> Result<String, Failure> {
        let client = self.client(write)?;
        let pull = client.review_pull(&Self::repository(write), write.job.number)?;
        if pull.state != crate::github::metadata::Lifecycle::Open {
            return Err(Failure::permanent("PR is no longer open."));
        }
        if write
            .reviewed_base_sha
            .as_ref()
            .is_some_and(|base| *base != pull.base_sha)
        {
            return Err(Failure::permanent(
                "PR base moved; nothing further was written.",
            ));
        }
        Ok(pull.head_sha)
    }

    fn proofs(&mut self, write: &ExitWrite) -> Result<Vec<Ownership>, Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| storage("Exit write storage unavailable."))?;
        crate::review::findings::proofs(&store, &write.job, &write.assignment_id).map_err(storage)
    }

    fn thread(&mut self, write: &ExitWrite, id: &str) -> Result<Option<Thread>, Failure> {
        let client = self.client(write)?;
        Ok(client.review_thread(&write.job.repository_id, &write.job.pull_request_id, id)?)
    }

    fn revision(&mut self, write: &ExitWrite, commit: Option<&str>) -> String {
        let known = self
            .app
            .state::<Host>()
            .store
            .lock()
            .ok()
            .and_then(|store| known_iterations(&store, &write.job).ok())
            .unwrap_or_default();
        targeted_revision(commit, &known)
    }

    fn reply(
        &mut self,
        write: &ExitWrite,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        let client = self.client(write).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        let mut result = client.reply_to_review_thread(
            &write.job.repository_name,
            write.job.number,
            &write.job.account_id,
            root_id,
            body,
        );
        self.pace(write, &mut result);
        result
    }

    fn resolve(
        &mut self,
        write: &ExitWrite,
        thread_id: &str,
        request_id: &str,
    ) -> Result<(), WriteFailure> {
        let client = self.client(write).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        let mut result = client.resolve_review_thread(thread_id, request_id);
        self.pace(write, &mut result);
        result
    }

    fn confirmed(&mut self, write: &ExitWrite) -> Result<(), Failure> {
        let client = self.client(write)?;
        crate::exits::consume::consume_after(
            &self.app.state::<Host>().store,
            &client,
            &Self::repository(write),
            &write.job,
            &write.review_operation_id,
        )
        .map(|_| ())
        .map_err(storage)
    }
}
