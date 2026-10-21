//! Service reachability is independent of credentials and retry policy.
use crate::{
    backpressure::{Origin, Scope},
    monitoring::{JobOperation, OperationFailure, OperationState},
    review::ReviewRun,
    storage::Store,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::Manager;

pub const CHECK_SECONDS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    Reachable,
    Unreachable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub scope: Scope,
    pub offline_since: Option<i64>,
    pub last_success: Option<i64>,
    pub next_check_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connectivity {
    pub services: Vec<Service>,
}

impl Connectivity {
    pub fn offline(&self, scopes: &[Scope]) -> Option<&Service> {
        self.services
            .iter()
            .find(|s| s.offline_since.is_some() && scopes.contains(&s.scope))
    }

    pub fn observe(&mut self, scope: Scope, observation: Observation, now: i64) {
        let index = self.services.iter().position(|s| s.scope == scope);
        let service = match index {
            Some(index) => &mut self.services[index],
            None => {
                self.services.push(Service {
                    scope,
                    offline_since: None,
                    last_success: None,
                    next_check_at: now,
                });
                self.services.last_mut().unwrap()
            }
        };
        service.next_check_at = now.saturating_add(CHECK_SECONDS);
        match observation {
            Observation::Reachable => {
                service.offline_since = None;
                service.last_success = Some(now);
            }
            Observation::Unreachable => {
                service.offline_since.get_or_insert(now);
            }
            Observation::Unknown => {}
        }
    }
}

pub fn scopes(run: &ReviewRun) -> Vec<Scope> {
    selection_scopes(&run.job.account_id, Some(&run.selection))
}

pub fn selection_scopes(account: &str, selection: Option<&crate::review::Selection>) -> Vec<Scope> {
    let mut scopes = vec![Scope::github(account)];
    if let Some(account) = selection.and_then(|s| s.agent.ai_account.as_ref()) {
        scopes.push(Scope::copilot(&account.account_id));
    }
    if let Some(account) = selection
        .and_then(|s| s.configuration.as_ref())
        .and_then(|c| c.rubber_duck.as_ref())
        .and_then(|d| d.agent.ai_account.as_ref())
    {
        scopes.push(Scope::copilot(&account.account_id));
    }
    scopes
}

/// Any HTTP response proves reachability, including 401/403/429/5xx. No
/// credentials, response bodies, redirects, or mutable provider requests.
pub(crate) fn probe(origin: Origin) -> Observation {
    let endpoint = match origin {
        Origin::Github => "https://api.github.com/",
        Origin::Copilot => "https://api.githubcopilot.com/",
    };
    probe_endpoint(endpoint)
}

fn probe_endpoint(endpoint: &str) -> Observation {
    let Ok(client) = reqwest::blocking::Client::builder()
        .user_agent("PR-Sniper")
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
    else {
        return Observation::Unknown;
    };
    match client.head(endpoint).send() {
        Ok(_) => Observation::Reachable,
        Err(error) if error.is_connect() || error.is_timeout() => Observation::Unreachable,
        Err(_) => Observation::Unknown,
    }
}

pub fn record(
    store: &Store,
    scope: Scope,
    observation: Observation,
    now: i64,
) -> Result<(), String> {
    let mut state = store.load_connectivity()?;
    state.observe(scope, observation, now);
    store.save_connectivity(&state)
}

/// Whether `sync_operation` would change the saved suspension; read-only.
pub(crate) fn would_sync(operation: &JobOperation, since: Option<i64>, now: i64) -> bool {
    let mut probe = operation.clone();
    if let Some(since) = since {
        probe.freeze_offline(since.max(probe.initial_attempt_at));
    } else if probe.state != OperationState::Running {
        probe.resume_online(now);
    }
    probe.offline_since != operation.offline_since
}

pub fn sync_operation(store: &Store, operation: &mut JobOperation, since: Option<i64>, now: i64) {
    let before = operation.offline_since;
    if let Some(since) = since {
        operation.freeze_offline(since.max(operation.initial_attempt_at));
    } else if operation.state != OperationState::Running {
        operation.resume_online(now);
    }
    if before.is_some() != operation.offline_since.is_some() {
        use crate::storage::diagnostics::{Connectivity, Event};
        let _ = store.record_attempt_decision(
            &operation.id,
            Event::Connectivity {
                state: if since.is_some() {
                    Connectivity::Suspended
                } else {
                    Connectivity::Resumed
                },
                next_check_at: since.map(|_| now.saturating_add(CHECK_SECONDS)),
            },
        );
    }
}

/// Confirm ambiguous execution/network errors independently; an SDK pipe error,
/// authentication failure, or malformed result alone is never an offline signal.
pub(crate) async fn confirm_failure(
    app: &tauri::AppHandle,
    run: &ReviewRun,
    error: &crate::review::Failure,
) -> Result<(), String> {
    if error.cancelled
        || !matches!(
            error.kind,
            OperationFailure::Network | OperationFailure::Timeout
        )
    {
        return Ok(());
    }
    let origin = error.origin;
    let observation = tauri::async_runtime::spawn_blocking(move || probe(origin))
        .await
        .map_err(|_| "Reachability check did not finish.")?;
    let scope = Scope::of(
        origin,
        &run.job.account_id,
        run.selection
            .agent
            .ai_account
            .as_ref()
            .map(|a| a.account_id.as_str()),
    );
    let host = app.state::<crate::Host>();
    let store = host.store.lock().map_err(|_| "Storage unavailable.")?;
    record(&store, scope, observation, crate::now_seconds()?)
}

/// One bounded probe per service per interval, not per repository. Wall-clock
/// scheduling collapses sleep/wake and network changes into a single fresh check.
pub(crate) async fn refresh(app: &tauri::AppHandle) -> Result<(), String> {
    let now = crate::now_seconds()?;
    let scopes = {
        let host = app.state::<crate::Host>();
        let accounts = host
            .github_auth
            .lock()
            .map_err(|_| "GitHub connection state unavailable.")?
            .monitoring_accounts();
        let store = host.store.lock().map_err(|_| "Storage unavailable.")?;
        due_scopes(&store, &accounts, now)?
    };
    for origin in [Origin::Github, Origin::Copilot] {
        let selected: Vec<_> = scopes
            .iter()
            .filter(|s| {
                matches!(
                    (origin, s),
                    (Origin::Github, Scope::Github { .. })
                        | (Origin::Copilot, Scope::Copilot { .. })
                )
            })
            .cloned()
            .collect();
        if selected.is_empty() {
            continue;
        }
        let observation = tauri::async_runtime::spawn_blocking(move || probe(origin))
            .await
            .map_err(|_| "Reachability check did not finish.")?;
        let host = app.state::<crate::Host>();
        let store = host.store.lock().map_err(|_| "Storage unavailable.")?;
        for scope in selected {
            record(&store, scope, observation, crate::now_seconds()?)?;
        }
    }
    Ok(())
}

pub fn due_scopes(
    store: &Store,
    accounts: &std::collections::BTreeMap<String, crate::monitoring::AccountAvailability>,
    now: i64,
) -> Result<Vec<Scope>, String> {
    if store.load_automation()?.paused {
        return Ok(Vec::new());
    }
    let state = store.load_connectivity()?;
    let settings = store.load_settings()?;
    let mut scopes = std::collections::BTreeSet::new();
    for repository in settings.repositories.iter().filter(|r| r.enabled) {
        if let Some(account) = &repository.provider_account_id {
            scopes.insert(Scope::github(account));
        }
    }
    for ticket in crate::monitoring::Monitor::lifecycle_candidates(store, accounts)? {
        scopes.insert(Scope::github(&ticket.provider_account_id));
    }
    // Planned work must clear the offline gate before its first run can exist.
    for candidate in crate::review::host::candidates(store)?
        .into_iter()
        .filter(|c| {
            c.run.is_none()
                && c.blocked.is_none()
                && !crate::monitoring::retired(&c.job)
                && settings
                    .repositories
                    .iter()
                    .any(|r| r.id == c.job.configuration_id && r.enabled)
        })
    {
        scopes.extend(selection_scopes(
            &candidate.job.account_id,
            candidate.planned_selection.as_ref(),
        ));
    }
    for run in store.load_reviews()?.iter().filter(|r| {
        matches!(
            r.operation.state,
            OperationState::Queued | OperationState::Interrupted | OperationState::Running
        ) || r.operation.resume_at.is_some()
    }) {
        scopes.extend(self::scopes(run));
    }
    Ok(scopes
        .into_iter()
        .filter(|scope| {
            state
                .services
                .iter()
                .find(|s| &s.scope == scope)
                .is_none_or(|s| now >= s.next_check_at)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn connectivity_http_rejections_prove_reachability_without_credentials_or_redirects() {
        for status in [200, 302, 401, 403, 429, 500] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let count = socket.read(&mut bytes).unwrap();
                let request = std::str::from_utf8(&bytes[..count]).unwrap();
                assert!(request.starts_with("HEAD / HTTP/1.1"));
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                write!(socket, "HTTP/1.1 {status} Fixture\r\nLocation: http://127.0.0.1:1/never\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            });
            assert_eq!(probe_endpoint(&endpoint), Observation::Reachable);
            server.join().unwrap();
        }
    }

    #[test]
    fn connectivity_transport_loss_is_offline_but_invalid_request_is_unknown() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(probe_endpoint(&endpoint), Observation::Unreachable);
        assert_eq!(probe_endpoint("not a URL"), Observation::Unknown);
    }

    #[test]
    fn connectivity_network_transitions_keep_original_outage_until_verified_recovery() {
        let mut state = Connectivity::default();
        let scope = Scope::github("fixture");
        state.observe(scope.clone(), Observation::Reachable, 100);
        state.observe(scope.clone(), Observation::Unreachable, 110);
        state.observe(scope.clone(), Observation::Unknown, 5000);
        let offline = state.offline(std::slice::from_ref(&scope)).unwrap();
        assert_eq!(offline.offline_since, Some(110));
        assert_eq!(offline.last_success, Some(100));
        assert_eq!(offline.next_check_at, 5030);
        assert!(state
            .offline(&[Scope::copilot("fixture"), Scope::github("other")])
            .is_none());
        state.observe(scope.clone(), Observation::Reachable, 10_000);
        assert!(state.offline(std::slice::from_ref(&scope)).is_none());
        state.observe(scope.clone(), Observation::Unreachable, 10_010);
        let offline = state.offline(&[scope]).unwrap();
        assert_eq!(offline.offline_since, Some(10_010));
        assert_eq!(offline.last_success, Some(10_000));
    }
}
