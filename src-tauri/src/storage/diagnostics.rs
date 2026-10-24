//! Content-free attempt evidence. Producers supply typed decisions, never error text.
use super::{Diagnostic, DiagnosticEvent, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::ErrorKind, io::Write, path::PathBuf, sync::Mutex};

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
// Writer-side view of what each profile's current attempts.jsonl already
// holds, so later lines can reference it. Reset on rotation or external edit.
static JOURNAL: Mutex<Option<HashMap<PathBuf, Journal>>> = Mutex::new(None);

struct Journal {
    len: u64,
    context: Context,
}

/// Per-file compaction state. Each generation is self-contained: an attempt
/// header, each Duck identity and each tool start are written in full in a file
/// before later lines omit them, so any retained file expands on its own.
#[derive(Clone, Default)]
struct Context {
    headers: HashMap<String, Value>,
    ducks: HashMap<(String, u64), [Value; 3]>,
    calls: HashMap<(String, Option<u64>, u64), [Value; 3]>,
}

const DUCK_IDENTITY: [&str; 3] = ["session_id", "runtime_version", "model"];
const CALL_IDENTITY: [&str; 2] = ["tool", "runtime_call_id"];

fn fields<const N: usize>(event: &Map<String, Value>, keys: [&str; N]) -> [Value; N] {
    keys.map(|key| event.get(key).cloned().unwrap_or(Value::Null))
}

// `missing` is `changed` minus `covered` whenever neither list was capped.
fn derived_missing(coverage: &Map<String, Value>) -> Option<Value> {
    let list = |key: &str| {
        let paths = coverage.get(key)?.as_object()?;
        (paths.get("omitted")?.as_u64()? == 0).then_some(paths.get("paths")?.as_array()?)
    };
    let covered = list("covered")?;
    let missing: Vec<_> = list("changed")?
        .iter()
        .filter(|path| !covered.contains(path))
        .cloned()
        .collect();
    Some(serde_json::json!({"count": missing.len(), "paths": missing, "omitted": 0}))
}

fn call_key(
    id: &str,
    ordinal: Option<u64>,
    event: &Map<String, Value>,
) -> Option<(String, Option<u64>, u64)> {
    Some((id.to_owned(), ordinal, event.get("call")?.as_u64()?))
}

// The evidence event: the record's own, or the one nested in a `duck_event`.
fn inner(record: &mut Map<String, Value>) -> Option<(Option<u64>, &mut Map<String, Value>)> {
    let event = record.get_mut("event")?.as_object_mut()?;
    if event.get("kind")? != "duck_event" {
        return Some((None, event));
    }
    let ordinal = event.get("ordinal")?.as_u64();
    Some((ordinal, event.get_mut("event")?.as_object_mut()?))
}

fn strip_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, field| !field.is_null());
            map.values_mut().for_each(strip_nulls);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

fn duck_key(id: &str, record: &Map<String, Value>) -> Option<(String, u64)> {
    let event = record.get("event")?.as_object()?;
    (event.get("kind")? == "duck_event").then_some(())?;
    Some((id.to_owned(), event.get("ordinal")?.as_u64()?))
}

impl Context {
    // Omits null fields, an unchanged attempt header (written as its id) and an
    // unchanged Duck identity. Older full lines remain valid input to `expand`.
    fn compact(&self, full: &Value) -> Option<Value> {
        let mut record = full.clone();
        let identity = full["event"]
            .as_object()
            .map(|event| DUCK_IDENTITY.map(|key| event.get(key).cloned().unwrap_or(Value::Null)));
        strip_nulls(&mut record);
        let map = record.as_object_mut()?;
        let id = map.get("attempt")?.get("id")?.as_str()?.to_owned();
        if let Some(key) = duck_key(&id, map) {
            let identity = identity?;
            let event = map.get_mut("event")?.as_object_mut()?;
            // A changed identity stays as written: `model` (never null) marks
            // the declaration and absent session/runtime fields mean `None`.
            if self.ducks.get(&key) == Some(&identity) {
                DUCK_IDENTITY.iter().for_each(|field| {
                    event.remove(*field);
                });
            }
        }
        if let Some((ordinal, event)) = inner(map) {
            if let Some(coverage) = event.get_mut("coverage").and_then(Value::as_object_mut) {
                if derived_missing(coverage).as_ref() == coverage.get("missing") {
                    coverage.remove("missing");
                }
            }
            if event
                .get("kind")
                .is_some_and(|kind| kind == "tool_finished")
            {
                if let Some([tool, call_id, requested]) =
                    call_key(&id, ordinal, event).and_then(|key| self.calls.get(&key))
                {
                    if [tool, call_id] == fields(event, CALL_IDENTITY).each_ref() {
                        CALL_IDENTITY.iter().for_each(|field| {
                            event.remove(*field);
                        });
                    }
                    if Some(requested) == event.get("returned") {
                        event.remove("returned");
                    }
                }
            }
        }
        if self.headers.get(&id) == map.get("attempt") {
            map.insert("attempt".into(), Value::String(id));
        }
        Some(record)
    }

