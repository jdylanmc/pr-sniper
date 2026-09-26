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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleHealth {
    pub repository_id: String,
    pub name: String,
    pub schedule_key: String,
    #[serde(default)]
    pub provider_repository_id: Option<String>,
    pub enabled: bool,
    pub last_attempt: Option<i64>,
    pub last_success: Option<i64>,
    pub next_run: i64,
    pub schedule_available: bool,
    pub last_failure: Option<String>,
    pub in_flight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueJob {
    pub provider: String,
    pub account_id: String,
    pub account_login: String,
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
    pub repository_id: String,
    pub name: String,
    pub provider_account_id: String,
    pub provider_repository_id: String,
    pub policy: Policy,
    pub watched_authors: Vec<WatchedIdentity>,
    pub trigger_policy: String,
    pub updated_after: Option<String>,
}

pub struct PollResult {
    pub connection: Connection,
    pub pull_requests: Vec<PullRequest>,
}

#[derive(Default)]
pub struct Monitor {
    state: MonitoringState,
}

impl Monitor {
    pub fn restore(store: &Store) -> Result<Self, String> {
        let mut state = store.load_monitoring_state()?;
        for health in state.health.values_mut() {
            if health.in_flight {
                health.in_flight = false;
                health.last_failure = Some("interrupted".into());
                health.next_run = 0;
            }
        }
        Ok(Self { state })
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
        let previous = self.state.clone();
        let tickets = self.begin(&settings, now, check_now);
        if self.state != previous {
            if let Err(error) = store.save_monitoring_state(&self.state) {
                for ticket in &tickets {
                    if let Some(health) = self.state.health.get_mut(&ticket.repository_id) {
                        health.in_flight = false;
                        health.last_failure = Some("storage".into());
                    }
                }
                return Err(error);
            }
        }
        Ok(tickets)
    }

    fn begin(&mut self, settings: &Settings, now: i64, check_now: bool) -> Vec<PollTicket> {
        let configured: HashSet<_> = settings
            .repositories
            .iter()
            .map(|repository| repository.id.as_str())
            .collect();
        self.state
            .health
            .retain(|repository_id, _| configured.contains(repository_id.as_str()));
        self.state
            .cursors
            .retain(|repository_id, _| configured.contains(repository_id.as_str()));
        let mut checking_repositories: HashSet<_> = self
            .state
            .health
            .values()
            .filter(|health| health.in_flight)
            .filter_map(|health| health.provider_repository_id.clone())
            .collect();

        let mut tickets = Vec::new();
        for repository in &settings.repositories {
            let policy = repository.overrides.effective(&settings.defaults);
            let schedule_key = schedule_key(&policy.schedule);
            let health = self
                .state
                .health
                .entry(repository.id.clone())
                .or_insert_with(|| ScheduleHealth {
                    repository_id: repository.id.clone(),
                    name: repository.name.clone(),
                    schedule_key: String::new(),
                    provider_repository_id: None,
                    enabled: repository.enabled,
                    last_attempt: None,
                    last_success: None,
                    next_run: 0,
                    schedule_available: false,
                    last_failure: None,
                    in_flight: false,
                });

            let name_changed = health.name != repository.name;
            health.name = repository.name.clone();
            health.enabled = repository.enabled;
            if name_changed {
                health.last_success = None;
                health.next_run = 0;
                self.state.cursors.remove(&repository.id);
            }
            if !repository.enabled {
                continue;
            }

            let Some(binding) = repository.account_binding() else {
                health.schedule_available = false;
                health.last_failure = Some("account_binding_required".into());
                continue;
            };
            if binding.account.provider != ProviderId::Github
                || binding.repository.provider != ProviderId::Github
            {
                health.schedule_available = false;
                health.last_failure = Some("provider_unavailable".into());
                continue;
            }
            if !health.in_flight {
                health.provider_repository_id = Some(binding.repository.repository_id.clone());
            }
            if health.in_flight || checking_repositories.contains(&binding.repository.repository_id)
            {
                continue;
            }

            let mut watched_authors = policy.watched_authors.clone();
            watched_authors.extend(repository.watched_authors.iter().cloned());
            watched_authors.sort_by(|left, right| left.id.cmp(&right.id));
            watched_authors.dedup_by(|left, right| left.id == right.id);
            let policy_key = match trigger_policy(&watched_authors, policy.reviewer_assignment) {
                Ok(key) => key,
                Err(_) => {
                    health.schedule_available = false;
                    health.last_failure = Some("configuration".into());
                    continue;
                }
            };
            let cursor = self
                .state
                .cursors
                .get(&repository.id)
                .filter(|cursor| {
                    cursor.name == repository.name
                        && cursor.account_id == binding.account.account_id
                        && cursor.repository_id == binding.repository.repository_id
                        && cursor.trigger_policy == policy_key
                })
                .cloned();
            if self.state.cursors.contains_key(&repository.id) && cursor.is_none() {
                self.state.cursors.remove(&repository.id);
            }

            let schedule_changed = health.schedule_key != schedule_key;
            let interrupted = health.last_failure.as_deref() == Some("interrupted");
            if schedule_changed || health.next_run == 0 {
                health.next_run = next_run(&policy.schedule, now).unwrap_or(0);
                health.schedule_key = schedule_key;
            }
            health.schedule_available = health.next_run > 0;
            if !health.schedule_available {
                health.last_failure = Some("invalid_schedule".into());
                continue;
            }
            if !check_now && now < health.next_run && !interrupted {
                continue;
            }

            health.last_attempt = Some(now);
            health.last_failure = None;
            health.in_flight = true;
            health.next_run = next_run(&policy.schedule, now).unwrap_or(0);
            checking_repositories.insert(binding.repository.repository_id.clone());
            tickets.push(PollTicket {
                repository_id: repository.id.clone(),
                name: repository.name.clone(),
                provider_account_id: binding.account.account_id,
                provider_repository_id: binding.repository.repository_id,
                policy,
                watched_authors,
                trigger_policy: policy_key,
                updated_after: cursor.and_then(|cursor| cursor.updated_after.clone()),
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
    ) -> Result<(), String> {
        let health = self
            .state
            .health
            .get_mut(&ticket.repository_id)
            .ok_or("Monitoring attempt no longer exists.")?;
        health.in_flight = false;

        let outcome = result.and_then(|result| {
            let settings = store
                .load_settings()
                .map_err(|_| ConnectionError::Configuration)?;
            let repository = settings
                .repositories
                .iter()
                .find(|repository| repository.id == ticket.repository_id && repository.enabled)
                .ok_or(ConnectionError::Configuration)?;
            let binding = repository
                .account_binding()
                .ok_or(ConnectionError::Configuration)?;
            let policy = repository.overrides.effective(&settings.defaults);
            let mut watched_authors = policy.watched_authors.clone();
            watched_authors.extend(repository.watched_authors.iter().cloned());
            watched_authors.sort_by(|left, right| left.id.cmp(&right.id));
            watched_authors.dedup_by(|left, right| left.id == right.id);
            if repository.name != ticket.name
                || binding.account.provider != ProviderId::Github
                || binding.account.account_id != ticket.provider_account_id
                || binding.repository.repository_id != ticket.provider_repository_id
                || policy != ticket.policy
                || trigger_policy(&watched_authors, policy.reviewer_assignment)?
                    != ticket.trigger_policy
            {
                return Err(ConnectionError::Configuration);
            }
            if result.connection.identity.id != ticket.provider_account_id {
                return Err(ConnectionError::WrongIdentity);
            }
            if result.connection.repository.id != ticket.provider_repository_id
                || result.connection.repository.name != ticket.name
            {
                return Err(ConnectionError::RepositoryChanged);
            }

            let mut jobs = store
                .load_queue()
                .map_err(|_| ConnectionError::Configuration)?;
            let previous_jobs = jobs.clone();
            let mut newest = ticket
                .updated_after
                .as_deref()
                .map(chrono::DateTime::parse_from_rfc3339)
                .transpose()
                .map_err(|_| ConnectionError::Configuration)?;
            for pull in result.pull_requests {
                let updated = chrono::DateTime::parse_from_rfc3339(&pull.updated_at)
                    .map_err(|_| ConnectionError::InvalidResponse)?;
                newest = Some(newest.map_or(updated, |previous| previous.max(updated)));
                if pull.base_repository_id != ticket.provider_repository_id {
                    return Err(ConnectionError::RepositoryChanged);
                }
                if ticket
                    .updated_after
                    .as_deref()
                    .and_then(|cursor| chrono::DateTime::parse_from_rfc3339(cursor).ok())
                    .is_some_and(|cursor| updated < cursor)
                    || pull.state != Lifecycle::Open
                    || pull.draft
                {
                    continue;
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
                if !watched_author && !requested_reviewer {
                    continue;
                }
                let job = QueueJob {
                    provider: "github".into(),
                    account_id: result.connection.identity.id.clone(),
                    account_login: result.connection.identity.login.clone(),
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
                    waiting: if !watched_author
                        || pull.head_repository_id.as_deref()
                            != Some(ticket.provider_repository_id.as_str())
                    {
                        "trust_confirmation".into()
                    } else if !ticket.policy.automatic_agent_start {
                        "human_start".into()
                    } else {
                        "agent_unavailable".into()
                    },
                    detected_at: now,
                };
                if let Some(existing) = jobs.iter_mut().find(|existing| same_job(existing, &job)) {
                    existing.account_login = job.account_login;
                    existing.repository_name = job.repository_name;
                    existing.title = job.title;
                    existing.author_login = job.author_login;
                } else {
                    jobs.push(job);
                }
            }
            if jobs != previous_jobs {
                store
                    .save_queue(&jobs)
                    .map_err(|_| ConnectionError::Configuration)?;
            }
            self.state.cursors.insert(
                ticket.repository_id.clone(),
                PollCursor {
                    name: ticket.name.clone(),
                    account_id: result.connection.identity.id,
                    repository_id: result.connection.repository.id,
                    trigger_policy: ticket.trigger_policy,
                    updated_after: newest
                        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                },
            );
            Ok(())
        });

        let health = self
            .state
            .health
            .get_mut(&ticket.repository_id)
            .ok_or("Monitoring attempt no longer exists.")?;
        health.last_failure = outcome.as_ref().err().map(|error| format!("{error:?}"));
        if outcome.is_ok() {
            health.last_success = Some(now);
        }
        let schedule = match store.load_settings() {
            Ok(settings) => settings
                .repositories
                .iter()
                .find(|repository| repository.id == ticket.repository_id && repository.enabled)
                .map(|repository| repository.overrides.effective(&settings.defaults).schedule),
            Err(error) => {
                health.schedule_available = false;
                health.next_run = 0;
                health.last_failure = Some("settings_unavailable".into());
                store.save_monitoring_state(&self.state)?;
                return Err(error);
            }
        };
        let schedule_error = if let Some(schedule) = schedule {
            match next_run(&schedule, now) {
                Ok(next) => {
                    health.next_run = next;
                    health.schedule_available = true;
                    None
                }
                Err(error) => {
                    health.next_run = 0;
                    health.schedule_available = false;
                    health.last_failure = Some("invalid_schedule".into());
                    Some(format!(
                        "Cannot calculate the next repository check: {error:?}"
                    ))
                }
            }
        } else {
            health.next_run = 0;
            health.schedule_available = false;
            None
        };
        store.save_monitoring_state(&self.state)?;
        if let Some(error) = schedule_error {
            return Err(error);
        }
        outcome.map_err(|error| format!("Repository check failed: {error:?}"))
    }
}

fn same_job(left: &QueueJob, right: &QueueJob) -> bool {
    left.provider == right.provider
        && left.account_id == right.account_id
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
