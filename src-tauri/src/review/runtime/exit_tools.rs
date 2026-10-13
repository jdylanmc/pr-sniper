//! Terminal exit tools. One shared slot, first valid call wins, later calls get an error
//! result so the agent cannot exit twice. A failed call leaves the loop running.

use crate::exits::{Exit, ExitCall, MAX_REJECTED_CALLS};
use crate::review::{diagnostics::Trace, Failure};
use crate::storage::diagnostics::{Event as DiagnosticEvent, FailureKind, Paths, ToolName};
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
}

pub(crate) type ExitGate = Arc<dyn Fn(&ExitCall) -> Result<(), Refusal> + Send + Sync>;

#[derive(Default)]
pub(super) struct ExitState {
    slot: Mutex<Option<ExitCall>>,
    rejected: AtomicU32,
    failure: Mutex<Option<Failure>>,
}

impl ExitState {
    pub fn take_exit(&self) -> Option<ExitCall> {
        self.slot.lock().ok().and_then(|mut slot| slot.take())
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
}

fn error(message: &str) -> ToolResult {
    ToolResult::Expanded(ToolResultExpanded::new(message, "failure").with_error(message))
}

impl ExitTools {
    pub fn new(offered: Vec<Exit>, gate: ExitGate, diagnostics: Option<Arc<Trace>>) -> Self {
        Self {
            state: Arc::new(ExitState::default()),
            offered,
            gate,
            diagnostics,
            calls: AtomicU64::new(0),
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

    fn handle(&self, name: &str, args: &serde_json::Value) -> (ToolResult, bool) {
        let Some(exit) = Exit::parse(name).filter(|exit| self.offered.contains(exit)) else {
            return (error("Exit not offered."), true);
        };
        let call = match ExitCall::parse(name, args) {
            Ok(call) => call,
            Err(message) => return (self.reject(message), true),
        };
        // Parallel exit calls race here: the slot lock serializes them, first one wins.
        let Ok(mut slot) = self.state.slot.lock() else {
            return (error("Exit tracking unavailable."), true);
        };
        if slot.is_some() {
            return (error("An exit was already taken. Stop."), true);
        }
        if exit != Exit::PanicExit {
            if let Err(refusal) = (self.gate)(&call) {
                return match refusal {
                    Refusal::Denied(reason) => (error(&reason), true),
                    Refusal::Moved(reason) => {
                        self.state.fail(Failure::tossed(&reason));
                        (error(&reason), true)
                    }
                };
            }
        }
        *slot = Some(call);
        (ToolResult::Text("ok".into()), false)
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
        let (result, failed) =
            super::off_runtime(|| self.handle(&invocation.tool_name, &invocation.arguments));
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
