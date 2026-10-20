//! Live end-to-end checks of the review-packet path against the disposable sandbox
//! repository and the real Copilot runtime. Never in CI: every test is `#[ignore]`
//! and returns early unless `PR_SNIPER_LIVE=1`.
//!
//! Drives production code: `GithubClient` over HTTPS, `pr_snapshot`, the heartbeat
//! (`PrStates::observe`), `review::host::request`/`prepare_dispatch`,
//! `packet_run::prepare`, `review::runtime::run`, `review::host::complete`,
//! `publication::execute` with `Batch::prepare_exit`, `exit_writes::run` and
//! `exits::consume`. Only the tauri-bound glue (Host locks, generations) is replaced
//! by thin environments over the same client calls.
//!
//! Run (owner-authorized sandbox only):
//! `PR_SNIPER_LIVE=1 cargo test --locked live_tests -- --ignored --nocapture --test-threads=1`
//! Optional: `PR_SNIPER_LIVE_MODEL` (default `gpt-5-mini`), `PR_SNIPER_LIVE_STORE`
//! (store root; default under the target directory).

use crate::{
    exit_writes::{self, Dependency, ExitWrite},
    exits::{consume, ExitOutcome},
    github::{
        http::HttpTransport,
        oauth::TokenPair,
        provider::{GithubClient, RemoteRepository},
        threads::{Ownership, Provenance, Thread},
        Identity,
    },
    monitoring::{self, OperationState, QueueJob},
    pr_state::{self, Change, Consumed},
    publication::{self, Batch, Gate, Mutation, Publication, Receipt, WriteFailure},
    retention::Binding,
    review::{
        self, host, packet_run,
        runtime::{self, ExitReview, Refusal},
        Failure, ReviewRun,
    },
    storage::{Settings, Store},
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::Command,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::{Duration, Instant},
};

const SANDBOX: &str = "jdylanmc/pr-sniper-sandbox";
const SANDBOX_ID: &str = "1416199617";
const LOGIN: &str = "jdylanmc";
const CONFIG: &str = "5a7d0000-0000-4000-8000-000000000001";
const PRIMARY_AGENT: &str = "5a7d0000-0000-4000-8000-0000000000a1";
const TEAMMATE_AGENT: &str = "5a7d0000-0000-4000-8000-0000000000a2";
const PRIMARY: &str = "5a7d0000-0000-4000-8000-0000000000c1";
const TEAMMATE: &str = "5a7d0000-0000-4000-8000-0000000000c2";

type Client = GithubClient<HttpTransport>;

struct Live {
    client: Arc<Client>,
    /// Copilot (AI) account: token mode, like the app's separate Copilot sign-in.
    pair: TokenPair,
    copilot: Identity,
    identity: Identity,
    repo: RemoteRepository,
    model: String,
    root: PathBuf,
    store: Mutex<Store>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn token(user: &str) -> TokenPair {
    let output = Command::new("gh")
        .args(["auth", "token", "--user", user])
        .output()
        .expect("gh auth token");
    assert!(output.status.success(), "gh has no token for {user}");
    TokenPair::new(
        String::from_utf8(output.stdout).unwrap().trim(),
        "live-test-no-refresh",
        Duration::from_secs(8 * 3600),
        Duration::from_secs(8 * 3600),
    )
}

fn live(name: &str) -> Option<Live> {
    if std::env::var("PR_SNIPER_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: PR_SNIPER_LIVE is not 1");
        return None;
    }
    let client = GithubClient::new(HttpTransport::from_token_pair(&token(LOGIN)).unwrap());
    let identity = client.current_identity().expect("identity");
    assert_eq!(identity.login, LOGIN);
    // jdylanmc has Copilot Free (model list: `auto` only); the AI account is separate.
    let copilot_user = std::env::var("PR_SNIPER_LIVE_COPILOT_USER")
        .unwrap_or_else(|_| "dylanmccurry_microsoft".into());
    let pair = token(&copilot_user);
    let copilot = GithubClient::new(HttpTransport::from_token_pair(&pair).unwrap())
        .current_identity()
        .expect("copilot identity");
    let base = std::env::var("PR_SNIPER_LIVE_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/live-store"));
    let root = base.join(format!("{name}-{}", now()));
    std::fs::create_dir_all(&root).unwrap();
    let store = Store::new(root.clone());
    let live = Live {
        client: Arc::new(client),
        pair,
        copilot,
        identity,
        repo: RemoteRepository {
            id: SANDBOX_ID.into(),
            name: SANDBOX.into(),
        },
        model: std::env::var("PR_SNIPER_LIVE_MODEL").unwrap_or_else(|_| "gpt-5-mini".into()),
        root,
        store: Mutex::new(store),
    };
    live.save_settings(true);
    Some(live)
}

