//! The primary's `rubber_duck` tool. Each call runs one fresh duck session on the duck's
//! own Copilot client, inside the primary's attempt: same AI slot, same deadline, same
//! cancellation. Calls are serialized, bounded and recorded. A failed call is a typed tool
//! error for the primary, never a run failure; a host panic in a call ends the run.

use super::{
    config_with, exit_tools::ExitTools, verified_intelligence, ExitState, Gate, HostFailure,
    ReadTools, DUCK_SYSTEM,
};
use crate::{
    copilot::operation::Operation,
    exits::{Exit, ExitCall},
    github::Identity,
    monitoring::OperationFailure,
    packet::{Directive, PacketInput, ToolFacts},
    provider::ReviewContext,
    review::{
        duck::{self, DuckCall, DuckFailure, DuckInput, DuckOutcome, DuckReason, Verdict},
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
    /// Held for the whole of a call; teardown takes it to let an in-flight call settle.
    pub serial: tokio::sync::Mutex<()>,
    /// Set when the primary attempt is ending for any reason: an in-flight call stops
    /// at once (abort, close, record) and no new call starts.
    pub ending: std::sync::atomic::AtomicBool,
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
            serial: tokio::sync::Mutex::new(()),
            ending: std::sync::atomic::AtomicBool::new(false),
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

pub(super) struct DuckTool {
    pub engine: Arc<DuckEngine>,
    pub context: ReviewContext,
    pub github: Arc<dyn crate::provider::ReviewSource + Send + Sync>,
    pub repository_name: String,
    pub facts: Arc<ToolFacts>,
    pub primary_exit: Arc<ExitState>,
    /// The primary attempt's deadline; duck time counts against it.
    pub deadline: Instant,
    pub operation: Operation,
    pub local_gate: Gate,
    pub diagnostics: Option<Arc<crate::review::diagnostics::Trace>>,
    progress: Option<Arc<super::Watchdog>>,
    /// The primary attempt's host-panic latch: a panic in a duck call ends the run.
    host_failure: Arc<HostFailure>,
    started: AtomicU32,
    tool_calls: AtomicU64,
    /// Every path any duck session of this attempt was served.
    read: Mutex<std::collections::BTreeSet<String>>,
    /// The primary's own served-evidence check: a verify_fixed claim it could not
    /// use for `fixed` is refused before a call is spent.
    claim_evidence: Option<ClaimEvidence>,
    /// Calls run as their own task so dropping the primary's handler cannot drop a
    /// call mid-turn: it still aborts its turn and records itself.
    me: std::sync::Weak<Self>,
}

/// Whether the primary was served one evidence ref in this attempt.
pub(super) type ClaimEvidence =
    Arc<dyn Fn(&duck::EvidenceRef) -> Result<(), duck::Unserved> + Send + Sync>;

impl DuckTool {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: Arc<DuckEngine>,
        context: ReviewContext,
        github: Arc<dyn crate::provider::ReviewSource + Send + Sync>,
        repository_name: String,
        facts: Arc<ToolFacts>,
        primary_exit: Arc<ExitState>,
        operation: Operation,
        local_gate: Gate,
        diagnostics: Option<Arc<crate::review::diagnostics::Trace>>,
        progress: Option<Arc<super::Watchdog>>,
        host_failure: Arc<HostFailure>,
        claim_evidence: Option<ClaimEvidence>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
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
            progress,
            host_failure,
            started: AtomicU32::new(0),
            tool_calls: AtomicU64::new(0),
            read: Default::default(),
            claim_evidence,
            me: me.clone(),
        })
    }

    pub fn read_paths(&self) -> std::collections::BTreeSet<String> {
        self.read.lock().map(|r| r.clone()).unwrap_or_default()
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
        if let Some(trace) = &self.diagnostics {
            trace.emit(DiagnosticEvent::Duck {
                ordinal: call.ordinal,
                outcome: match &call.outcome {
                    DuckOutcome::Verdict { verdict } => Some(verdict.verdict),
                    _ => None,
                },
                failure: match &call.outcome {
                    DuckOutcome::Failure { failure } => Some(*failure),
                    _ => None,
                },
                duration_ms: call.session_id.as_ref().map(|_| call.duration_ms),
                session_id: call.session_id.clone(),
                reason: call.reason,
                input_tokens: call.session_id.as_ref().map(|_| call.input_tokens),
                output_tokens: call.session_id.as_ref().map(|_| call.output_tokens),
                tool_calls: call.session_id.as_ref().map(|_| call.tool_calls),
            });
        }
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
            limits: DuckLimits {
                call_timeout_seconds: self.call_timeout_seconds(),
                ..setup.limits
            },
            agent: setup.selection.agent.name.clone(),
            model: setup.selection.agent.model.clone(),
            state_id: setup.prep.base.state_id.clone(),
            packet: None,
            session_id: None,
            reason: None,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
            coverage: None,
            duration_ms: 0,
            repeats: 0,
            others: Default::default(),
        }
    }

    /// The per-call timeout for this PR: the saved preference, scaled by PR size.
    fn call_timeout_seconds(&self) -> u32 {
        duck::call_timeout(
            self.engine.setup.limits.call_timeout_seconds,
            self.context.files.len(),
        )
    }

    /// The whole original thread when small enough to serve inside the duck packet.
    fn inline_thread(&self, input: &DuckInput) -> Option<(String, String)> {
        let id = input.thread_id.as_ref().filter(|_| input.verify_fixed)?;
        let thread = self
            .facts
            .conversation
            .threads
            .iter()
            .find(|t| &t.id == id)?;
        let text = crate::review::tools::thread_text(thread);
        (text.len() <= duck::MAX_INLINE_THREAD_BYTES).then(|| (id.clone(), text))
    }

    /// One call, start to finish. `Err` is the typed failure the primary sees.
    pub async fn consult(&self, args: &serde_json::Value) -> Result<String, String> {
        if self.primary_exit.taken() {
            self.refused(DuckFailure::ExitTaken);
            return Err(DuckFailure::ExitTaken.message(None));
        }
        let setup = &self.engine.setup;
        let input =
            DuckInput::parse(args, &setup.prep.changed, &setup.prep.threads).map_err(|reason| {
                self.refused(DuckFailure::InvalidInput);
                DuckFailure::InvalidInput.message(Some(&reason))
            })?;
        if let Some(reason) = self.unusable_claim(&input) {
            self.refused(DuckFailure::InvalidInput);
            return Err(DuckFailure::InvalidInput.message(Some(&reason)));
        }
        let _serial = self.engine.serial.lock().await;
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
        let timeout = Duration::from_secs(u64::from(self.call_timeout_seconds()));
        if available < timeout.min(Duration::from_secs(duck::MIN_CALL_SECONDS)) {
            return refuse(DuckFailure::BudgetExhausted, input);
        }
        let call_end = now + timeout.min(available);
        if (setup.gated)() {
            return refuse(DuckFailure::RateLimited, input);
        }
        if !self.engine.account_ok() || self.engine.ending.load(Ordering::SeqCst) {
            return refuse(DuckFailure::Unavailable, input);
        }
        self.started.store(ordinal, Ordering::SeqCst);
        let mut call = self.blank(ordinal, input, DuckFailure::Failed);
        let inline = self.inline_thread(&call.input);
        let packet = crate::packet::duck::build(&crate::packet::duck::DuckPacketInput {
            base: &setup.prep.base,
            primary_hash: &setup.prep.primary_hash,
            ordinal,
            thread: inline.as_ref().map(|(_, text)| text.as_str()),
            claim: &call.input,
            directive: &setup.prep.directive,
            tools: &setup.prep.tools,
            created_at: &chrono::Utc::now().to_rfc3339(),
        });
        call.packet = Some(packet.record.clone());
        let started = Instant::now();
        let outcome = self.session(packet.text, call_end, &mut call, inline).await;
        call.duration_ms = started.elapsed().as_millis() as u64;
        let result = match outcome {
            Ok(verdict) => {
                let text = duck::result_text(&call, &verdict);
                call.outcome = DuckOutcome::Verdict { verdict };
                Ok(text)
            }
            Err((failure, reason)) => {
                call.outcome = DuckOutcome::Failure { failure };
                call.reason = reason;
                Err(failure.explained(reason))
            }
        };
        self.record(call);
        result
    }

    /// A verify_fixed claim whose evidence `fixed` would refuse: no call is spent.
    fn unusable_claim(&self, input: &DuckInput) -> Option<String> {
        let check = self
            .claim_evidence
            .as_ref()
            .filter(|_| input.verify_fixed)?;
        if input.evidence.is_empty() {
            return Some("verify_fixed needs the current-head evidence you read; fixed is refused without it. No duck call was spent.".into());
        }
        duck::unserved(&input.evidence, |e| check(e)).map(|reason| {
            format!("{reason} fixed would be refused with this evidence. No duck call was spent.")
        })
    }

    fn refused(&self, failure: DuckFailure) {
        if let Some(trace) = &self.diagnostics {
            trace.emit(DiagnosticEvent::Duck {
                ordinal: self.started.load(Ordering::SeqCst) + 1,
                outcome: None,
                failure: Some(failure),
                duration_ms: None,
                session_id: None,
                reason: None,
                input_tokens: None,
                output_tokens: None,
                tool_calls: None,
            });
        }
    }

    /// Ends a call that outlives its window or its primary.
    async fn watch(&self, call_end: Instant) -> DuckFailure {
        loop {
            if Instant::now() >= call_end {
                return DuckFailure::TimedOut;
            }
            if self.primary_exit.taken() {
                return DuckFailure::ExitTaken;
            }
            if self.engine.ending.load(Ordering::SeqCst) {
                return DuckFailure::Unavailable;
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
        inline: Option<(String, String)>,
    ) -> Result<Verdict, Fail> {
        let setup = &self.engine.setup;
        let diagnostics = self
            .diagnostics
            .as_ref()
            .map(|trace| trace.duck(call.ordinal, &setup.selection.agent.model));
        if let Some(trace) = &diagnostics {
            trace.runtime(self.engine.version.clone());
        }
        let names = setup.prep.tools.clone();
        let reads = Arc::new(
            ReadTools::new(
                diagnostics.clone(),
                self.context.clone(),
                self.github.clone(),
                self.repository_name.clone(),
                self.local_gate.clone(),
            )
            .with_facts(self.facts.clone())
            .with_progress(self.progress.clone())
            .with_host_failure(self.host_failure.clone()),
        );
        if let Some((id, text)) = &inline {
            reads.seed_thread(id, text.len());
        }
        // verify_fixed: a confirmed verdict needs the whole original thread and served
        // head lines. Refused at the exit so the duck can read and resubmit.
        let refused: Arc<Mutex<Option<DuckReason>>> = Default::default();
        let verdict_gate: super::exit_tools::ExitGate = {
            let reads = reads.clone();
            let input = call.input.clone();
            let refused = refused.clone();
            Arc::new(move |exit: &ExitCall| {
                let ExitCall::SubmitVerdict { verdict } = exit else {
                    return Ok(());
                };
                #[cfg(test)]
                if verdict.nano == super::duck_tests::PANIC_VERDICT {
                    let empty: Vec<bool> = Vec::new();
                    let _ = empty[1];
                }
                match unproven(
                    &input,
                    verdict,
                    |e| reads.served_evidence(e),
                    |id| reads.thread_read(id),
                ) {
                    None => Ok(()),
                    Some(reason) => {
                        if let Ok(mut slot) = refused.lock() {
                            *slot = Some(reason);
                        }
                        Err(super::exit_tools::Refusal::Denied(
                            duck::refusal(reason).into(),
                        ))
                    }
                }
            })
        };
        let exits = Arc::new(
            ExitTools::new(vec![Exit::SubmitVerdict], verdict_gate, diagnostics.clone())
                .with_host_failure(self.host_failure.clone()),
        );
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
            created = create => created.map_err(|_| (DuckFailure::Failed, Some(DuckReason::SessionCreate)))?,
            failure = self.watch(call_end) => return Err((failure, None)),
        };
        call.session_id = Some(session.id().to_string());
        if let Some(trace) = &diagnostics {
            trace.session(session.id().to_string());
            trace.emit(DiagnosticEvent::EvidenceUnavailable {
                runtime_truncation: true,
                runtime_stop_reason: true,
                inactivity_watchdog: true,
                connectivity_transitions: true,
            });
        }
        let usage = Mutex::new((0, 0, 0));
        let result = tokio::select! {
            biased;
            result = self.converse(&session, packet_text, &names, &reads, &exits, diagnostics.as_ref(), &usage) => result,
            failure = self.watch(call_end) => Err((failure, None)),
        };
        if let Ok(usage) = usage.lock() {
            (call.input_tokens, call.output_tokens, call.tool_calls) = *usage;
        }
        // A duck that ended after a refused confirmation gets that typed cause.
        let result = result.map_err(|(failure, reason)| match failure {
            DuckFailure::NoVerdict => match refused.lock().ok().and_then(|r| *r) {
                Some(refusal) => (DuckFailure::Unverified, Some(refusal)),
                None => (failure, reason),
            },
            _ => (failure, reason),
        });
        let abort = if result.is_err() {
            match tokio::time::timeout(Duration::from_secs(2), session.abort()).await {
                Ok(Ok(_)) => crate::storage::diagnostics::Outcome::Success,
                _ => crate::storage::diagnostics::Outcome::Failed,
            }
        } else {
            crate::storage::diagnostics::Outcome::NotRequired
        };
        let shutdown =
            match tokio::time::timeout(Duration::from_secs(2), session.disconnect()).await {
                Ok(Ok(_)) => crate::storage::diagnostics::Outcome::Success,
                _ => crate::storage::diagnostics::Outcome::Failed,
            };
        if let Some(trace) = &diagnostics {
            trace.emit(DiagnosticEvent::DuckTeardown {
                abort,
                disconnect: shutdown,
                tool_calls: Some(trace.tool_calls()),
            });
        }
        if let Ok(mut all) = self.read.lock() {
            all.extend(reads.touched_paths());
        }
        let read = reads.read.lock().map(|r| r.len()).unwrap_or(0);
        call.coverage = Some(ReadCoverage {
            read: u32::try_from(read).unwrap_or(u32::MAX),
            changed: u32::try_from(self.context.files.len()).unwrap_or(u32::MAX),
        });
        let (events, verdict) = result?;
        (call.input_tokens, call.output_tokens, call.tool_calls) = events;
        if let Some(reason) = unproven(
            &call.input,
            &verdict,
            |e| reads.served_evidence(e),
            |id| reads.thread_read(id),
        ) {
            return Err((DuckFailure::Unverified, Some(reason)));
        }
        Ok(verdict)
    }

    #[allow(clippy::too_many_arguments)]
    async fn converse(
        &self,
        session: &github_copilot_sdk::session::Session,
        packet_text: String,
        names: &[String],
        reads: &ReadTools,
        exits: &ExitTools,
        diagnostics: Option<&Arc<crate::review::diagnostics::Trace>>,
        usage: &Mutex<(u64, u64, u64)>,
    ) -> Result<((u64, u64, u64), Verdict), Fail> {
        let setup = &self.engine.setup;
        verified_intelligence(session, &setup.selection)
            .await
            .map_err(|_| (DuckFailure::Failed, Some(DuckReason::Intelligence)))?;
        super::verify_catalog(session, names)
            .await
            .map_err(|_| (DuckFailure::Failed, Some(DuckReason::Catalog)))?;
        let mut subscription = session.subscribe();
        session
            .send(packet_text)
            .await
            .map_err(|error| reasoned(classify(&Failure::sdk(error)), DuckReason::Send))?;
        let mut events = Events::default();
        loop {
            let event = subscription.recv().await.map_err(|_| {
                if let Some(trace) = diagnostics {
                    trace.emit(DiagnosticEvent::EventStreamLost { lost_events: None });
                }
                (DuckFailure::Failed, Some(DuckReason::StreamLost))
            })?;
            if let Some(trace) = diagnostics {
                trace.event(&event.event_type, &event.data);
            }
            if let Some(progress) = &self.progress {
                progress.event(&event.event_type, &event.data);
            }
            let pushed = events.push(&event.event_type, &event.data);
            if let Ok(mut usage) = usage.lock() {
                *usage = events.usage();
            }
            pushed.map_err(|failure| {
                if failure.kind == OperationFailure::RateLimited {
                    (setup.rate_limited)(failure.retry_after_seconds);
                }
                (classify(&failure), None)
            })?;
            if exits.state.take_failure().is_some() {
                return Err((DuckFailure::NoVerdict, Some(DuckReason::ExitInvalid)));
            }
            if event.event_type == "session.idle" {
                break;
            }
        }
        if reads.source_failure.lock().is_ok_and(|f| f.is_some()) {
            return Err((DuckFailure::Failed, Some(DuckReason::SourceRead)));
        }
        if reads.limit_failure.lock().is_ok_and(|f| f.is_some()) {
            return Err((DuckFailure::Failed, Some(DuckReason::ReadLimit)));
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
                    (DuckFailure::NoVerdict, None)
                } else {
                    (DuckFailure::Failed, Some(DuckReason::Task))
                }
            })?;
        Ok((
            (result.input_tokens, result.output_tokens, result.tool_calls),
            result.output,
        ))
    }
}

