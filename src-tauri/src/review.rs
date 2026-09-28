pub(crate) mod host;
pub(crate) mod runtime;

use crate::{
    github::{metadata::PullRequest, ConnectionError},
    monitoring::{self, JobOperation, MonitoringError, OperationFailure, OperationState, QueueJob},
    policy::Policy,
    storage::{Agent, Settings, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) use host::Coordinator;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub agent: Agent,
    pub policy: Policy,
    pub doctrine: Option<String>,
    pub preset: Option<String>,
}

impl Selection {
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
        let doctrine = agent
            .doctrine
            .as_ref()
            .map(|title| {
                settings
                    .doctrines
                    .iter()
                    .find(|d| d.title.trim().to_lowercase() == title.trim().to_lowercase())
                    .map(|d| d.body.clone())
                    .ok_or("The Agent's doctrine is unavailable.")
            })
            .transpose()?;
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
        })
    }
}

pub fn key(job: &QueueJob, assignment_id: &str) -> String {
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
    pub key: String,
    pub assignment_id: String,
    pub job: QueueJob,
    pub selection: Selection,
    pub operation: JobOperation,
    pub manual_start: bool,
    pub trust_confirmed: bool,
    pub phase: String,
    pub error: Option<String>,
    pub result: Option<ReviewResult>,
}

pub fn restore(store: &Store) -> Result<(), String> {
    let mut reviews = store.load_reviews()?;
    let mut changed = false;
    for review in &mut reviews {
        if review.operation.state == OperationState::Running {
            review.operation.state = OperationState::Interrupted;
            review.operation.next_attempt_at = Some(0);
            review.phase = "Interrupted; revalidating before retry".into();
            changed = true;
        }
    }
    if changed {
        store.save_reviews(&reviews)?;
    }
    Ok(())
}