fn assignment(id: &str, agent: &str, primary: bool) -> Value {
    let schedule = json!({"kind":"interval","minutes":5,"timezone":"UTC"});
    if primary {
        json!({"id":id,"agent_id":agent,"schedule":schedule,"comment":true,"approve":false,
            "actions":{"reply":true,"approve":true,"merge":false},
            "tools":["read_changes","read_source","get_thread","get_checks","read_check_log","search_code",
                "leave_comments","ready_for_human","approve_pull_request","panic_exit"]})
    } else {
        json!({"id":id,"agent_id":agent,"schedule":schedule,"comment":false,"approve":false,
            "tools":["read_changes","read_source","search_code","submit_report","panic_exit"]})
    }
}

fn agent(id: &str, name: &str, model: &str, account: &str, prompt: &str) -> Value {
    json!({"id":id,"name":name,"model":model,
        "ai_account":{"provider":"copilot","account_id":account},
        "prompt":prompt,"signature":"PR Sniper live test"})
}

impl Live {
    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap()
    }

    fn save_settings(&self, teammate: bool) {
        let mut assignments = vec![assignment(PRIMARY, PRIMARY_AGENT, true)];
        let mut agents = vec![agent(
            PRIMARY_AGENT,
            "Primary",
            &self.model,
            &self.copilot.id,
            "Review for correctness and security. Be brief.",
        )];
        if teammate {
            assignments.push(assignment(TEAMMATE, TEAMMATE_AGENT, false));
            agents.push(agent(
                TEAMMATE_AGENT,
                "Bug hunter",
                &self.model,
                &self.copilot.id,
                "Hunt logic bugs and unsafe calls. Report each with file and line.",
            ));
        }
        let mut settings: Settings = serde_json::from_value(json!({
            "launch_at_login":false,"doctrines":[],
            "repositories":[{"id":CONFIG,"provider":"github","name":SANDBOX,"enabled":true,
                "provider_account_id":self.identity.id,"provider_repository_id":SANDBOX_ID,
                "watched_authors":[{"id":self.identity.id,"login":LOGIN}],
                "assignments":assignments,"primary_assignment_id":PRIMARY}],
            "agents":agents
        }))
        .unwrap();
        settings.defaults.automatic_agent_start = false;
        let store = self.store();
        store.save_settings(&settings).unwrap();
        // A first load may migrate admission rules and pause monitoring; re-enable.
        let mut loaded = store.load_settings().unwrap();
        if !loaded.repositories[0].enabled {
            loaded.repositories[0].enabled = true;
            store.save_settings(&loaded).unwrap();
        }
    }

    /// Poll step of the follow-up scan: record owned threads of published batches
    /// (`Ledger::observe`), which the next dispatch of this PR waits for.
    fn observe_feedback(&self, number: u64) {
        let head = self
            .client
            .review_pull(&self.repo, number)
            .unwrap()
            .head_sha;
        let store = self.store();
        let publications = store.load_publications().unwrap();
        let mut ledger = store.load_feedback().unwrap();
        for origin in publications
            .iter()
            .filter(|p| p.review.job.number == number && p.phase == publication::Phase::Published)
        {
            let threads = self.client.owned_threads_at(origin, &head).unwrap();
            ledger.observe(origin, &head, &threads).unwrap();
        }
        store.save_feedback(&ledger).unwrap();
    }

    /// Heartbeat: observe the PR's normalized snapshot; returns the change and seq.
    fn heartbeat(&self, number: u64, new_iteration: bool) -> (Change, u64, String) {
        self.observe_feedback(number);
        let snapshot = self.client.pr_snapshot(&self.repo, number).unwrap();
        let normalized = snapshot.normalize();
        let hash = normalized.hash();
        let binding = self.binding(number, &snapshot.head);
        let store = self.store();
        let mut states = store.load_pr_states().unwrap();
        let change = states.observe(
            &binding,
            pr_state::Observation {
                iteration_id: &snapshot.head,
                new_iteration,
                snapshot: Some(&normalized),
                already_reviewed: false,
                now: now(),
            },
        );
        let seq = states.state_seq(&binding);
        store.save_pr_states(&states).unwrap();
        (change, seq, hash)
    }

    fn binding(&self, number: u64, head: &str) -> Binding {
        let pull = self.client.review_pull(&self.repo, number).unwrap();
        Binding::job(&self.job(&pull, PRIMARY, PRIMARY_AGENT, 0, head))
    }

    fn job(
        &self,
        pull: &crate::github::metadata::PullRequest,
        assignment: &str,
        agent: &str,
        seq: u64,
        iteration: &str,
    ) -> QueueJob {
        serde_json::from_value(json!({
            "work":{"id":format!("live-{}-{assignment}-{seq}", pull.number),
                "item_id":format!("live-item-{}", pull.number),"iteration_id":iteration,
                "iteration":1,"agent_id":agent,"enqueue_order":seq,"pass_ordinal":1,
                "trigger":"admission","admission":{"watched_author":true,"all_authors":false,
                "requested_reviewer":false},"state_seq":seq},
            "assignment_id":assignment,"provider":"github","account_id":self.identity.id,
            "account_login":LOGIN,"configuration_id":CONFIG,"repository_id":SANDBOX_ID,
            "repository_name":SANDBOX,"pull_request_id":pull.id,"number":pull.number,
            "title":pull.title,"head_sha":pull.head_sha,"observed_base_sha":pull.base_sha,
            "trigger_policy":"live","author_id":pull.author.as_ref().map(|a| a.id.clone()),
            "author_login":pull.author.as_ref().map(|a| a.login.clone()),
            "watched_author":true,"all_authors":false,"requested_reviewer":false,
            "waiting":monitoring::WAITING_HUMAN_START,"detected_at":now()
        }))
        .unwrap()
    }

    /// Queue one job per assignment for the PR's current state (same work item and seq).
    fn enqueue(&self, number: u64, seq: u64, assignments: &[(&str, &str)]) -> Vec<String> {
        let pull = self.client.review_pull(&self.repo, number).unwrap();
        let store = self.store();
        let mut queue = store.load_queue().unwrap();
        let mut keys = vec![];
        for (assignment, agent) in assignments {
            let job = self.job(&pull, assignment, agent, seq, &pull.head_sha);
            keys.push(review::key(&job, assignment));
            queue.retain(|j| review::key(j, assignment) != review::key(&job, assignment));
            queue.push(job);
        }
        store.save_queue(&queue).unwrap();
        keys
    }

    /// Request + dispatch through the real host queue functions.
    fn dispatch(&self, key: &str) -> ReviewRun {
        let store = self.store();
        host::request(&store, key, true, now()).unwrap();
        let run = host::prepare_dispatch(&store, key, now()).unwrap();
        assert_eq!(run.mode, review::RunMode::Packet);
        run
    }
}

