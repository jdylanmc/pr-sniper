use crate::github::{
    metadata::{Lifecycle, PullRequest},
    provider::Connection,
    ConnectionError,
};
use crate::policy::{Policy, Schedule};
use crate::storage::{Settings, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleHealth {
    pub repository_id: String,
    pub name: String,
    pub last_attempt: Option<i64>,
    pub last_success: Option<i64>,
    pub next_run: i64,
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

struct Entry {
    health: ScheduleHealth,
    schedule: Schedule,
    account_id: Option<String>,
    remote_id: Option<String>,
}

#[derive(Default)]
pub struct Monitor {
    entries: BTreeMap<String, Entry>,
}

impl Monitor {
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
        self.entries.retain(|id, entry| {
            entry.health.in_flight || settings.repositories.iter().any(|repo| &repo.id == id)
        });
        let mut tickets = Vec::new();
        for repository in &settings.repositories {
            if !repository.enabled {
                continue;
            }
            let policy = repository.overrides.effective(&settings.defaults);
            let next = next_run(&policy.schedule, now)?;
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
                expected_account_id: entry.account_id.clone(),
                expected_repository_id: entry.remote_id.clone(),
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
            let mut authors: Vec<_> = policy
                .watched_authors
                .iter()
                .map(|author| author.id.as_str())
                .collect();
            authors.sort_unstable();
            let trigger_policy = serde_json::to_string(&(
                authors,
                policy.reviewer_assignment,
                &result.connection.identity.id,
            ))
            .map_err(|_| ConnectionError::Configuration)?;
            for pull in result.pull_requests {
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
        Schedule::Cron { .. } => Err(ConnectionError::Configuration),
    }
}
