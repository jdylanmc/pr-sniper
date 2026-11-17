use super::*;
use crate::host_errors::Source;
use crate::{now_seconds, provider::RemoteRepository, storage::Store, Host, Session};
use std::sync::atomic::Ordering;
use tauri::Manager;

/// Mutation-owner id; shares the provider mutation lock with every other writer.
const OWNER_PREFIX: &str = "sign-off:";

/// Scheduler tick: plan sign-offs for revisions that settled clean, then run the next due one.
pub(crate) fn pump(app: &tauri::AppHandle, view: &Store) -> Result<(), String> {
    let host = app.state::<Host>();
    if host.quitting.load(Ordering::SeqCst) {
        return Ok(());
    }
    let now = now_seconds()?;
    let active = view
        .load_settings()?
        .repositories
        .iter()
        .any(|r| r.enabled && r.sign_off_comments_active());
    let pending = view.load_feedback()?.sign_offs.iter().any(|r| r.due(now));
    if !active && !pending {
        return Ok(());
    }
    if active {
        let snapshot = crate::queue_snapshot(&host)?;
        let store = host
            .store
            .lock()
            .map_err(|_| "Sign-off storage unavailable.")?;
        let settings = store.load_settings()?;
        let mut ledger = store.load_feedback()?;
        let planned = plan(&settings, &snapshot, &ledger.sign_offs, now);
        if !planned.is_empty() {
            ledger.sign_offs.extend(planned);
            store.save_feedback(&ledger)?;
        }
    }
    let next = {
        let store = host
            .store
            .lock()
            .map_err(|_| "Sign-off storage unavailable.")?;
        next_due(&store.load_feedback()?.sign_offs, now).cloned()
    };
    let Some(mut record) = next else {
        return Ok(());
    };
    let owner = format!("{OWNER_PREFIX}{}", record.id);
    if !host.mutations.acquire(&owner)? {
        return Ok(());
    }
    let generation = host
        .github_generations
        .lock()
        .map_err(|_| "GitHub coordination unavailable.")?
        .get(&record.job.account_id)
        .copied()
        .unwrap_or(0);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut env = Native {
            app: app.clone(),
            generation,
        };
        // Provider failures stay on the record; only host failures surface here.
        let outcome = run(&mut env, &mut record).map_err(|error| error.message);
        let released = app.state::<Host>().mutations.release(&owner);
        crate::settle(&app, Source::SignOffs, outcome.and(released));
    });
    Ok(())
}

struct Native {
    app: tauri::AppHandle,
    generation: u64,
}

fn storage(message: impl Into<String>) -> Failure {
    Failure::permanent(message)
}

impl Native {
    fn repository(record: &SignOff) -> RemoteRepository {
        RemoteRepository {
            id: record.job.repository_id.clone(),
            name: record.job.repository_name.clone(),
        }
    }

    /// The acting account's session, unchanged since the pass started; `None` holds.
    fn client(&self, record: &SignOff) -> Result<Option<Session>, Failure> {
        let host = self.app.state::<Host>();
        if host.quitting.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let current = host
            .github_generations
            .lock()
            .map_err(|_| storage("GitHub coordination unavailable."))?
            .get(&record.job.account_id)
            .copied()
            .unwrap_or(0);
        let allowed = host
            .github_auth
            .lock()
            .map_err(|_| storage("GitHub account state unavailable."))?
            .account_session_allowed(&record.job.account_id)
            .is_ok();
        if current != self.generation || !allowed {
            return Ok(None);
        }
        Ok(crate::provider_session(&host, &record.job.account_id)
            .ok()
            .filter(|(identity, _)| identity.id == record.job.account_id)
            .map(|(_, client)| client))
    }

    fn session(&self, record: &SignOff) -> Result<Session, Failure> {
        self.client(record)?.ok_or_else(|| Failure {
            kind: crate::monitoring::OperationFailure::Network,
            ..Failure::permanent("The acting account is unavailable.")
        })
    }
}

impl Environment for Native {
    fn now(&self) -> Result<i64, Failure> {
        now_seconds().map_err(storage)
    }

    fn save(&mut self, record: &SignOff) -> Result<(), Failure> {
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| storage("Sign-off storage unavailable."))?;
        let mut ledger = store.load_feedback().map_err(storage)?;
        match ledger.sign_offs.iter_mut().find(|r| r.id == record.id) {
            Some(saved) => *saved = record.clone(),
            None => ledger.sign_offs.push(record.clone()),
        }
        store.save_feedback(&ledger).map_err(storage)
    }

    fn gate(&mut self, record: &SignOff) -> Result<Gate, Failure> {
        if self.client(record)?.is_none() {
            return Ok(Gate::Hold);
        }
        let host = self.app.state::<Host>();
        let store = host
            .store
            .lock()
            .map_err(|_| storage("Sign-off storage unavailable."))?;
        if store.load_automation().map_err(storage)?.paused {
            return Ok(Gate::Hold);
        }
        let settings = store.load_settings().map_err(storage)?;
        let Some(repository) = settings.repositories.iter().find(|r| {
            r.id == record.job.configuration_id && r.enabled && r.sign_off_comments_active()
        }) else {
            return Ok(Gate::Refused);
        };
        let active = host
            .monitor
            .lock()
            .map_err(|_| storage("Monitoring state unavailable."))?
            .activation_status(&settings, &record.job.configuration_id)
            .active;
        Ok(if active {
            Gate::Open {
                review_drafts: repository.review_drafts,
            }
        } else {
            Gate::Refused
        })
    }

    fn pull(&mut self, record: &SignOff) -> Result<PullRequest, Failure> {
        let client = self.session(record)?;
        Ok(client.pull_request(&Self::repository(record), record.job.number)?)
    }

    fn comments(&mut self, record: &SignOff) -> Result<Vec<TopComment>, Failure> {
        let client = self.session(record)?;
        Ok(client.pr_level_comments(&Self::repository(record), record.job.number)?)
    }

    fn post(&mut self, record: &SignOff, body: &str) -> Result<String, WriteFailure> {
        let client = self.session(record).map_err(|failure| WriteFailure {
            failure,
            uncertain: false,
        })?;
        client.post_pr_comment(
            &Self::repository(record),
            record.job.number,
            &record.job.account_id,
            body,
        )
    }
}