/// What one live run produced, for the results doc.
#[derive(Debug)]
struct Report {
    label: String,
    exit: String,
    exit_attempts: Vec<String>,
    tools: Vec<String>,
    packet_hash: String,
    packet_bytes: usize,
    exits_offered: Vec<String>,
    exits_denied: Vec<String>,
    input_tokens: u64,
    output_tokens: u64,
    tool_calls: u64,
    seconds: u64,
    outcome: Result<ReviewRun, String>,
    packet_text: String,
}

impl Report {
    fn print(&self) {
        println!(
            "LIVE {} | exit={} | attempts={:?} | tools={:?} | packet={}..({} B) | offered={:?} | denied={:?} | tokens in/out={}/{} | tool_calls={} | {}s",
            self.label,
            self.exit,
            self.exit_attempts,
            self.tools,
            &self.packet_hash[..self.packet_hash.len().min(12)],
            self.packet_bytes,
            self.exits_offered,
            self.exits_denied,
            self.input_tokens,
            self.output_tokens,
            self.tool_calls,
            self.seconds
        );
        if let Err(error) = &self.outcome {
            println!("LIVE {} | failure={error}", self.label);
        }
    }
}

/// The `execute_attempt` preparation + runtime + completion, minus the tauri Host.
fn execute(live: &Live, run: ReviewRun, label: &str) -> Report {
    let started = Instant::now();
    let snapshot = live.client.pr_snapshot(&live.repo, run.job.number).unwrap();
    assert_eq!(snapshot.state, "OPEN");
    assert_eq!(snapshot.head, run.job.head_sha);
    let context = live
        .client
        .review_context(&live.repo, run.job.number, &run.job.head_sha)
        .unwrap();
    let write_capability = live
        .client
        .connect(&live.repo.name, Some(&run.job.account_id))
        .unwrap()
        .capabilities
        .comment
        == crate::github::provider::CommentCapability::Available;
    let (teammates, iterations, last_consumed_at) = {
        let store = live.store();
        (
            review::teammates::entries(&store, &run).unwrap(),
            exit_writes::known_iterations(&store, &run.job).unwrap_or_default(),
            store
                .load_pr_states()
                .unwrap()
                .get(&Binding::job(&run.job))
                .and_then(|r| r.consumed_at),
        )
    };
    let mut prepared = packet_run::prepare(
        &live.client,
        &live.repo,
        &run,
        &context,
        &snapshot,
        packet_run::Caller {
            acting_login: &live.identity.login,
            ownership: crate::review::findings::proofs(&live.store(), &run.job, &run.assignment_id)
                .unwrap(),
            write_capability,
            teammates,
            iterations: &iterations,
            last_consumed_at,
        },
    )
    .unwrap();
    let record = prepared.packet.record.clone();
    let packet_text = prepared.packet.text.clone();
    {
        let store = live.store();
        let mut reviews = store.load_reviews().unwrap();
        let current = reviews
            .iter_mut()
            .find(|r| r.operation.id == run.operation.id)
            .unwrap();
        current.packet = Some(record.clone());
        store.save_reviews(&reviews).unwrap();
    }
    std::fs::write(live.root.join(format!("{label}.packet.txt")), &packet_text).unwrap();
    // Record every exit attempt and the gate verdict; the gate itself is production.
    let attempts = Arc::new(Mutex::new(Vec::<String>::new()));
    let inner = prepared.setup.gate.clone();
    let log = attempts.clone();
    prepared.setup.gate = Arc::new(move |call| {
        let verdict = inner(call);
        log.lock().unwrap().push(format!(
            "{}:{}",
            call.exit().id(),
            match &verdict {
                Ok(()) => "ok".to_string(),
                Err(Refusal::Denied(r)) => format!("denied({r})"),
                Err(Refusal::Moved(r)) => format!("moved({r})"),
            }
        ));
        verdict
    });
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sink_events = events.clone();
    let trace = review::diagnostics::Trace::new(
        review::diagnostics::attempt(&run.job, &run.operation, &run.selection.agent.model),
        Arc::new(move |record| {
            sink_events
                .lock()
                .unwrap()
                .push(serde_json::to_value(&record.event).unwrap_or(Value::Null));
            Ok(())
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(600);
    let operation = crate::copilot::operation::Operation::new(
        Arc::new(crate::copilot::operation::AccountWork::default()),
        Arc::new(AtomicBool::new(false)),
        deadline,
    )
    .unwrap();
    let allow: runtime::Gate = Arc::new(|| Ok(()));
    let request = runtime::Request {
        inactivity: Duration::from_secs(900),
        exits: prepared.setup,
        diagnostics: Some(trace),
        task: ExitReview {
            packet_text: prepared.packet.text,
        },
        context,
        client: live.client.clone(),
        repository_name: run.job.repository_name.clone(),
        selection: run.selection.clone(),
        before_send: allow.clone(),
        local_gate: allow,
        duck: None,
    };
    let result = runtime::run(&live.copilot, &live.pair, &operation, request);
    let base = snapshot.base.clone();
    // Host check for a local-only exit: the state it read must still be the state.
    let result = result.and_then(|mut result| {
        if let Some(ExitOutcome::ReadyForHuman { .. }) = &result.output.exit {
            let fresh = consume::fresh_hash(&live.client, &live.repo, run.job.number)
                .map_err(Failure::permanent)?;
            if Some(fresh) != record.state_hash {
                return Err(Failure::tossed("PR state moved during the run."));
            }
        }
        result.reviewed_base_sha = Some(base);
        Ok(result)
    });
    let (exit, input_tokens, output_tokens, tool_calls) = match &result {
        Ok(r) => (
            r.output
                .exit
                .as_ref()
                .map(|e| serde_json::to_value(e).unwrap()["exit"].clone())
                .map_or("none".into(), |v| v.as_str().unwrap_or("?").to_string()),
            r.input_tokens,
            r.output_tokens,
            r.tool_calls,
        ),
        Err(e) => (format!("error:{:?}", e.kind), 0, 0, 0),
    };
    let outcome = {
        let store = live.store();
        host::complete(&store, &run.operation.id, result, now()).unwrap();
        let saved = store
            .load_reviews()
            .unwrap()
            .into_iter()
            .find(|r| r.operation.id == run.operation.id)
            .unwrap();
        if saved.operation.state == OperationState::Completed {
            Ok(saved)
        } else {
            Err(format!(
                "{:?}: {}",
                saved.operation.state,
                saved.error.clone().unwrap_or_default()
            ))
        }
    };
    let tools = events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "tool_started" || e["type"] == "tool_started")
        .map(|e| e["tool"].as_str().unwrap_or("?").to_string())
        .collect();
    std::fs::write(
        live.root.join(format!("{label}.events.json")),
        serde_json::to_vec_pretty(&*events.lock().unwrap()).unwrap(),
    )
    .unwrap();
    let report = Report {
        label: label.into(),
        exit,
        exit_attempts: attempts.lock().unwrap().clone(),
        tools,
        packet_hash: record.hash.clone(),
        packet_bytes: packet_text.len(),
        exits_offered: record
            .exits_offered
            .iter()
            .map(|e| e.id().to_string())
            .collect(),
        exits_denied: record
            .exits_denied
            .iter()
            .map(|d| format!("{}: {}", d.exit.id(), d.reason))
            .collect(),
        input_tokens,
        output_tokens,
        tool_calls,
        seconds: started.elapsed().as_secs(),
        outcome,
        packet_text,
    };
    report.print();
    report
}

/// Publication environment over the real client: no tauri Host, same calls.
struct Publish<'a> {
    live: &'a Live,
}

impl publication::Environment for Publish<'_> {
    fn now(&self) -> Result<i64, Failure> {
        Ok(now())
    }
    fn save(&mut self, run: &mut Publication) -> Result<(), Failure> {
        let store = self.live.store();
        let mut runs = store.load_publications().map_err(Failure::permanent)?;
        match runs.iter_mut().find(|p| p.id == run.id) {
            Some(saved) => *saved = run.clone(),
            None => runs.push(run.clone()),
        }
        store.save_publications(&runs).map_err(Failure::permanent)
    }
    fn inspect(&mut self, _: &Publication) -> Result<Gate, Failure> {
        Ok(Gate {
            stop: None,
            stale: false,
            requeue: false,
            discard: false,
        })
    }
    fn prepare(&mut self, run: &Publication) -> Result<Batch, Failure> {
        let context = self.live.client.review_context(
            &self.live.repo,
            run.review.job.number,
            &run.review.job.head_sha,
        )?;
        Batch::prepare(run, &context)
    }
    fn reconcile(&mut self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        self.live.client.reconcile_publication(run)
    }
    fn divergent(&mut self, run: &Publication) -> Result<Option<Receipt>, Failure> {
        self.live.client.divergent_publication(run)
    }
    fn mutate(&mut self, run: &Publication, mutation: Mutation) -> Result<Receipt, WriteFailure> {
        self.live.client.mutate_publication(run, mutation)
    }
    fn requeue(&mut self, _: &Publication) -> Result<(), Failure> {
        Ok(())
    }
    /// Same rule as the host: comments without replies consume here.
    fn confirmed(&mut self, run: &Publication) -> Result<(), Failure> {
        if run.review.mode != review::RunMode::Packet || exit_writes::plan(&run.review, 0).is_some()
        {
            return Ok(());
        }
        consume::consume_after(
            &self.live.store,
            &self.live.client,
            &self.live.repo,
            &run.review.job,
            &run.review.operation.id,
        )
        .map(|_| ())
        .map_err(Failure::permanent)
    }
}

