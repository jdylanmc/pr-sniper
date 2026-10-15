//! The primary's `rubber_duck` tool. Each call runs one fresh duck session on the duck's
//! own Copilot client, inside the primary's attempt: same AI slot, same deadline, same
//! cancellation. Calls are serialized, bounded and recorded. A failed call is a typed tool
//! error for the primary, never a run failure.

use super::{
    config_with, exit_tools::ExitTools, verified_intelligence, ExitState, Gate, ReadTools,
    DUCK_SYSTEM,
};
use crate::{
    copilot::operation::Operation,
    exits::{Exit, ExitCall},
    github::{
        provider::{GithubClient, Transport},
        review::ReviewContext,
        Identity,
    },
    monitoring::OperationFailure,
    packet::{Directive, PacketInput, ToolFacts},
    review::{
        duck::{self, DuckCall, DuckFailure, DuckInput, DuckOutcome, Verdict},
        Events, Failure, ReadCoverage, Selection,
    },
    storage::{
        diagnostics::{Event as DiagnosticEvent, FailureKind, Paths, ToolName},
        DuckLimits,
    },
};
use async_trait::async_trait;
use github_copilot_sdk::{
    tool::ToolHandler, Client, ClientOptions, ToolInvocation, ToolResult, ToolResultExpanded,
};
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

/// Built with the primary packet: everything the duck packet needs from the primary run.
#[derive(Clone)]
pub(crate) struct DuckPrep {
    pub base: PacketInput,
    pub primary_hash: String,
    pub directive: Directive,
    /// Duck tool ids: the primary's granted reads, then `submit_verdict`.
    pub tools: Vec<String>,
    pub changed: Vec<String>,
    pub threads: Vec<String>,
}

/// How the duck's Copilot client is reached. The host supplies the credential;
/// `run` turns it into client options; tests supply options directly.
pub(crate) enum DuckLaunch {
    Pending,
    Credential {
        identity: Identity,
        pair: crate::github::oauth::TokenPair,
    },
    Options {
        identity: Identity,
        options: Box<ClientOptions>,
    },
}

pub(crate) type Pacing = Arc<dyn Fn(Option<i64>) + Send + Sync>;
/// Every duck call of one attempt, shared with the host so a failed attempt keeps them.
pub(crate) type DuckLog = Arc<Mutex<Vec<DuckCall>>>;
pub(crate) type Gated = Arc<dyn Fn() -> bool + Send + Sync>;

pub(crate) struct DuckSetup {
    pub prep: DuckPrep,
    /// The duck Agent as captured at job start; runs as its own Selection.
    pub selection: Selection,
    pub limits: DuckLimits,
    pub launch: DuckLaunch,
    /// Records a duck-account rate limit (Retry-After seconds when known).
    pub rate_limited: Pacing,
    /// The duck account is behind a rate-limit gate now.
    pub gated: Gated,
    /// The duck account's own operation: its generation and cancellation. A change
    /// (disconnect, reconnect, account edit) makes later calls `unavailable`.
    pub account: Option<Operation>,
    pub log: DuckLog,
}

/// A verified, started duck client. Setup failures are job errors (`duck_setup`).
pub(super) struct DuckEngine {
    pub client: Client,
    setup: DuckSetup,
    version: String,
}

impl DuckEngine {
    /// The duck account is still the one this attempt verified.
    fn account_ok(&self) -> bool {
        self.setup
            .account
            .as_ref()
            .is_none_or(|operation| operation.check().is_ok())
    }

    pub async fn start(mut setup: DuckSetup, operation: &Operation) -> Result<Self, Failure> {
        let agent = &setup.selection.agent;
        if agent.model.trim().is_empty()
            || agent
                .ai_account
                .as_ref()
                .is_none_or(|a| a.provider != "copilot")
        {
            return Err(Failure::permanent(
                "Choose a Copilot account and model for the Rubber duck Agent.",
            )
            .duck_setup());
        }
        let DuckLaunch::Options { identity, options } =
            std::mem::replace(&mut setup.launch, DuckLaunch::Pending)
        else {
            return Err(Failure::permanent("Rubber duck runtime was not prepared.").duck_setup());
        };
        let client = operation
            .wait(Client::start(*options))
            .await
            .map_err(Failure::operation)
            .and_then(|result| result.map_err(Failure::sdk))
            .map_err(Failure::duck_setup)?;
        let engine = Self {
            client,
            setup,
            version: String::new(),
        };
        match engine.verify(&identity).await {
            Ok(version) => Ok(Self { version, ..engine }),
            Err(error) => {
                crate::copilot::runtime::shutdown(&engine.client).await;
                Err(error.duck_setup())
            }
        }
    }

