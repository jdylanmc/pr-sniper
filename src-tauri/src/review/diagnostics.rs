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

// Itemized routine session events per trace; notable ones are never capped.
const SESSION_SUMMARY_EVENTS: u64 = 32;

pub(crate) type Sink = Arc<dyn Fn(Record) -> Result<(), String> + Send + Sync>;

struct State {
    attempt: Attempt,
    sequence: u64,
    failure: Option<FailureKind>,
    coverage: Option<Coverage>,
    session_events: u64,
    summarized_events: u64,
    tool_calls: u64,
    stop_reason: StopReason,
}

pub(crate) struct Trace {
    state: Mutex<State>,
    started: Instant,
    sink: Sink,
    nested: bool,
}

impl Trace {
    pub(crate) fn duck(self: &Arc<Self>, ordinal: u32, model: &str) -> Arc<Self> {
        let parent = self.clone();
        let mut attempt = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .attempt
            .clone();
        attempt.session_id = None;
        attempt.runtime_version = None;
        attempt.model = model.into();
        Self::build(
            attempt,
            Arc::new(move |record| {
                parent.emit(Event::DuckEvent {
                    ordinal,
                    session_id: record.attempt.session_id,
                    runtime_version: record.attempt.runtime_version,
                    model: record.attempt.model,
                    event: Box::new(record.event),
                });
                Ok(())
            }),
            true,
        )
    }

    pub(crate) fn new(attempt: Attempt, sink: Sink) -> Arc<Self> {
        Self::build(attempt, sink, false)
    }

