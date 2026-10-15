pub(crate) mod diagnostics;
pub(crate) mod host;
pub(crate) mod packet_run;
pub(crate) mod runtime;
pub(crate) mod teammates;
pub(crate) mod tools;

use crate::{
    github::ConnectionError,
    monitoring::{self, JobOperation, MonitoringError, OperationFailure, OperationState, QueueJob},
    policy::Policy,
    storage::{Agent, AssignmentAuthority, Doctrine, Repository, Settings, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub agent: Agent,
    pub policy: Policy,
    pub doctrine: Option<String>,
    pub preset: Option<String>,
    /// Missing on legacy executions; never filled from current settings on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ExecutionConfiguration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfiguration {
    pub repository: Repository,
    pub authority: AssignmentAuthority,
    /// In Agent-selected order, with the exact saved title and full body.
    pub doctrines: Vec<Doctrine>,
    /// Absent on older jobs; never inferred from a later library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doctrine_catalog: Option<crate::storage::DoctrineCatalog>,
}

impl Selection {
    /// Compare execution inputs and effective authority, not sibling assignment archives.
    /// Callers must still revalidate the job, account and permission gates.
    pub fn same_execution(&self, other: &Self) -> bool {
        // Ignore retired start preferences without rewriting captured evidence.
        let mut policy = self.policy.clone();
        policy.automatic_agent_start = other.policy.automatic_agent_start;
        policy.automatic_comment_publication = other.policy.automatic_comment_publication;
        self.agent == other.agent
            && policy == other.policy
            && self.doctrine == other.doctrine
            && self.preset == other.preset
            && match (&self.configuration, &other.configuration) {
                (Some(left), Some(right)) => {
                    let a = &left.repository;
                    let b = &right.repository;
                    let mut overrides = a.overrides.clone();
                    overrides.automatic_agent_start = b.overrides.automatic_agent_start;
                    overrides.automatic_comment_publication =
                        b.overrides.automatic_comment_publication;
                    // Role/capability evidence is archival, not a read-only
                    // execution grant. Provider stages revalidate current authority.
                    left.doctrines == right.doctrines
                        && a.id == b.id
                        && a.provider == b.provider
                        && a.name == b.name
                        && a.enabled == b.enabled
                        && a.provider_account_id == b.provider_account_id
                        && a.legacy_installation_id == b.legacy_installation_id
                        && a.provider_repository_id == b.provider_repository_id
                        && overrides == b.overrides
                        && a.review_preset == b.review_preset
                        && a.watched_authors == b.watched_authors
                }
                (None, None) => true,
                _ => false,
            }
    }

    pub fn resolve(
        settings: &Settings,
        job: &QueueJob,
        assignment_id: &str,
    ) -> Result<Self, String> {
        if job.assignment_id.as_deref() != Some(assignment_id) {
            return Err(
                "Waiting for this assignment's next poll; legacy detections do not start reviews."
                    .into(),
            );
        }
        let policy = monitoring::review_policy(settings, job, None)?;
        let repository = settings
            .repositories
            .iter()
            .find(|r| r.id == job.configuration_id)
            .ok_or("Repository was removed.")?;
        let assignment = repository
            .assignments
            .iter()
            .find(|a| a.id == assignment_id)
            .ok_or("Configure an Agent assignment for this repository.")?;
        let agent = settings
            .agents
            .iter()
            .find(|a| a.id == assignment.agent_id)
            .ok_or("The assigned Agent is unavailable.")?
            .clone();
        if agent.model.is_empty()
            || agent
                .ai_account
                .as_ref()
                .is_none_or(|a| a.provider != "copilot")
        {
            return Err(
                "Choose a Copilot account and account-returned model for this Agent.".into(),
            );
        }
        let doctrines = agent
            .doctrine_titles()
            .into_iter()
            .map(|title| {
                settings
                    .doctrines
                    .iter()
                    .find(|d| d.title.trim().to_lowercase() == title.trim().to_lowercase())
                    .cloned()
                    .ok_or("The Agent's doctrine is unavailable.")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let doctrine = match doctrines.as_slice() {
            [] => None,
            [single] => Some(single.body.clone()),
            many => Some(
                many.iter()
                    .map(|d| format!("## {}\n\n{}", d.title, d.body))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            ),
        };
        let preset = repository
            .review_preset
            .as_ref()
            .or(settings.default_review_preset.as_ref())
            .map(|id| {
                settings
                    .presets
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| p.body.clone())
                    .ok_or("The review preset is unavailable.")
            })
            .transpose()?;
        Ok(Self {
            agent,
            policy,
            doctrine,
            preset,
            configuration: Some(ExecutionConfiguration {
                repository: repository.clone(),
                authority: repository.assignment_authority(assignment),
                doctrines,
                doctrine_catalog: Some(settings.doctrine_catalog()),
            }),
        })
    }
}

pub fn key(job: &QueueJob, assignment_id: &str) -> String {
    if let Some(work) = &job.work {
        if job.assignment_id.as_deref() == Some(assignment_id) {
            return work.id.clone();
        }
    }
    // Tuple encoding is collision-free even when user configuration IDs contain delimiters.
    serde_json::json!([
        job.provider,
        job.account_id,
        job.configuration_id,
        job.repository_id,
        job.pull_request_id,
        job.head_sha,
        job.trigger_policy,
        assignment_id
    ])
    .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRun {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_context: Option<Vec<crate::feedback::Context>>,
    pub key: String,
    pub assignment_id: String,
    pub job: QueueJob,
    pub selection: Selection,
    pub operation: JobOperation,
    pub manual_start: bool,
    /// Legacy evidence only; assignment and scope activation now authorize review.
    #[serde(default)]
    pub trust_confirmed: bool,
    pub phase: String,
    pub error: Option<String>,
    pub result: Option<ReviewResult>,
    #[serde(default)]
    pub mode: RunMode,
    /// Content-free record of the packet this run was given (Packet mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet: Option<crate::packet::PacketRecord>,
}

impl ReviewRun {
    pub fn matches_job(&self, job: &QueueJob) -> bool {
        job.assignment_id.as_deref() == Some(&self.assignment_id)
            && key(job, &self.assignment_id) == self.key
            && job.account_id == self.job.account_id
            && (job.configuration_id == self.job.configuration_id
                || self.job.configuration_id.is_empty())
            && job.repository_id == self.job.repository_id
            && job.pull_request_id == self.job.pull_request_id
            && job.head_sha == self.job.head_sha
    }
}

pub fn validate_execution_selection(store: &Store, run: &ReviewRun) -> Result<(), Failure> {
    let settings = store.load_settings().map_err(Failure::permanent)?;
    let jobs = store.load_queue().map_err(Failure::permanent)?;
    let job = jobs
        .iter()
        .find(|j| run.matches_job(j))
        .ok_or_else(|| Failure::permanent("Review detection is no longer available."))?;
    let current = Selection::resolve(&settings, job, &run.assignment_id).map_err(|error| {
        match monitoring::check_review_policy(&settings, job, None) {
            Err(rejection) if rejection.superseded() => Failure::policy(rejection),
            _ => Failure::permanent(error),
        }
    })?;
    if !current.same_execution(&run.selection) {
        return Err(Failure::permanent(
            "Agent configuration changed; explicitly retry.",
        ));
    }
    if let Some(context) = &run.feedback_context {
        crate::feedback::validate_context(store, job, &run.selection.agent.id, context)?;
    }
    Ok(())
}

pub fn restore(store: &Store) -> Result<(), String> {
    let now = crate::now_seconds()?;
    let jobs = store.load_queue()?;
    let mut reviews = store.load_reviews()?;
    let mut changed = false;
    for review in &mut reviews {
        if retire_legacy(review) {
            changed = true;
            continue;
        }
        if recover_parked_supersession(review, &jobs, now) {
            changed = true;
            continue;
        }
        if review.operation.state == OperationState::Running {
            if review.operation.interruption.is_some() {
                review.operation.requeue_intentional(0);
                review.result = None;
                review.phase = "Waiting after intentional interruption".into();
                changed = true;
                continue;
            }
            review.operation.state = OperationState::Interrupted;
            review.operation.next_attempt_at = Some(0);
            review.phase = "Interrupted; revalidating before retry".into();
            changed = true;
        }
    }
    changed |= end_retired_runs(&mut reviews, &jobs);
    if changed {
        store.save_reviews(&reviews)?;
    }
    Ok(())
}

/// Ends waiting runs whose queue entry a scan retired (merged, closed,
/// superseded, out of scope, assignment removed): they can never run, so they
/// leave Running quietly instead of waiting forever.
pub fn end_retired(store: &Store) -> Result<(), String> {
    let jobs = store.load_queue()?;
    let mut reviews = store.load_reviews()?;
    if end_retired_runs(&mut reviews, &jobs) {
        store.save_reviews(&reviews)?;
    }
    Ok(())
}

fn retired_job(run: &ReviewRun, jobs: &[QueueJob]) -> bool {
    jobs.iter()
        .find(|job| run.matches_job(job))
        .is_some_and(monitoring::retired)
}

fn end_retired_runs(reviews: &mut [ReviewRun], jobs: &[QueueJob]) -> bool {
    let mut changed = false;
    for run in reviews {
        if matches!(
            run.operation.state,
            OperationState::Queued | OperationState::Interrupted
        ) && retired_job(run, jobs)
        {
            end_quietly(run);
            changed = true;
        }
    }
    changed
}

fn end_quietly(run: &mut ReviewRun) {
    run.operation.end_superseded();
    run.result = None;
    run.error = None;
    run.phase = "Superseded".into();
}

/// Builds before typed supersession parked a stale Iteration or assignment as a
/// permanent failure. No typed cause was stored, so the exact retired texts
/// identify those records; requeue them with a fresh budget so the queue
/// retargets them like any superseded attempt, or ends them when retired.
fn recover_parked_supersession(run: &mut ReviewRun, jobs: &[QueueJob], now: i64) -> bool {
    const PARKED: [monitoring::PolicyRejection; 2] = [
        monitoring::PolicyRejection::RevisionChanged,
        monitoring::PolicyRejection::AssignmentChanged,
    ];
    if run.operation.state != OperationState::Failed
        || run.operation.failure != Some(OperationFailure::Permanent)
        || !PARKED
            .iter()
            .any(|rejection| run.error.as_deref() == Some(rejection.message()))
    {
        return false;
    }
    // Only live work earns a new attempt; work a scan retired just ends.
    if !jobs
        .iter()
        .find(|job| run.matches_job(job))
        .is_some_and(monitoring::actionable)
    {
        end_quietly(run);
        return true;
    }
    run.operation.requeue_fresh(now);
    run.result = None;
    run.error = None;
    run.phase = "Waiting for current PR state".into();
    true
}

/// The legacy review path is removed: unfinished legacy work is cancelled, never run.
pub(crate) const LEGACY_RETIRED: &str = "Legacy review retired.";

/// Cancel an unfinished legacy run in place. True when changed.
pub(crate) fn retire_legacy(run: &mut ReviewRun) -> bool {
    if run.mode != RunMode::Legacy
        || !matches!(
            run.operation.state,
            OperationState::Queued | OperationState::Running | OperationState::Interrupted
        )
    {
        return false;
    }
    run.operation.state = OperationState::Failed;
    run.operation.failure = Some(OperationFailure::Permanent);
    run.operation.next_attempt_at = None;
    run.operation.ai_attempt = None;
    run.operation.interruption = None;
    run.operation.resume_at = None;
    run.phase = "Cancelled".into();
    run.error = Some(LEGACY_RETIRED.into());
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileGuide {
    pub path: String,
    pub explanation: String,
    pub order: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    MachineSignOff,
    #[default]
    HumanInputRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_id: Option<String>,
    pub path: String,
    pub side: String,
    pub line: u64,
    pub severity: Severity,
    pub title: String,
    pub explanation: String,
    pub confidence: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewOutput {
    #[serde(default)]
    pub feedback_conflict: bool,
    #[serde(default)]
    pub held_findings: Vec<Finding>,
    #[serde(default)]
    pub feedback_assessments: Vec<crate::feedback::Assessment>,
    // Legacy fields: read from old records only; packet runs leave them empty.
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub files: Vec<FileGuide>,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub decision: Decision,
    /// The one exit a Packet-mode run ended on. Host-owned, never model output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<crate::exits::ExitOutcome>,
    /// A teammate's stored report (`submit_report`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<crate::exits::TeammateReport>,
}

impl ReviewOutput {
    /// Output of a Packet-mode run: legacy fields stay empty, the exit decides.
    pub fn from_exit(
        outcome: crate::exits::ExitOutcome,
        report: Option<crate::exits::TeammateReport>,
    ) -> Self {
        Self {
            feedback_conflict: false,
            held_findings: vec![],
            feedback_assessments: vec![],
            synopsis: String::new(),
            files: vec![],
            findings: vec![],
            // Never MachineSignOff: legacy clearance and action paths must not read an exit run.
            decision: Decision::HumanInputRequired,
            exit: Some(outcome),
            report,
        }
    }
}

/// How a run reviews. Every new run is `Packet`. `Legacy` (the removed JSON
/// decision) is the default only so old records load; it is never executed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    #[default]
    Legacy,
    Packet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewResult<O = ReviewOutput> {
    #[serde(default)]
    pub reviewed_base_sha: Option<String>,
    pub output: O,
    pub session_id: String,
    pub model: String,
    pub runtime_version: String,
    /// Read back from this actual runtime session, never reconstructed on reload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intelligence: Option<crate::storage::AgentIntelligence>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u64,
    /// Legacy record: true when the host served every byte of every changed file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub byte_coverage: bool,
    /// Informational count of changed files read in full; never a gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<ReadCoverage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadCoverage {
    pub read: u32,
    pub changed: u32,
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub cancelled: bool,
    pub kind: OperationFailure,
    pub message: String,
    pub retry_after_seconds: Option<i64>,
    /// Provider that signalled a rate limit; meaningful only for `RateLimited`.
    pub origin: crate::backpressure::Origin,
    /// When a `Superseded` attempt may run again; `None` retries shortly (a local
    /// race such as a teammate report landing). Boxed to keep `Failure` small.
    pub resume: Option<Box<Resume>>,
}

/// When a superseded attempt may run again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resume {
    /// After a poll observes the PR: only the provider state can change the outcome.
    NextPoll,
    /// After a poll observes a base other than the one an AI attempt reviewed.
    NewBase(String),
}

impl Failure {
    /// A rate limit is backpressure, never a failure.
    pub fn rate_limited(&self) -> bool {
        !self.cancelled && self.kind == OperationFailure::RateLimited
    }

    pub fn cancelled() -> Self {
        Self {
            cancelled: true,
            ..Self::permanent("AI work cancelled intentionally.")
        }
    }
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            cancelled: false,
            kind: OperationFailure::Permanent,
            message: message.into(),
            retry_after_seconds: None,
            origin: crate::backpressure::Origin::Github,
            resume: None,
        }
    }
    /// Agent-quality rejections are transient: a fresh attempt may succeed.
    pub fn provider(message: impl Into<String>) -> Self {
        Self {
            kind: OperationFailure::Provider,
            ..Self::permanent(message)
        }
    }
    pub fn timeout() -> Self {
        Self {
            cancelled: false,
            kind: OperationFailure::Timeout,
            message: "Copilot review timed out.".into(),
            retry_after_seconds: None,
            origin: crate::backpressure::Origin::Copilot,
            resume: None,
        }
    }
    pub fn schema() -> Self {
        Self::permanent("Copilot returned invalid or incomplete review output.")
    }
    /// The agent chose `panic_exit`. Typed, parks at once, resumes on cooldown.
    pub fn panic(reason: &str) -> Self {
        Self {
            kind: OperationFailure::Panic,
            origin: crate::backpressure::Origin::Copilot,
            ..Self::permanent(format!("Agent panic exit: {reason}"))
        }
    }
    /// Repeated malformed or over-limit exit calls. Permanent, human needed.
    pub fn contract(message: impl Into<String>) -> Self {
        Self::permanent(message)
    }
    /// The PR state moved under the run: tossed, not a failure of the agent.
    pub fn superseded(message: &str) -> Self {
        Self {
            kind: OperationFailure::Superseded,
            ..Self::permanent(message)
        }
    }
    /// A stale Iteration or assignment is superseded; other rejections need a human.
    pub fn policy(rejection: monitoring::PolicyRejection) -> Self {
        if rejection.superseded() {
            Self::superseded(rejection.message()).after(Resume::NextPoll)
        } else {
            Self::permanent(rejection.message())
        }
    }
    pub fn after(self, resume: Resume) -> Self {
        Self {
            resume: Some(Box::new(resume)),
            ..self
        }
    }
    /// A result or write discarded because the PR state moved: retried through the budget.
    pub fn tossed(message: &str) -> Self {
        Self {
            kind: OperationFailure::Tossed,
            ..Self::permanent(message)
        }
    }
    pub fn operation(message: String) -> Self {
        if message == "Copilot operation cancelled." {
            return Self::cancelled();
        }
        if message == "Copilot operation timed out. Retry." {
            Self::timeout()
        } else {
            let kind = match message.as_str() {
                "Cannot reach GitHub to verify this Copilot identity. Retry." => {
                    OperationFailure::Network
                }
                "GitHub could not verify this Copilot identity. Retry or reconnect." => {
                    OperationFailure::Provider
                }
                message if crate::copilot::credential_error(message) => {
                    OperationFailure::Credentials
                }
                _ => OperationFailure::Permanent,
            };
            Self {
                cancelled: false,
                kind,
                message,
                retry_after_seconds: None,
                origin: crate::backpressure::Origin::Copilot,
                resume: Default::default(),
            }
        }
    }
    pub fn sdk(error: github_copilot_sdk::Error) -> Self {
        use github_copilot_sdk::{ErrorKind, ProtocolErrorKind, SessionErrorKind};
        let kind = match error.kind() {
            ErrorKind::Io => OperationFailure::Network,
            ErrorKind::Protocol(ProtocolErrorKind::CliStartupTimeout)
            | ErrorKind::Session(SessionErrorKind::Timeout(_)) => OperationFailure::Timeout,
            ErrorKind::GitHubTokenProvider => OperationFailure::Credentials,
            ErrorKind::Protocol(_) | ErrorKind::Rpc { .. } | ErrorKind::Session(_) => {
                OperationFailure::Provider
            }
            _ => OperationFailure::Permanent,
        };
        Self { cancelled: false, kind, message: "Copilot runtime request failed; check account, model, and runtime availability before retrying.".into(), retry_after_seconds: None, origin: crate::backpressure::Origin::Copilot, resume: None }
    }
    fn event(data: &Value) -> Self {
        let kind = match data["statusCode"].as_u64() {
            Some(408) => OperationFailure::Timeout,
            Some(429) => OperationFailure::RateLimited,
            Some(401) => OperationFailure::Credentials,
            Some(403) => OperationFailure::Permission,
            Some(500..=599) => OperationFailure::Provider,
            Some(_) => OperationFailure::Provider,
            None => match data["errorType"].as_str() {
                Some("timeout") => OperationFailure::Timeout,
                Some("network") => OperationFailure::Network,
                _ => OperationFailure::Provider,
            },
        };
        let retry_after_seconds = data["retryAfterSeconds"].as_i64().filter(|n| *n >= 0);
        Self {
            cancelled: false,
            kind,
            message: "Copilot reported an execution failure; no review result was accepted.".into(),
            retry_after_seconds,
            origin: crate::backpressure::Origin::Copilot,
            resume: None,
        }
    }
    pub fn monitoring(&self) -> MonitoringError {
        if self.rate_limited() {
            return MonitoringError::Backpressure {
                origin: self.origin,
                message: self.message.clone(),
                retry_after_seconds: self.retry_after_seconds,
            };
        }
        MonitoringError::Recoverable {
            code: "review_failed".into(),
            message: self.message.clone(),
            failure: self.kind.clone(),
            retry_after_seconds: self.retry_after_seconds,
        }
    }
}

impl From<ConnectionError> for Failure {
    fn from(error: ConnectionError) -> Self {
        match monitoring::check_error(error) {
            MonitoringError::Recoverable {
                message,
                failure,
                retry_after_seconds,
                ..
            } => Self {
                cancelled: false,
                kind: failure,
                message,
                retry_after_seconds,
                origin: crate::backpressure::Origin::Github,
                resume: None,
            },
            MonitoringError::Backpressure {
                origin,
                message,
                retry_after_seconds,
            } => Self {
                cancelled: false,
                kind: OperationFailure::RateLimited,
                message,
                retry_after_seconds,
                origin,
                resume: None,
            },
            MonitoringError::Storage(message) => Self::permanent(message),
        }
    }
}

/// The SDK frames JSON events over RPC; the same normalized stream can be replayed from JSONL.
#[derive(Default)]
pub struct Events {
    idle: bool,
    usage_seen: bool,
    input_tokens: u64,
    output_tokens: u64,
    tool_calls: u64,
}

impl Events {
    pub fn push(&mut self, kind: &str, data: &Value) -> Result<(), Failure> {
        match kind {
            "assistant.usage" => {
                let count = |name: &str| -> Result<u64, Failure> {
                    let n = data[name].as_f64().ok_or_else(|| {
                        Failure::permanent("Copilot omitted required token usage metadata.")
                    })?;
                    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n >= u64::MAX as f64 {
                        return Err(Failure::schema());
                    }
                    Ok(n as u64)
                };
                self.input_tokens = self
                    .input_tokens
                    .checked_add(count("inputTokens")?)
                    .ok_or_else(Failure::schema)?;
                self.output_tokens = self
                    .output_tokens
                    .checked_add(count("outputTokens")?)
                    .ok_or_else(Failure::schema)?;
                self.usage_seen = true;
            }
            "tool.execution_start" => self.tool_calls += 1,
            "session.idle" => self.idle = true,
            "session.error" => return Err(Failure::event(data)),
            "session.abort" => return Err(Failure::permanent("Copilot review was cancelled.")),
            _ => {}
        }
        Ok(())
    }

    /// Packet-mode completion: the host-held exit slot decides; any final message is ignored.
    /// Idle without an exit is agent-quality (retryable), never a schema failure.
    pub fn finish_exit<O>(
        self,
        exit: Option<crate::exits::ExitCall>,
        session_id: String,
        model: String,
        runtime_version: String,
        build: impl FnOnce(crate::exits::ExitCall) -> Result<O, Failure>,
    ) -> Result<ReviewResult<O>, Failure> {
        if !self.idle || !self.usage_seen {
            return Err(Failure::permanent(
                "Copilot ended without completion and usage evidence.",
            ));
        }
        let exit = exit.ok_or_else(|| {
            Failure::provider("Agent ended without calling an exit. No review result was accepted.")
        })?;
        Ok(ReviewResult {
            reviewed_base_sha: None,
            output: build(exit)?,
            session_id,
            model,
            runtime_version,
            intelligence: None,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            tool_calls: self.tool_calls,
            byte_coverage: false,
            coverage: None,
        })
    }
}
