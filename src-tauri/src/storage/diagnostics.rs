//! Content-free attempt evidence. Producers supply typed decisions, never error text.
use super::{Diagnostic, DiagnosticEvent, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::ErrorKind, io::Write, sync::Mutex};

const LOG_BYTES: usize = 1024 * 1024;
const RECORD_BYTES: usize = 128 * 1024;
const PATH_LIMIT: usize = 128;
const TEXT_LIMIT: usize = 256;
const FILES: [&str; 4] = [
    "attempts.jsonl",
    "attempts.1.jsonl",
    "attempts.2.jsonl",
    "attempts.3.jsonl",
];
static JOURNAL: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub operation_id: String,
    pub work_id: String,
    pub number: u64,
    pub head: String,
    pub model: String,
    pub started_at_ms: Option<u64>,
    pub session_id: Option<String>,
    pub runtime_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    pub count: usize,
    pub paths: Vec<String>,
    pub omitted: usize,
}

impl Paths {
    pub fn new(paths: impl IntoIterator<Item = String>) -> Self {
        let mut result = Self {
            count: 0,
            paths: Vec::new(),
            omitted: 0,
        };
        for path in paths {
            result.count += 1;
            if result.paths.len() < PATH_LIMIT {
                result.paths.push(safe_text(&path));
            }
        }
        result.omitted = result.count - result.paths.len();
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub changed: Paths,
    pub covered: Paths,
    pub missing: Paths,
    pub responsible_tool: ToolName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolName {
    ReadChanges,
    ReadSource,
    SearchPaths,
    GetThread,
    GetChecks,
    ReadCheckLog,
    ListTree,
    SearchCode,
    RubberDuck,
    Exit,
    NotPermitted,
}

impl ToolName {
    pub fn from_name(name: &str) -> Self {
        match name {
            "read_changes" => Self::ReadChanges,
            "read_source" => Self::ReadSource,
            "search_paths" => Self::SearchPaths,
            "get_thread" => Self::GetThread,
            "get_checks" => Self::GetChecks,
            "read_check_log" => Self::ReadCheckLog,
            "list_tree" => Self::ListTree,
            "search_code" => Self::SearchCode,
            "rubber_duck" => Self::RubberDuck,
            "leave_comments"
            | "ready_for_human"
            | "wait"
            | "approve_pull_request"
            | "approve_and_merge"
            | "panic_exit"
            | "submit_report"
            | "submit_verdict" => Self::Exit,
            _ => Self::NotPermitted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// Legacy: coverage is informational now; only old records carry this.
    IncompleteCoverage,
    ToolError,
    ResponseLimit,
    InvalidOutput,
    RuntimeExecution,
    EventStreamLost,
    Timeout,
    TotalDurationTimeout,
    InactivityTimeout,
    Cancelled,
    Network,
    RateLimited,
    Provider,
    Credentials,
    Permission,
    Permanent,
    EligibilityOrConfiguration,
    Superseded,
    Panic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failed,
    Unavailable,
    NotRequired,
    ForcedStopUnverified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEvent {
    Message,
    Usage,
    Idle,
    Abort,
    Error,
    ToolStart,
    ToolComplete,
    TurnEnd,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeError {
    Authentication,
    Authorization,
    Quota,
    RateLimit,
    ContextLimit,
    Query,
    Timeout,
    Network,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Stop,
    EndTurn,
    ToolCalls,
    Length,
    ContentFilter,
    Other,
    NotExposed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryDecision {
    Scheduled,
    Manual,
    Terminal,
    Completed,
    IntentionallyRequeued,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStage {
    Runtime,
    Attempt,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    PreparingContext,
    RestoringAccount,
    StartingRuntime,
    CheckingFinalEligibility,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connectivity {
    Suspended,
    Resumed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStage {
    Ownership,
    Create,
    VerifyPending,
    Submit,
    VerifySubmitted,
    Discard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationField {
    Author,
    ReviewId,
    Marker,
    CommentCount,
    Path,
    Body,
    Commit,
    Position,
    Side,
    Line,
    State,
    Repository,
    Batch,
    Diff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Started,
    Replacement {
        previous_attempt_id: String,
        previous_operation_id: String,
    },
    Duck {
        ordinal: u32,
        outcome: Option<crate::review::duck::VerdictKind>,
        failure: Option<crate::review::duck::DuckFailure>,
        duration_ms: Option<u64>,
        session_id: Option<String>,
    },
    DuckEvent {
        ordinal: u32,
        session_id: Option<String>,
        runtime_version: Option<String>,
        model: String,
        event: Box<Event>,
    },
    DuckTeardown {
        abort: Outcome,
        disconnect: Outcome,
    },
    DuckClientTeardown {
        drain: Outcome,
        shutdown: Outcome,
    },
    Phase {
        phase: Phase,
    },
    RuntimeReady,
    SessionCreated,
    Coverage {
        coverage: Coverage,
    },
    ToolStarted {
        call: u64,
        runtime_call_id: Option<String>,
        tool: ToolName,
        requested: Paths,
        unknown_paths: usize,
        side: Option<bool>,
        query_bytes: Option<usize>,
    },
    ToolFinished {
        call: u64,
        runtime_call_id: Option<String>,
        tool: ToolName,
        duration_ms: u64,
        response_bytes: Option<usize>,
        response_limit_bytes: usize,
        rejected_by_limit: bool,
        returned: Paths,
        covered: Paths,
        failure: Option<FailureKind>,
    },
    Session {
        event: SessionEvent,
        status_code: Option<u16>,
        failure: Option<FailureKind>,
        runtime_error: Option<RuntimeError>,
        tool_success: Option<bool>,
        aborted: Option<bool>,
        runtime_call_id: Option<String>,
        tool: Option<ToolName>,
    },
    RuntimeTruncation {
        messages_removed: Option<u64>,
    },
    AgentStopped {
        reason: StopReason,
    },
    EventStreamLost {
        lost_events: Option<u64>,
    },
    Watchdog {
        failure: FailureKind,
        idle_ms: Option<u64>,
        total_ms: u64,
    },
    Cancelled,
    Teardown {
        abort: Outcome,
        shutdown: Outcome,
    },
    WorkspaceCleanup {
        outcome: Outcome,
    },
    Finished {
        stage: CompletionStage,
        failure: Option<FailureKind>,
        coverage: Option<Coverage>,
        stop_reason: StopReason,
        session_events: u64,
        summarized_events: u64,
    },
    Retry {
        decision: RetryDecision,
        failure: Option<FailureKind>,
        next_attempt_at: Option<i64>,
        attempt_count: u64,
    },
    Connectivity {
        state: Connectivity,
        next_check_at: Option<i64>,
    },
    Publication {
        stage: PublicationStage,
        mismatch: Option<VerificationField>,
        ownership: Outcome,
        failure: Option<FailureKind>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        uncertain: Option<bool>,
    },
    PublicationAttempt {
        publication_id: String,
        operation_id: String,
        attempt_count: u64,
    },
    PublicationProviderError {
        error: crate::github::ConnectionError,
    },
    PublicationRetry {
        operation_id: String,
        decision: RetryDecision,
        failure: Option<FailureKind>,
        next_attempt_at: Option<i64>,
        attempt_count: u64,
    },
    EvidenceUnavailable {
        runtime_truncation: bool,
        runtime_stop_reason: bool,
        inactivity_watchdog: bool,
        connectivity_transitions: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub attempt: Attempt,
    pub sequence: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub event: Event,
}

// Metadata is bounded and rejects credential-shaped text. Free-form arguments,
// provider/runtime errors, responses and prompts have no fields in this schema.
fn safe_text(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if text.len() > TEXT_LIMIT
        || serde_json::to_string(text).is_ok_and(|encoded| encoded.len() > TEXT_LIMIT)
        || text.chars().any(char::is_control)
        || [
            "ghp_",
            "gho_",
            "ghu_",
            "ghs_",
            "ghr_",
            "github_pat_",
            "bearer ",
            "-----begin",
            "access_token",
            "refresh_token",
            "sk-",
        ]
        .iter()
        .any(|prefix| lower.contains(prefix))
    {
        return format!("[redacted:{:x}]", Sha256::digest(text.as_bytes()));
    }
    text.to_owned()
}

impl Record {
    fn sanitize(&mut self) {
        for text in [
            &mut self.attempt.id,
            &mut self.attempt.operation_id,
            &mut self.attempt.work_id,
            &mut self.attempt.head,
            &mut self.attempt.model,
        ] {
            *text = safe_text(text);
        }
        for text in [
            &mut self.attempt.session_id,
            &mut self.attempt.runtime_version,
        ]
        .into_iter()
        .flatten()
        {
            *text = safe_text(text);
        }
        fn paths(value: &mut Paths) {
            value.paths.truncate(PATH_LIMIT);
            for path in &mut value.paths {
                *path = safe_text(path);
            }
            value.omitted = value.count.saturating_sub(value.paths.len());
        }
        fn coverage(value: &mut Coverage) {
            paths(&mut value.changed);
            paths(&mut value.covered);
            paths(&mut value.missing);
        }
        match &mut self.event {
            Event::Coverage { coverage: value } => coverage(value),
            Event::Finished {
                coverage: Some(value),
                ..
            } => coverage(value),
            Event::ToolStarted { requested, .. } => paths(requested),
            Event::ToolFinished {
                returned, covered, ..
            } => {
                paths(returned);
                paths(covered);
            }
            _ => {}
        }
        match &mut self.event {
            Event::ToolStarted {
                runtime_call_id: Some(id),
                ..
            }
            | Event::ToolFinished {
                runtime_call_id: Some(id),
                ..
            }
            | Event::Session {
                runtime_call_id: Some(id),
                ..
            } => *id = safe_text(id),
            Event::Replacement {
                previous_attempt_id,
                previous_operation_id,
            } => {
                *previous_attempt_id = safe_text(previous_attempt_id);
                *previous_operation_id = safe_text(previous_operation_id);
            }
            Event::PublicationAttempt {
                publication_id,
                operation_id,
                ..
            } => {
                *publication_id = safe_text(publication_id);
                *operation_id = safe_text(operation_id);
            }
            Event::PublicationRetry { operation_id, .. } => *operation_id = safe_text(operation_id),
            Event::Duck {
                session_id: Some(id),
                ..
            } => *id = safe_text(id),
            Event::DuckEvent {
                event,
                session_id,
                runtime_version,
                model,
                ..
            } => {
                for text in [session_id, runtime_version].into_iter().flatten() {
                    *text = safe_text(text);
                }
                *model = safe_text(model);
                // Only one nesting level is valid: a Duck cannot call another Duck.
                if matches!(event.as_ref(), Event::DuckEvent { .. }) {
                    **event = Event::EvidenceUnavailable {
                        runtime_truncation: true,
                        runtime_stop_reason: true,
                        inactivity_watchdog: true,
                        connectivity_transitions: true,
                    };
                }
                let mut nested = Record {
                    attempt: self.attempt.clone(),
                    sequence: None,
                    elapsed_ms: None,
                    event: *event.clone(),
                };
                nested.sanitize();
                **event = nested.event;
            }
            _ => {}
        }
    }
}

impl Store {
    pub fn record_attempt_start(&self, record: Record) -> Result<(), String> {
        let previous = self
            .attempt_diagnostics()?
            .into_iter()
            .rev()
            .filter_map(|entry| entry.attempt)
            .find(|entry| {
                entry.attempt.work_id == record.attempt.work_id
                    && entry.attempt.id != record.attempt.id
            });
        self.record_attempt(record.clone())?;
        if let Some(previous) = previous {
            self.record_attempt(Record {
                event: Event::Replacement {
                    previous_attempt_id: previous.attempt.id,
                    previous_operation_id: previous.attempt.operation_id,
                },
                sequence: None,
                ..record
            })?;
        }
        Ok(())
    }

    pub fn record_attempt_decision(&self, operation_id: &str, event: Event) -> Result<(), String> {
        let Some(latest) = self
            .attempt_diagnostics()?
            .into_iter()
            .rev()
            .filter_map(|record| record.attempt)
            .find(|record| record.attempt.operation_id == operation_id)
        else {
            return Err("Attempt diagnostics missing; decision could not be correlated.".into());
        };
        self.record_attempt(Record {
            attempt: latest.attempt,
            sequence: None,
            elapsed_ms: None,
            event,
        })
    }

    pub fn record_attempt(&self, mut record: Record) -> Result<(), String> {
        let _guard = JOURNAL
            .lock()
            .map_err(|_| "Attempt diagnostics lock unavailable.")?;
        record.sanitize();
        let diagnostic = Diagnostic {
            timestamp_secs: super::diagnostic_time()?,
            event: DiagnosticEvent::Attempt,
            attempt: Some(record),
        };
        let mut bytes =
            serde_json::to_vec(&diagnostic).map_err(|_| "Cannot encode attempt diagnostics.")?;
        bytes.push(b'\n');
        if bytes.len() > RECORD_BYTES {
            return Err("Attempt diagnostic exceeds record limit.".into());
        }
        let directory = self
            .directory("state")
            .map_err(|_| "Cannot create attempt diagnostics directory.")?;
        let path = directory.join(FILES[0]);
        match super::private_fs::existing_file(&path).and_then(|_| std::fs::metadata(&path)) {
            Ok(metadata)
                if metadata.len().saturating_add(bytes.len() as u64) > LOG_BYTES as u64 =>
            {
                for index in (0..FILES.len() - 1).rev() {
                    let from = directory.join(FILES[index]);
                    match super::private_fs::existing_file(&from) {
                        Ok(()) => {
                            super::private_fs::rotate(&from, &directory.join(FILES[index + 1]))
                                .map_err(|_| "Cannot rotate attempt diagnostics.")?
                        }
                        Err(error) if error.kind() == ErrorKind::NotFound => {}
                        Err(_) => return Err("Cannot inspect attempt diagnostics rotation.".into()),
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot inspect attempt diagnostics.".into()),
        }
        super::private_fs::append(&path)
            .and_then(|mut file| file.write_all(&bytes).and_then(|_| file.sync_data()))
            .map_err(|_| "Cannot write attempt diagnostics.".into())
    }

    pub(super) fn attempt_diagnostics(&self) -> Result<Vec<Diagnostic>, String> {
        let _guard = JOURNAL
            .lock()
            .map_err(|_| "Attempt diagnostics lock unavailable.")?;
        let mut records = Vec::new();
        for name in FILES.into_iter().rev() {
            // Check length before allocation, including on a corrupt/oversized file.
            let path = self.root.join("state").join(name);
            match super::private_fs::existing_file(&path).and_then(|_| std::fs::metadata(&path)) {
                Ok(meta) if meta.len() > LOG_BYTES as u64 => {
                    return Err("Attempt diagnostics exceed size limit.".into())
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(_) => return Err("Cannot inspect attempt diagnostics.".into()),
            }
            let bytes = self
                .read_state(name)
                .map_err(|_| "Cannot read attempt diagnostics.")?;
            let text = std::str::from_utf8(&bytes).map_err(|_| "Invalid attempt diagnostics.")?;
            for line in text.lines() {
                if line.len() > RECORD_BYTES {
                    return Err("Attempt diagnostic exceeds record limit.".into());
                }
                let mut record: Diagnostic =
                    serde_json::from_str(line).map_err(|_| "Invalid attempt diagnostics.")?;
                if record.event != DiagnosticEvent::Attempt || record.attempt.is_none() {
                    return Err("Invalid attempt diagnostic event.".into());
                }
                record.attempt.as_mut().unwrap().sanitize();
                records.push(record);
            }
        }
        Ok(records)
    }
}
