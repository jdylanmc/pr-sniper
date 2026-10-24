//! Terminal exit tools. One shared slot, first valid call wins, later calls get an error
//! result so the agent cannot exit twice. A failed call leaves the loop running.

use crate::exits::{Exit, ExitCall, MAX_REJECTED_CALLS};
use crate::review::{diagnostics::Trace, Failure};
use crate::storage::diagnostics::{
    Event as DiagnosticEvent, ExitRefusal, FailureKind, Paths, ToolName,
};
use async_trait::async_trait;
use github_copilot_sdk::{tool::ToolHandler, ToolInvocation, ToolResult, ToolResultExpanded};
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Instant;

/// Why the host refuses an exit at call time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A gate denies this exit; the agent may pick another.
    Denied(String),
    /// The PR state moved since the packet; the run is tossed.
    Moved(String),
    /// The exit-time PR re-read failed. The run ends with the typed provider failure:
    /// the agent cannot fix it, and retrying in the loop only ends in `panic_exit`.
    Unreadable(crate::github::ConnectionError),
}

pub(crate) type ExitGate = Arc<dyn Fn(&ExitCall) -> Result<(), Refusal> + Send + Sync>;
pub(super) type FindingGate = Arc<
    dyn Fn(&mut ExitCall, &mut Vec<crate::review::findings::Assessment>) -> Result<(), String>
        + Send
        + Sync,
>;

#[derive(Default)]
pub(super) struct ExitState {
    slot: Mutex<Option<ExitCall>>,
    rejected: AtomicU32,
    failure: Mutex<Option<Failure>>,
    pub(super) findings: Mutex<Vec<crate::review::findings::Assessment>>,
}

impl ExitState {
    pub fn take_exit(&self) -> Option<ExitCall> {
        self.slot.lock().ok().and_then(|mut slot| slot.take())
    }
    /// An exit already won the slot.
    pub fn taken(&self) -> bool {
        self.slot.lock().map(|slot| slot.is_some()).unwrap_or(true)
    }
    pub fn take_failure(&self) -> Option<Failure> {
        self.failure.lock().ok().and_then(|mut f| f.take())
    }
    fn fail(&self, failure: Failure) {
        if let Ok(mut slot) = self.failure.lock() {
            slot.get_or_insert(failure);
        }
    }
}

pub(super) struct ExitTools {
    pub state: Arc<ExitState>,
    pub offered: Vec<Exit>,
    pub gate: ExitGate,
    pub diagnostics: Option<Arc<Trace>>,
    pub calls: AtomicU64,
    pub(super) finding_gate: Option<FindingGate>,
    pub(super) require_finding_identity: bool,
}

fn error(message: &str) -> ToolResult {
    ToolResult::Expanded(ToolResultExpanded::new(message, "failure").with_error(message))
}

/// A refused exit call: the agent's tool result and the typed cause for diagnostics.
pub(super) struct Rejection {
    result: ToolResult,
    refusal: ExitRefusal,
    provider_error: Option<crate::github::ConnectionError>,
}

fn refused(refusal: ExitRefusal, result: ToolResult) -> Box<Rejection> {
    Box::new(Rejection {
        result,
        refusal,
        provider_error: None,
    })
}

impl ExitTools {
    pub fn new(offered: Vec<Exit>, gate: ExitGate, diagnostics: Option<Arc<Trace>>) -> Self {
        Self {
            state: Arc::new(ExitState::default()),
            offered,
            gate,
            diagnostics,
            calls: AtomicU64::new(0),
            finding_gate: None,
            require_finding_identity: false,
        }
    }

    fn reject(&self, message: String) -> ToolResult {
        let count = self.state.rejected.fetch_add(1, Ordering::SeqCst) + 1;
        if count >= MAX_REJECTED_CALLS {
            self.state.fail(Failure::contract(
                "The agent repeatedly sent malformed or over-limit exit calls.",
            ));
        }
        error(&message)
    }

    #[cfg(test)]
    fn handle(&self, name: &str, args: &serde_json::Value) -> (ToolResult, bool) {
        match self.judge(name, args) {
            Ok(result) => (result, false),
            Err(rejection) => (rejection.result, true),
        }
    }