/// A call failure and its typed cause, when known.
type Fail = (DuckFailure, Option<DuckReason>);

fn reasoned(failure: DuckFailure, reason: DuckReason) -> Fail {
    (failure, (failure == DuckFailure::Failed).then_some(reason))
}

/// Why a verify_fixed `confirmed` verdict is not proven by what the duck was served.
fn unproven(
    input: &DuckInput,
    verdict: &Verdict,
    served: impl Fn(&duck::EvidenceRef) -> bool,
    thread_read: impl Fn(&str) -> bool,
) -> Option<DuckReason> {
    if !input.verify_fixed || verdict.verdict != duck::VerdictKind::Confirmed {
        return None;
    }
    if !input.thread_id.as_deref().is_some_and(thread_read) {
        return Some(DuckReason::ThreadUnread);
    }
    if verdict.evidence.is_empty() {
        return Some(DuckReason::EvidenceMissing);
    }
    if !verdict.evidence.iter().all(served) {
        return Some(DuckReason::EvidenceUnserved);
    }
    None
}

fn classify(failure: &Failure) -> DuckFailure {
    match failure.kind {
        OperationFailure::RateLimited => DuckFailure::RateLimited,
        OperationFailure::Timeout => DuckFailure::TimedOut,
        OperationFailure::Credentials | OperationFailure::Permission | OperationFailure::Model => {
            DuckFailure::Unavailable
        }
        _ => DuckFailure::Failed,
    }
}

fn error(message: &str) -> ToolResult {
    ToolResult::Expanded(ToolResultExpanded::new(message, "failure").with_error(message))
}

#[async_trait]
impl ToolHandler for DuckTool {
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
        let outcome = match self.me.upgrade() {
            Some(this) => {
                let args = invocation.arguments.clone();
                match tokio::spawn(async move {
                    crate::host_panic::catch_future(this.consult(&args)).await
                })
                .await
                {
                    Ok(Ok(outcome)) => outcome,
                    Ok(Err(panic)) => {
                        if let Some(trace) = &self.diagnostics {
                            trace.host_panic(&panic);
                        }
                        self.host_failure.raise(Failure::host_panic(&panic));
                        Err("Host fault. The host ends this run. Stop.".into())
                    }
                    Err(_) => Err(DuckFailure::Failed.message(None)),
                }
            }
            None => Err(DuckFailure::Unavailable.message(None)),
        };
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