pub fn requires_trust(job: &QueueJob, pull: &PullRequest) -> bool {
    !job.watched_author || pull.head_repository_id.as_deref() != Some(&job.repository_id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileGuide {
    pub path: String,
    pub explanation: String,
    pub order: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    MachineSignOff,
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
    pub synopsis: String,
    pub files: Vec<FileGuide>,
    pub findings: Vec<Finding>,
    pub decision: Decision,
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
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u64,
}

pub(crate) fn json_body(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix("```json\n")
        .or_else(|| text.strip_prefix("```\n"))
        .and_then(|body| body.strip_suffix("\n```"))
        .unwrap_or(text)
}

pub fn validate_output(text: &str, paths: &[String]) -> Result<ReviewOutput, Failure> {
    let mut output: ReviewOutput = serde_json::from_str(json_body(text)).map_err(|_| {
        Failure::permanent(
            "Copilot's final response does not match the required review JSON schema.",
        )
    })?;
    let synopsis = output.synopsis.trim();
    let sentences = synopsis
        .char_indices()
        .filter(|(i, c)| {
            matches!(c, '.' | '!' | '?')
                && synopsis[i + c.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        })
        .count();
    if synopsis.is_empty()
        || synopsis.contains(['\n', '\r'])
        || sentences != 1
        || !synopsis.ends_with(['.', '!', '?'])
    {
        return Err(Failure::permanent(
            "Review synopsis must be exactly one sentence.",
        ));
    }
    let expected: BTreeSet<_> = paths.iter().collect();
    let actual: BTreeSet<_> = output.files.iter().map(|f| &f.path).collect();
    if expected.len() != paths.len()
        || actual.len() != output.files.len()
        || actual != expected
        || output.files.iter().any(|f| f.explanation.trim().is_empty())
    {
        return Err(Failure::permanent(
            "Review output does not cover every changed file exactly once.",
        ));
    }
    let order: BTreeSet<_> = output.files.iter().filter_map(|f| f.order).collect();
    if order == (1..=paths.len()).collect() {
        output.files.sort_by_key(|f| f.order);
    } else {
        output
            .files
            .sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    }
    for (index, file) in output.files.iter_mut().enumerate() {
        file.order = Some(index + 1);
    }
    if output.findings.iter().any(|f| {
        !expected.contains(&f.path)
            || !matches!(f.side.as_str(), "base" | "head")
            || f.line == 0
            || f.confidence > 100
            || f.title.trim().is_empty()
            || f.explanation.trim().is_empty()
    }) || (output.decision == Decision::MachineSignOff && !output.findings.is_empty())
    {
        return Err(Failure::permanent("Review findings contain invalid locations, severity, confidence, or an inconsistent machine decision."));
    }
    Ok(output)
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub kind: OperationFailure,
    pub message: String,
    pub retry_after_seconds: Option<i64>,
}

impl Failure {
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: OperationFailure::Permanent,
            message: message.into(),
            retry_after_seconds: None,
        }
    }
    pub fn timeout() -> Self {
        Self {
            kind: OperationFailure::Timeout,
            message: "Copilot review timed out.".into(),
            retry_after_seconds: None,
        }
    }
    pub fn schema() -> Self {
        Self::permanent("Copilot returned invalid or incomplete review output.")
    }
    pub fn operation(message: String) -> Self {
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
                _ => OperationFailure::Permanent,
            };
            Self {
                kind,
                message,
                retry_after_seconds: None,
            }
        }
    }
    pub fn sdk(error: github_copilot_sdk::Error) -> Self {
        use github_copilot_sdk::{ErrorKind, ProtocolErrorKind, SessionErrorKind};
        let kind = match error.kind() {
            ErrorKind::Io => OperationFailure::Network,
            ErrorKind::Protocol(ProtocolErrorKind::CliStartupTimeout)
            | ErrorKind::Session(SessionErrorKind::Timeout(_)) => OperationFailure::Timeout,
            _ => OperationFailure::Permanent,
        };
        Self { kind, message: "Copilot runtime request failed; check account, model, and runtime availability before retrying.".into(), retry_after_seconds: None }
    }
    fn event(data: &Value) -> Self {
        let kind = match data["statusCode"].as_u64() {
            Some(408) => OperationFailure::Timeout,
            Some(429) => OperationFailure::RateLimited,
            Some(500..=599) => OperationFailure::Provider,
            _ => match data["errorType"].as_str() {
                Some("timeout") => OperationFailure::Timeout,
                Some("network") => OperationFailure::Network,
                _ => OperationFailure::Permanent,
            },
        };
        let retry_after_seconds = data["retryAfterSeconds"].as_i64().filter(|n| *n >= 0);
        Self {
            kind,
            message: "Copilot reported an execution failure; no review result was accepted.".into(),
            retry_after_seconds,
        }
    }
    pub fn monitoring(&self) -> MonitoringError {
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
                kind: failure,
                message,
                retry_after_seconds,
            },
            MonitoringError::Storage(message) => Self::permanent(message),
        }
    }
}

/// The SDK frames JSON events over RPC; the same normalized stream can be replayed from JSONL.
#[derive(Default)]
pub struct Events {
    message: Option<String>,
    idle: bool,
    usage_seen: bool,
    input_tokens: u64,
    output_tokens: u64,
    tool_calls: u64,
}

impl Events {
    pub fn push(&mut self, kind: &str, data: &Value) -> Result<(), Failure> {
        match kind {
            "assistant.message" => {
                if data
                    .get("toolRequests")
                    .is_none_or(|v| v.is_null() || v.as_array().is_some_and(Vec::is_empty))
                {
                    self.message = Some(
                        data["content"]
                            .as_str()
                            .ok_or_else(|| {
                                Failure::permanent("Copilot emitted a malformed assistant message.")
                            })?
                            .into(),
                    );
                }
            }
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

    pub fn finish(
        self,
        paths: &[String],
        session_id: String,
        model: String,
        runtime_version: String,
    ) -> Result<ReviewResult, Failure> {
        self.finish_with(session_id, model, runtime_version, |text| {
            validate_output(text, paths)
        })
    }

    pub fn finish_with<O>(
        self,
        session_id: String,
        model: String,
        runtime_version: String,
        validate: impl FnOnce(&str) -> Result<O, Failure>,
    ) -> Result<ReviewResult<O>, Failure> {
        if !self.idle || !self.usage_seen {
            return Err(Failure::permanent(
                "Copilot ended without completion and usage evidence.",
            ));
        }
        Ok(ReviewResult {
            reviewed_base_sha: None,
            output: validate(self.message.as_deref().ok_or_else(Failure::schema)?)?,
            session_id,
            model,
            runtime_version,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            tool_calls: self.tool_calls,
        })
    }
}