/// Exit-writes environment over the real client.
struct Writes<'a> {
    live: &'a Live,
}

impl exit_writes::Environment for Writes<'_> {
    fn now(&self) -> Result<i64, Failure> {
        Ok(now())
    }
    fn save(&mut self, write: &mut ExitWrite) -> Result<(), Failure> {
        let store = self.live.store();
        let mut ledger = store.load_feedback().map_err(Failure::permanent)?;
        match ledger.exit_writes.iter_mut().find(|w| w.id == write.id) {
            Some(saved) => *saved = write.clone(),
            None => ledger.exit_writes.push(write.clone()),
        }
        store.save_feedback(&ledger).map_err(Failure::permanent)
    }
    fn gate(&mut self, write: &ExitWrite) -> Result<(), Failure> {
        let settings = self
            .live
            .store()
            .load_settings()
            .map_err(Failure::permanent)?;
        let repository = &settings.repositories[0];
        let allowed = repository
            .assignments
            .iter()
            .find(|a| a.id == write.assignment_id)
            .is_some_and(|a| repository.assignment_authority(a).reply);
        if !allowed {
            return Err(Failure::permanent(
                "Reply Comment permission is not granted.",
            ));
        }
        Ok(())
    }
    fn dependency(&mut self, write: &ExitWrite) -> Result<Dependency, Failure> {
        let publications = self
            .live
            .store()
            .load_publications()
            .map_err(Failure::permanent)?;
        Ok(exit_writes::publication_dependency(
            &publications,
            &write.review_operation_id,
        ))
    }
    fn head(&mut self, write: &ExitWrite) -> Result<String, Failure> {
        Ok(self
            .live
            .client
            .review_pull(&self.live.repo, write.job.number)?
            .head_sha)
    }
    fn proofs(&mut self, write: &ExitWrite) -> Result<Vec<Ownership>, Failure> {
        Ok(self
            .live
            .store()
            .load_publications()
            .map_err(Failure::permanent)?
            .iter()
            .filter(|p| crate::feedback::same_pr(&p.review.job, &write.job))
            .filter_map(|p| p.ownership().ok())
            .collect())
    }
    fn thread(&mut self, write: &ExitWrite, id: &str) -> Result<Option<Thread>, Failure> {
        Ok(self.live.client.review_thread(
            &write.job.repository_id,
            &write.job.pull_request_id,
            id,
        )?)
    }
    fn revision(&mut self, write: &ExitWrite, commit: Option<&str>) -> String {
        let known =
            exit_writes::known_iterations(&self.live.store(), &write.job).unwrap_or_default();
        exit_writes::targeted_revision(commit, &known)
    }
    fn reply(
        &mut self,
        write: &ExitWrite,
        root_id: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        self.live.client.reply_to_review_thread(
            &write.job.repository_name,
            write.job.number,
            &write.job.account_id,
            root_id,
            body,
        )
    }
    fn resolve(
        &mut self,
        _: &ExitWrite,
        thread_id: &str,
        request_id: &str,
    ) -> Result<(), WriteFailure> {
        self.live
            .client
            .resolve_review_thread(thread_id, request_id)
    }
    fn confirmed(&mut self, write: &ExitWrite) -> Result<(), Failure> {
        consume::consume_after(
            &self.live.store,
            &self.live.client,
            &self.live.repo,
            &write.job,
            &write.review_operation_id,
        )
        .map(|_| ())
        .map_err(Failure::permanent)
    }
}

