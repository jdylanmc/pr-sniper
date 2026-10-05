use crate::github::{
    metadata::{Lifecycle, PullRequest},
    provider::Connection,
    ConnectionError,
};
use crate::policy::{Policy, Schedule, WatchedIdentity};
use crate::storage::{ProviderId, SavedResources, Settings, Store};
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
pub const WAITING_CLOSED: &str = "closed";
pub const WAITING_MERGED: &str = "merged";
pub const WAITING_ASSIGNMENT_REMOVED: &str = "assignment_removed";
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_attempt: Option<AttemptBudget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interruption: Option<crate::capacity::Interruption>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptBudget {
    pub attempt_count: u8,
    pub initial_attempt_at: i64,
    pub retry_deadline: i64,
    pub failure: Option<OperationFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub conversation_admission_pending: bool,
    #[serde(default)]
    pub manual_pending: bool,
    #[serde(default)]
    pub operation: Option<JobOperation>,
    #[serde(default)]
    pub scan_assignments: Vec<ScanAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueJob {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<NormalWork>,
    #[serde(default)]
    pub assignment_id: Option<String>,
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
    #[serde(default)]
    pub observed_base_sha: Option<String>,
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
pub struct ScanAssignment {
    pub assignment_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkTrigger {
    Admission,
    NewRevision,
    Reopened,
    AssignmentAdded,
    LegacyAdmission,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub watched_author: bool,
    pub all_authors: bool,
    pub requested_reviewer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalWork {
    pub id: String,
    pub item_id: String,
    pub iteration_id: String,
    pub iteration: u64,
    pub agent_id: String,
    pub enqueue_order: u64,
    pub pass_ordinal: u64,
    pub trigger: WorkTrigger,
    pub admission: Admission,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_item_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackedPullRequest {
    pub provider: String,
    pub configuration_id: String,
    pub account_id: String,
    pub repository_id: String,
    pub pull_request_id: String,
    pub number: u64,
    pub head_sha: String,
    pub lifecycle: Lifecycle,
    pub iteration_id: String,
    pub item_id: String,
    pub iteration: u64,
    pub admission: Admission,
    pub admitted_at: i64,
    pub observed_at: i64,
}

impl TrackedPullRequest {
    fn matches(&self, job: &QueueJob) -> bool {
        self.provider == job.provider
            && self.configuration_id == job.configuration_id
            && self.account_id == job.account_id
            && self.repository_id == job.repository_id
            && self.pull_request_id == job.pull_request_id
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueState {
    pub jobs: Vec<QueueJob>,
    pub tracked: Vec<TrackedPullRequest>,
    pub next_enqueue_order: u64,
}

impl QueueState {
    pub(crate) fn recover_evidence(&mut self, store: &Store) -> Result<(), String> {
        for run in store.review_evidence()? {
            if run.job.assignment_id.as_deref() == Some(&run.assignment_id)
                && !self.jobs.iter().any(|job| run.matches_job(job))
            {
                self.jobs.push(run.job.clone());
            }
        }
        self.next_enqueue_order = self.next_enqueue_order.max(
            store
                .load_follow_ups()?
                .iter()
                .filter_map(|f| f.enqueue_order)
                .max()
                .unwrap_or(0),
        );
        self.next_enqueue_order = self.next_enqueue_order.max(
            store
                .load_actions()?
                .finals
                .iter()
                .map(|f| f.enqueue_order)
                .max()
                .unwrap_or(0),
        );
        self.next_enqueue_order = self.next_enqueue_order.max(
            store
                .load_feedback()?
                .mentions
                .iter()
                .map(|m| m.enqueue_order)
                .max()
                .unwrap_or(0),
        );
        Ok(())
    }

    pub fn allocate_order(&mut self) -> Result<u64, String> {
        self.next_enqueue_order = self
            .next_enqueue_order
            .max(
                self.jobs
                    .iter()
                    .filter_map(|j| j.work.as_ref().map(|w| w.enqueue_order))
                    .max()
                    .unwrap_or(0),
            )
            .checked_add(1)
            .ok_or("The durable queue order is exhausted.")?;
        Ok(self.next_enqueue_order)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalScan {
    pub schedule_key: String,
    pub next_run: i64,
    pub pending: Vec<String>,
    #[serde(default)]
    pub requested: bool,
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
    pub global_scan: Option<GlobalScan>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupActivation {
    pub repository_id: String,
    pub preview_id: String,
    pub mode: ActivationMode,
    pub selected_pull_request_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SetupReview {
    pub resources: SavedResources,
    pub repository_accounts: BTreeMap<String, AccountAvailability>,
    pub ai_accounts: BTreeMap<String, AccountAvailability>,
    pub scopes: Vec<ActivationStatus>,
    pub watched_authors: BTreeMap<String, Vec<WatchedIdentity>>,
    pub configured_inactive: bool,
    pub paused: bool,
    pub confirmation: String,
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
    pub assignment_id: Option<String>,
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
    pub assignments: Vec<ScanAssignment>,
    pub tracked: Vec<(String, u64)>,
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
            || matches!(self, Self::Recoverable { code, .. } if code == "conversation_admission")
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
    assignments: Vec<ScanAssignment>,
}

#[derive(Default)]
pub struct Monitor {
    state: MonitoringState,
    leases: BTreeMap<String, String>,
    previews: BTreeMap<String, ActivationPreview>,
}

impl Monitor {
    pub(crate) fn restore_readonly(state: MonitoringState) -> Self {
        Self {
            state,
            leases: Default::default(),
            previews: Default::default(),
        }
    }
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
                health.conversation_admission_pending = true;
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
        let activation = self.activation_for_application(settings, &application)?;
        let previous = self.state.clone();
        self.state
            .activations
            .insert(application.repository_id.into(), activation);
        if let Err(error) = store.save_monitoring_state(&self.state) {
            self.state = previous;
            return Err(error);
        }
        self.previews.remove(application.preview_id);
        Ok(self.activation_status(settings, application.repository_id))
    }

    fn activation_for_application(
        &mut self,
        settings: &Settings,
        application: &ActivationApplication<'_>,
    ) -> Result<MonitoringActivation, String> {
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
        Ok(MonitoringActivation {
            version: uuid::Uuid::new_v4().to_string(),
            repository_id: context.repository_id.clone(),
            name: context.name,
            account_id: context.account_id,
            provider_repository_id: context.provider_repository_id,
            trigger_policy: context.trigger_policy,
            creation_watermark: preview.creation_watermark,
            mode: application.mode.clone(),
            selected_existing: selected.len(),
            baseline,
            confirmed_at: application.now,
        })
    }

    /// The caller holds the account, Store and Monitor locks through review/apply.
    /// Hash only activation versions, not scan progress, so existing work can continue.
    pub fn setup_review(
        &self,
        store: &Store,
        repository_accounts: BTreeMap<String, AccountAvailability>,
        ai_accounts: BTreeMap<String, AccountAvailability>,
        generations: &BTreeMap<String, u64>,
        ai_generations: &BTreeMap<String, u64>,
    ) -> Result<SetupReview, String> {
        let resources = store.saved_resources()?;
        let paused = store.load_automation()?.paused;
        let versions: BTreeMap<_, _> = self
            .state
            .activations
            .iter()
            .map(|(id, activation)| (id, &activation.version))
            .collect();
        let evidence = serde_json::to_vec(&(
            &resources.settings,
            &repository_accounts,
            &ai_accounts,
            generations,
            ai_generations,
            versions,
            paused,
        ))
        .map_err(|_| "Cannot prepare the final monitoring confirmation.")?;
        let confirmation = format!("{:x}", Sha256::digest(evidence));
        let scopes = resources
            .settings
            .repositories
            .iter()
            .map(|repository| self.activation_status(&resources.settings, &repository.id))
            .collect();
        let watched_authors = resources
            .settings
            .repositories
            .iter()
            .map(|repository| {
                let policy = repository.overrides.effective(&resources.settings.defaults);
                (
                    repository.id.clone(),
                    effective_watched_authors(repository, &policy),
                )
            })
            .collect();
        // Saved, bound, assigned repositories deliberately switched off are not
        // a fresh install, even after reconciliation removes their active scopes.
        let settings = &resources.settings;
        let configured_inactive = !settings.repositories.is_empty()
            && settings
                .repositories
                .iter()
                .all(|repository| !repository.enabled)
            && settings.repositories.iter().any(|repository| {
                repository.provider_account_id.is_some()
                    && repository.provider_repository_id.is_some()
                    && !repository.assignments.is_empty()
                    && repository.assignments.iter().all(|assignment| {
                        settings.agents.iter().any(|agent| {
                            agent.id == assignment.agent_id
                                && agent.ai_account.is_some()
                                && !agent.model.trim().is_empty()
                        })
                    })
            });
        Ok(SetupReview {
            resources,
            repository_accounts,
            ai_accounts,
            scopes,
            watched_authors,
            configured_inactive,
            paused,
            confirmation,
        })
    }

    pub fn apply_setup(
        &mut self,
        store: &Store,
        review: &SetupReview,
        expected_confirmation: &str,
        requests: &[SetupActivation],
        generations: &BTreeMap<String, u64>,
        now: i64,
    ) -> Result<(), String> {
        if review.confirmation != expected_confirmation {
            return Err("Setup changed. Review the current accounts, permissions, scope, schedule and capacity again.".into());
        }
        let settings = &review.resources.settings;
        if !review.resources.readiness.configuration_ready {
            return Err(
                "Complete the saved repository and Agent configuration before confirming.".into(),
            );
        }
        let connected = |accounts: &BTreeMap<String, AccountAvailability>, id: &str| {
            accounts.get(id).is_some_and(|account| account.connected)
        };
        let mut pending = HashSet::new();
        for repository in settings.repositories.iter().filter(|r| r.enabled) {
            if !connected(
                &review.repository_accounts,
                repository.provider_account_id.as_deref().unwrap_or(""),
            ) {
                return Err("Reconnect the acting GitHub account, then review setup again.".into());
            }
            for assignment in &repository.assignments {
                let account = settings
                    .agents
                    .iter()
                    .find(|agent| agent.id == assignment.agent_id)
                    .and_then(|agent| agent.ai_account.as_ref());
                if !account.is_some_and(|a| connected(&review.ai_accounts, &a.account_id)) {
                    return Err(
                        "Reconnect the assigned Copilot account, then review setup again.".into(),
                    );
                }
            }
            if !self.activation_status(settings, &repository.id).active {
                pending.insert(repository.id.as_str());
            }
        }
        if requests.len() != pending.len()
            || requests
                .iter()
                .any(|request| !pending.remove(request.repository_id.as_str()))
        {
            return Err("Choose scope for each pending repository. Already-authorized monitoring is not replayed.".into());
        }
        let mut activations = Vec::new();
        for request in requests {
            let context = Self::activation_context(settings, &request.repository_id)
                .map_err(|_| "Repository monitoring configuration changed. Preview again.")?;
            activations.push(self.activation_for_application(
                settings,
                &ActivationApplication {
                    repository_id: &request.repository_id,
                    preview_id: &request.preview_id,
                    mode: request.mode.clone(),
                    selected_pull_request_ids: &request.selected_pull_request_ids,
                    account_generation: generations.get(&context.account_id).copied().unwrap_or(0),
                    now,
                },
            )?);
        }
        if activations.is_empty() {
            return Ok(());
        }
        let previous = self.state.clone();
        for activation in activations {
            self.state
                .activations
                .insert(activation.repository_id.clone(), activation);
        }
        if let Err(error) = store.save_monitoring_state(&self.state) {
            self.state = previous;
            return Err(error);
        }
        for request in requests {
            self.previews.remove(&request.preview_id);
        }
        Ok(())
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
        if store.load_automation()?.paused {
            return Ok(Vec::new());
        }
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
        let mut queue = store.load_queue_state()?;
        queue.recover_evidence(store)?;
        let tickets = self.begin(&configured, &queue, now, check_now);
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
        if let Some(scan) = &mut self.state.global_scan {
            scan.requested = false;
            scan.pending.retain(|id| {
                self.state
                    .health
                    .get(id)
                    .is_some_and(|h| !h.manual_pending || h.in_flight)
            });
        }
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

    pub fn request_revision_check(
        &mut self,
        store: &Store,
        job: &QueueJob,
        now: i64,
    ) -> Result<(), String> {
        let previous = self.state.clone();
        let health = self
            .state
            .health
            .values_mut()
            .find(|health| {
                health.repository_id == job.configuration_id
                    && health.enabled
                    && health.schedule_available
                    && health.provider_account_id.as_deref() == Some(&job.account_id)
                    && health.provider_repository_id.as_deref() == Some(&job.repository_id)
            })
            .ok_or("The eligible revision could not be queued; check monitoring health.")?;
        if health.operation.as_ref().is_some_and(|operation| {
            matches!(
                operation.state,
                OperationState::Failed | OperationState::ManualRetry
            )
        }) {
            return Err("Monitoring needs correction or manual retry before the new revision can be queued.".into());
        }
        // A publication recheck must not bypass a poll's existing retry budget or Retry-After.
        if health.operation.as_ref().is_none_or(|operation| {
            !matches!(
                operation.state,
                OperationState::Queued | OperationState::Interrupted
            )
        }) {
            if let Some(scan) = &mut self.state.global_scan {
                scan.next_run = scan.next_run.min(now.max(1));
            }
        }
        if let Err(error) = store.save_monitoring_state(&self.state) {
            self.state = previous;
            return Err(error);
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
        let schedule = configured.first().map(|c| &c.schedule);
        let any_enabled = configured.iter().any(|configuration| configuration.enabled);
        let valid_global_cron = schedule.is_some_and(|schedule| {
            matches!(schedule, Schedule::Cron { .. }) && next_run(schedule, now).is_ok()
        });
        if let Some(schedule) = schedule {
            let key = schedule_key(schedule);
            let next = if any_enabled && matches!(schedule, Schedule::Cron { .. }) {
                next_run(schedule, now).unwrap_or(0)
            } else {
                0
            };
            let scan = self.state.global_scan.get_or_insert_with(|| GlobalScan {
                schedule_key: key.clone(),
                next_run: next,
                pending: Vec::new(),
                requested: false,
            });
            if scan.schedule_key != key {
                scan.schedule_key = key;
                scan.next_run = next;
            } else if any_enabled && scan.next_run == 0 {
                scan.next_run = next;
            }
        }
        // Collapse legacy assignment health without losing its retry budget.
        for configuration in configured {
            if self.state.health.contains_key(&configuration.health_key) {
                continue;
            }
            let previous = self
                .state
                .health
                .values()
                .filter(|h| {
                    h.repository_id == configuration.repository_id
                        && h.provider_account_id == configuration.provider_account_id
                        && h.provider_repository_id == configuration.provider_repository_id
                })
                .max_by_key(|h| {
                    (
                        h.operation
                            .as_ref()
                            .map(|o| match o.state {
                                OperationState::ManualRetry => 4,
                                OperationState::Failed => 3,
                                OperationState::Queued
                                | OperationState::Interrupted
                                | OperationState::Running => 2,
                                OperationState::Completed => 1,
                            })
                            .unwrap_or(0),
                        h.operation
                            .as_ref()
                            .and_then(|o| o.next_attempt_at)
                            .unwrap_or(0),
                        h.last_attempt,
                    )
                })
                .cloned();
            if let Some(mut previous) = previous {
                previous.scan_assignments = configuration.assignments.clone();
                if previous.operation.as_ref().is_some_and(|o| {
                    matches!(
                        o.state,
                        OperationState::Queued | OperationState::Interrupted
                    )
                }) {
                    if let Some(scan) = &mut self.state.global_scan {
                        scan.pending.push(configuration.health_key.clone());
                    }
                }
                self.state
                    .health
                    .insert(configuration.health_key.clone(), previous);
            }
        }
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
            .filter(|operation| {
                !self.state.health.iter().any(|(key, health)| {
                    expected.contains(key.as_str())
                        && health
                            .operation
                            .as_ref()
                            .is_some_and(|current| current.id == operation.id)
                })
            })
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
                    conversation_admission_pending: false,
                    manual_pending: false,
                    operation: None,
                    scan_assignments: Vec::new(),
                });
            let binding_changed = health.name != configuration.name
                || health.provider_account_id != configuration.provider_account_id
                || health.provider_repository_id != configuration.provider_repository_id;
            let key = schedule_key(&configuration.schedule);
            let schedule_changed = health.schedule_key != key;
            health.schedule_key = key;
            if valid_global_cron
                && matches!(
                    health.last_failure.as_deref(),
                    Some("invalid_global_cron") | Some("invalid_schedule")
                )
            {
                health.last_failure = None;
            }
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
            if !matches!(configuration.schedule, Schedule::Cron { .. }) {
                unavailable_health(health, "invalid_global_cron");
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
            if !health.in_flight
                && configuration_failure(health.last_failure.as_deref())
                && health
                    .operation
                    .as_ref()
                    .is_some_and(|o| o.state == OperationState::Failed)
            {
                if let Some(operation) = health.operation.take() {
                    self.state
                        .operations
                        .insert(operation.id.clone(), operation);
                }
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

            if schedule_changed || health.next_run == 0 {
                health.next_run = self
                    .state
                    .global_scan
                    .as_ref()
                    .map(|s| s.next_run)
                    .unwrap_or(0);
            }
            health.schedule_available = health.next_run > 0;
            if !health.schedule_available {
                health.last_failure = Some("invalid_schedule".into());
                health.manual_pending = false;
            } else if configuration_failure(health.last_failure.as_deref()) {
                health.last_failure = None;
            }
        }
        let has_schedulable_repository = self
            .state
            .health
            .values()
            .any(|health| health.enabled && health.schedule_available);
        if let Some(scan) = &mut self.state.global_scan {
            if has_schedulable_repository {
                if scan.next_run == 0 {
                    scan.next_run = schedule
                        .and_then(|schedule| next_run(schedule, now).ok())
                        .unwrap_or(0);
                }
            } else {
                scan.next_run = 0;
                scan.requested = false;
            }
            scan.pending.retain(|id| {
                self.state
                    .health
                    .get(id)
                    .is_some_and(|h| h.enabled && (h.schedule_available || h.in_flight))
            });
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
            if !actionable(job) && job.work.is_none()
                || matches!(
                    job.waiting.as_str(),
                    WAITING_SUPERSEDED | WAITING_CLOSED | WAITING_MERGED
                )
            {
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
            if job.work.is_none()
                && configuration.trigger_policy.as_deref() != Some(&job.trigger_policy)
            {
                job.waiting = WAITING_POLICY_CHANGED.into();
                continue;
            }
            if let Some(work) = &job.work {
                if !configuration.assignments.iter().any(|a| {
                    Some(a.assignment_id.as_str()) == job.assignment_id.as_deref()
                        && a.agent_id == work.agent_id
                }) {
                    job.waiting = WAITING_ASSIGNMENT_REMOVED.into();
                    continue;
                }
                if actionable(job) {
                    job.watched_author = job.author_id.as_ref().is_some_and(|id| {
                        configuration.watched_authors.iter().any(|a| &a.id == id)
                    });
                    if !job.watched_author {
                        job.waiting = WAITING_TRUST_CONFIRMATION.into();
                    }
                }
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
        queue: &QueueState,
        now: i64,
        check_now: bool,
    ) -> Vec<PollTicket> {
        let mut checking_repositories: HashSet<_> = self.leases.keys().cloned().collect();
        let mut tickets = Vec::new();
        if let Some(scan) = &mut self.state.global_scan {
            if check_now
                && !scan.pending.is_empty()
                && self.state.health.values().any(|h| h.in_flight)
            {
                scan.requested = true;
            }
            if scan.pending.is_empty()
                && scan.next_run > 0
                && (check_now || scan.requested || now >= scan.next_run)
            {
                scan.requested = false;
                scan.next_run = configured
                    .first()
                    .and_then(|c| next_run(&c.schedule, now).ok())
                    .unwrap_or(0);
                for configuration in configured {
                    if let Some(health) = self.state.health.get_mut(&configuration.health_key) {
                        if health.enabled
                            && health.schedule_available
                            && health.operation.as_ref().is_none_or(|o| {
                                !matches!(
                                    o.state,
                                    OperationState::Failed | OperationState::ManualRetry
                                )
                            })
                        {
                            scan.pending.push(configuration.health_key.clone());
                            health.scan_assignments = configuration.assignments.clone();
                            health.manual_pending = check_now;
                        }
                        if health
                            .operation
                            .as_ref()
                            .is_none_or(|o| o.state == OperationState::Completed)
                        {
                            health.next_run = scan.next_run;
                        }
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
            let retry_waiting = health.operation.as_ref().is_some_and(|o| {
                matches!(
                    o.state,
                    OperationState::Queued | OperationState::Interrupted
                ) && o.next_attempt_at.is_none_or(|t| now < t)
            });
            if health.in_flight
                || !health.enabled
                || !health.schedule_available
                || !self
                    .state
                    .global_scan
                    .as_ref()
                    .is_some_and(|scan| scan.pending.contains(&configuration.health_key))
                || retry_waiting
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
            start_poll_operation(health, configuration, now, false);
            health.next_run = self
                .state
                .global_scan
                .as_ref()
                .map(|s| s.next_run)
                .unwrap_or(0);
            checking_repositories.insert(repository_id.clone());
            self.leases
                .insert(repository_id.clone(), configuration.health_key.clone());
            tickets.push(PollTicket {
                health_key: configuration.health_key.clone(),
                assignment_id: configuration.assignment_id.clone(),
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
                assignments: health.scan_assignments.clone(),
                tracked: queue
                    .tracked
                    .iter()
                    .filter(|p| {
                        p.lifecycle == Lifecycle::Open
                            && p.configuration_id == configuration.repository_id
                            && p.account_id == *account_id
                            && p.repository_id == *repository_id
                    })
                    .map(|p| (p.pull_request_id.clone(), p.number))
                    .chain(
                        queue
                            .jobs
                            .iter()
                            .filter(|j| {
                                (j.configuration_id == configuration.repository_id
                                    || j.configuration_id.is_empty() && j.work.is_none())
                                    && j.account_id == *account_id
                                    && j.repository_id == *repository_id
                                    && !queue.tracked.iter().any(|p| p.matches(j))
                            })
                            .map(|j| (j.pull_request_id.clone(), j.number)),
                    )
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect(),
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
        self.finish_with_admission(store, accounts, ticket, result, now, |_, _| Ok(()))
    }

    pub(crate) fn finish_with_admission(
        &mut self,
        store: &Store,
        accounts: &BTreeMap<String, AccountAvailability>,
        ticket: PollTicket,
        result: Result<PollResult, ConnectionError>,
        now: i64,
        admit: impl FnOnce(&Store, &PollTicket) -> Result<(), String>,
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
                Ok(result) => self
                    .commit_success(store, &configured, &ticket, result, now)
                    .and_then(|login| {
                        if let Some(health) = self.state.health.get_mut(&ticket.health_key) {
                            health.conversation_admission_pending = true;
                        }
                        admit(store, &ticket).map_err(|message| MonitoringError::Recoverable {
                            code: "conversation_admission".into(),
                            message,
                            failure: OperationFailure::Permanent,
                            retry_after_seconds: None,
                        })?;
                        Ok(login)
                    }),
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
                        health.conversation_admission_pending = false;
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
        let retrying = self
            .state
            .health
            .get(&ticket.health_key)
            .and_then(|h| h.operation.as_ref())
            .is_some_and(|o| {
                matches!(
                    o.state,
                    OperationState::Queued | OperationState::Interrupted
                )
            });
        if !retrying {
            if let Some(scan) = &mut self.state.global_scan {
                scan.pending.retain(|id| id != &ticket.health_key);
            }
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
        if let Some(scan) = &mut self.state.global_scan {
            if !scan.pending.contains(&health.repository_id) {
                scan.pending.push(health.repository_id.clone());
            }
        }
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
        let mut queue = store.load_queue_state().map_err(MonitoringError::Storage)?;
        let previous_queue = queue.clone();
        queue
            .recover_evidence(store)
            .map_err(MonitoringError::Storage)?;
        let reviews = store.review_evidence().map_err(MonitoringError::Storage)?;
        let reserved: HashSet<_> = store
            .load_publications()
            .map_err(MonitoringError::Storage)?
            .into_iter()
            .filter(|p| p.reserves_revision())
            .map(|p| p.review.key)
            .collect();
        let mut pulls = result.pull_requests;
        pulls.sort_by_key(|p| p.number);
        let mut seen = HashSet::new();
        for pull in pulls {
            if !seen.insert(pull.id.clone()) {
                return Err(check_error(ConnectionError::IncompleteRead));
            }
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
            let bound = |job: &QueueJob| {
                job.provider == "github"
                    && (job.configuration_id == ticket.repository_id
                        || job.configuration_id.is_empty() && job.work.is_none())
                    && job.account_id == ticket.provider_account_id
                    && job.repository_id == ticket.provider_repository_id
                    && job.pull_request_id == pull.id
            };
            let mut index = queue.tracked.iter().position(|p| {
                p.configuration_id == ticket.repository_id
                    && p.account_id == ticket.provider_account_id
                    && p.repository_id == ticket.provider_repository_id
                    && p.pull_request_id == pull.id
            });
            let mut trigger = WorkTrigger::AssignmentAdded;
            if index.is_none() {
                let legacy = queue.jobs.iter().filter(|j| bound(j)).max_by_key(|j| {
                    (
                        j.work.as_ref().map(|w| w.iteration).unwrap_or(0),
                        j.detected_at,
                    )
                });
                if legacy.is_none()
                    && (pull.state != Lifecycle::Open
                        || pull.draft
                        || !eligibility.eligible()
                        || !(admission_candidate || eligibility.requested_reviewer))
                {
                    continue;
                }
                let iteration_id = legacy
                    .and_then(|j| j.work.as_ref())
                    .map(|w| w.iteration_id.clone())
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let admission = legacy
                    .and_then(|j| j.work.as_ref())
                    .map(|w| w.admission.clone())
                    .or_else(|| {
                        legacy.map(|job| Admission {
                            watched_author: job.watched_author,
                            all_authors: job.all_authors,
                            requested_reviewer: job.requested_reviewer,
                        })
                    })
                    .unwrap_or(Admission {
                        watched_author: eligibility.watched_author,
                        all_authors: eligibility.all_authors,
                        requested_reviewer: eligibility.requested_reviewer,
                    });
                trigger = match legacy {
                    Some(job) if job.work.is_some() => WorkTrigger::AssignmentAdded,
                    Some(_) => WorkTrigger::LegacyAdmission,
                    None => WorkTrigger::Admission,
                };
                queue.tracked.push(TrackedPullRequest {
                    provider: "github".into(),
                    configuration_id: ticket.repository_id.clone(),
                    account_id: ticket.provider_account_id.clone(),
                    repository_id: ticket.provider_repository_id.clone(),
                    pull_request_id: pull.id.clone(),
                    number: pull.number,
                    head_sha: legacy
                        .map(|j| j.head_sha.clone())
                        .unwrap_or_else(|| pull.head_sha.clone()),
                    lifecycle: match legacy.map(|j| j.waiting.as_str()) {
                        Some(WAITING_CLOSED) => Lifecycle::Closed,
                        Some(WAITING_MERGED) => Lifecycle::Merged,
                        _ => Lifecycle::Open,
                    },
                    iteration: legacy
                        .and_then(|j| j.work.as_ref())
                        .map(|w| w.iteration)
                        .unwrap_or(1),
                    item_id: legacy
                        .map(crate::queue::item_id)
                        .unwrap_or_else(|| format!("iteration-{iteration_id}")),
                    iteration_id,
                    admission,
                    admitted_at: legacy.map(|j| j.detected_at).unwrap_or(now),
                    observed_at: now,
                });
                index = Some(queue.tracked.len() - 1);
                mark_activation_admitted(&mut activation, &pull);
            }
            let tracked = &mut queue.tracked[index.unwrap()];
            if pull.state == Lifecycle::Open
                && (tracked.lifecycle != Lifecycle::Open || tracked.head_sha != pull.head_sha)
            {
                trigger = if tracked.lifecycle != Lifecycle::Open {
                    WorkTrigger::Reopened
                } else {
                    WorkTrigger::NewRevision
                };
                tracked.iteration = tracked.iteration.checked_add(1).ok_or_else(|| {
                    MonitoringError::Storage("PR iteration counter exhausted.".into())
                })?;
                tracked.iteration_id = uuid::Uuid::new_v4().to_string();
                tracked.item_id = format!("iteration-{}", tracked.iteration_id);
            }
            tracked.head_sha = pull.head_sha.clone();
            tracked.lifecycle = pull.state.clone();
            tracked.observed_at = now;
            let tracked = tracked.clone();
            for job in queue.jobs.iter_mut().filter(|j| bound(j)) {
                if matches!(job.waiting.as_str(), WAITING_CLOSED | WAITING_MERGED) {
                    continue;
                }
                if pull.state != Lifecycle::Open {
                    job.waiting = if pull.state == Lifecycle::Merged {
                        WAITING_MERGED
                    } else {
                        WAITING_CLOSED
                    }
                    .into();
                } else if job
                    .work
                    .as_ref()
                    .is_some_and(|w| w.iteration_id != tracked.iteration_id)
                    || job.head_sha != pull.head_sha
                {
                    job.waiting = WAITING_SUPERSEDED.into();
                } else if pull.draft {
                    job.waiting = WAITING_INELIGIBLE.into();
                }
            }
            if pull.state != Lifecycle::Open || pull.draft {
                continue;
            }
            for assignment in &ticket.assignments {
                if !configuration.assignments.contains(assignment) {
                    continue;
                }
                let mut existing = queue.jobs.iter().position(|j| {
                    tracked.matches(j)
                        && j.assignment_id.as_deref() == Some(&assignment.assignment_id)
                        && j.work.as_ref().is_some_and(|w| {
                            w.iteration_id == tracked.iteration_id
                                && w.agent_id == assignment.agent_id
                        })
                });
                let mut adopting = false;
                if existing.is_none() && tracked.iteration == 1 {
                    existing = queue
                        .jobs
                        .iter()
                        .enumerate()
                        .filter(|(_, j)| {
                            bound(j)
                                && j.head_sha == pull.head_sha
                                && j.work.is_none()
                                && j.assignment_id.as_deref() == Some(&assignment.assignment_id)
                                && reviews
                                    .iter()
                                    .rev()
                                    .find(|r| r.matches_job(j))
                                    .is_none_or(|r| r.selection.agent.id == assignment.agent_id)
                        })
                        .max_by_key(|(_, j)| {
                            (
                                reserved
                                    .contains(&crate::review::key(j, &assignment.assignment_id)),
                                reviews.iter().any(|r| {
                                    r.matches_job(j)
                                        && r.operation.state == OperationState::Completed
                                }),
                                j.detected_at,
                            )
                        })
                        .map(|(i, _)| i);
                    adopting = existing.is_some();
                }
                let work = if let Some(work) = existing.and_then(|i| queue.jobs[i].work.clone()) {
                    work
                } else {
                    let prior: Vec<_> = queue
                        .jobs
                        .iter()
                        .filter(|j| {
                            tracked.matches(j)
                                && j.work
                                    .as_ref()
                                    .map(|w| w.agent_id == assignment.agent_id)
                                    .unwrap_or_else(|| {
                                        j.assignment_id.as_deref()
                                            == Some(&assignment.assignment_id)
                                    })
                        })
                        .collect();
                    let ordinal = prior
                        .iter()
                        .filter_map(|j| j.work.as_ref().map(|w| w.pass_ordinal))
                        .max()
                        .unwrap_or(0)
                        .max(prior.len() as u64)
                        .checked_add(u64::from(!adopting))
                        .ok_or_else(|| {
                            MonitoringError::Storage("Normal pass counter exhausted.".into())
                        })?;
                    let order = queue.allocate_order().map_err(MonitoringError::Storage)?;
                    let legacy = existing.map(|i| &queue.jobs[i]);
                    NormalWork {
                        id: legacy
                            .map(|j| crate::review::key(j, &assignment.assignment_id))
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                        item_id: tracked.item_id.clone(),
                        iteration_id: tracked.iteration_id.clone(),
                        iteration: tracked.iteration,
                        agent_id: assignment.agent_id.clone(),
                        enqueue_order: order,
                        pass_ordinal: ordinal,
                        trigger: trigger.clone(),
                        admission: tracked.admission.clone(),
                        legacy_item_id: legacy.map(crate::queue::item_id),
                    }
                };
                let job = QueueJob {
                    work: Some(work),
                    assignment_id: Some(assignment.assignment_id.clone()),
                    provider: "github".into(),
                    account_id: result.connection.identity.id.clone(),
                    account_login: result.connection.identity.login.clone(),
                    configuration_id: configuration.repository_id.clone(),
                    repository_id: ticket.provider_repository_id.clone(),
                    repository_name: ticket.name.clone(),
                    pull_request_id: pull.id.clone(),
                    number: pull.number,
                    title: pull.title.clone(),
                    head_sha: pull.head_sha.clone(),
                    observed_base_sha: Some(pull.base_sha.clone()),
                    trigger_policy: existing
                        .map(|i| queue.jobs[i].trigger_policy.clone())
                        .unwrap_or_else(|| ticket.trigger_policy.clone()),
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
                    detected_at: existing.map(|i| queue.jobs[i].detected_at).unwrap_or(now),
                };
                if let Some(index) = existing {
                    queue.jobs[index] = job;
                } else {
                    queue.jobs.push(job);
                }
            }
            for legacy in queue
                .jobs
                .iter_mut()
                .filter(|j| bound(j) && j.work.is_none())
            {
                legacy.waiting = WAITING_SUPERSEDED.into();
            }
        }
        if queue != previous_queue {
            store
                .save_queue_state(&queue)
                .map_err(MonitoringError::Storage)?;
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
        let mut policy = repository.overrides.effective(&settings.defaults);
        policy.schedule = settings.defaults.schedule.clone();
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
        configured.push(ConfiguredSchedule {
            health_key: repository.id.clone(),
            repository_id: repository.id.clone(),
            name: repository.name.clone(),
            enabled: repository.enabled,
            provider_account_id,
            provider_repository_id,
            provider_supported,
            schedule: settings.defaults.schedule.clone(),
            assignment_id: None,
            agent_id: None,
            agent_name: None,
            policy,
            watched_authors,
            trigger_policy: policy_key,
            assignments: repository
                .assignments
                .iter()
                .map(|a| ScanAssignment {
                    assignment_id: a.id.clone(),
                    agent_id: a.agent_id.clone(),
                })
                .collect(),
        });
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
        ai_attempt: None,
        interruption: None,
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
    operation.fail(error, now);
    health.next_run = operation.next_attempt_at.unwrap_or(0);
    health.schedule_available = operation.next_attempt_at.is_some();
}

impl JobOperation {
    pub fn begin_ai_attempt(&mut self, now: i64) -> Result<(), String> {
        let budget = AttemptBudget {
            attempt_count: self.attempt_count,
            initial_attempt_at: self.initial_attempt_at,
            retry_deadline: self.retry_deadline,
            failure: self.failure.clone(),
        };
        self.begin_attempt(now)?;
        self.ai_attempt = Some(budget);
        self.interruption = None;
        Ok(())
    }

    pub fn requeue_intentional(&mut self, now: i64) {
        if let Some(budget) = self.ai_attempt.take() {
            self.attempt_count = budget.attempt_count;
            self.initial_attempt_at = budget.initial_attempt_at;
            self.retry_deadline = budget.retry_deadline;
            self.failure = budget.failure;
        }
        self.state = OperationState::Queued;
        self.next_attempt_at = Some(now);
    }

    pub fn fail(&mut self, error: &MonitoringError, now: i64) {
        self.ai_attempt = None;
        self.interruption = None;
        let operation = self;
        let failure = retryable_failure(error);
        operation.failure = Some(failure.clone());
        if failure == OperationFailure::Permanent {
            operation.state = OperationState::Failed;
            operation.next_attempt_at = None;
            return;
        }
        if operation.attempt_count > MAX_RETRIES || now >= operation.retry_deadline {
            operation.state = OperationState::ManualRetry;
            operation.next_attempt_at = None;
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
            return;
        }
        operation.state = OperationState::Queued;
        operation.next_attempt_at = Some(next_attempt);
    }

    pub fn begin_attempt(&mut self, now: i64) -> Result<(), String> {
        // Waiting for the first execution is not part of an AI retry window.
        if self.attempt_count == 0
            && self.failure.is_none()
            && matches!(
                self.operation_type.as_str(),
                "copilot_review" | "primary_final_review" | "thread_analysis" | "mention_analysis"
            )
            && matches!(
                self.state,
                OperationState::Queued | OperationState::Interrupted
            )
        {
            self.initial_attempt_at = now;
            self.retry_deadline = now + RETRY_WINDOW_SECONDS;
        }
        if now >= self.retry_deadline || self.attempt_count > MAX_RETRIES {
            self.state = OperationState::ManualRetry;
            self.next_attempt_at = None;
            return Err("Review retry budget exhausted. Manual retry is required.".into());
        }
        if !matches!(
            self.state,
            OperationState::Queued | OperationState::Interrupted
        ) || self.next_attempt_at.is_none_or(|next| now < next)
        {
            return Err("This review is not ready to run.".into());
        }
        self.attempt_count += 1;
        self.state = OperationState::Running;
        self.next_attempt_at = None;
        Ok(())
    }

    pub fn review(job: &QueueJob, now: i64) -> Self {
        Self {
            ai_attempt: None,
            interruption: None,
            id: uuid::Uuid::new_v4().to_string(),
            provider: job.provider.clone(),
            account_id: job.account_id.clone(),
            configuration_id: job.configuration_id.clone(),
            repository_id: job.repository_id.clone(),
            pull_request_id: Some(job.pull_request_id.clone()),
            head_sha: Some(job.head_sha.clone()),
            trigger_policy: job.trigger_policy.clone(),
            operation_type: "copilot_review".into(),
            state: OperationState::Queued,
            attempt_count: 0,
            initial_attempt_at: now,
            retry_deadline: now + RETRY_WINDOW_SECONDS,
            next_attempt_at: Some(now),
            failure: None,
            attempted_mutation: None,
            pending_review_id: None,
            owned_thread_id: None,
            triggering_external_comment_id: None,
            confirmed_receipt: None,
        }
    }
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
                | "invalid_global_cron"
                | "configuration_changed"
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

pub(crate) fn check_error(error: ConnectionError) -> MonitoringError {
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

pub fn review_policy(
    settings: &Settings,
    job: &QueueJob,
    pull: Option<&PullRequest>,
) -> Result<Policy, String> {
    let configuration = configured_schedules(settings)
        .into_iter()
        .find(|c| c.repository_id == job.configuration_id)
        .ok_or("Repository was removed.")?;
    if !configuration.enabled
        || !configuration.provider_supported
        || configuration.name != job.repository_name
        || configuration.provider_account_id.as_deref() != Some(&job.account_id)
        || configuration.provider_repository_id.as_deref() != Some(&job.repository_id)
        || (job.work.is_none()
            && configuration.trigger_policy.as_deref() != Some(&job.trigger_policy))
        || !actionable(job)
    {
        return Err("Review eligibility or repository configuration changed.".into());
    }
    if let Some(work) = &job.work {
        if !configuration.assignments.iter().any(|a| {
            Some(a.assignment_id.as_str()) == job.assignment_id.as_deref()
                && a.agent_id == work.agent_id
        }) {
            return Err(
                "This Agent assignment was removed or replaced; wait for the next global scan."
                    .into(),
            );
        }
    }
    if let Some(pull) = pull {
        if pull.id != job.pull_request_id
            || pull.number != job.number
            || pull.head_sha != job.head_sha
            || pull.base_repository_id != job.repository_id
            || pull.state != Lifecycle::Open
            || pull.draft
            || (job.work.is_none()
                && !eligibility(
                    &configuration.watched_authors,
                    &configuration.policy,
                    &job.account_id,
                    pull,
                )
                .eligible())
        {
            return Err(
                "Pull request revision or eligibility changed; wait for the next poll.".into(),
            );
        }
    }
    Ok(configuration.policy)
}

pub fn new_revision_eligible(settings: &Settings, job: &QueueJob, pull: &PullRequest) -> bool {
    let Some(configuration) = configured_schedules(settings)
        .into_iter()
        .find(|c| c.repository_id == job.configuration_id)
    else {
        return false;
    };
    let Some(trigger) = configuration.trigger_policy else {
        return false;
    };
    let mut current = job.clone();
    current.head_sha = pull.head_sha.clone();
    current.trigger_policy = trigger;
    current.waiting = WAITING_HUMAN_START.into();
    review_policy(settings, &current, Some(pull)).is_ok()
}

pub fn currently_watched(settings: &Settings, job: &QueueJob, author_id: Option<&str>) -> bool {
    configured_schedules(settings)
        .iter()
        .find(|c| c.repository_id == job.configuration_id)
        .is_some_and(|c| author_id.is_some_and(|id| c.watched_authors.iter().any(|a| a.id == id)))
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