    fn expand(&mut self, mut record: Value) -> Option<Value> {
        let map = record.as_object_mut()?;
        let id = match map.get_mut("attempt")? {
            Value::String(id) => {
                let id = id.clone();
                map.insert("attempt".into(), self.headers.get(&id)?.clone());
                id
            }
            header @ Value::Object(_) => {
                let mut stored = header.clone();
                strip_nulls(&mut stored);
                let id = stored.get("id")?.as_str()?.to_owned();
                self.headers.insert(id.clone(), stored);
                id
            }
            _ => return None,
        };
        if let Some(key) = duck_key(&id, map) {
            let event = map.get_mut("event")?.as_object_mut()?;
            if event.contains_key("model") {
                let identity =
                    DUCK_IDENTITY.map(|field| event.get(field).cloned().unwrap_or(Value::Null));
                self.ducks.insert(key, identity);
            } else {
                for (field, value) in DUCK_IDENTITY.iter().zip(self.ducks.get(&key)?.clone()) {
                    event.insert((*field).into(), value);
                }
            }
        }
        let (ordinal, event) = inner(map)?;
        if let Some(coverage) = event.get_mut("coverage").and_then(Value::as_object_mut) {
            if !coverage.contains_key("missing") {
                let missing = derived_missing(coverage)?;
                coverage.insert("missing".into(), missing);
            }
        }
        match event.get("kind")?.as_str()? {
            "tool_started" => {
                let [tool, call_id] = fields(event, CALL_IDENTITY);
                let requested = event.get("requested")?.clone();
                self.calls
                    .insert(call_key(&id, ordinal, event)?, [tool, call_id, requested]);
            }
            "tool_finished" if !event.contains_key("tool") || !event.contains_key("returned") => {
                let [tool, call_id, requested] =
                    self.calls.get(&call_key(&id, ordinal, event)?)?.clone();
                if !event.contains_key("tool") {
                    event.insert("tool".into(), tool);
                    event.insert("runtime_call_id".into(), call_id);
                }
                event.entry("returned").or_insert(requested);
            }
            _ => {}
        }
        Some(record)
    }

