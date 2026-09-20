use crate::github::{
    metadata::{Lifecycle, PullRequest},
    provider::Connection,
    ConnectionError,
};
use crate::policy::{Policy, Schedule};
use crate::storage::{Settings, Store};
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleHealth {
    pub repository_id: String,
    pub name: String,
    pub last_attempt: Option<i64>,
    pub last_success: Option<i64>,
    pub next_run: i64,
    pub schedule_available: bool,
    pub last_failure: Option<ConnectionError>,
    pub in_flight: bool,
}

#[derive(Debug)]
pub struct PollTicket {
    pub repository_id: String,
    pub name: String,
    pub policy: Policy,
    pub expected_account_id: Option<String>,
    pub expected_repository_id: Option<String>,
    pub updated_after: Option<String>,
}

pub struct PollResult {
    pub connection: Connection,
    pub pull_requests: Vec<PullRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueJob {
    pub provider: String,
    pub repository_id: String,
    pub repository_name: String,
    pub pull_request_id: String,
    pub number: u64,
    pub title: String,
    pub head_sha: String,
    pub trigger_policy: String,
    pub watched_author: bool,
    pub requested_reviewer: bool,
    pub waiting: String,
    pub detected_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollCursor {
    pub name: String,
    pub account_id: String,
    pub remote_id: String,
    pub trigger_policy: String,
    pub updated_after: Option<String>,
}

fn trigger_policy(policy: &Policy, account_id: &str) -> Result<String, ConnectionError> {
    let mut authors: Vec<_> = policy
        .watched_authors
        .iter()
        .map(|author| author.id.as_str())
        .collect();
    authors.sort_unstable();
    serde_json::to_string(&(authors, policy.reviewer_assignment, account_id))
        .map_err(|_| ConnectionError::Configuration)
}

struct Entry {
    health: ScheduleHealth,
    schedule: Schedule,
    account_id: Option<String>,
    remote_id: Option<String>,
}

#[derive(Default)]
pub struct Monitor {
    entries: BTreeMap<String, Entry>,
    cursors: BTreeMap<String, PollCursor>,
}

impl Monitor {
    pub fn checkpoint(&self, store: &Store) -> Result<(), String> {
        store.save_poll_cursors(&self.cursors)?;
        store.save_polling_health(&self.snapshot())
    }

    pub fn restore(store: &Store) -> Result<Self, String> {
        Ok(Self {
            entries: BTreeMap::new(),
            cursors: store.load_poll_cursors()?,
        })
    }

    pub fn snapshot(&self) -> Vec<ScheduleHealth> {
        self.entries
            .values()
            .map(|entry| entry.health.clone())
            .collect()
    }

    pub fn begin(
        &mut self,
        settings: &Settings,
        now: i64,
        check_now: bool,
    ) -> Result<Vec<PollTicket>, ConnectionError> {
        self.cursors.retain(|id, cursor| {
            settings.repositories.iter().any(|repository| {
                repository.id == *id
                    && repository.enabled
                    && repository.name == cursor.name
                    && trigger_policy(
                        &repository.overrides.effective(&settings.defaults),
                        &cursor.account_id,
                    )
                    .is_ok_and(|key| key == cursor.trigger_policy)
            })
        });
        self.entries.retain(|id, entry| {
            entry.health.in_flight
                || settings
                    .repositories
                    .iter()
                    .any(|repo| &repo.id == id && repo.enabled)
        });
        let mut tickets = Vec::new();
        for repository in &settings.repositories {
            if !repository.enabled {
                continue;
            }
            let policy = repository.overrides.effective(&settings.defaults);
            let cursor = self.cursors.get(&repository.id);
            let known = self.entries.get(&repository.id).filter(|entry| {
                entry.schedule == policy.schedule && entry.health.name == repository.name
            });
            if known.is_some_and(|entry| entry.health.in_flight) {
                continue;
            }
            let next_result = match known {
                Some(entry) if !entry.health.schedule_available => {
                    Err(ConnectionError::Configuration)
                }
                Some(entry) if now < entry.health.next_run => Ok(entry.health.next_run),
                _ => next_run(&policy.schedule, now),
            };
            let (next, available) = match next_result {
                Ok(next) => (next, true),
                Err(_) => (0, false),
            };
            let entry = self
                .entries
                .entry(repository.id.clone())
                .or_insert_with(|| Entry {
                    health: ScheduleHealth {
                        repository_id: repository.id.clone(),
                        name: repository.name.clone(),
                        last_attempt: None,
                        last_success: None,
                        next_run: next,
                        schedule_available: available,
                        last_failure: None,
                        in_flight: false,
                    },
                    schedule: policy.schedule.clone(),
                    account_id: None,
                    remote_id: None,
                });
            if entry.health.in_flight {
                continue;
            }
            if entry.health.name != repository.name {
                entry.health.name = repository.name.clone();
                entry.account_id = None;
                entry.remote_id = None;
                entry.health.last_success = None;
            }
            if entry.schedule != policy.schedule {
                entry.schedule = policy.schedule.clone();
                entry.health.next_run = next;
            }
            entry.health.schedule_available = available;
            if !available {
                entry.health.next_run = 0;
                entry.health.last_failure = Some(ConnectionError::Configuration);
                continue;
            }
            let due = now >= entry.health.next_run;
            if !due && !check_now {
                continue;
            }
            if due {
                entry.health.next_run = next;
            }
            entry.health.last_attempt = Some(now);
            entry.health.in_flight = true;
            tickets.push(PollTicket {
                repository_id: repository.id.clone(),
                name: repository.name.clone(),
                policy,
                expected_account_id: cursor
                    .map(|value| value.account_id.clone())
                    .or_else(|| entry.account_id.clone()),
                expected_repository_id: cursor
                    .map(|value| value.remote_id.clone())
                    .or_else(|| entry.remote_id.clone()),
                updated_after: cursor.and_then(|value| value.updated_after.clone()),
            });
        }
        Ok(tickets)
    }

    pub fn finish(
        &mut self,
        store: &Store,
        ticket: PollTicket,
        result: Result<PollResult, ConnectionError>,
        now: i64,
    ) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&ticket.repository_id)
            .ok_or("Polling attempt is no longer active.")?;
        entry.health.in_flight = false;
        let outcome = result.and_then(|result| {
            if !result.connection.capabilities.read {
                return Err(ConnectionError::MissingReadPermission);
            }
            let settings = store
                .load_settings()
                .map_err(|_| ConnectionError::Configuration)?;
            let repository = settings
                .repositories
                .iter()
                .find(|repo| repo.id == ticket.repository_id && repo.enabled)
                .ok_or(ConnectionError::Configuration)?;
            if repository.name != ticket.name || result.connection.repository.name != ticket.name {
                return Err(ConnectionError::RepositoryChanged);
            }
            let policy = repository.overrides.effective(&settings.defaults);
            if policy != ticket.policy {
                return Err(ConnectionError::Configuration);
            }
            if ticket
                .expected_account_id
                .as_ref()
                .is_some_and(|id| id != &result.connection.identity.id)
            {
                return Err(ConnectionError::WrongIdentity);
            }
            if ticket
                .expected_repository_id
                .as_ref()
                .is_some_and(|id| id != &result.connection.repository.id)
            {
                return Err(ConnectionError::RepositoryChanged);
            }
            let mut jobs = store
                .load_queue()
                .map_err(|_| ConnectionError::Configuration)?;
            let trigger_policy = trigger_policy(&policy, &result.connection.identity.id)?;
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
                if pull.base_repository_id != result.connection.repository.id {
                    return Err(ConnectionError::RepositoryChanged);
                }
                if pull.state != Lifecycle::Open || pull.draft {
                    continue;
                }
                let watched_author = pull.author.as_ref().is_some_and(|author| {
                    policy
                        .watched_authors
                        .iter()
                        .any(|watched| watched.id == author.id)
                });
                let requested_reviewer = policy.reviewer_assignment
                    && pull
                        .requested_reviewers
                        .iter()
                        .any(|reviewer| reviewer.id == result.connection.identity.id);
                if !watched_author && !requested_reviewer {
                    continue;
                }
                if jobs.iter().any(|job| {
                    job.provider == "github"
                        && job.repository_id == result.connection.repository.id
                        && job.pull_request_id == pull.id
                        && job.head_sha == pull.head_sha
                        && job.trigger_policy == trigger_policy
                }) {
                    continue;
                }
                let waiting = if !watched_author
                    || pull.head_repository_id.as_ref() != Some(&result.connection.repository.id)
                {
                    "trust_confirmation"
                } else if !policy.automatic_agent_start {
                    "human_start"
                } else {
                    "agent_not_implemented"
                };
                jobs.push(QueueJob {
                    provider: "github".into(),
                    repository_id: result.connection.repository.id.clone(),
                    repository_name: ticket.name.clone(),
                    pull_request_id: pull.id,
                    number: pull.number,
                    title: pull.title,
                    head_sha: pull.head_sha,
                    trigger_policy: trigger_policy.clone(),
                    watched_author,
                    requested_reviewer,
                    waiting: waiting.into(),
                    detected_at: now,
                });
            }
            store
                .save_queue(&jobs)
                .map_err(|_| ConnectionError::Configuration)?;
            let cursor = PollCursor {
                name: ticket.name.clone(),
                account_id: result.connection.identity.id.clone(),
                remote_id: result.connection.repository.id.clone(),
                trigger_policy,
                updated_after: newest
                    .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            };
            let mut cursors = self.cursors.clone();
            cursors.insert(ticket.repository_id.clone(), cursor);
            store
                .save_poll_cursors(&cursors)
                .map_err(|_| ConnectionError::Configuration)?;
            self.cursors = cursors;
            entry.account_id = Some(result.connection.identity.id);
            entry.remote_id = Some(result.connection.repository.id);
            Ok(())
        });
        match outcome {
            Ok(()) => {
                entry.health.last_success = Some(now);
                entry.health.last_failure = None;
            }
            Err(error) => entry.health.last_failure = Some(error),
        }
        store.save_polling_health(&self.snapshot())
    }
}

fn next_run(schedule: &Schedule, now: i64) -> Result<i64, ConnectionError> {
    match schedule {
        Schedule::Interval { minutes, .. } => now
            .checked_add(i64::from(*minutes) * 60)
            .ok_or(ConnectionError::Configuration),
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