    async fn verify(&self, identity: &Identity) -> Result<String, Failure> {
        let auth =
            self.client.get_auth_status().await.map_err(|_| {
                Failure::permanent("Rubber duck Copilot authentication probe failed.")
            })?;
        if !auth.is_authenticated
            || auth
                .login
                .as_deref()
                .is_some_and(|s| !s.eq_ignore_ascii_case(&identity.login))
            || auth.auth_type.as_deref() != Some("token")
        {
            return Err(Failure::permanent(
                "Copilot did not verify the Rubber duck account; ambient identity is not accepted.",
            ));
        }
        let status = self
            .client
            .get_status()
            .await
            .map_err(|_| Failure::permanent("Rubber duck Copilot probe failed."))?;
        let models = self.client.list_models().await.map_err(Failure::sdk)?;
        let agent = &self.setup.selection.agent;
        let model = models.iter().find(|m| m.id == agent.model).ok_or_else(|| {
            Failure::permanent("The Rubber duck model is not available for its Copilot account.")
        })?;
        crate::copilot::runtime::validate_intelligence(model, agent.intelligence.as_ref())
            .map_err(Failure::permanent)?;
        Ok(status.version)
    }
}

pub(super) struct DuckTool<T: Transport> {
    pub engine: Arc<DuckEngine>,
    pub context: ReviewContext,
    pub github: Arc<GithubClient<T>>,
    pub repository_name: String,
    pub facts: Arc<ToolFacts>,
    pub primary_exit: Arc<ExitState>,
    /// The primary attempt's deadline; duck time counts against it.
    pub deadline: Instant,
    pub operation: Operation,
    pub local_gate: Gate,
    pub diagnostics: Option<Arc<crate::review::diagnostics::Trace>>,
    serial: tokio::sync::Mutex<()>,
    started: AtomicU32,
    tool_calls: AtomicU64,
}