    fn judge(&self, name: &str, args: &serde_json::Value) -> Result<ToolResult, Box<Rejection>> {
        use ExitRefusal as R;
        let Some(exit) = Exit::parse(name).filter(|exit| self.offered.contains(exit)) else {
            return Err(refused(R::NotOffered, error("Exit not offered.")));
        };
        if self.require_finding_identity
            && args
                .get("new_comments")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|comments| comments.iter().any(|c| c.get("finding_id").is_none()))
        {
            return Err(refused(R::Malformed, self.reject("Each new_comments item needs finding_id: null for a distinct new finding, or the existing thread id to update it.".into())));
        }
        let (arguments, mut findings) = match crate::review::findings::extract(args) {
            Ok(value) => value,
            Err(message) => return Err(refused(R::Malformed, self.reject(message))),
        };
        if !findings.is_empty()
            && matches!(
                exit,
                Exit::PanicExit | Exit::SubmitReport | Exit::SubmitVerdict
            )
        {
            return Err(refused(
                R::Malformed,
                self.reject("Finding dispositions are primary-only.".into()),
            ));
        }
        let mut call = match ExitCall::parse(name, &arguments) {
            Ok(call) => call,
            Err(message) => return Err(refused(R::Malformed, self.reject(message))),
        };
        // Parallel exit calls race here: the slot lock serializes them, first one wins.
        let Ok(mut slot) = self.state.slot.lock() else {
            return Err(refused(R::Unavailable, error("Exit tracking unavailable.")));
        };
        if slot.is_some() {
            return Err(refused(
                R::AlreadyTaken,
                error("An exit was already taken. Stop."),
            ));
        }
        if let Some(gate) = &self.finding_gate {
            if let Err(reason) = gate(&mut call, &mut findings) {
                return Err(refused(R::Findings, error(&reason)));
            }
        }
        if exit != Exit::PanicExit {
            if let Err(refusal) = (self.gate)(&call) {
                return Err(match refusal {
                    Refusal::Denied(reason) => refused(R::Gate, error(&reason)),
                    Refusal::Moved(reason) => {
                        self.state.fail(Failure::tossed(&reason));
                        refused(R::Moved, error(&reason))
                    }
                    Refusal::Unreadable(cause) => {
                        self.state.fail(Failure::from(cause));
                        Box::new(Rejection {
                            result: error("PR state unreadable. The host ends this run. Stop."),
                            refusal: R::ProviderRead,
                            provider_error: Some(cause),
                        })
                    }
                });
            }
        }
        let Ok(mut saved) = self.state.findings.lock() else {
            return Err(refused(
                R::Unavailable,
                error("Finding tracking unavailable."),
            ));
        };
        *saved = findings;
        *slot = Some(call);
        Ok(ToolResult::Text("ok".into()))
    }

    /// `judge`, with a host panic turned into the run's typed failure.
    fn judge_caught(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<ToolResult, Box<Rejection>> {
        crate::host_panic::catch(|| self.judge(name, args)).unwrap_or_else(|panic| {
            if let Some(trace) = &self.diagnostics {
                trace.host_panic(&panic);
            }
            self.state.fail(Failure::host_panic(&panic));
            Err(refused(
                ExitRefusal::HostPanic,
                error("Host fault. The host ends this run. Stop."),
            ))
        })
    }
}

#[async_trait]
impl ToolHandler for ExitTools {
    async fn call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ToolResult, github_copilot_sdk::Error> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let started = Instant::now();
        if let Some(trace) = &self.diagnostics {
            trace.emit(DiagnosticEvent::ToolStarted {
                call,
                runtime_call_id: Some(invocation.tool_call_id.clone()),
                tool: ToolName::Exit,
                requested: Paths::new([]),
                unknown_paths: 0,
                side: None,
                query_bytes: Some(invocation.arguments.to_string().len()),
            });
        }

        // The exit gate re-reads the PR over blocking HTTP.
        let outcome =
            super::off_runtime(|| self.judge_caught(&invocation.tool_name, &invocation.arguments));
        let failed = outcome.is_err();
        let result = match outcome {
            Ok(result) => result,
            Err(rejection) => {
                if let Some(trace) = &self.diagnostics {
                    trace.emit(DiagnosticEvent::ExitRefused {
                        call,
                        refusal: rejection.refusal,
                        provider_error: rejection.provider_error,
                    });
                }
                rejection.result
            }
        };
        if let Some(trace) = &self.diagnostics {
            trace.emit(DiagnosticEvent::ToolFinished {
                call,
                runtime_call_id: Some(invocation.tool_call_id),
                tool: ToolName::Exit,
                duration_ms: started.elapsed().as_millis() as u64,
                response_bytes: None,
                response_limit_bytes: 0,
                rejected_by_limit: false,
                returned: Paths::new([]),
                covered: Paths::new([]),
                failure: failed.then_some(FailureKind::ToolError),
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "exit_tools_tests.rs"]
mod tests;