/// Effects of a completed `leave_comments` exit in host order: preflight replies,
/// publish the batch, then replies and resolutions. Returns the publication, if any.
fn effects(live: &Live, review: &ReviewRun) -> (Option<Publication>, Option<ExitWrite>) {
    let mut write = exit_writes::plan(review, now());
    if let Some(write) = &mut write {
        let outcome = exit_writes::run(&mut Writes { live }, write);
        println!(
            "LIVE exit_writes preflight: {:?} phase={:?}",
            outcome.as_ref().map_err(|e| &e.message),
            write.phase
        );
    }
    let mut publication = None;
    let held = exit_writes::holds_publication(&live.store(), review).unwrap();
    if let Some(reason) = &held {
        println!("LIVE publication held: {reason}");
    }
    if held.is_none() && publication::writes_comments(review.result.as_ref().unwrap()) {
        let mut run = Publication::new(review.clone(), false, true, now()).unwrap();
        let outcome = publication::execute(&mut Publish { live }, &mut run);
        println!(
            "LIVE publication: {:?} phase={:?} receipts={}",
            outcome.as_ref().map_err(|e| &e.message),
            run.phase,
            run.receipts.len()
        );
        outcome.unwrap();
        publication = Some(run);
    }
    if let Some(write) = &mut write {
        if write.phase != exit_writes::Phase::Tossed {
            let outcome = exit_writes::run(&mut Writes { live }, write);
            println!(
                "LIVE exit_writes: {:?} phase={:?} steps={:?}",
                outcome.as_ref().map_err(|e| &e.message),
                write.phase,
                write
                    .steps
                    .iter()
                    .map(|s| (s.kind, s.state))
                    .collect::<Vec<_>>()
            );
        }
    }
    (publication, write)
}