    /// Encodes one line, committing compaction state only when the compact
    /// line expands back to exactly the full record; otherwise writes in full.
    fn encode(&mut self, timestamp_secs: u64, full: &Value) -> Result<Vec<u8>, String> {
        let mut next = self.clone();
        let compact = self.compact(full).filter(|compact| {
            next.expand(compact.clone())
                .and_then(|expanded| serde_json::from_value::<Record>(expanded).ok())
                .and_then(|record| serde_json::to_value(record).ok())
                .is_some_and(|expanded| &expanded == full)
        });
        let line = match compact {
            Some(compact) => {
                *self = next;
                compact
            }
            None => {
                self.expand(full.clone());
                full.clone()
            }
        };
        let mut bytes = serde_json::to_vec(&serde_json::json!({
            "timestamp_secs": timestamp_secs,
            "event": "attempt",
            "attempt": line,
        }))
        .map_err(|_| "Cannot encode attempt diagnostics.")?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

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
            | "submit_verdict"
            | "lgtm" => Self::Exit,
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
    /// The agent chose `panic_exit`.
    Panic,
    /// Host code panicked; see the attempt's `host_panic` event.
    HostPanic,
}

/// Why the host refused an exit call. The refusal text is never recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitRefusal {
    NotOffered,
    Malformed,
    AlreadyTaken,
    Unavailable,
    Findings,
    Gate,
    Moved,
    /// The exit-time PR re-read failed; `provider_error` carries the typed cause.
    ProviderRead,
    HostPanic,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<crate::review::duck::DuckReason>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_calls: Option<u64>,
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
        /// Nested tool calls observed; successful ones are not itemized.
        #[serde(default)]
        tool_calls: Option<u64>,
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
    ExitRefused {
        call: u64,
        refusal: ExitRefusal,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_error: Option<crate::github::ConnectionError>,
    },
    /// Host code panicked inside the attempt: source location only, never the payload.
    HostPanic {
        panic: crate::host_panic::HostPanic,
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
pub(super) fn safe_text(text: &str) -> String {
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
            Event::HostPanic { panic } => {
                panic.file = safe_text(&panic.file);
            }
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
    /// Drops this profile's writer compaction state, as a process restart
    /// does. Test seam.
    #[doc(hidden)]
    pub fn forget_attempt_journal(&self) {
        if let Ok(mut journal) = JOURNAL.lock() {
            if let Some(journal) = journal.as_mut() {
                journal.remove(&self.root.join("state").join(FILES[0]));
            }
        }
    }

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
        let mut journal = JOURNAL
            .lock()
            .map_err(|_| "Attempt diagnostics lock unavailable.")?;
        record.sanitize();
        let timestamp_secs = super::diagnostic_time()?;
        let full =
            serde_json::to_value(&record).map_err(|_| "Cannot encode attempt diagnostics.")?;
        // The record cap applies to the uncompacted form, as in older files.
        let full_bytes = serde_json::to_vec(&Diagnostic {
            timestamp_secs,
            event: DiagnosticEvent::Attempt,
            attempt: Some(record),
            panic: None,
        })
        .map_err(|_| "Cannot encode attempt diagnostics.")?;
        if full_bytes.len() + 1 > RECORD_BYTES {
            return Err("Attempt diagnostic exceeds record limit.".into());
        }
        let directory = self
            .directory("state")
            .map_err(|_| "Cannot create attempt diagnostics directory.")?;
        let path = directory.join(FILES[0]);
        let existing =
            match super::private_fs::existing_file(&path).and_then(|_| std::fs::metadata(&path)) {
                Ok(metadata) => Some(metadata.len()),
                Err(error) if error.kind() == ErrorKind::NotFound => None,
                Err(_) => return Err("Cannot inspect attempt diagnostics.".into()),
            };
        let mut len = existing.unwrap_or(0);
        let journal = journal.get_or_insert_with(HashMap::new);
        let mut context = match journal.remove(&path) {
            Some(current) if current.len == len && existing.is_some() => current.context,
            _ => Context::default(),
        };
        let mut bytes = context.clone().encode(timestamp_secs, &full)?;
        if existing.is_some() && len.saturating_add(bytes.len() as u64) > LOG_BYTES as u64 {
            for index in (0..FILES.len() - 1).rev() {
                let from = directory.join(FILES[index]);
                match super::private_fs::existing_file(&from) {
                    Ok(()) => super::private_fs::rotate(&from, &directory.join(FILES[index + 1]))
                        .map_err(|_| "Cannot rotate attempt diagnostics.")?,
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(_) => return Err("Cannot inspect attempt diagnostics rotation.".into()),
                }
            }
            context = Context::default();
            len = 0;
        }
        bytes = context.encode(timestamp_secs, &full)?;
        super::private_fs::append(&path)
            .and_then(|mut file| file.write_all(&bytes).and_then(|_| file.sync_data()))
            .map_err(|_| "Cannot write attempt diagnostics.")?;
        journal.insert(
            path,
            Journal {
                len: len + bytes.len() as u64,
                context,
            },
        );
        Ok(())
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
            let mut context = Context::default();
            for line in text.lines() {
                if line.len() > RECORD_BYTES {
                    return Err("Attempt diagnostic exceeds record limit.".into());
                }
                let mut line: Value =
                    serde_json::from_str(line).map_err(|_| "Invalid attempt diagnostics.")?;
                if let Some(attempt) = line.get_mut("attempt") {
                    *attempt = context
                        .expand(attempt.take())
                        .ok_or("Invalid attempt diagnostics.")?;
                }
                let mut record: Diagnostic =
                    serde_json::from_value(line).map_err(|_| "Invalid attempt diagnostics.")?;
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
