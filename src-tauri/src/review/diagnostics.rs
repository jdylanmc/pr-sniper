use super::Failure;
use crate::{
    monitoring::{JobOperation, OperationFailure, OperationState, QueueJob},
    storage::diagnostics::{
        Attempt, Coverage, Event, FailureKind, Paths, Record, RetryDecision, StopReason,
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

pub(crate) type Sink = Arc<dyn Fn(Record) -> Result<(), String> + Send + Sync>;

struct State {
    attempt: Attempt,
    sequence: u64,
    failure: Option<FailureKind>,
    coverage: Option<Coverage>,
    session_events: u64,
    summarized_events: u64,
    stop_reason: StopReason,
}

pub(crate) struct Trace {
    state: Mutex<State>,
    started: Instant,
    sink: Sink,
}

impl Trace {
    pub(crate) fn new(attempt: Attempt, sink: Sink) -> Arc<Self> {
        let trace = Arc::new(Self {
            state: Mutex::new(State {
                attempt,
                sequence: 0,
                failure: None,
                coverage: None,
                session_events: 0,
                summarized_events: 0,
                stop_reason: StopReason::NotExposed,
            }),
            started: Instant::now(),
            sink,
        });
        trace.emit(Event::Started);
        trace
    }

    fn update(&self, change: impl FnOnce(&mut State) -> Option<Event>) {
        let Ok(mut state) = self.state.lock() else {
            eprintln!("Attempt diagnostic coordination unavailable.");
            return;
        };
        if let Some(event) = change(&mut state) {
            state.sequence += 1;
            let record = Record {
                attempt: state.attempt.clone(),
                sequence: Some(state.sequence),
                elapsed_ms: Some(self.started.elapsed().as_millis().min(u64::MAX as u128) as u64),
                event,
            };
            // The native sink also reports this to the host's visible error surface.
            // Diagnostics never replace the observed execution/publication outcome.
            if (self.sink)(record).is_err() {
                eprintln!("Attempt diagnostics could not be persisted; evidence is incomplete.");
            }
        }
    }

    pub(crate) fn emit(&self, event: Event) {
        self.update(|_| Some(event));
    }

    pub(crate) fn runtime(&self, version: String) {
        self.update(|state| {
            state.attempt.runtime_version = Some(version);
            Some(Event::RuntimeReady)
        });
    }

    pub(crate) fn session(&self, id: String) {
        self.update(|state| {
            state.attempt.session_id = Some(id);
            Some(Event::SessionCreated)
        });
    }

    pub(crate) fn fail(&self, kind: FailureKind) {
        self.update(|state| {
            state.failure = Some(kind);
            None
        });
    }

    pub(crate) fn coverage(&self, changed: &[String], covered: &BTreeSet<String>) {
        self.update(|state| {
            let coverage = Coverage {
                changed: Paths::new(changed.iter().cloned()),
                covered: Paths::new(covered.iter().cloned()),
                missing: Paths::new(changed.iter().filter(|p| !covered.contains(*p)).cloned()),
                responsible_tool: crate::storage::diagnostics::ToolName::ReadChanges,
            };
            state.coverage = Some(coverage.clone());
            Some(Event::Coverage { coverage })
        });
    }

    pub(crate) fn event(&self, kind: &str, data: &serde_json::Value) {
        use crate::storage::diagnostics::{RuntimeError, SessionEvent};
        if kind == "session.truncation" {
            self.emit(Event::RuntimeTruncation {
                messages_removed: data["messagesRemovedDuringTruncation"].as_u64(),
            });
            return;
        }
        let event = match kind {
            "assistant.message" => SessionEvent::Message,
            "assistant.usage" => SessionEvent::Usage,
            "session.idle" => SessionEvent::Idle,
            "session.abort" => SessionEvent::Abort,
            "session.error" => SessionEvent::Error,
            "tool.execution_start" => SessionEvent::ToolStart,
            "tool.execution_complete" => SessionEvent::ToolComplete,
            "assistant.turn_end" => SessionEvent::TurnEnd,
            _ => SessionEvent::Other,
        };
        self.update(|state| {
            state.session_events += 1;
            let failure = match event {
                SessionEvent::Error => Some(FailureKind::RuntimeExecution),
                SessionEvent::Abort => Some(FailureKind::Cancelled),
                _ => None,
            };
            if failure.is_some() {
                state.failure = failure;
            }
            // Bound heartbeat/progress volume; errors and terminal evidence always survive.
            if state.session_events > 128
                && !matches!(
                    event,
                    SessionEvent::Error | SessionEvent::Abort | SessionEvent::Idle
                )
            {
                return None;
            }
            state.summarized_events += 1;
            Some(Event::Session {
                event,
                status_code: data["statusCode"]
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok()),
                failure,
                runtime_error: data["errorType"].as_str().map(|value| match value {
                    "authentication" => RuntimeError::Authentication,
                    "authorization" => RuntimeError::Authorization,
                    "quota" => RuntimeError::Quota,
                    "rate_limit" => RuntimeError::RateLimit,
                    "context_limit" => RuntimeError::ContextLimit,
                    "query" => RuntimeError::Query,
                    "timeout" => RuntimeError::Timeout,
                    "network" => RuntimeError::Network,
                    _ => RuntimeError::Other,
                }),
                tool_success: if matches!(event, SessionEvent::ToolComplete) {
                    data["success"].as_bool()
                } else {
                    None
                },
                aborted: if matches!(event, SessionEvent::Idle) {
                    data["aborted"].as_bool()
                } else {
                    None
                },
                runtime_call_id: data["toolCallId"].as_str().map(str::to_owned),
                tool: data["toolName"]
                    .as_str()
                    .map(crate::storage::diagnostics::ToolName::from_name),
            })
        });
    }

    pub(crate) fn stopped(&self, reason: Option<&str>) {
        let reason = match reason {
            Some("stop") => StopReason::Stop,
            Some("end_turn") => StopReason::EndTurn,
            Some("tool_calls") => StopReason::ToolCalls,
            Some("length") => StopReason::Length,
            Some("content_filter") => StopReason::ContentFilter,
            Some(_) => StopReason::Other,
            None => StopReason::NotExposed,
        };
        self.update(|state| {
            state.stop_reason = reason;
            Some(Event::AgentStopped { reason })
        });
    }

    pub(crate) fn finish(
        &self,
        stage: crate::storage::diagnostics::CompletionStage,
        error: Option<&Failure>,
    ) {
        self.update(|state| {
            Some(Event::Finished {
                stage,
                failure: error.map(|error| state.failure.unwrap_or_else(|| failure_kind(error))),
                coverage: state.coverage.clone(),
                stop_reason: state.stop_reason,
                session_events: state.session_events,
                summarized_events: state.summarized_events,
            })
        });
    }
}

pub(crate) fn failure_kind(error: &Failure) -> FailureKind {
    if error.cancelled {
        return FailureKind::Cancelled;
    }
    match error.kind {
        OperationFailure::Timeout => FailureKind::TotalDurationTimeout,
        OperationFailure::Network => FailureKind::Network,
        OperationFailure::RateLimited => FailureKind::RateLimited,
        OperationFailure::Provider => FailureKind::Provider,
        OperationFailure::Superseded => FailureKind::Superseded,
        OperationFailure::Permanent => FailureKind::EligibilityOrConfiguration,
    }
}

pub(crate) fn attempt(job: &QueueJob, operation: &JobOperation, model: &str) -> Attempt {
    Attempt {
        id: uuid::Uuid::new_v4().to_string(),
        operation_id: operation.id.clone(),
        work_id: format!(
            "{:x}",
            Sha256::digest(super::key(job, job.assignment_id.as_deref().unwrap_or("")).as_bytes())
        ),
        number: job.number,
        head: job.head_sha.clone(),
        model: model.into(),
        started_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis().min(u64::MAX as u128) as u64),
        session_id: None,
        runtime_version: None,
    }
}

pub(crate) fn native(
    app: &tauri::AppHandle,
    job: &QueueJob,
    operation: &JobOperation,
    model: &str,
) -> Arc<Trace> {
    let app = app.clone();
    Trace::new(
        attempt(job, operation, model),
        Arc::new(move |record| {
            let result = app
                .state::<crate::Host>()
                .store
                .lock()
                .map_err(|_| "Attempt diagnostic storage unavailable.".to_string())
                .and_then(|store| store.record_attempt(record));
            if let Err(error) = &result {
                crate::report(&app, error.clone());
            }
            result
        }),
    )
}

pub(crate) fn retry_event(operation: &JobOperation) -> Event {
    let decision = match operation.state {
        OperationState::Completed => RetryDecision::Completed,
        OperationState::ManualRetry => RetryDecision::Manual,
        OperationState::Queued | OperationState::Interrupted => RetryDecision::Scheduled,
        _ => RetryDecision::Terminal,
    };
    Event::Retry {
        decision,
        next_attempt_at: operation.next_attempt_at,
        attempt_count: operation.attempt_count as u64,
    }
}