fn gh(args: &[&str]) -> Value {
    let token = Command::new("gh")
        .args(["auth", "token", "--user", LOGIN])
        .output()
        .unwrap()
        .stdout;
    let token = String::from_utf8(token).unwrap();
    let mut last = String::new();
    for attempt in 0..3 {
        let output = Command::new("gh")
            .env("GH_TOKEN", token.trim())
            .args(args)
            .output()
            .unwrap();
        if output.status.success() {
            return serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        }
        last = String::from_utf8_lossy(&output.stderr).into_owned();
        eprintln!("LIVE gh retry {attempt}: {last}");
        std::thread::sleep(Duration::from_secs(3));
    }
    panic!("gh {args:?}: {last}")
}

fn review_comments(number: u64) -> Vec<Value> {
    gh(&[
        "api",
        &format!("repos/{SANDBOX}/pulls/{number}/comments?per_page=100"),
    ])
    .as_array()
    .cloned()
    .unwrap_or_default()
}

fn reviews(number: u64) -> Vec<Value> {
    gh(&[
        "api",
        &format!("repos/{SANDBOX}/pulls/{number}/reviews?per_page=100"),
    ])
    .as_array()
    .cloned()
    .unwrap_or_default()
}

fn excerpt(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with("<!--"))
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

/// After the consume hook: the next heartbeat sees the baseline, nothing new.
fn assert_own_writes_do_not_retrigger(live: &Live, number: u64) {
    let (change, _, hash) = live.heartbeat(number, false);
    let head = live
        .client
        .review_pull(&live.repo, number)
        .unwrap()
        .head_sha;
    let record = live
        .store()
        .load_pr_states()
        .unwrap()
        .get(&live.binding(number, &head))
        .cloned()
        .unwrap();
    println!(
        "LIVE heartbeat after own writes: {change:?} baseline={:?} now={}",
        record.baseline_hash.as_deref().map(|h| &h[..12]),
        &hash[..12]
    );
    assert_eq!(record.baseline_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(change, Change::None);
}

#[test]
#[ignore = "live: PR_SNIPER_LIVE=1, owner-authorized sandbox only"]
fn live_pr2_teammate_primary_publish_consume_and_reply() {
    let Some(live) = live("pr2") else { return };
    let number = std::env::var("PR_SNIPER_LIVE_PR")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2);
    let (change, seq, _) = live.heartbeat(number, true);
    assert!(matches!(change, Change::Pending(_)));
    let keys = live.enqueue(
        number,
        seq,
        &[(TEAMMATE, TEAMMATE_AGENT), (PRIMARY, PRIMARY_AGENT)],
    );

    // (a) Teammate ends on submit_report with findings.
    let teammate = execute(&live, live.dispatch(&keys[0]), "a-teammate-pr2");
    let teammate_run = teammate.outcome.as_ref().expect("teammate completed");
    let report = teammate_run.result.as_ref().unwrap().output.report.clone();
    let report = report.expect("teammate report stored");
    assert_eq!(teammate.exit, "submit_report");
    // Earlier runs left open threads on PR #2: points already raised are not repeated.
    let existing = review_comments(number);
    if existing.is_empty() {
        assert!(!report.findings.is_empty());
    }
    for f in &report.findings {
        println!("LIVE finding {}:{} {}", f.file, f.line, excerpt(&f.nano));
    }

    // (b) Primary with the teammate report, then publish + consume.
    let primary = execute(&live, live.dispatch(&keys[1]), "b-primary-pr2");
    assert!(
        primary.packet_text.contains("Bug hunter"),
        "teammate section"
    );
    let review_run = primary.outcome.clone().expect("primary completed");
    if !existing.is_empty() {
        no_repeat(&live, number, &existing, &review_run, &primary.exit);
        return;
    }
    let Some(ExitOutcome::Comments { new_comments, .. }) =
        review_run.result.as_ref().unwrap().output.exit.clone()
    else {
        panic!("expected leave_comments, got {}", primary.exit);
    };
    assert!(!new_comments.is_empty());
    let before = review_comments(number).len();
    let publication = effects(&live, &review_run).0.expect("published");
    assert_eq!(publication.phase, publication::Phase::Published);
    let posted: Vec<Value> = review_comments(number)
        .into_iter()
        .filter(|c| {
            c["body"]
                .as_str()
                .is_some_and(|b| b.contains(&format!("pr-sniper:comment:{}", publication.id)))
        })
        .collect();
    assert_eq!(posted.len(), new_comments.len());
    assert_eq!(review_comments(number).len(), before + new_comments.len());
    for (comment, asked) in posted.iter().zip(&new_comments) {
        assert_eq!(comment["path"], asked.path.as_str());
        assert_eq!(comment["line"].as_u64(), Some(asked.line));
        println!(
            "LIVE posted {}:{} {}",
            asked.path,
            asked.line,
            excerpt(comment["body"].as_str().unwrap())
        );
    }
    assert_own_writes_do_not_retrigger(&live, number);

    // (e) A human-like reply on the agent's own thread, then a new primary run.
    let root = &posted[0];
    gh(&[
        "api",
        "-X",
        "POST",
        &format!(
            "repos/{SANDBOX}/pulls/{number}/comments/{}/replies",
            root["id"]
        ),
        "-f",
        "body=Disagree: index 0 is a header row on purpose, so range(1, ...) is intended. Please confirm or drop this.",
    ]);
    // GitHub's PR record settles after a new comment; a read that races it is RevisionChanged.
    std::thread::sleep(std::time::Duration::from_secs(10));
    let (change, seq, _) = live.heartbeat(number, false);
    assert!(
        matches!(change, Change::Pending(_)),
        "a human reply is new state: {change:?}"
    );
    let keys = live.enqueue(
        number,
        seq,
        &[(TEAMMATE, TEAMMATE_AGENT), (PRIMARY, PRIMARY_AGENT)],
    );
    let teammate = execute(&live, live.dispatch(&keys[0]), "e-teammate-pr2");
    assert!(teammate.outcome.is_ok());
    let again = execute(&live, live.dispatch(&keys[1]), "e-primary-pr2");
    let again_run = again.outcome.clone().expect("second primary completed");
    match again_run.result.as_ref().unwrap().output.exit.clone() {
        Some(ExitOutcome::Comments {
            reply_comments,
            resolve_own_threads,
            new_comments,
            ..
        }) => {
            println!(
                "LIVE second exit: {} new, {} replies, {} resolves",
                new_comments.len(),
                reply_comments.len(),
                resolve_own_threads.len()
            );
            let before = review_comments(number).len();
            let (_, write) = effects(&live, &again_run);
            let after = review_comments(number);
            for c in after.iter().skip(before) {
                println!(
                    "LIVE new comment in_reply_to={} {}",
                    c["in_reply_to_id"],
                    excerpt(c["body"].as_str().unwrap_or(""))
                );
            }
            if write.is_some_and(|w| w.phase == exit_writes::Phase::Tossed) {
                assert_eq!(after.len(), before, "a tossed exit posts nothing");
            } else {
                assert_own_writes_do_not_retrigger(&live, number);
            }
        }
        other => println!("LIVE second exit (no writes): {other:?}"),
    }
}

