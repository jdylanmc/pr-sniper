use crate::github::{
    metadata::{Lifecycle, PullRequest},
    provider::Connection,
    ConnectionError,
};
use crate::policy::{Policy, Schedule, WatchedIdentity};
use crate::storage::{ProviderId, Settings, Store};
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const WAITING_TRUST_CONFIRMATION: &str = "trust_confirmation";
pub const WAITING_HUMAN_START: &str = "human_start";
pub const WAITING_AGENT_UNAVAILABLE: &str = "agent_unavailable";
pub const WAITING_ACCOUNT_DISCONNECTED: &str = "account_disconnected";
pub const WAITING_REPOSITORY_DISABLED: &str = "repository_disabled";
pub const WAITING_REPOSITORY_REMOVED: &str = "repository_removed";
pub const WAITING_BINDING_CHANGED: &str = "binding_changed";
pub const WAITING_POLICY_CHANGED: &str = "policy_changed";
pub const WAITING_SUPERSEDED: &str = "superseded";
pub const WAITING_INELIGIBLE: &str = "ineligible";
pub const WAITING_NO_LONGER_CURRENT: &str = "no_longer_current";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountAvailability {
    pub login: String,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleHealth {
    pub repository_id: String,
    pub name: String,
    pub schedule_key: String,
    #[serde(default)]
    pub provider_account_id: Option<String>,
    #[serde(default)]
    pub account_login: Option<String>,
    #[serde(default)]
    pub provider_repository_id: Option<String>,
    #[serde(default)]
    pub assignment_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    pub enabled: bool,
    pub last_attempt: Option<i64>,
    pub last_success: Option<i64>,
    pub next_run: i64,
    pub schedule_available: bool,
    pub last_failure: Option<String>,
    pub in_flight: bool,
    #[serde(default)]
    pub manual_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueJob {
    pub provider: String,
    pub account_id: String,
    pub account_login: String,
    #[serde(default)]
    pub configuration_id: String,
    pub repository_id: String,
    pub repository_name: String,
    pub pull_request_id: String,
    pub number: u64,
    pub title: String,
    pub head_sha: String,
    pub trigger_policy: String,
    pub author_id: Option<String>,
    pub author_login: Option<String>,
    pub watched_author: bool,
    pub requested_reviewer: bool,
    pub waiting: String,
    pub detected_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollCursor {
    pub name: String,
    pub account_id: String,
    pub repository_id: String,
    pub trigger_policy: String,
    pub updated_after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct MonitoringState {
    #[serde(default)]
    pub health: BTreeMap<String, ScheduleHealth>,
    #[serde(default)]
    pub cursors: BTreeMap<String, PollCursor>,
}

#[derive(Debug, Clone)]
pub struct PollTicket {
    pub health_key: String,
    pub repository_id: String,
    pub name: String,
    pub provider_account_id: String,
    pub provider_repository_id: String,
    pub policy: Policy,
    pub watched_authors: Vec<WatchedIdentity>,
    pub trigger_policy: String,
    pub updated_after: Option<String>,
    pub account_generation: u64,
}

pub struct PollResult {
    pub connection: Connection,
    pub pull_requests: Vec<PullRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitoringError {
    Recoverable { code: String, message: String },
    Storage(String),
}

impl MonitoringError {
    pub fn requires_host_report(&self) -> bool {
        matches!(self, Self::Storage(_))
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Recoverable { message, .. } | Self::Storage(message) => message,
        }
    }

    fn health_failure(&self) -> &str {
        match self {
            Self::Recoverable { code, .. } => code,
            Self::Storage(_) => "storage",
        }
    }
}

#[derive(Clone)]
struct ConfiguredSchedule {
    health_key: String,
    repository_id: String,
    name: String,
    enabled: bool,
    provider_account_id: Option<String>,
    provider_repository_id: Option<String>,
    provider_supported: bool,
    schedule: Schedule,
    assignment_id: Option<String>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    policy: Policy,
    watched_authors: Vec<WatchedIdentity>,
    trigger_policy: Option<String>,
}

#[derive(Default)]
pub struct Monitor {
    state: MonitoringState,
    leases: BTreeMap<String, String>,
}

impl Monitor {
    pub fn restore(store: &Store) -> Result<Self, String> {
        let mut state = store.load_monitoring_state()?;
        for health in state.health.values_mut() {
            health.manual_pending = false;
            if health.in_flight {
                health.in_flight = false;
                health.last_failure = Some("interrupted".into());
                health.next_run = 0;
            }
        }
        Ok(Self {
            state,
            leases: BTreeMap::new(),
        })
    }

    pub fn snapshot(&self) -> Vec<ScheduleHealth> {
        self.state.health.values().cloned().collect()
    }

    pub fn prepare_checks(
        &mut self,
        store: &Store,
        now: i64,
        check_now: bool,
    ) -> Result<Vec<PollTicket>, String> {
        let settings = store.load_settings()?;
        let accounts = configured_accounts(&settings);
        self.prepare_checks_with_accounts(store, &accounts, now, check_now)
    }

    pub fn prepare_checks_with_accounts(
        &mut self,
        store: &Store,
        accounts: &BTreeMap<String, AccountAvailability>,
        now: i64,
        check_now: bool,
    ) -> Result<Vec<PollTicket>, String> {
        let settings = store.load_settings()?;
        let previous = self.state.clone();
        let previous_leases = self.leases.clone();
        let configured = configured_schedules(&settings);
        self.synchronize_health(&configured, accounts, now);
        if let Err(error) = self.synchronize_jobs(store, &configured, accounts) {
            self.state = previous;
            self.leases = previous_leases;
            return Err(error);
        }
        let tickets = self.begin(&configured, now, check_now);
        if self.state != previous {
            if let Err(error) = store.save_monitoring_state(&self.state) {
                self.state = previous;
                self.leases = previous_leases;
                return Err(error);
            }
        }
        Ok(tickets)
    }

    pub fn synchronize_configuration(
        &mut self,
        store: &Store,
        accounts: &BTreeMap<String, AccountAvailability>,
        now: i64,
    ) -> Result<(), String> {
        let settings = store.load_settings()?;
        let configured = configured_schedules(&settings);
        let previous = self.state.clone();
        self.synchronize_health(&configured, accounts, now);
        if let Err(error) = self.synchronize_jobs(store, &configured, accounts) {
            self.state = previous;
            return Err(error);
        }
        if self.state != previous {
            if let Err(error) = store.save_monitoring_state(&self.state) {
                self.state = previous;
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn cancel_pending_checks(&mut self, store: &Store) -> Result<(), String> {
        let previous = self.state.clone();
        for health in self.state.health.values_mut() {
            health.manual_pending = false;
        }
        if self.state != previous {
            if let Err(error) = store.save_monitoring_state(&self.state) {
                self.state = previous;
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn disconnect_account(&mut self, store: &Store, account_id: &str) -> Result<(), String> {
        let mut jobs = store.load_queue()?;
        let previous_jobs = jobs.clone();
        for job in &mut jobs {
            if job.account_id == account_id && actionable(job) {
                job.waiting = WAITING_ACCOUNT_DISCONNECTED.into();
            }
        }
        if jobs != previous_jobs {
            store.save_queue(&jobs)?;
        }

        let previous = self.state.clone();
        self.state
            .cursors
            .retain(|_, cursor| cursor.account_id != account_id);
        for health in self.state.health.values_mut() {
            if health.provider_account_id.as_deref() == Some(account_id) {
                health.schedule_available = false;
                health.next_run = 0;
                health.last_failure = Some(WAITING_ACCOUNT_DISCONNECTED.into());
                health.manual_pending = false;
            }
        }
        if self.state != previous {
            if let Err(error) = store.save_monitoring_state(&self.state) {
                self.state = previous;
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn discard_account_result(
        &mut self,
        store: &Store,
        accounts: &BTreeMap<String, AccountAvailability>,
        ticket: PollTicket,
        now: i64,
    ) -> Result<(), MonitoringError> {
        self.leases.remove(&ticket.provider_repository_id);
        if let Some(health) = self.state.health.get_mut(&ticket.health_key) {
            health.in_flight = false;
            health.manual_pending = false;
            health.schedule_available = false;
            health.next_run = 0;
            health.last_failure = Some(WAITING_ACCOUNT_DISCONNECTED.into());
        }
        self.state.cursors.remove(&ticket.health_key);
        let settings = store.load_settings().map_err(MonitoringError::Storage)?;
        let configured = configured_schedules(&settings);
        self.synchronize_health(&configured, accounts, now);
        store
            .save_monitoring_state(&self.state)
            .map_err(MonitoringError::Storage)
    }

    fn synchronize_health(
        &mut self,
        configured: &[ConfiguredSchedule],
        accounts: &BTreeMap<String, AccountAvailability>,
        now: i64,
    ) {
        let expected: HashSet<_> = configured
            .iter()
            .map(|configuration| configuration.health_key.as_str())
            .collect();
        self.state
            .health
            .retain(|key, health| expected.contains(key.as_str()) || health.in_flight);
        self.state
            .cursors
            .retain(|key, _| expected.contains(key.as_str()));

        for configuration in configured {
            let health = self
                .state
                .health
                .entry(configuration.health_key.clone())
                .or_insert_with(|| ScheduleHealth {
                    repository_id: configuration.repository_id.clone(),
                    name: configuration.name.clone(),
                    schedule_key: String::new(),
                    provider_account_id: None,
                    account_login: None,
                    provider_repository_id: None,
                    assignment_id: None,
                    agent_id: None,
                    agent_name: None,
                    enabled: configuration.enabled,
                    last_attempt: None,
                    last_success: None,
                    next_run: 0,
                    schedule_available: false,
                    last_failure: None,
                    in_flight: false,
                    manual_pending: false,
                });
            let binding_changed = health.name != configuration.name
                || health.provider_account_id != configuration.provider_account_id
                || health.provider_repository_id != configuration.provider_repository_id;
            if binding_changed {
                health.last_success = None;
                health.next_run = 0;
                health.account_login = None;
                self.state.cursors.remove(&configuration.health_key);
            }
            health.repository_id = configuration.repository_id.clone();
            health.name = configuration.name.clone();
            health.provider_account_id = configuration.provider_account_id.clone();
            health.provider_repository_id = configuration.provider_repository_id.clone();
            health.assignment_id = configuration.assignment_id.clone();
            health.agent_id = configuration.agent_id.clone();
            health.agent_name = configuration.agent_name.clone();
            health.enabled = configuration.enabled;
            health.account_login = configuration
                .provider_account_id
                .as_ref()
                .and_then(|id| accounts.get(id))
                .map(|account| account.login.clone())
                .filter(|login| !login.is_empty());

            if !configuration.enabled {
                health.next_run = 0;
                health.schedule_available = false;
                health.manual_pending = false;
                continue;
            }
            let Some(account_id) = configuration.provider_account_id.as_ref() else {
                unavailable_health(health, "account_binding_required");
                continue;
            };
            if !configuration.provider_supported || configuration.provider_repository_id.is_none() {
                unavailable_health(health, "provider_unavailable");
                continue;
            }
            if !accounts
                .get(account_id)
                .is_some_and(|account| account.connected)
            {
                unavailable_health(health, WAITING_ACCOUNT_DISCONNECTED);
                self.state.cursors.remove(&configuration.health_key);
                continue;
            }
            if configuration.trigger_policy.is_none() {
                unavailable_health(health, "configuration");
                continue;
            }

            let key = schedule_key(&configuration.schedule);
            if health.schedule_key != key || health.next_run == 0 {
                health.next_run = next_run(&configuration.schedule, now).unwrap_or(0);
                health.schedule_key = key;
            }
            health.schedule_available = health.next_run > 0;
            if !health.schedule_available {
                health.last_failure = Some("invalid_schedule".into());
                health.manual_pending = false;
            } else if configuration_failure(health.last_failure.as_deref()) {
                health.last_failure = None;
            }
        }
    }

    fn synchronize_jobs(
        &self,
        store: &Store,
        configured: &[ConfiguredSchedule],
        accounts: &BTreeMap<String, AccountAvailability>,
    ) -> Result<(), String> {
        let mut jobs = store.load_queue()?;
        let previous = jobs.clone();
        for job in &mut jobs {
            if !actionable(job) {
                continue;
            }
            let configuration = if job.configuration_id.is_empty() {
                configured.iter().find(|configuration| {
                    configuration.provider_account_id.as_deref() == Some(&job.account_id)
                        && configuration.provider_repository_id.as_deref()
                            == Some(&job.repository_id)
                })
            } else {
                configured
                    .iter()
                    .find(|configuration| configuration.repository_id == job.configuration_id)
            };
            let Some(configuration) = configuration else {
                job.waiting = WAITING_REPOSITORY_REMOVED.into();
                continue;
            };
            if !configuration.enabled {
                job.waiting = WAITING_REPOSITORY_DISABLED.into();
                continue;
            }
            if !configuration.provider_supported
                || configuration.provider_account_id.as_deref() != Some(&job.account_id)
                || configuration.provider_repository_id.as_deref() != Some(&job.repository_id)
            {
                job.waiting = WAITING_BINDING_CHANGED.into();
                continue;
            }
            if !accounts
                .get(&job.account_id)
                .is_some_and(|account| account.connected)
            {
                job.waiting = WAITING_ACCOUNT_DISCONNECTED.into();
                continue;
            }
            if configuration.trigger_policy.as_deref() != Some(&job.trigger_policy) {
                job.waiting = WAITING_POLICY_CHANGED.into();
                continue;
            }
            job.configuration_id = configuration.repository_id.clone();
            job.repository_name = configuration.name.clone();
            if let Some(login) = accounts
                .get(&job.account_id)
                .map(|account| account.login.as_str())
                .filter(|login| !login.is_empty())
            {
                job.account_login = login.into();
            }
        }
        if jobs != previous {
            store.save_queue(&jobs)?;
        }
        Ok(())
    }

    fn begin(
        &mut self,
        configured: &[ConfiguredSchedule],
        now: i64,
        check_now: bool,
    ) -> Vec<PollTicket> {
        let mut checking_repositories: HashSet<_> = self.leases.keys().cloned().collect();
        let mut tickets = Vec::new();
        if check_now {
            for configuration in configured {
                if let Some(health) = self.state.health.get_mut(&configuration.health_key) {
                    if health.enabled && health.schedule_available {
                        health.manual_pending = true;
                    }
                }
            }
        }

        for configuration in configured {
            let Some(account_id) = configuration.provider_account_id.as_ref() else {
                continue;
            };
            let Some(repository_id) = configuration.provider_repository_id.as_ref() else {
                continue;
            };
            let Some(policy_key) = configuration.trigger_policy.as_ref() else {
                continue;
            };
            let Some(health) = self.state.health.get_mut(&configuration.health_key) else {
                continue;
            };
            let interrupted = health.last_failure.as_deref() == Some("interrupted");
            let manual = health.manual_pending;
            if health.in_flight
                || !health.enabled
                || !health.schedule_available
                || (!manual && now < health.next_run && !interrupted)
                || checking_repositories.contains(repository_id)
            {
                continue;
            }

            let cursor = self
                .state
                .cursors
                .get(&configuration.health_key)
                .filter(|cursor| {
                    cursor.name == configuration.name
                        && cursor.account_id == *account_id
                        && cursor.repository_id == *repository_id
                        && cursor.trigger_policy == *policy_key
                })
                .cloned();
            if self.state.cursors.contains_key(&configuration.health_key) && cursor.is_none() {
                self.state.cursors.remove(&configuration.health_key);
            }

            health.last_attempt = Some(now);
            health.last_failure = None;
            health.in_flight = true;
            health.manual_pending = false;
            if !manual {
                health.next_run = next_run(&configuration.schedule, now).unwrap_or(0);
            }
            checking_repositories.insert(repository_id.clone());
            self.leases
                .insert(repository_id.clone(), configuration.health_key.clone());
            tickets.push(PollTicket {
                health_key: configuration.health_key.clone(),
                repository_id: configuration.repository_id.clone(),
                name: configuration.name.clone(),
                provider_account_id: account_id.clone(),
                provider_repository_id: repository_id.clone(),
                policy: configuration.policy.clone(),
                watched_authors: configuration.watched_authors.clone(),
                trigger_policy: policy_key.clone(),
                updated_after: cursor.and_then(|cursor| cursor.updated_after),
                account_generation: 0,
            });
        }
        tickets
    }

    pub fn finish(
        &mut self,
        store: &Store,
        ticket: PollTicket,
        result: Result<PollResult, ConnectionError>,
        now: i64,
    ) -> Result<(), MonitoringError> {
        let settings = store.load_settings().map_err(MonitoringError::Storage)?;
        let mut accounts = configured_accounts(&settings);
        if let Ok(result) = &result {
            accounts.insert(
                result.connection.identity.id.clone(),
                AccountAvailability {
                    login: result.connection.identity.login.clone(),
                    connected: true,
                },
            );
        }
        self.finish_with_accounts(store, &accounts, ticket, result, now)
    }

    pub fn finish_with_accounts(
        &mut self,
        store: &Store,
        accounts: &BTreeMap<String, AccountAvailability>,
        ticket: PollTicket,
        result: Result<PollResult, ConnectionError>,
        now: i64,
    ) -> Result<(), MonitoringError> {
        if let Some(health) = self.state.health.get_mut(&ticket.health_key) {
            health.in_flight = false;
        }
        let settings = match store.load_settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.leases.remove(&ticket.provider_repository_id);
                if let Some(health) = self.state.health.get_mut(&ticket.health_key) {
                    unavailable_health(health, "settings_unavailable");
                }
                let _ = store.save_monitoring_state(&self.state);
                return Err(MonitoringError::Storage(error));
            }
        };
        let configured = configured_schedules(&settings);
        let outcome = match self.synchronize_jobs(store, &configured, accounts) {
            Ok(()) => match result {
                Ok(result) => self.commit_success(store, &configured, &ticket, result, now),
                Err(error) => Err(check_error(error)),
            },
            Err(error) => Err(MonitoringError::Storage(error)),
        };
        self.leases.remove(&ticket.provider_repository_id);
        self.synchronize_health(&configured, accounts, now);
        let current = configured
            .iter()
            .find(|configuration| configuration.health_key == ticket.health_key);
        if current.is_some_and(|configuration| configuration_matches_ticket(configuration, &ticket))
        {
            if let Some(health) = self.state.health.get_mut(&ticket.health_key) {
                match &outcome {
                    Ok(login) => {
                        health.account_login = Some(login.clone());
                        health.last_success = Some(now);
                        health.last_failure = None;
                    }
                    Err(error) => {
                        health.last_failure = Some(error.health_failure().into());
                    }
                }
            }
        }
        store
            .save_monitoring_state(&self.state)
            .map_err(MonitoringError::Storage)?;
        outcome.map(|_| ())
    }

    fn commit_success(
        &mut self,
        store: &Store,
        configured: &[ConfiguredSchedule],
        ticket: &PollTicket,
        result: PollResult,
        now: i64,
    ) -> Result<String, MonitoringError> {
        let configuration = configured
            .iter()
            .find(|configuration| configuration.health_key == ticket.health_key)
            .filter(|configuration| configuration_matches_ticket(configuration, ticket))
            .ok_or_else(configuration_changed)?;
        if result.connection.identity.id != ticket.provider_account_id {
            return Err(check_error(ConnectionError::WrongIdentity));
        }
        if result.connection.repository.id != ticket.provider_repository_id
            || result.connection.repository.name != ticket.name
        {
            return Err(check_error(ConnectionError::RepositoryChanged));
        }

        let mut newest = ticket
            .updated_after
            .as_deref()
            .map(chrono::DateTime::parse_from_rfc3339)
            .transpose()
            .map_err(|_| check_error(ConnectionError::Configuration))?;
        let mut observed = Vec::new();
        for pull in result.pull_requests {
            let updated = chrono::DateTime::parse_from_rfc3339(&pull.updated_at)
                .map_err(|_| check_error(ConnectionError::InvalidResponse))?;
            newest = Some(newest.map_or(updated, |previous| previous.max(updated)));
            if pull.base_repository_id != ticket.provider_repository_id {
                return Err(check_error(ConnectionError::RepositoryChanged));
            }
            let watched_author = pull.author.as_ref().is_some_and(|author| {
                ticket
                    .watched_authors
                    .iter()
                    .any(|watched| watched.id == author.id)
            });
            let requested_reviewer = ticket.policy.reviewer_assignment
                && pull
                    .requested_reviewers
                    .iter()
                    .any(|reviewer| reviewer.id == result.connection.identity.id);
            observed.push((pull, watched_author, requested_reviewer));
        }

        let mut jobs = store.load_queue().map_err(MonitoringError::Storage)?;
        let previous_jobs = jobs.clone();
        for job in &mut jobs {
            if !actionable(job)
                || job.configuration_id != ticket.repository_id
                || job.account_id != ticket.provider_account_id
                || job.repository_id != ticket.provider_repository_id
                || job.trigger_policy != ticket.trigger_policy
            {
                continue;
            }
            job.waiting = match observed
                .iter()
                .find(|(pull, _, _)| pull.id == job.pull_request_id)
            {
                Some((pull, _, _)) if pull.head_sha != job.head_sha => WAITING_SUPERSEDED.into(),
                Some((pull, watched, reviewer))
                    if pull.state != Lifecycle::Open || pull.draft || (!watched && !reviewer) =>
                {
                    WAITING_INELIGIBLE.into()
                }
                Some(_) => continue,
                None => WAITING_NO_LONGER_CURRENT.into(),
            };
        }

        for (pull, watched_author, requested_reviewer) in observed {
            if pull.state != Lifecycle::Open
                || pull.draft
                || (!watched_author && !requested_reviewer)
            {
                continue;
            }
            let job = QueueJob {
                provider: "github".into(),
                account_id: result.connection.identity.id.clone(),
                account_login: result.connection.identity.login.clone(),
                configuration_id: configuration.repository_id.clone(),
                repository_id: ticket.provider_repository_id.clone(),
                repository_name: ticket.name.clone(),
                pull_request_id: pull.id,
                number: pull.number,
                title: pull.title,
                head_sha: pull.head_sha,
                trigger_policy: ticket.trigger_policy.clone(),
                author_id: pull.author.as_ref().map(|author| author.id.clone()),
                author_login: pull.author.as_ref().map(|author| author.login.clone()),
                watched_author,
                requested_reviewer,
                waiting: waiting_state(watched_author, pull.head_repository_id.as_deref(), ticket)
                    .into(),
                detected_at: now,
            };
            if let Some(existing) = jobs.iter_mut().find(|existing| same_job(existing, &job)) {
                let detected_at = existing.detected_at;
                *existing = job;
                existing.detected_at = detected_at;
            } else {
                jobs.push(job);
            }
        }
        if jobs != previous_jobs {
            store.save_queue(&jobs).map_err(MonitoringError::Storage)?;
        }
        self.state.cursors.insert(
            ticket.health_key.clone(),
            PollCursor {
                name: ticket.name.clone(),
                account_id: result.connection.identity.id,
                repository_id: result.connection.repository.id,
                trigger_policy: ticket.trigger_policy.clone(),
                updated_after: newest
                    .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            },
        );
        Ok(result.connection.identity.login)
    }
}

fn configured_accounts(settings: &Settings) -> BTreeMap<String, AccountAvailability> {
    settings
        .repositories
        .iter()
        .filter_map(|repository| repository.provider_account_id.clone())
        .map(|account_id| {
            (
                account_id,
                AccountAvailability {
                    login: String::new(),
                    connected: true,
                },
            )
        })
        .collect()
}

fn configured_schedules(settings: &Settings) -> Vec<ConfiguredSchedule> {
    let mut configured = Vec::new();
    for repository in &settings.repositories {
        let policy = repository.overrides.effective(&settings.defaults);
        let mut watched_authors = policy.watched_authors.clone();
        watched_authors.extend(repository.watched_authors.iter().cloned());
        watched_authors.sort_by(|left, right| left.id.cmp(&right.id));
        watched_authors.dedup_by(|left, right| left.id == right.id);
        let policy_key = trigger_policy(&watched_authors, policy.reviewer_assignment).ok();
        let binding = repository.account_binding();
        let provider_supported = binding.as_ref().is_some_and(|binding| {
            binding.account.provider == ProviderId::Github
                && binding.repository.provider == ProviderId::Github
        });
        let provider_account_id = binding
            .as_ref()
            .map(|binding| binding.account.account_id.clone());
        let provider_repository_id = binding
            .as_ref()
            .map(|binding| binding.repository.repository_id.clone());
        if repository.assignments.is_empty() {
            configured.push(ConfiguredSchedule {
                health_key: repository.id.clone(),
                repository_id: repository.id.clone(),
                name: repository.name.clone(),
                enabled: repository.enabled,
                provider_account_id,
                provider_repository_id,
                provider_supported,
                schedule: policy.schedule.clone(),
                assignment_id: None,
                agent_id: None,
                agent_name: None,
                policy,
                watched_authors,
                trigger_policy: policy_key,
            });
            continue;
        }
        for assignment in &repository.assignments {
            configured.push(ConfiguredSchedule {
                health_key: format!("{}:assignment:{}", repository.id, assignment.id),
                repository_id: repository.id.clone(),
                name: repository.name.clone(),
                enabled: repository.enabled,
                provider_account_id: provider_account_id.clone(),
                provider_repository_id: provider_repository_id.clone(),
                provider_supported,
                schedule: assignment.schedule.clone(),
                assignment_id: Some(assignment.id.clone()),
                agent_id: Some(assignment.agent_id.clone()),
                agent_name: settings
                    .agents
                    .iter()
                    .find(|agent| agent.id == assignment.agent_id)
                    .map(|agent| agent.name.clone()),
                policy: policy.clone(),
                watched_authors: watched_authors.clone(),
                trigger_policy: policy_key.clone(),
            });
        }
    }
    configured
}

fn configuration_matches_ticket(configuration: &ConfiguredSchedule, ticket: &PollTicket) -> bool {
    configuration.enabled
        && configuration.provider_supported
        && configuration.repository_id == ticket.repository_id
        && configuration.name == ticket.name
        && configuration.provider_account_id.as_deref() == Some(&ticket.provider_account_id)
        && configuration.provider_repository_id.as_deref() == Some(&ticket.provider_repository_id)
        && configuration.policy == ticket.policy
        && configuration.trigger_policy.as_deref() == Some(&ticket.trigger_policy)
}

fn unavailable_health(health: &mut ScheduleHealth, failure: &str) {
    health.next_run = 0;
    health.schedule_available = false;
    health.last_failure = Some(failure.into());
    health.manual_pending = false;
}

fn configuration_failure(failure: Option<&str>) -> bool {
    matches!(
        failure,
        Some(
            "account_binding_required"
                | "provider_unavailable"
                | "account_disconnected"
                | "configuration"
                | "invalid_schedule"
                | "settings_unavailable"
        )
    )
}

fn actionable(job: &QueueJob) -> bool {
    matches!(
        job.waiting.as_str(),
        WAITING_TRUST_CONFIRMATION | WAITING_HUMAN_START | WAITING_AGENT_UNAVAILABLE
    )
}

fn waiting_state(
    watched_author: bool,
    head_repository_id: Option<&str>,
    ticket: &PollTicket,
) -> &'static str {
    if !watched_author || head_repository_id != Some(ticket.provider_repository_id.as_str()) {
        WAITING_TRUST_CONFIRMATION
    } else if !ticket.policy.automatic_agent_start {
        WAITING_HUMAN_START
    } else {
        WAITING_AGENT_UNAVAILABLE
    }
}

fn check_error(error: ConnectionError) -> MonitoringError {
    MonitoringError::Recoverable {
        code: format!("{error:?}"),
        message: format!("Repository check failed: {error:?}"),
    }
}

fn configuration_changed() -> MonitoringError {
    MonitoringError::Recoverable {
        code: "configuration_changed".into(),
        message: "Repository check was discarded because its configuration changed.".into(),
    }
}

fn same_job(left: &QueueJob, right: &QueueJob) -> bool {
    left.provider == right.provider
        && left.account_id == right.account_id
        && left.configuration_id == right.configuration_id
        && left.repository_id == right.repository_id
        && left.pull_request_id == right.pull_request_id
        && left.head_sha == right.head_sha
        && left.trigger_policy == right.trigger_policy
}

fn trigger_policy(
    watched_authors: &[WatchedIdentity],
    reviewer_assignment: bool,
) -> Result<String, ConnectionError> {
    let mut authors: Vec<_> = watched_authors
        .iter()
        .map(|author| author.id.as_str())
        .collect();
    authors.sort_unstable();
    serde_json::to_string(&(authors, reviewer_assignment))
        .map_err(|_| ConnectionError::Configuration)
}

fn schedule_key(schedule: &Schedule) -> String {
    match schedule {
        Schedule::Interval { minutes, timezone } => format!("interval:{minutes}:{timezone}"),
        Schedule::Cron {
            expression,
            timezone,
        } => format!("cron:{expression}:{timezone}"),
    }
}

pub fn next_run(schedule: &Schedule, now: i64) -> Result<i64, ConnectionError> {
    match schedule {
        Schedule::Interval { minutes, timezone } => {
            if timezone.parse::<chrono_tz::Tz>().is_err() || *minutes == 0 {
                return Err(ConnectionError::Configuration);
            }
            now.checked_add(i64::from(*minutes) * 60)
                .ok_or(ConnectionError::Configuration)
        }
        Schedule::Cron {
            expression,
            timezone,
        } => {
            let zone = timezone
                .parse::<chrono_tz::Tz>()
                .map_err(|_| ConnectionError::Configuration)?;
            let current = zone
                .timestamp_opt(now, 0)
                .single()
                .ok_or(ConnectionError::Configuration)?;
            expression
                .parse::<croner::Cron>()
                .map_err(|_| ConnectionError::Configuration)?
                .find_next_occurrence(&current, false)
                .map(|time| time.timestamp())
                .map_err(|_| ConnectionError::Configuration)
        }
    }
}