    fn build(attempt: Attempt, sink: Sink, nested: bool) -> Arc<Self> {
        let trace = Arc::new(Self {
            state: Mutex::new(State {
                attempt,
                sequence: 0,
                failure: None,
                coverage: None,
                session_events: 0,
                summarized_events: 0,
                tool_calls: 0,
                stop_reason: StopReason::NotExposed,
            }),
            started: Instant::now(),
            sink,
            nested,
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
        self.update(|state| {
            state.tool_calls += u64::from(matches!(event, Event::ToolFinished { .. }));
            // Nested Duck tool calls are counted in `DuckTeardown`; failures,
            // response-limit rejections and everything else stay itemized.
            let routine_tool = matches!(
                event,
                Event::ToolStarted { .. }
                    | Event::ToolFinished {
                        failure: None,
                        rejected_by_limit: false,
                        ..
                    }
            );
            (!(self.nested && routine_tool)).then_some(event)
        });
    }

    pub(crate) fn tool_calls(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.tool_calls)
            .unwrap_or_default()
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
            // Coverage is re-observed after every tool call. Only the baseline is
            // its own record: progress is each `ToolFinished.covered`, and the
            // final state is carried by `Finished`.
            let baseline = state.coverage.is_none();
            state.coverage = Some(coverage.clone());
            baseline.then_some(Event::Coverage { coverage })
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
            let status_code = data["statusCode"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok());
            let runtime_error = data["errorType"].as_str().map(|value| match value {
                "authentication" => RuntimeError::Authentication,
                "authorization" => RuntimeError::Authorization,
                "quota" => RuntimeError::Quota,
                "rate_limit" => RuntimeError::RateLimit,
                "context_limit" => RuntimeError::ContextLimit,
                "query" => RuntimeError::Query,
                "timeout" => RuntimeError::Timeout,
                "network" => RuntimeError::Network,
                _ => RuntimeError::Other,
            });
            let tool_success = if matches!(event, SessionEvent::ToolComplete) {
                data["success"].as_bool()
            } else {
                None
            };
            // Errors, terminal and failure-bearing evidence always survive.
            // Unrecognized streaming chatter and routine events past the
            // budget are only counted (`session_events`).
            let notable = matches!(
                event,
                SessionEvent::Error | SessionEvent::Abort | SessionEvent::Idle
            ) || status_code.is_some()
                || runtime_error.is_some()
                || tool_success == Some(false);
            // Successful tool start/complete duplicate ToolStarted/ToolFinished.
            let routine = matches!(
                event,
                SessionEvent::Other | SessionEvent::ToolStart | SessionEvent::ToolComplete
            );
            if !notable && (routine || state.summarized_events >= SESSION_SUMMARY_EVENTS) {
                return None;
            }
            state.summarized_events += 1;
            Some(Event::Session {
                event,
                status_code,
                failure,
                runtime_error,
                tool_success,
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
    operation_failure(&error.kind)
}

fn operation_failure(failure: &OperationFailure) -> FailureKind {
    match failure {
        OperationFailure::Timeout => FailureKind::Timeout,
        OperationFailure::Network => FailureKind::Network,
        OperationFailure::RateLimited => FailureKind::RateLimited,
        OperationFailure::Provider => FailureKind::Provider,
        OperationFailure::Panic => FailureKind::Panic,
        OperationFailure::Superseded | OperationFailure::Tossed => FailureKind::Superseded,
        OperationFailure::Permanent => FailureKind::Permanent,
        OperationFailure::Credentials => FailureKind::Credentials,
        OperationFailure::Permission => FailureKind::Permission,
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
                .and_then(|store| {
                    if matches!(record.event, Event::Started) {
                        store.record_attempt_start(record)
                    } else {
                        store.record_attempt(record)
                    }
                });
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
        failure: operation.failure.as_ref().map(operation_failure),
        next_attempt_at: operation.next_attempt_at,
        attempt_count: operation.attempt_count as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::duck::{DuckFailure, VerdictKind};
    use crate::storage::diagnostics::{
        CompletionStage, Outcome, Phase, RuntimeError, SessionEvent, ToolName,
    };
    use serde_json::json;
    use std::path::Path;

    // This simulation measured on origin/main 69f60b1 (legacy producer + format).
    const LEGACY_BYTES: u64 = 976_708;

    fn tool_call(
        trace: &Trace,
        call: u64,
        read: &mut BTreeSet<String>,
        changed: &[String],
        failure: Option<FailureKind>,
    ) {
        let id = format!("toolu_{call:024}");
        let requested = || Paths::new(changed.iter().skip(call as usize % 30).take(3).cloned());
        trace.event(
            "tool.execution_start",
            &json!({"toolCallId": id, "toolName": "read_changes"}),
        );
        for _ in 0..8 {
            trace.event("assistant.message_delta", &json!({}));
        }
        trace.emit(Event::ToolStarted {
            call,
            runtime_call_id: Some(id.clone()),
            tool: ToolName::ReadChanges,
            requested: requested(),
            unknown_paths: 0,
            side: None,
            query_bytes: None,
        });
        let served = call
            .is_multiple_of(4)
            .then(|| changed[call as usize % changed.len()].clone());
        read.extend(served.clone());
        trace.emit(Event::ToolFinished {
            call,
            runtime_call_id: Some(id.clone()),
            tool: ToolName::ReadChanges,
            duration_ms: 40,
            response_bytes: Some(9_000),
            response_limit_bytes: 32_000,
            rejected_by_limit: false,
            returned: requested(),
            covered: Paths::new(served),
            failure,
        });
        trace.event(
            "tool.execution_complete",
            &json!({"toolCallId": id, "toolName": "read_changes", "success": true}),
        );
        trace.coverage(changed, read);
        if call.is_multiple_of(5) {
            for kind in ["assistant.message", "assistant.usage", "assistant.turn_end"] {
                trace.event(kind, &json!({}));
            }
        }
    }

    fn session(trace: &Trace, calls: u64, changed: &[String], failing: bool) {
        trace.runtime("1.0.85".into());
        trace.session(uuid::Uuid::new_v4().to_string());
        let mut read = BTreeSet::new();
        trace.coverage(changed, &read);
        for call in 1..=calls {
            let failure = (failing && call == 3).then_some(FailureKind::ToolError);
            tool_call(trace, call, &mut read, changed, failure);
            if failing && call == calls / 2 {
                trace.event(
                    "session.error",
                    &json!({"statusCode": 500, "errorType": "network", "message": "secret"}),
                );
            }
        }
        trace.event("session.idle", &json!({"aborted": failing}));
        trace.stopped(Some("end_turn"));
    }

    // One representative review: 30 primary tool calls, three Rubber Duck
    // sessions of 20 calls each, one Duck failing mid-session.
    fn simulate(profile: &Path) -> u64 {
        let path = profile.to_path_buf();
        let trace = Trace::new(
            Attempt {
                id: uuid::Uuid::new_v4().to_string(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                work_id: format!("{:x}", Sha256::digest(b"work")),
                number: 291,
                head: "d".repeat(40),
                model: "gpt-5.6-sol-fast".into(),
                started_at_ms: Some(1_792_617_629_256),
                session_id: None,
                runtime_version: None,
            },
            Arc::new(move |record| crate::storage::Store::new(path.clone()).record_attempt(record)),
        );
        for phase in [
            Phase::PreparingContext,
            Phase::RestoringAccount,
            Phase::StartingRuntime,
            Phase::CheckingFinalEligibility,
        ] {
            trace.emit(Event::Phase { phase });
        }
        let changed: Vec<String> = (0..40)
            .map(|i| format!("src-tauri/src/module_{i}/component_{i}.rs"))
            .collect();
        session(&trace, 30, &changed, false);
        for ordinal in 1..=3 {
            let duck = trace.duck(ordinal, "claude-duck-model");
            session(&duck, 20, &changed, ordinal == 2);
            duck.emit(Event::DuckTeardown {
                abort: Outcome::NotRequired,
                disconnect: Outcome::Success,
                tool_calls: Some(duck.tool_calls()),
            });
            trace.emit(Event::Duck {
                ordinal,
                outcome: (ordinal != 2).then_some(VerdictKind::Confirmed),
                failure: (ordinal == 2).then_some(DuckFailure::Unavailable),
                duration_ms: Some(30_000),
                session_id: None,
                reason: None,
                input_tokens: None,
                output_tokens: None,
                tool_calls: None,
            });
        }
        trace.emit(Event::Teardown {
            abort: Outcome::NotRequired,
            shutdown: Outcome::Success,
        });
        trace.finish(CompletionStage::Attempt, None);
        std::fs::read_dir(profile.join("state"))
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("attempts"))
            .map(|entry| entry.metadata().unwrap().len())
            .sum()
    }

    #[test]
    fn representative_attempt_volume_is_a_tenth_of_legacy_and_keeps_failures() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile");
        let bytes = simulate(&profile);
        let entries = crate::storage::Store::new(profile).diagnostics().unwrap();
        // The same records in the uncompacted (legacy) line encoding.
        let uncompacted: u64 = entries
            .iter()
            .map(|entry| serde_json::to_vec(entry).unwrap().len() as u64 + 1)
            .sum();
        eprintln!("attempt log bytes: {bytes}; uncompacted {uncompacted}");
        assert!(
            bytes * 10 <= LEGACY_BYTES,
            "{bytes} vs legacy {LEGACY_BYTES}"
        );
        assert!(bytes * 3 <= uncompacted * 2, "{bytes} vs {uncompacted}");
        let records: Vec<_> = entries
            .into_iter()
            .filter_map(|entry| entry.attempt)
            .collect();
        let error = records
            .iter()
            .find(|record| match &record.event {
                Event::DuckEvent {
                    ordinal: 2,
                    session_id: Some(_),
                    runtime_version: Some(_),
                    event,
                    ..
                } => matches!(
                    event.as_ref(),
                    Event::Session {
                        event: SessionEvent::Error,
                        status_code: Some(500),
                        runtime_error: Some(RuntimeError::Network),
                        failure: Some(FailureKind::RuntimeExecution),
                        ..
                    }
                ),
                _ => false,
            })
            .expect("Duck session error retained in full");
        assert!(error.attempt.session_id.is_some() && error.attempt.number == 291);
        let finished = records
            .iter()
            .filter_map(|record| match &record.event {
                Event::Finished {
                    session_events,
                    summarized_events,
                    coverage: Some(_),
                    ..
                } => Some((*session_events, *summarized_events)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(finished.len(), 1);
        assert!(finished
            .iter()
            .all(|(total, summarized)| total > summarized && *summarized <= 34));
        let failed_duck_tool = records.iter().any(|record| {
            matches!(&record.event, Event::DuckEvent { ordinal: 2, event, .. }
                if matches!(event.as_ref(), Event::ToolFinished { call: 3, failure: Some(FailureKind::ToolError), .. }))
        });
        assert!(failed_duck_tool, "failed nested tool call retained in full");
        let duck_tools = records
            .iter()
            .filter_map(|record| match &record.event {
                Event::DuckEvent { event, .. } => match event.as_ref() {
                    Event::DuckTeardown { tool_calls, .. } => *tool_calls,
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(duck_tools, [20, 20, 20]);
        for kind in ["idle", "duck_teardown", "teardown", "duck"] {
            assert!(
                serde_json::to_string(&records).unwrap().contains(kind),
                "{kind}"
            );
        }
        assert!(!serde_json::to_string(&records).unwrap().contains("secret"));
    }
}