/// Schema 2 rule: a point already in an open thread is not posted again as a new comment.
fn no_repeat(live: &Live, number: u64, existing: &[Value], run: &ReviewRun, exit: &str) {
    println!("LIVE primary on PR with open threads: exit={exit}");
    let Some(ExitOutcome::Comments { new_comments, .. }) =
        run.result.as_ref().unwrap().output.exit.clone()
    else {
        assert!(exit == "ready_for_human", "unexpected exit {exit}");
        return;
    };
    let taken: std::collections::BTreeSet<(String, u64)> = existing
        .iter()
        .filter(|c| c["in_reply_to_id"].is_null())
        .filter_map(|c| Some((c["path"].as_str()?.to_string(), c["line"].as_u64()?)))
        .collect();
    for c in &new_comments {
        println!("LIVE new {}:{} {}", c.path, c.line, excerpt(&c.body));
        assert!(
            !taken.contains(&(c.path.clone(), c.line)),
            "repeated a point on an open thread at {}:{}",
            c.path,
            c.line
        );
    }
    let before = review_comments(number).len();
    let (_, write) = effects(live, run);
    if write.is_some_and(|w| w.phase == exit_writes::Phase::Tossed) {
        assert_eq!(review_comments(number).len(), before);
    } else {
        assert_own_writes_do_not_retrigger(live, number);
    }
}

