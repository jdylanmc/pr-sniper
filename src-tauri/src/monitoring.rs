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
pub const WAITING_SCOPE_EXCLUDED: &str = "scope_excluded";
pub const SCOPE_CONFIRMATION_REQUIRED: &str = "scope_confirmation_required";
const RETRY_WINDOW_SECONDS: i64 = 15 * 60;
const MAX_RETRIES: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Queued,
    Running,
    Interrupted,
    Completed,
    Failed,
    ManualRetry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationFailure {
    Timeout,
    RateLimited,
    Network,
    Provider,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobOperation {
    pub id: String,
    pub provider: String,
    pub account_id: String,
    pub configuration_id: String,
    pub repository_id: String,
    pub pull_request_id: Option<String>,
    pub head_sha: Option<String>,
    pub trigger_policy: String,
    pub operation_type: String,
    pub state: OperationState,
    pub attempt_count: u8,
    pub initial_attempt_at: i64,
    pub retry_deadline: i64,
    pub next_attempt_at: Option<i64>,
    pub failure: Option<OperationFailure>,
    pub attempted_mutation: Option<String>,
    pub pending_review_id: Option<String>,
    pub owned_thread_id: Option<String>,
    pub triggering_external_comment_id: Option<String>,
    pub confirmed_receipt: Option<String>,
}

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
    #[serde(default)]
    pub operation: Option<JobOperation>,
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
    #[serde(default)]
    pub all_authors: bool,
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
    #[serde(default)]
    pub activations: BTreeMap<String, MonitoringActivation>,
    #[serde(default)]
    pub operations: BTreeMap<String, JobOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationMode {
    NewOnly,
    SelectedExisting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationBaseline {
    pub number: u64,
    #[serde(alias = "head_sha")]
    pub initial_head_sha: String,
    #[serde(default)]
    pub observed_head_sha: String,
    #[serde(default, alias = "selected")]
    pub initially_selected: bool,
    #[serde(default)]
    pub admitted_head_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitoringActivation {
    pub version: String,
    pub repository_id: String,
    pub name: String,
    pub account_id: String,
    pub provider_repository_id: String,
    pub trigger_policy: String,
    pub creation_watermark: u64,
    pub mode: ActivationMode,
    pub selected_existing: usize,
    pub baseline: BTreeMap<String, ActivationBaseline>,
    pub confirmed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivationCandidate {
    pub pull_request_id: String,
    pub number: u64,
    pub title: String,
    pub head_sha: String,
    pub author_id: Option<String>,
    pub author_login: Option<String>,
    pub watched_author: bool,
    pub all_authors: bool,
    pub requested_reviewer: bool,
    pub trust_confirmation_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivationPreviewView {
    pub preview_id: String,
    pub repository_id: String,
    pub name: String,
    pub account_id: String,
    pub account_login: String,
    pub creation_watermark: u64,
    pub candidates: Vec<ActivationCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivationStatus {
    pub repository_id: String,
    pub active: bool,
    pub reason: Option<String>,
    pub mode: Option<ActivationMode>,
    pub selected_existing: usize,
    pub creation_watermark: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationContext {
    pub repository_id: String,
    pub name: String,
    pub account_id: String,
    pub provider_repository_id: String,
    pub policy: Policy,
    pub watched_authors: Vec<WatchedIdentity>,
    pub trigger_policy: String,
}

pub struct ActivationPreviewEvidence {
    pub context: ActivationContext,
    pub connection: Connection,
    pub pull_requests: Vec<PullRequest>,
    pub creation_watermark: u64,
    pub account_generation: u64,
}

pub struct ActivationApplication<'a> {
    pub repository_id: &'a str,
    pub preview_id: &'a str,
    pub mode: ActivationMode,
    pub selected_pull_request_ids: &'a [String],
    pub account_generation: u64,
    pub now: i64,
}

#[derive(Clone)]
struct ActivationPreview {
    context: ActivationContext,
    candidates: BTreeMap<String, ActivationCandidate>,
    baseline: BTreeMap<String, ActivationBaseline>,
    creation_watermark: u64,
    account_generation: u64,
    previous_activation_version: Option<String>,
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
    pub activation_version: String,
    pub account_generation: u64,
}

pub struct PollResult {
    pub connection: Connection,
    pub pull_requests: Vec<PullRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitoringError {
    Recoverable {
        code: String,
        message: String,
        failure: OperationFailure,
        retry_after_seconds: Option<i64>,
    },
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
    previews: BTreeMap<String, ActivationPreview>,
}

impl Monitor {
    pub fn restore(store: &Store) -> Result<Self, String> {
        let mut state = store.load_monitoring_state()?;
        let previous = state.clone();
        for activation in state.activations.values_mut() {
            for baseline in activation.baseline.values_mut() {
                if baseline.observed_head_sha.is_empty() {
                    baseline.observed_head_sha = baseline.initial_head_sha.clone();
                }
                if baseline.initially_selected && baseline.admitted_head_sha.is_none() {
                    baseline.admitted_head_sha = Some(baseline.initial_head_sha.clone());
                }
            }
        }
        for health in state.health.values_mut() {
            health.manual_pending = false;
            if health.in_flight {
                health.in_flight = false;
                health.last_failure = Some("interrupted".into());
                health.next_run = 0;
                if let Some(operation) = health.operation.as_mut() {
                    if operation.state == OperationState::Running {
                        operation.state = OperationState::Interrupted;
                        operation.next_attempt_at = Some(0);
                    }
                }
            }
        }
        if state != previous {
            store.save_monitoring_state(&state)?;
        }
        Ok(Self {
            state,
            leases: BTreeMap::new(),
            previews: BTreeMap::new(),
        })
    }

    pub fn snapshot(&self) -> Vec<ScheduleHealth> {
        self.state.health.values().cloned().collect()
    }

    pub fn activation_context(
        settings: &Settings,
        repository_id: &str,
    ) -> Result<ActivationContext, ConnectionError> {
        let repository = settings
            .repositories
            .iter()
            .find(|repository| repository.id == repository_id && repository.enabled)
            .ok_or(ConnectionError::Configuration)?;
        let binding = repository
            .account_binding()
            .ok_or(ConnectionError::Configuration)?;
        if binding.account.provider != ProviderId::Github
            || binding.repository.provider != ProviderId::Github
        {
            return Err(ConnectionError::Configuration);
        }
        let policy = repository.overrides.effective(&settings.defaults);
        let watched_authors = effective_watched_authors(repository, &policy);
        let trigger_policy = trigger_policy(&watched_authors, policy.reviewer_assignment)?;
        Ok(ActivationContext {
            repository_id: repository.id.clone(),
            name: repository.name.clone(),
            account_id: binding.account.account_id,
            provider_repository_id: binding.repository.repository_id,
            policy,
            watched_authors,
            trigger_policy,
        })
    }

    pub fn activation_status(&self, settings: &Settings, repository_id: &str) -> ActivationStatus {
        let context = Self::activation_context(settings, repository_id).ok();
        let activation = context.as_ref().and_then(|context| {
            self.state
                .activations
                .get(repository_id)
                .filter(|activation| activation_matches_context(activation, context))
        });
        ActivationStatus {
            repository_id: repository_id.into(),
            active: activation.is_some(),
            reason: activation
                .is_none()
                .then(|| SCOPE_CONFIRMATION_REQUIRED.into()),
            mode: activation.map(|activation| activation.mode.clone()),
            selected_existing: activation
                .map(|activation| activation.selected_existing)
                .unwrap_or(0),
            creation_watermark: activation.map(|activation| activation.creation_watermark),
        }
    }

    pub fn stage_activation_preview(
        &mut self,
        settings: &Settings,
        evidence: ActivationPreviewEvidence,
        current_generation: u64,
    ) -> Result<ActivationPreviewView, ConnectionError> {
        let current = Self::activation_context(settings, &evidence.context.repository_id)?;
        if current != evidence.context
            || current_generation != evidence.account_generation
            || evidence.connection.identity.id != current.account_id
            || evidence.connection.repository.id != current.provider_repository_id
            || evidence.connection.repository.name != current.name
        {
            return Err(ConnectionError::Configuration);
        }
        let mut candidates = BTreeMap::new();
        let mut baseline = BTreeMap::new();
        for pull in evidence.pull_requests {
            if pull.base_repository_id != current.provider_repository_id {
                return Err(ConnectionError::RepositoryChanged);
            }
            if pull.state != Lifecycle::Open || pull.number > evidence.creation_watermark {
                continue;
            }
            if baseline
                .insert(
                    pull.id.clone(),
                    ActivationBaseline {
                        number: pull.number,
                        initial_head_sha: pull.head_sha.clone(),
                        observed_head_sha: pull.head_sha.clone(),
                        initially_selected: false,
                        admitted_head_sha: None,
                    },
                )
                .is_some()
            {
                return Err(ConnectionError::IncompleteRead);
            }
            if pull.draft {
                continue;
            }
            let eligibility = eligibility(
                &current.watched_authors,
                &current.policy,
                &current.account_id,
                &pull,
            );
            if !eligibility.eligible() {
                continue;
            }
            let candidate = ActivationCandidate {
                pull_request_id: pull.id.clone(),
                number: pull.number,
                title: pull.title,
                head_sha: pull.head_sha,
                author_id: pull.author.as_ref().map(|author| author.id.clone()),
                author_login: pull.author.as_ref().map(|author| author.login.clone()),
                watched_author: eligibility.watched_author,
                all_authors: eligibility.all_authors,
                requested_reviewer: eligibility.requested_reviewer,
                trust_confirmation_required: !eligibility.watched_author
                    || pull.head_repository_id.as_deref()
                        != Some(current.provider_repository_id.as_str()),
            };
            if candidates
                .insert(candidate.pull_request_id.clone(), candidate)
                .is_some()
            {
                return Err(ConnectionError::IncompleteRead);
            }
        }
        let preview_id = uuid::Uuid::new_v4().to_string();
        let mut visible_candidates: Vec<_> = candidates.values().cloned().collect();
        visible_candidates.sort_by_key(|candidate| candidate.number);
        let view = ActivationPreviewView {
            preview_id: preview_id.clone(),
            repository_id: current.repository_id.clone(),
            name: current.name.clone(),
            account_id: current.account_id.clone(),
            account_login: evidence.connection.identity.login,
            creation_watermark: evidence.creation_watermark,
            candidates: visible_candidates,
        };
        self.previews
            .retain(|_, preview| preview.context.repository_id != current.repository_id);
        self.previews.insert(
            preview_id,
            ActivationPreview {
                context: current.clone(),
                candidates,
                baseline,
                creation_watermark: view.creation_watermark,
                account_generation: current_generation,
                previous_activation_version: self
                    .state
                    .activations
                    .get(&current.repository_id)
                    .map(|activation| activation.version.clone()),
            },
        );
        Ok(view)
    }

    pub fn cancel_activation_preview(&mut self, preview_id: &str) {
        self.previews.remove(preview_id);
    }

    pub fn activation_preview_context(&self, preview_id: &str) -> Option<ActivationContext> {
        self.previews
            .get(preview_id)
            .map(|preview| preview.context.clone())
    }

    pub fn apply_activation(
        &mut self,
        store: &Store,
        settings: &Settings,
        application: ActivationApplication<'_>,
    ) -> Result<ActivationStatus, String> {
        let preview = self
            .previews
            .get(application.preview_id)
            .cloned()
            .ok_or("Monitoring scope preview expired. Preview again.")?;
        if preview.context.repository_id != application.repository_id {
            return Err("Monitoring scope preview belongs to another repository.".into());
        }
        let context = Self::activation_context(settings, &preview.context.repository_id)
            .map_err(|_| "Repository monitoring configuration changed. Preview again.")?;
        if context != preview.context
            || application.account_generation != preview.account_generation
        {
            self.previews.remove(application.preview_id);
            return Err("Repository monitoring configuration changed. Preview again.".into());
        }
        let current_version = self
            .state
            .activations
            .get(&context.repository_id)
            .map(|activation| activation.version.clone());
        if current_version != preview.previous_activation_version {
            self.previews.remove(application.preview_id);
            return Err("Monitoring scope changed in another operation. Preview again.".into());
        }
        let selected: HashSet<_> = application.selected_pull_request_ids.iter().collect();
        if selected.len() != application.selected_pull_request_ids.len()
            || selected
                .iter()
                .any(|id| !preview.candidates.contains_key(*id))
            || (application.mode == ActivationMode::NewOnly && !selected.is_empty())
            || (application.mode == ActivationMode::SelectedExisting && selected.is_empty())
        {
            return Err("Choose a valid monitoring scope from the current preview.".into());
        }
        let previous = self.state.clone();
        let mut baseline = preview.baseline;
        for selected_id in &selected {
            let candidate = preview
                .candidates
                .get(*selected_id)
                .ok_or("Choose a valid monitoring scope from the current preview.")?;
            let entry = baseline
                .get_mut(*selected_id)
                .ok_or("Monitoring scope preview is incomplete. Preview again.")?;
            entry.initially_selected = true;
            entry.admitted_head_sha = Some(candidate.head_sha.clone());
        }
        self.state.activations.insert(
            context.repository_id.clone(),
            MonitoringActivation {
                version: uuid::Uuid::new_v4().to_string(),
                repository_id: context.repository_id.clone(),
                name: context.name,
                account_id: context.account_id,
                provider_repository_id: context.provider_repository_id,
                trigger_policy: context.trigger_policy,
                creation_watermark: preview.creation_watermark,
                mode: application.mode,
                selected_existing: selected.len(),
                baseline,
                confirmed_at: application.now,
            },
        );
        if let Err(error) = store.save_monitoring_state(&self.state) {
            self.state = previous;
            return Err(error);
        }
        self.previews.remove(application.preview_id);
        Ok(self.activation_status(settings, &context.repository_id))
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
        self.reconcile_activations(&configured);
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
        self.reconcile_activations(&configured);
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
        self.state
            .activations
            .retain(|_, activation| activation.account_id != account_id);
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

    fn reconcile_activations(&mut self, configured: &[ConfiguredSchedule]) {
        self.state.activations.retain(|repository_id, activation| {
            configured.iter().any(|configuration| {
                configuration.repository_id == *repository_id
                    && configuration.enabled
                    && activation_matches_configuration(activation, configuration)
            })
        });
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
            stop_operation_for_correction(health);
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
        let removed_operations: Vec<_> = self
            .state
            .health
            .iter()
            .filter(|(key, health)| !expected.contains(key.as_str()) && !health.in_flight)
            .filter_map(|(_, health)| health.operation.clone())
            .map(|mut operation| {
                if operation.state != OperationState::Completed {
                    operation.state = OperationState::Failed;
                    operation.next_attempt_at = None;
                    operation.failure = Some(OperationFailure::Permanent);
                }
                operation
            })
            .collect();
        for operation in removed_operations {
            self.state
                .operations
                .insert(operation.id.clone(), operation);
        }
        self.state
            .health
            .retain(|key, health| expected.contains(key.as_str()) || health.in_flight);
        self.state
            .cursors
            .retain(|key, _| expected.contains(key.as_str()));

        for configuration in configured {
            let previous = self
                .state
                .health
                .get(&configuration.health_key)
                .filter(|health| {
                    health.name != configuration.name
                        || health.provider_account_id != configuration.provider_account_id
                        || health.provider_repository_id != configuration.provider_repository_id
                })
                .and_then(|health| health.operation.clone());
            if let Some(mut operation) = previous {
                if operation.state != OperationState::Completed {
                    operation.state = OperationState::Failed;
                    operation.next_attempt_at = None;
                    operation.failure = Some(OperationFailure::Permanent);
                }
                self.state
                    .operations
                    .insert(operation.id.clone(), operation);
            }
        }

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
                    operation: None,
                });
            let binding_changed = health.name != configuration.name
                || health.provider_account_id != configuration.provider_account_id
                || health.provider_repository_id != configuration.provider_repository_id;
            if binding_changed {
                health.last_success = None;
                health.next_run = 0;
                health.account_login = None;
                health.operation = None;
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
            if !self
                .state
                .activations
                .get(&configuration.repository_id)
                .is_some_and(|activation| {
                    activation_matches_configuration(activation, configuration)
                })
            {
                unavailable_health(health, SCOPE_CONFIRMATION_REQUIRED);
                self.state.cursors.remove(&configuration.health_key);
                continue;
            }
            if health
                .operation
                .as_ref()
                .is_some_and(|operation| operation.state == OperationState::ManualRetry)
            {
                health.next_run = 0;
                health.schedule_available = false;
                continue;
            }
            if health.operation.as_ref().is_some_and(|operation| {
                matches!(
                    operation.state,
                    OperationState::Queued | OperationState::Interrupted
                ) && now >= operation.retry_deadline
            }) {
                if let Some(operation) = health.operation.as_mut() {
                    operation.state = OperationState::ManualRetry;
                    operation.next_attempt_at = None;
                    operation.failure = Some(OperationFailure::Permanent);
                }
                health.next_run = 0;
                health.schedule_available = false;
                continue;
            }
            if let Some(next_attempt) = health.operation.as_ref().and_then(|operation| {
                matches!(
                    operation.state,
                    OperationState::Queued | OperationState::Interrupted
                )
                .then_some(operation.next_attempt_at)
                .flatten()
            }) {
                health.next_run = next_attempt;
                health.schedule_available = true;
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
            let Some(activation) = self
                .state
                .activations
                .get(&configuration.repository_id)
                .filter(|activation| activation_matches_configuration(activation, configuration))
            else {
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
            start_poll_operation(health, configuration, now, manual);
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
                activation_version: activation.version.clone(),
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
                    stop_operation_for_correction(health);
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
                        if let Some(operation) = health.operation.as_mut() {
                            operation.state = OperationState::Completed;
                            operation.next_attempt_at = None;
                            operation.failure = None;
                        }
                    }
                    Err(error) => {
                        health.last_failure = Some(error.health_failure().into());
                        finish_failed_operation(health, error, now);
                    }
                }
            }
        } else if let Some(health) = self.state.health.get_mut(&ticket.health_key) {
            health.last_failure = Some("configuration_changed".into());
            stop_operation_for_correction(health);
        }
        if let Some(operation) = self
            .state
            .health
            .get(&ticket.health_key)
            .and_then(|health| health.operation.as_ref())
            .filter(|operation| {
                matches!(
                    operation.state,
                    OperationState::Completed
                        | OperationState::Failed
                        | OperationState::ManualRetry
                )
            })
            .cloned()
        {
            self.state
                .operations
                .insert(operation.id.clone(), operation);
        }
        store
            .save_monitoring_state(&self.state)
            .map_err(MonitoringError::Storage)?;
        outcome.map(|_| ())
    }

    pub fn manual_retry_operation(
        &mut self,
        store: &Store,
        operation_id: &str,
        now: i64,
    ) -> Result<(), String> {
        let previous = self.state.clone();
        let health = self
            .state
            .health
            .values_mut()
            .find(|health| {
                health.operation.as_ref().is_some_and(|operation| {
                    operation.id == operation_id
                        && matches!(
                            operation.state,
                            OperationState::Failed | OperationState::ManualRetry
                        )
                })
            })
            .ok_or("This operation is not waiting for manual retry.")?;
        let previous_operation = health
            .operation
            .clone()
            .ok_or("This operation is not waiting for manual retry.")?;
        let operation = health
            .operation
            .as_mut()
            .ok_or("This operation is not waiting for manual retry.")?;
        operation.id = uuid::Uuid::new_v4().to_string();
        operation.state = OperationState::Queued;
        operation.attempt_count = 0;
        operation.initial_attempt_at = now;
        operation.retry_deadline = now + RETRY_WINDOW_SECONDS;
        operation.next_attempt_at = Some(now);
        operation.failure = None;
        health.last_failure = None;
        health.next_run = now;
        health.schedule_available = true;
        self.state
            .operations
            .insert(previous_operation.id.clone(), previous_operation);
        if let Err(error) = store.save_monitoring_state(&self.state) {
            self.state = previous;
            return Err(error);
        }
        Ok(())
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
        let mut activation = self
            .state
            .activations
            .get(&ticket.repository_id)
            .filter(|activation| {
                activation.version == ticket.activation_version
                    && activation_matches_configuration(activation, configuration)
            })
            .cloned()
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
            let admission_candidate = activation_admission_candidate(&mut activation, &pull);
            let eligibility = eligibility(
                &ticket.watched_authors,
                &ticket.policy,
                &result.connection.identity.id,
                &pull,
            );
            let admitted = eligibility.eligible() && admission_candidate;
            if admitted {
                mark_activation_admitted(&mut activation, &pull);
            }
            observed.push((pull, eligibility, admitted));
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
                Some((pull, eligibility, _))
                    if pull.state != Lifecycle::Open || pull.draft || !eligibility.eligible() =>
                {
                    WAITING_INELIGIBLE.into()
                }
                Some((_, _, false)) => WAITING_SCOPE_EXCLUDED.into(),
                Some(_) => continue,
                None => WAITING_NO_LONGER_CURRENT.into(),
            };
        }

        for (pull, eligibility, admitted) in observed {
            if pull.state != Lifecycle::Open || pull.draft || !eligibility.eligible() || !admitted {
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
                watched_author: eligibility.watched_author,
                all_authors: eligibility.all_authors,
                requested_reviewer: eligibility.requested_reviewer,
                waiting: waiting_state(
                    eligibility.watched_author,
                    pull.head_repository_id.as_deref(),
                    ticket,
                )
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
        self.state
            .activations
            .insert(ticket.repository_id.clone(), activation);
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

#[derive(Clone, Copy)]
struct Eligibility {
    watched_author: bool,
    all_authors: bool,
    requested_reviewer: bool,
}

impl Eligibility {
    fn eligible(self) -> bool {
        self.watched_author || self.all_authors || self.requested_reviewer
    }
}

fn eligibility(
    watched_authors: &[WatchedIdentity],
    policy: &Policy,
    account_id: &str,
    pull: &PullRequest,
) -> Eligibility {
    let watched_author = pull.author.as_ref().is_some_and(|author| {
        watched_authors
            .iter()
            .any(|watched| watched.id == author.id)
    });
    let all_authors = watched_authors.is_empty() && pull.author.is_some();
    let requested_reviewer = policy.reviewer_assignment
        && pull
            .requested_reviewers
            .iter()
            .any(|reviewer| reviewer.id == account_id);
    Eligibility {
        watched_author,
        all_authors,
        requested_reviewer,
    }
}

fn activation_admission_candidate(
    activation: &mut MonitoringActivation,
    pull: &PullRequest,
) -> bool {
    if pull.number > activation.creation_watermark {
        return true;
    }
    match activation.baseline.get_mut(&pull.id) {
        Some(baseline) => {
            baseline.number = pull.number;
            baseline.observed_head_sha = pull.head_sha.clone();
            baseline.admitted_head_sha.as_deref() == Some(pull.head_sha.as_str())
                || baseline.initial_head_sha != pull.head_sha
        }
        None => {
            activation.baseline.insert(
                pull.id.clone(),
                ActivationBaseline {
                    number: pull.number,
                    initial_head_sha: pull.head_sha.clone(),
                    observed_head_sha: pull.head_sha.clone(),
                    initially_selected: false,
                    admitted_head_sha: None,
                },
            );
            false
        }
    }
}

fn mark_activation_admitted(activation: &mut MonitoringActivation, pull: &PullRequest) {
    if pull.number <= activation.creation_watermark {
        if let Some(baseline) = activation.baseline.get_mut(&pull.id) {
            baseline.observed_head_sha = pull.head_sha.clone();
            baseline.admitted_head_sha = Some(pull.head_sha.clone());
        }
    }
}

fn effective_watched_authors(
    repository: &crate::storage::Repository,
    policy: &Policy,
) -> Vec<WatchedIdentity> {
    let mut watched_authors = policy.watched_authors.clone();
    watched_authors.extend(repository.watched_authors.iter().cloned());
    watched_authors.sort_by(|left, right| left.id.cmp(&right.id));
    watched_authors.dedup_by(|left, right| left.id == right.id);
    watched_authors
}

fn activation_matches_context(
    activation: &MonitoringActivation,
    context: &ActivationContext,
) -> bool {
    activation.repository_id == context.repository_id
        && activation.name == context.name
        && activation.account_id == context.account_id
        && activation.provider_repository_id == context.provider_repository_id
}

fn activation_matches_configuration(
    activation: &MonitoringActivation,
    configuration: &ConfiguredSchedule,
) -> bool {
    configuration.enabled
        && activation.repository_id == configuration.repository_id
        && activation.name == configuration.name
        && configuration.provider_account_id.as_deref() == Some(&activation.account_id)
        && configuration.provider_repository_id.as_deref()
            == Some(&activation.provider_repository_id)
}

fn configured_schedules(settings: &Settings) -> Vec<ConfiguredSchedule> {
    let mut configured = Vec::new();
    for repository in &settings.repositories {
        let policy = repository.overrides.effective(&settings.defaults);
        let watched_authors = effective_watched_authors(repository, &policy);
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
    stop_operation_for_correction(health);
}

fn stop_operation_for_correction(health: &mut ScheduleHealth) {
    let Some(operation) = health.operation.as_mut() else {
        return;
    };
    if matches!(
        operation.state,
        OperationState::Running | OperationState::Queued | OperationState::Interrupted
    ) {
        operation.state = OperationState::Failed;
        operation.next_attempt_at = None;
        operation.failure = Some(OperationFailure::Permanent);
    }
}

fn start_poll_operation(
    health: &mut ScheduleHealth,
    configuration: &ConfiguredSchedule,
    now: i64,
    manual: bool,
) {
    let retrying = health.operation.as_ref().is_some_and(|operation| {
        matches!(
            operation.state,
            OperationState::Queued | OperationState::Interrupted
        ) && operation
            .next_attempt_at
            .is_some_and(|next| manual || now >= next)
            && now < operation.retry_deadline
            && operation.attempt_count <= MAX_RETRIES
    });
    if retrying {
        if let Some(operation) = health.operation.as_mut() {
            operation.state = OperationState::Running;
            operation.attempt_count += 1;
            operation.next_attempt_at = None;
            return;
        }
    }
    health.operation = Some(JobOperation {
        id: uuid::Uuid::new_v4().to_string(),
        provider: "github".into(),
        account_id: configuration
            .provider_account_id
            .clone()
            .unwrap_or_default(),
        configuration_id: configuration.repository_id.clone(),
        repository_id: configuration
            .provider_repository_id
            .clone()
            .unwrap_or_default(),
        pull_request_id: None,
        head_sha: None,
        trigger_policy: configuration.trigger_policy.clone().unwrap_or_default(),
        operation_type: "repository_poll".into(),
        state: OperationState::Running,
        attempt_count: 1,
        initial_attempt_at: now,
        retry_deadline: now + RETRY_WINDOW_SECONDS,
        next_attempt_at: None,
        failure: None,
        attempted_mutation: None,
        pending_review_id: None,
        owned_thread_id: None,
        triggering_external_comment_id: None,
        confirmed_receipt: None,
    });
}

fn finish_failed_operation(health: &mut ScheduleHealth, error: &MonitoringError, now: i64) {
    let Some(operation) = health.operation.as_mut() else {
        return;
    };
    let failure = retryable_failure(error);
    operation.failure = Some(failure.clone());
    if failure == OperationFailure::Permanent {
        operation.state = OperationState::Failed;
        operation.next_attempt_at = None;
        health.next_run = 0;
        health.schedule_available = false;
        return;
    }
    if operation.attempt_count > MAX_RETRIES || now >= operation.retry_deadline {
        operation.state = OperationState::ManualRetry;
        operation.next_attempt_at = None;
        health.next_run = 0;
        health.schedule_available = false;
        return;
    }
    let retry_count = operation.attempt_count;
    let delay = match error {
        MonitoringError::Recoverable {
            retry_after_seconds: Some(delay),
            ..
        } => *delay,
        _ => retry_delay_seconds(&operation.id, retry_count),
    };
    let next_attempt = now.saturating_add(delay);
    if next_attempt >= operation.retry_deadline {
        operation.state = OperationState::ManualRetry;
        operation.next_attempt_at = None;
        health.next_run = 0;
        health.schedule_available = false;
        return;
    }
    operation.state = OperationState::Queued;
    operation.next_attempt_at = Some(next_attempt);
    health.next_run = next_attempt;
    health.schedule_available = true;
}

fn retryable_failure(error: &MonitoringError) -> OperationFailure {
    match error {
        MonitoringError::Recoverable { failure, .. } => failure.clone(),
        MonitoringError::Storage(_) => OperationFailure::Permanent,
    }
}

fn retry_delay_seconds(operation_id: &str, retry_count: u8) -> i64 {
    let base = 5_i64.saturating_mul(1_i64 << retry_count.saturating_sub(1).min(6));
    let jitter = operation_id
        .bytes()
        .chain([retry_count])
        .fold(0_u64, |value, byte| {
            value.wrapping_mul(31).wrapping_add(byte as u64)
        })
        % 4;
    base + jitter as i64
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
                | "scope_confirmation_required"
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
    let (failure, retry_after_seconds) = match error {
        ConnectionError::Timeout => (OperationFailure::Timeout, None),
        ConnectionError::RateLimited => (OperationFailure::RateLimited, None),
        ConnectionError::RateLimitedAfter(retry_after_seconds) => {
            (OperationFailure::RateLimited, Some(retry_after_seconds))
        }
        ConnectionError::Network => (OperationFailure::Network, None),
        ConnectionError::ProviderFailure => (OperationFailure::Provider, None),
        ConnectionError::ProviderFailureAfter(retry_after_seconds) => {
            (OperationFailure::Provider, Some(retry_after_seconds))
        }
        _ => (OperationFailure::Permanent, None),
    };
    MonitoringError::Recoverable {
        code: connection_error_code(error).into(),
        message: format!("Repository check failed: {error:?}"),
        failure,
        retry_after_seconds,
    }
}

fn connection_error_code(error: ConnectionError) -> &'static str {
    match error {
        ConnectionError::WrongIdentity => "WrongIdentity",
        ConnectionError::InvalidResponse => "InvalidResponse",
        ConnectionError::MissingCli => "MissingCli",
        ConnectionError::BrokenCli => "BrokenCli",
        ConnectionError::SignedOut => "SignedOut",
        ConnectionError::Timeout => "Timeout",
        ConnectionError::MissingReadPermission => "MissingReadPermission",
        ConnectionError::MissingScope => "MissingScope",
        ConnectionError::OrganizationPolicyDenied => "OrganizationPolicyDenied",
        ConnectionError::RateLimited | ConnectionError::RateLimitedAfter(_) => "RateLimited",
        ConnectionError::Network => "Network",
        ConnectionError::ProviderFailure => "ProviderFailure",
        ConnectionError::ProviderFailureAfter(_) => "ProviderFailure",
        ConnectionError::IncompleteRead => "IncompleteRead",
        ConnectionError::RevisionChanged => "RevisionChanged",
        ConnectionError::InvalidRepository => "InvalidRepository",
        ConnectionError::RepositoryChanged => "RepositoryChanged",
        ConnectionError::Configuration => "Configuration",
    }
}

fn configuration_changed() -> MonitoringError {
    MonitoringError::Recoverable {
        code: "configuration_changed".into(),
        message: "Repository check was discarded because its configuration changed.".into(),
        failure: OperationFailure::Permanent,
        retry_after_seconds: None,
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