impl<T: Transport + Send + Sync + 'static> DuckTool<T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: Arc<DuckEngine>,
        context: ReviewContext,
        github: Arc<GithubClient<T>>,
        repository_name: String,
        facts: Arc<ToolFacts>,
        primary_exit: Arc<ExitState>,
        operation: Operation,
        local_gate: Gate,
        diagnostics: Option<Arc<crate::review::diagnostics::Trace>>,
    ) -> Self {
        Self {
            engine,
            context,
            github,
            repository_name,
            facts,
            primary_exit,
            deadline: operation.deadline,
            operation,
            local_gate,
            diagnostics,
            serial: tokio::sync::Mutex::new(()),
            started: AtomicU32::new(0),
            tool_calls: AtomicU64::new(0),
        }
    }

    pub fn calls(&self) -> Vec<DuckCall> {
        self.engine
            .setup
            .log
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    fn record(&self, call: DuckCall) {
        if let Ok(mut calls) = self.engine.setup.log.lock() {
            duck::log_call(&mut calls, call);
        }
    }

    fn blank(&self, ordinal: u32, input: DuckInput, failure: DuckFailure) -> DuckCall {
        let setup = &self.engine.setup;
        DuckCall {
            ordinal,
            input,
            outcome: DuckOutcome::Failure { failure },
            limits: setup.limits,
            agent: setup.selection.agent.name.clone(),
            model: setup.selection.agent.model.clone(),
            state_id: setup.prep.base.state_id.clone(),
            packet: None,
            session_id: None,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
            coverage: None,
            duration_ms: 0,
            repeats: 0,
        }
    }

    /// One call, start to finish. `Err` is the typed failure the primary sees.
    pub async fn consult(&self, args: &serde_json::Value) -> Result<String, String> {
        if self.primary_exit.taken() {
            return Err(DuckFailure::ExitTaken.message(None));
        }
        let setup = &self.engine.setup;
        let input = DuckInput::parse(args, &setup.prep.changed, &setup.prep.threads)
            .map_err(|reason| DuckFailure::InvalidInput.message(Some(&reason)))?;
        let _serial = self.serial.lock().await;
        let ordinal = self.started.load(Ordering::SeqCst) + 1;
        let refuse = |failure: DuckFailure, input: DuckInput| {
            self.record(self.blank(ordinal, input, failure));
            Err(failure.message(None))
        };
        if self.primary_exit.taken() {
            return refuse(DuckFailure::ExitTaken, input);
        }
        if self.started.load(Ordering::SeqCst) >= setup.limits.max_calls {
            return refuse(DuckFailure::LimitReached, input);
        }
        let now = Instant::now();
        let available = self
            .deadline
            .checked_sub(Duration::from_secs(duck::EXIT_RESERVE_SECONDS))
            .map_or(Duration::ZERO, |end| end.saturating_duration_since(now));
        let timeout = Duration::from_secs(u64::from(setup.limits.call_timeout_seconds));
        if available < timeout.min(Duration::from_secs(duck::MIN_CALL_SECONDS)) {
            return refuse(DuckFailure::BudgetExhausted, input);
        }
        let call_end = now + timeout.min(available);
        if (setup.gated)() {
            return refuse(DuckFailure::RateLimited, input);
        }
        if !self.engine.account_ok() {
            return refuse(DuckFailure::Unavailable, input);
        }
        self.started.store(ordinal, Ordering::SeqCst);
        let mut call = self.blank(ordinal, input, DuckFailure::Failed);
        let packet = crate::packet::duck::build(&crate::packet::duck::DuckPacketInput {
            base: &setup.prep.base,
            primary_hash: &setup.prep.primary_hash,
            ordinal,
            claim: &call.input,
            directive: &setup.prep.directive,
            tools: &setup.prep.tools,
            created_at: &chrono::Utc::now().to_rfc3339(),
        });
        call.packet = Some(packet.record.clone());
        let started = Instant::now();
        let outcome = self.session(packet.text, call_end, &mut call).await;
        call.duration_ms = started.elapsed().as_millis() as u64;
        let result = match outcome {
            Ok(verdict) => {
                let text = duck::result_text(&call, &verdict);
                call.outcome = DuckOutcome::Verdict { verdict };
                Ok(text)
            }
            Err(failure) => {
                call.outcome = DuckOutcome::Failure { failure };
                Err(failure.message(None))
            }
        };
        self.record(call);
        result
    }

    /// Ends a call that outlives its window or its primary.
    async fn watch(&self, call_end: Instant) -> DuckFailure {
        loop {
            if Instant::now() >= call_end {
                return DuckFailure::TimedOut;
            }
            if self.operation.check().is_err()
                || (self.local_gate)().is_err()
                || !self.engine.account_ok()
            {
                return DuckFailure::Unavailable;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// One duck session. The session handle outlives the turn: whatever ends the
    /// turn (timeout, gate, error), a turn without a verdict is aborted, and every
    /// session is closed, so no duck inference continues once the primary resumes.
    async fn session(
        &self,
        packet_text: String,
        call_end: Instant,
        call: &mut DuckCall,
    ) -> Result<Verdict, DuckFailure> {
        let setup = &self.engine.setup;
        let names = setup.prep.tools.clone();
        let reads = Arc::new(
            ReadTools::new(
                None,
                self.context.clone(),
                self.github.clone(),
                self.repository_name.clone(),
                self.local_gate.clone(),
            )
            .with_facts(self.facts.clone()),
        );
        let exits = Arc::new(ExitTools::new(
            vec![Exit::SubmitVerdict],
            Arc::new(|_: &ExitCall| Ok(())),
            None,
        ));
        let create = self.engine.client.create_session(config_with(
            &setup.selection,
            reads.clone(),
            &names,
            exits.clone(),
            None,
            DUCK_SYSTEM,
        ));
        // No turn has started before the session exists: nothing to abort here.
        let session = tokio::select! {
            biased;
            created = create => created.map_err(|_| DuckFailure::Failed)?,
            failure = self.watch(call_end) => return Err(failure),
        };
        call.session_id = Some(session.id().to_string());
        let result = tokio::select! {
            biased;
            result = self.converse(&session, packet_text, &names, &reads, &exits) => result,
            failure = self.watch(call_end) => Err(failure),
        };
        if result.is_err() {
            let _ = tokio::time::timeout(Duration::from_secs(2), session.abort()).await;
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), session.disconnect()).await;
        let read = reads.read.lock().map(|r| r.len()).unwrap_or(0);
        call.coverage = Some(ReadCoverage {
            read: u32::try_from(read).unwrap_or(u32::MAX),
            changed: u32::try_from(self.context.files.len()).unwrap_or(u32::MAX),
        });
        let (events, verdict) = result?;
        call.input_tokens = events.0;
        call.output_tokens = events.1;
        call.tool_calls = events.2;
        Ok(verdict)
    }

    async fn converse(
        &self,
        session: &github_copilot_sdk::session::Session,
        packet_text: String,
        names: &[String],
        reads: &ReadTools<T>,
        exits: &ExitTools,
    ) -> Result<((u64, u64, u64), Verdict), DuckFailure> {
        let setup = &self.engine.setup;
        verified_intelligence(session, &setup.selection)
            .await
            .map_err(|_| DuckFailure::Failed)?;
        super::verify_catalog(session, names)
            .await
            .map_err(|_| DuckFailure::Failed)?;
        let mut subscription = session.subscribe();
        session
            .send(packet_text)
            .await
            .map_err(|error| classify(&Failure::sdk(error)))?;
        let mut events = Events::default();
        loop {
            let event = subscription.recv().await.map_err(|_| DuckFailure::Failed)?;
            events
                .push(&event.event_type, &event.data)
                .map_err(|failure| {
                    if failure.kind == OperationFailure::RateLimited {
                        (setup.rate_limited)(failure.retry_after_seconds);
                    }
                    classify(&failure)
                })?;
            if exits.state.take_failure().is_some() {
                return Err(DuckFailure::NoVerdict);
            }
            if event.event_type == "session.idle" {
                break;
            }
        }
        if reads.source_failure.lock().is_ok_and(|f| f.is_some())
            || reads.limit_failure.lock().is_ok_and(|f| f.is_some())
        {
            return Err(DuckFailure::Failed);
        }
        let model = setup.selection.agent.model.clone();
        let result = events
            .finish_exit(
                exits.state.take_exit(),
                session.id().to_string(),
                model,
                self.engine.version.clone(),
                |call| match call {
                    ExitCall::SubmitVerdict { verdict } => Ok(verdict),
                    _ => Err(Failure::schema()),
                },
            )
            .map_err(|failure| {
                if failure.kind == OperationFailure::Provider {
                    DuckFailure::NoVerdict
                } else {
                    DuckFailure::Failed
                }
            })?;
        Ok((
            (result.input_tokens, result.output_tokens, result.tool_calls),
            result.output,
        ))
    }
}

fn classify(failure: &Failure) -> DuckFailure {
    match failure.kind {
        OperationFailure::RateLimited => DuckFailure::RateLimited,
        OperationFailure::Timeout => DuckFailure::TimedOut,
        OperationFailure::Credentials | OperationFailure::Permission => DuckFailure::Unavailable,
        _ => DuckFailure::Failed,
    }
}

fn error(message: &str) -> ToolResult {
    ToolResult::Expanded(ToolResultExpanded::new(message, "failure").with_error(message))
}

#[async_trait]
impl<T: Transport + Send + Sync + 'static> ToolHandler for DuckTool<T> {
    async fn call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ToolResult, github_copilot_sdk::Error> {
        let number = self.tool_calls.fetch_add(1, Ordering::SeqCst) + 1;
        let started = Instant::now();
        if let Some(trace) = &self.diagnostics {
            trace.emit(DiagnosticEvent::ToolStarted {
                call: number,
                runtime_call_id: Some(invocation.tool_call_id.clone()),
                tool: ToolName::RubberDuck,
                requested: Paths::new([]),
                unknown_paths: 0,
                side: None,
                query_bytes: Some(invocation.arguments.to_string().len()),
            });
        }
        let outcome = self.consult(&invocation.arguments).await;
        if let Some(trace) = &self.diagnostics {
            trace.emit(DiagnosticEvent::ToolFinished {
                call: number,
                runtime_call_id: Some(invocation.tool_call_id),
                tool: ToolName::RubberDuck,
                duration_ms: started.elapsed().as_millis() as u64,
                response_bytes: outcome.as_ref().ok().map(String::len),
                response_limit_bytes: super::MAX_TOOL_BYTES,
                rejected_by_limit: false,
                returned: Paths::new([]),
                covered: Paths::new([]),
                failure: outcome.is_err().then_some(FailureKind::ToolError),
            });
        }
        Ok(match outcome {
            Ok(text) => ToolResult::Text(text),
            Err(message) => error(&message),
        })
    }
}