#[test]
#[ignore = "live: PR_SNIPER_LIVE=1, owner-authorized sandbox only"]
fn live_pr3_ci_red_denies_approve() {
    let Some(live) = live("pr3") else { return };
    live.save_settings(false);
    let number = 3;
    let before = (review_comments(number).len(), reviews(number).len());
    let (_, seq, _) = live.heartbeat(number, true);
    let keys = live.enqueue(number, seq, &[(PRIMARY, PRIMARY_AGENT)]);
    let report = execute(&live, live.dispatch(&keys[0]), "c-primary-pr3");
    let read_logs = report
        .tools
        .iter()
        .filter(|tool| *tool == "read_check_log")
        .count();
    println!("LIVE pr3 read_check_log calls={read_logs}");
    assert!(
        report
            .exits_denied
            .iter()
            .any(|d| d.starts_with("approve_pull_request")),
        "approve must be denied: {:?}",
        report.exits_denied
    );
    assert!(!report
        .exits_offered
        .contains(&"approve_pull_request".to_string()));
    assert!(
        ["comments", "leave_comments", "ready_for_human"].contains(&report.exit.as_str()),
        "unexpected exit {}",
        report.exit
    );
    if let Ok(run) = &report.outcome {
        if publication::writes_comments(run.result.as_ref().unwrap()) {
            let publication = effects(&live, run).0.expect("published");
            for c in review_comments(number)
                .iter()
                .filter(|c| c["body"].as_str().unwrap_or("").contains(&publication.id))
            {
                println!(
                    "LIVE posted {}:{} {}",
                    c["path"],
                    c["line"],
                    excerpt(c["body"].as_str().unwrap())
                );
            }
            assert_own_writes_do_not_retrigger(&live, number);
        } else {
            assert_eq!(
                (review_comments(number).len(), reviews(number).len()),
                before
            );
        }
    }
    assert!(reviews(number)
        .iter()
        .all(|r| r["state"].as_str() != Some("APPROVED")));
}

#[test]
#[ignore = "live: PR_SNIPER_LIVE=1, owner-authorized sandbox only"]
fn live_pr1_clean_ready_for_human_writes_nothing() {
    let Some(live) = live("pr1") else { return };
    live.save_settings(false);
    let number = 1;
    let before = (review_comments(number).len(), reviews(number).len());
    let (_, seq, read_hash) = live.heartbeat(number, true);
    let keys = live.enqueue(number, seq, &[(PRIMARY, PRIMARY_AGENT)]);
    let report = execute(&live, live.dispatch(&keys[0]), "d-primary-pr1");
    assert!(
        report
            .exits_denied
            .iter()
            .any(|d| d.starts_with("approve_pull_request")),
        "self-authored approve must be denied: {:?}",
        report.exits_denied
    );
    let run = report.outcome.as_ref().expect("completed");
    assert_eq!(report.exit, "ready_for_human");
    assert!(!publication::writes_comments(run.result.as_ref().unwrap()));
    assert_eq!(
        (review_comments(number).len(), reviews(number).len()),
        before,
        "no provider write"
    );
    let record = live
        .store()
        .load_pr_states()
        .unwrap()
        .get(&Binding::job(&run.job))
        .cloned()
        .unwrap();
    assert_eq!(record.baseline_hash.as_deref(), Some(read_hash.as_str()));
    println!(
        "LIVE local summary: {}",
        excerpt(record.local_summary.as_deref().unwrap_or(""))
    );
    assert_eq!(
        consume::consume(&live.store(), &run.job, &read_hash).unwrap(),
        Consumed::Consumed,
        "re-consuming the same state is idempotent"
    );
    assert_own_writes_do_not_retrigger(&live, number);
}

#[test]
#[ignore = "live: PR_SNIPER_LIVE=1"]
fn live_models() {
    let Some(live) = live("models") else { return };
    let models = crate::copilot::runtime::models(
        &live.copilot,
        &live.pair,
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(120),
    )
    .unwrap();
    println!(
        "LIVE models: {:?}",
        models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>()
    );
}
