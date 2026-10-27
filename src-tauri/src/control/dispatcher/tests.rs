use super::*;
use crate::{
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::{Capabilities, CommentCapability, Connection, RemoteRepository},
        Identity,
    },
    monitoring::OperationState,
    publication::{Phase, Publication, Receipt, RemoteState},
};
use std::net::{TcpListener, TcpStream};

struct Fixture {
    _root: tempfile::TempDir,
    store: Mutex<Store>,
    monitor: Mutex<Monitor>,
    ai: Coordinator,
    auth: Mutex<GithubAuth>,
    copilot: Mutex<GithubAuth>,
    generations: Mutex<BTreeMap<String, u64>>,
    launched: Mutex<Vec<String>>,
    polls: Mutex<Vec<String>>,
    head: Mutex<String>,
    azure: Option<crate::azure_devops::setup::Report>,
}
impl Fixture {
    fn new() -> Self {
        let (root, store) = crate::capacity::tests::fixture(1, 1, true);
        let old = store.load_settings().unwrap().repositories.remove(0);
        let mut repo = old.clone();
        repo.assignments[0].actions = Some(crate::storage::ActionPermissions {
            approve: true,
            merge: true,
            ..Default::default()
        });
        store
            .save_resource(crate::storage::ResourceEdit::Repository {
                id: repo.id.clone(),
                expected: Some(Box::new(old)),
                value: Some(Box::new(repo)),
            })
            .unwrap();
        store
            .save_automation(&crate::capacity::Automation { paused: true })
            .unwrap();
        let monitor = Monitor::restore(&store).unwrap();
        let mut auth = GithubAuth::new();
        auth.accounts.insert(
            "101".into(),
            crate::GithubAccountState::Connected(Identity {
                id: "101".into(),
                login: "actor".into(),
            }),
        );
        Self {
            _root: root,
            store: Mutex::new(store),
            monitor: Mutex::new(monitor),
            ai: Coordinator::default(),
            auth: Mutex::new(auth),
            copilot: Mutex::new(GithubAuth::new()),
            generations: Mutex::new(BTreeMap::from([("101".into(), 7)])),
            launched: Mutex::new(vec![]),
            polls: Mutex::new(vec![]),
            head: Mutex::new("a".repeat(40)),
            azure: None,
        }
    }
    fn watch(&self, again: bool) -> Value {
        json!({"operation":"watch","expected":self.store.lock().unwrap().load_settings().unwrap().repositories[0],
            "url":"https://github.com/fixture/repo-1/pull/1","account_generation":7,"review_again":again})
    }
    fn call(&self, input: Value) -> Value {
        self.call_with_auth_failure(input, false)
    }
    fn call_with_auth_failure(&self, mut input: Value, fail_refresh: bool) -> Value {
        input
            .as_object_mut()
            .unwrap()
            .entry("request_id")
            .or_insert(json!(uuid::Uuid::new_v4().to_string()));
        input
            .as_object_mut()
            .unwrap()
            .entry("issued_at")
            .or_insert(json!(crate::now_seconds().unwrap()));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::scope(|scope| {
            let server = scope.spawn(|| {
                let endpoint = super::super::Endpoint {
                    protocol: super::super::PROTOCOL,
                    port,
                    token: "a".repeat(64),
                    app_version: "fixture".into(),
                    pid: std::process::id(),
                };
                let (stream, _) = listener.accept().unwrap();
                super::super::serve_connection(stream, &endpoint, &mut |input| {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_time()
                        .build()
                        .unwrap()
                        .block_on(super::super::operations::execute_request(
                            &self.store,
                            input,
                            |command| {
                                super::super::operations::execute_with_auth_refresh(
                                    command,
                                    async {
                                        if fail_refresh {
                                            Err(operation("Copilot secure storage unavailable"))
                                        } else {
                                            Ok(())
                                        }
                                    },
                                    |command| async {
                                        assert!(
                                            handles(&command),
                                            "Command must use the native shared route"
                                        );
                                        dispatch(self, command)
                                    },
                                )
                            },
                        ))
                })
                .unwrap();
            });
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let reply =
                super::super::transport::client_exchange(&mut stream, &"a".repeat(64), input);
            server.join().unwrap();
            reply
        })
    }
    fn complete(&self) -> crate::review::ReviewRun {
        let store = self.store.lock().unwrap();
        let job = store
            .load_queue()
            .unwrap()
            .into_iter()
            .filter(|j| !crate::monitoring::retired(j))
            .max_by_key(|j| j.work.as_ref().unwrap().state_seq)
            .unwrap();
        let mut run = crate::review::host::request(
            &store,
            &job.work.as_ref().unwrap().id,
            true,
            crate::now_seconds().unwrap(),
        )
        .unwrap();
        run.operation.state = OperationState::Completed;
        store.save_reviews(&[run.clone()]).unwrap();
        let binding = crate::retention::Binding::job(&job);
        let mut states = store.load_pr_states().unwrap();
        let seq = states.state_seq(&binding);
        states.consume(&binding, seq, "baseline");
        store.save_pr_states(&states).unwrap();
        run
    }
    fn restart(&mut self) {
        self.monitor = Mutex::new(Monitor::restore(&self.store.lock().unwrap()).unwrap());
        self.ai = Coordinator::default();
    }
}
impl Runtime for Fixture {
    fn store(&self) -> &Mutex<Store> {
        &self.store
    }
    fn monitor(&self) -> &Mutex<Monitor> {
        &self.monitor
    }
    fn ai(&self) -> &Coordinator {
        &self.ai
    }
    fn auth(&self) -> &Mutex<GithubAuth> {
        &self.auth
    }
    fn copilot_auth(&self) -> &Mutex<GithubAuth> {
        &self.copilot
    }
    fn generations(&self) -> &Mutex<BTreeMap<String, u64>> {
        &self.generations
    }
    fn resolve(
        &self,
        account: &str,
        _: &str,
    ) -> Result<(crate::provider::ResolvedTarget, u64), Error> {
        assert_eq!(account, "101");
        Ok((
            crate::provider::ResolvedTarget {
                connection: Connection {
                    identity: Identity {
                        id: "101".into(),
                        login: "actor".into(),
                    },
                    repository: RemoteRepository {
                        id: "301".into(),
                        name: "fixture/repo-1".into(),
                    },
                    capabilities: Capabilities {
                        read: true,
                        comment: CommentCapability::Available,
                    },
                },
                pull_request: Some(PullRequest {
                    mentioned: false,
                    id: "1".into(),
                    number: 1,
                    title: "Fixture PR".into(),
                    author: Some(Identity {
                        id: "11".into(),
                        login: "author".into(),
                    }),
                    requested_reviewers: vec![],
                    requested_teams: vec![],
                    state: Lifecycle::Open,
                    draft: false,
                    head_sha: self.head.lock().unwrap().clone(),
                    base_sha: "b".repeat(40),
                    head_repository_id: Some("301".into()),
                    base_repository_id: "301".into(),
                    updated_at: "2026-10-20T00:00:00Z".into(),
                    files: vec![],
                }),
            },
            7,
        ))
    }
    fn launch(&self, batch: Batch) {
        assert!(batch.errors.is_empty(), "{:?}", batch.errors);
        self.launched
            .lock()
            .unwrap()
            .extend(batch.dispatched.iter().map(|d| d.key().id));
    }
    fn start_checks(&self) -> Result<(), Error> {
        let accounts = self.auth.lock().unwrap().monitoring_accounts();
        let store = self.store.lock().unwrap();
        let tickets = self
            .monitor
            .lock()
            .unwrap()
            .prepare_checks_with_accounts(&store, &accounts, crate::now_seconds().unwrap(), false)
            .map_err(storage)?;
        self.polls
            .lock()
            .unwrap()
            .extend(tickets.into_iter().map(|ticket| ticket.repository_id));
        Ok(())
    }
    fn login_status(&self) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn quitting(&self) -> bool {
        false
    }
    fn azure_devops(&self) -> Option<crate::azure_devops::setup::Report> {
        self.azure.clone()
    }
    fn azure_devops_identity(&self) -> Option<String> {
        self.azure
            .as_ref()?
            .account
            .as_ref()
            .map(|account| account.account_id.clone())
    }
}

const AVERY: &str = "00000000-0000-0000-0000-000000000001";

fn azure_report(connected: bool) -> crate::azure_devops::setup::Report {
    use crate::azure_devops::{connection::Identity, setup::*};
    let identity = Identity {
        id: AVERY.into(),
        display_name: "Avery Example".into(),
        unique_name: None,
    };
    Report {
        setup: SetupView {
            cli: CliView::Ready {
                version: "2.90.0".into(),
            },
            minimum: "2.54.0".into(),
            installer: Installer::Homebrew,
            organization: OrganizationView::TenantKnown {
                name: "contoso".into(),
                tenant: "00000000-0000-0000-0000-00000000000a".into(),
            },
            account: if connected {
                AccountView::Connected { identity }
            } else {
                AccountView::SignedOut
            },
            checking: false,
        },
        account: Some(RepositoryAccount {
            state: if connected {
                "connected"
            } else {
                "reconnect_required"
            },
            provider: "azure_devops",
            account_id: AVERY.into(),
            login: "Avery Example".into(),
            organization: "contoso".into(),
            connection_generation: 0,
            reason: (!connected).then_some(crate::provider::ConnectionError::SignedOut),
        }),
    }
}

#[test]
fn repository_accounts_include_azure_devops_with_setup_state() {
    let mut fixture = Fixture::new();
    let github_only = fixture.call(json!({"operation":"accounts","kind":"repository"}));
    assert_eq!(github_only["result"]["total"], 1);
    assert!(github_only["result"].get("azure_devops").is_none());
    fixture.azure = Some(azure_report(false));
    let accounts = fixture.call(json!({"operation":"accounts","kind":"repository"}));
    assert_eq!(accounts["ok"], true, "{accounts}");
    let result = &accounts["result"];
    assert_eq!(result["total"], 2);
    assert_eq!(
        result["values"][1],
        json!({"state":"reconnect_required","provider":"azure_devops","account_id":AVERY,
            "login":"Avery Example","organization":"contoso","connection_generation":0,
            "reason":"signed_out"})
    );
    assert_eq!(result["azure_devops"]["cli"]["state"], "ready");
    assert_eq!(
        result["azure_devops"]["organization"]["state"],
        "tenant_known"
    );
    assert_eq!(result["azure_devops"]["account"]["state"], "signed_out");
    let copilot = fixture.call(json!({"operation":"accounts","kind":"copilot"}));
    assert!(copilot["result"].get("azure_devops").is_none());
}

#[test]
fn azure_devops_repositories_save_only_as_the_bound_identity() {
    let mut fixture = Fixture::new();
    let id = uuid::Uuid::new_v4().to_string();
    let repository = json!({"id":id,"provider":"azure_devops",
        "name":"contoso/Web/app","enabled":false,"provider_account_id":AVERY,
        "provider_repository_id":format!("contoso/{}/{}",
            "00000000-0000-0000-0000-0000000000b1","00000000-0000-0000-0000-0000000000c1"),
        "watched_authors":[{"id":"00000000-0000-0000-0000-000000000002","login":"Blake Example"}]});
    let save = |fixture: &Fixture| {
        let revision = fixture.call(json!({"operation":"inspect"}))["result"]["revision"].clone();
        fixture.call(json!({"operation":"save","revision":revision,
            "edit":{"kind":"repository","id":id,"expected":null,"value":repository}}))
    };
    let unbound = save(&fixture);
    assert_eq!(unbound["ok"], false, "{unbound}");
    assert_eq!(unbound["error"]["detail"]["error"], "wrong_identity");
    fixture.azure = Some(azure_report(true));
    let saved = save(&fixture);
    assert_eq!(saved["ok"], true, "{saved}");
    let repositories =
        fixture.call(json!({"operation":"resources","kind":"repositories","limit":100}));
    let stored = repositories["result"]["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == json!(id))
        .unwrap()
        .clone();
    assert_eq!(stored["provider"], "azure_devops");
    assert_eq!(stored["provider_account_id"], AVERY);
    assert_eq!(stored["watched_authors"][0]["login"], "Blake Example");
}

#[test]
fn unit_commands_reject_unknown_fields_before_dispatch() {
    let fixture = Fixture::new();
    let response = fixture.call(json!({"operation":"inspect","unexpected":true}));
    assert_eq!(response["error"]["code"], "invalid_request");
    assert!(fixture.launched.lock().unwrap().is_empty());
}

#[test]
fn repository_accounts_cross_facade_without_refreshing_unrelated_copilot_auth() {
    let fixture = Fixture::new();
    let response =
        fixture.call_with_auth_failure(json!({"operation":"accounts","kind":"repository"}), true);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["result"]["values"][0]["account_id"], "101");
    assert_eq!(response["result"]["values"][0]["connection_generation"], 7);
    for command in [
        json!({"operation":"accounts","kind":"copilot"}),
        json!({"operation":"inspect"}),
    ] {
        let response = fixture.call_with_auth_failure(command, true);
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "operation_failed");
        assert_eq!(
            response["error"]["detail"],
            "Copilot secure storage unavailable"
        );
        assert!(response.get("result").is_none());
    }
}

#[test]
fn rejected_watch_again_does_not_orphan_hold_and_resolved_wait_dispatches_after_restart() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.call(fixture.watch(false))["ok"], true);
    let run = fixture.complete();
    let snapshot = crate::github::state::PrSnapshot {
        state: "open".into(),
        head: run.job.head_sha.clone(),
        base: "b".repeat(40),
        required_checks: vec!["build".into()],
        ..Default::default()
    }
    .normalize();
    let binding = crate::retention::Binding::job(&run.job);
    {
        let store = fixture.store.lock().unwrap();
        let mut states = store.load_pr_states().unwrap();
        let seq = states.state_seq(&binding);
        assert_eq!(
            states.wait(
                &binding,
                &snapshot.hash(),
                crate::pr_state::ExternalWait {
                    reason: crate::exits::WaitReason::Ci,
                    state_seq: seq,
                    assignment_id: run.assignment_id.clone(),
                    agent_id: run.selection.agent.id.clone(),
                    snapshot: snapshot.clone()
                }
            ),
            crate::pr_state::Consumed::Consumed
        );
        store.save_pr_states(&states).unwrap();
    }
    assert_eq!(fixture.call(fixture.watch(true))["ok"], false);
    {
        let store = fixture.store.lock().unwrap();
        assert!(work::holds(&store).unwrap().is_empty());
        let mut states = store.load_pr_states().unwrap();
        let mut resolved = snapshot;
        resolved.checks = vec![("build".into(), "completed".into(), "success".into())];
        states.observe(
            &binding,
            crate::pr_state::Observation {
                iteration_id: &run.job.work.as_ref().unwrap().iteration_id,
                new_iteration: false,
                snapshot: Some(&resolved),
                already_reviewed: true,
                now: crate::now_seconds().unwrap(),
            },
        );
        store.save_pr_states(&states).unwrap();
        store
            .save_automation(&crate::capacity::Automation { paused: false })
            .unwrap();
    }
    fixture.restart();
    assert_eq!(fixture.call(fixture.watch(false))["ok"], true);
    assert_eq!(fixture.launched.lock().unwrap().len(), 1);
}

#[test]
fn ordinary_watch_preserves_an_existing_unresolved_configuration_hold() {
    let fixture = Fixture::new();
    let repo = fixture
        .store
        .lock()
        .unwrap()
        .load_settings()
        .unwrap()
        .repositories[0]
        .id
        .clone();
    work::hold(&fixture.store.lock().unwrap(), &repo, 1, true).unwrap();
    assert_eq!(fixture.call(fixture.watch(false))["ok"], true);
    assert_eq!(
        work::holds(&fixture.store.lock().unwrap()).unwrap(),
        vec![(repo, 1)]
    );
    assert!(fixture.launched.lock().unwrap().is_empty());
}

fn publication(run: crate::review::ReviewRun, uncertain: bool) -> Publication {
    let mut op = run.operation.clone();
    op.id = uuid::Uuid::new_v4().to_string();
    op.state = OperationState::Completed;
    op.pending_review_id = Some("original-provider-review".into());
    Publication {
        id: uuid::Uuid::new_v4().to_string(),
        review: run,
        operation: op,
        history: vec![],
        automatic: false,
        confirmed: true,
        cancelled: false,
        phase: Phase::Published,
        error: None,
        batch: None,
        mutation: None,
        uncertain,
        receipts: vec![Receipt {
            review_id: "original-provider-review".into(),
            state: RemoteState::Commented,
            comment_ids: vec!["original-comment".into()],
        }],
    }
}

#[test]
fn watch_again_preserves_confirmed_publications_from_current_and_older_heads() {
    for earlier_head in [false, true] {
        let mut fixture = Fixture::new();
        assert_eq!(fixture.call(fixture.watch(false))["ok"], true);
        let mut run = fixture.complete();
        if earlier_head {
            run.job.head_sha = "c".repeat(40);
            run.key = "historical".into();
        }
        let published = publication(run, false);
        fixture
            .store
            .lock()
            .unwrap()
            .save_publications(std::slice::from_ref(&published))
            .unwrap();
        fixture.restart();
        let mut input = fixture.watch(true);
        input["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        input["issued_at"] = json!(crate::now_seconds().unwrap());
        assert_eq!(fixture.call(input.clone())["ok"], true);
        let count = fixture.store.lock().unwrap().load_queue().unwrap().len();
        assert_eq!(count, 2);
        assert_eq!(fixture.call(input)["result"]["replayed"], true);
        assert_eq!(
            fixture.store.lock().unwrap().load_queue().unwrap().len(),
            count
        );
        assert_eq!(
            fixture.store.lock().unwrap().load_publications().unwrap(),
            vec![published]
        );
    }
}

#[test]
fn watch_again_keeps_uncertain_publication_blocked_without_adding_hold() {
    let fixture = Fixture::new();
    fixture.call(fixture.watch(false));
    let run = fixture.complete();
    let published = publication(run, true);
    fixture
        .store
        .lock()
        .unwrap()
        .save_publications(std::slice::from_ref(&published))
        .unwrap();
    assert_eq!(fixture.call(fixture.watch(true))["ok"], false);
    let store = fixture.store.lock().unwrap();
    assert!(work::holds(&store).unwrap().is_empty());
    assert_eq!(store.load_publications().unwrap(), vec![published]);
    assert_eq!(store.load_queue().unwrap().len(), 1);
}

#[test]
fn retry_review_routes_native_cancelled_and_failed_recovery_without_active_duplication() {
    for cancelled in [false, true] {
        let fixture = Fixture::new();
        fixture.call(fixture.watch(false));
        let (key, old_id) = {
            let store = fixture.store.lock().unwrap();
            let job = store.load_queue().unwrap().remove(0);
            let key = job.work.as_ref().unwrap().id.clone();
            let mut run =
                crate::review::host::request(&store, &key, true, crate::now_seconds().unwrap())
                    .unwrap();
            run.operation.fail(
                &crate::review::Failure::permanent(if cancelled {
                    "Review cancelled by user."
                } else {
                    "Permanent failure"
                })
                .monitoring(),
                crate::now_seconds().unwrap(),
            );
            let old = run.operation.id.clone();
            store.save_reviews(&[run]).unwrap();
            store
                .save_automation(&crate::capacity::Automation { paused: false })
                .unwrap();
            (key, old)
        };
        let request = json!({"operation":"retry_review","candidate_key":key,"request_id":uuid::Uuid::new_v4().to_string(),"issued_at":crate::now_seconds().unwrap()});
        assert_eq!(fixture.call(request.clone())["ok"], true);
        assert_eq!(fixture.call(request)["result"]["replayed"], true);
        assert_eq!(fixture.launched.lock().unwrap().len(), 1);
        assert_eq!(
            fixture.call(json!({"operation":"retry_review","candidate_key":key}))["ok"],
            false
        );
        let store = fixture.store.lock().unwrap();
        let runs = store.load_reviews().unwrap();
        assert!(
            runs.iter().any(|r| r.operation.id == old_id),
            "original failure remains history"
        );
        let active = runs
            .iter()
            .find(|r| r.operation.state == OperationState::Running)
            .unwrap();
        assert_ne!(active.operation.id, old_id);
        assert!(store.load_publications().unwrap().is_empty());
    }
}

#[test]
fn ordinary_ai_retry_cannot_replace_a_completed_published_pass() {
    let fixture = Fixture::new();
    fixture.call(fixture.watch(false));
    let run = fixture.complete();
    let key = run.key.clone();
    let published = publication(run, false);
    fixture
        .store
        .lock()
        .unwrap()
        .save_publications(std::slice::from_ref(&published))
        .unwrap();
    assert_eq!(
        fixture.call(json!({"operation":"retry_review","candidate_key":key}))["ok"],
        false
    );
    assert!(fixture.launched.lock().unwrap().is_empty());
    assert_eq!(
        fixture.store.lock().unwrap().load_publications().unwrap(),
        vec![published]
    );
}

#[test]
fn completed_unpublished_and_stale_failed_retries_require_a_fresh_explicit_pass() {
    for previously_failed in [false, true] {
        let fixture = Fixture::new();
        assert_eq!(fixture.call(fixture.watch(false))["ok"], true);
        if previously_failed {
            let store = fixture.store.lock().unwrap();
            let job = store.load_queue().unwrap().remove(0);
            let mut failed = crate::review::host::request(
                &store,
                &job.work.as_ref().unwrap().id,
                true,
                crate::now_seconds().unwrap(),
            )
            .unwrap();
            failed.operation.fail(
                &crate::review::Failure::permanent("Synthetic failed attempt").monitoring(),
                crate::now_seconds().unwrap(),
            );
            store.save_reviews(&[failed]).unwrap();
        }
        let observed = fixture.call(json!({"operation":"queue"}));
        if previously_failed {
            assert!(matches!(
                observed["result"]["values"][0]["reviews"][0]["state"].as_str(),
                Some("failed" | "manual_retry")
            ));
        }
        let key = observed["result"]["values"][0]["reviews"][0]["candidate_key"].clone();
        let completed = fixture.complete();
        let before = {
            let store = fixture.store.lock().unwrap();
            (
                store.load_reviews().unwrap(),
                store.load_queue_state().unwrap(),
                store.load_pr_states().unwrap(),
                store.load_publications().unwrap(),
            )
        };
        let rejected = fixture.call(json!({"operation":"retry_review","candidate_key":key}));
        assert_eq!(rejected["error"]["code"], "review_again_required");
        {
            let store = fixture.store.lock().unwrap();
            assert_eq!(
                (
                    store.load_reviews().unwrap(),
                    store.load_queue_state().unwrap(),
                    store.load_pr_states().unwrap(),
                    store.load_publications().unwrap()
                ),
                before
            );
        }
        assert!(fixture.launched.lock().unwrap().is_empty());

        let mut rerun = fixture.watch(true);
        rerun["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        rerun["issued_at"] = json!(crate::now_seconds().unwrap());
        assert_eq!(fixture.call(rerun.clone())["ok"], true);
        assert_eq!(fixture.call(rerun)["result"]["replayed"], true);
        let store = fixture.store.lock().unwrap();
        let jobs = store.load_queue().unwrap();
        assert_eq!(jobs.len(), before.1.jobs.len() + 1);
        let fresh = jobs.last().unwrap().work.as_ref().unwrap();
        let original = completed.job.work.as_ref().unwrap();
        assert_ne!(fresh.id, original.id);
        assert_eq!(fresh.state_seq, original.state_seq + 1);
        assert_eq!(fresh.pass_ordinal, original.pass_ordinal + 1);
        assert_eq!(store.load_reviews().unwrap(), before.0);
    }
}

#[test]
fn failed_initial_polls_are_discoverable_and_only_the_selected_poll_is_retried() {
    let mut fixture = Fixture::new();
    let (target_id, neighbor_id) = {
        let store = fixture.store.lock().unwrap();
        store.save_queue_state(&Default::default()).unwrap();
        let mut settings = store.load_settings().unwrap();
        let target_id = settings.repositories[0].id.clone();
        let mut neighbor = settings.repositories[0].clone();
        neighbor.id = uuid::Uuid::new_v4().to_string();
        neighbor.name = "fixture/repo-2".into();
        neighbor.provider_repository_id = Some("302".into());
        neighbor.assignments[0].id = uuid::Uuid::new_v4().to_string();
        let neighbor_id = neighbor.id.clone();
        settings.repositories.push(neighbor.clone());
        store.save_settings(&settings).unwrap();
        store
            .save_resource(crate::storage::ResourceEdit::Repository {
                id: neighbor_id.clone(),
                expected: Some(Box::new(neighbor.clone())),
                value: Some(Box::new(neighbor)),
            })
            .unwrap();
        store
            .save_automation(&crate::capacity::Automation { paused: false })
            .unwrap();
        (target_id, neighbor_id)
    };
    fixture.restart();
    {
        let accounts = fixture.auth.lock().unwrap().monitoring_accounts();
        let store = fixture.store.lock().unwrap();
        let mut monitor = fixture.monitor.lock().unwrap();
        let now = crate::now_seconds().unwrap() - 10;
        let tickets = monitor
            .prepare_checks_with_accounts(&store, &accounts, now, true)
            .unwrap();
        assert_eq!(tickets.len(), 2);
        for ticket in tickets {
            assert!(monitor
                .finish_with_accounts(
                    &store,
                    &accounts,
                    ticket,
                    Err(crate::provider::ConnectionError::MissingReadPermission),
                    now + 1
                )
                .is_err());
        }
        assert!(store.load_queue().unwrap().is_empty());
    }
    fixture.restart();
    let summary = fixture.call(json!({"operation":"inspect"}));
    assert_eq!(summary["result"]["counts"]["queue_jobs"], 0);
    assert_eq!(summary["result"]["counts"]["repository_health"], 2);
    let first = fixture.call(json!({"operation":"health","limit":1}));
    assert_eq!(first["ok"], true);
    assert_eq!(first["result"]["next_offset"], 1);
    let second = fixture.call(json!({"operation":"health","limit":1,"offset":1}));
    assert_eq!(second["result"]["next_offset"], Value::Null);
    let rows = [
        &first["result"]["values"][0],
        &second["result"]["values"][0],
    ];
    let target = rows
        .iter()
        .find(|row| row["configuration_id"] == target_id)
        .unwrap();
    assert_eq!(target["provider_account_id"], "101");
    assert_eq!(target["provider_repository_id"], "301");
    assert_eq!(target["repository_name"], "fixture/repo-1");
    assert_eq!(target["failure_class"], "permission");
    assert_eq!(target["retryable"], true);
    let original_id = target["operation_id"].as_str().unwrap().to_owned();
    let neighbor_before = fixture
        .store
        .lock()
        .unwrap()
        .load_monitoring_state()
        .unwrap()
        .health
        .into_values()
        .find(|health| health.repository_id == neighbor_id)
        .unwrap()
        .operation;
    let request = json!({"operation":"retry","operation_id":original_id,
        "request_id":uuid::Uuid::new_v4().to_string(),"issued_at":crate::now_seconds().unwrap()});
    assert_eq!(fixture.call(request.clone())["ok"], true);
    assert_eq!(fixture.call(request)["result"]["replayed"], true);
    assert_eq!(
        fixture.call(json!({"operation":"retry","operation_id":original_id}))["ok"],
        false
    );
    assert_eq!(*fixture.polls.lock().unwrap(), vec![target_id.clone()]);
    let store = fixture.store.lock().unwrap();
    let state = store.load_monitoring_state().unwrap();
    let retried = state
        .health
        .values()
        .find(|health| health.repository_id == target_id)
        .unwrap();
    assert!(retried.in_flight);
    assert_eq!(
        retried.operation.as_ref().unwrap().state,
        OperationState::Running
    );
    assert_ne!(retried.operation.as_ref().unwrap().id, original_id);
    assert!(state.operations.contains_key(&original_id));
    assert_eq!(
        state
            .health
            .values()
            .find(|health| health.repository_id == neighbor_id)
            .unwrap()
            .operation,
        neighbor_before
    );
    assert!(store.load_queue().unwrap().is_empty());
}

#[test]
fn large_profile_has_bounded_inspection_accounts_resources_details_and_native_edit() {
    let fixture = Fixture::new();
    fixture.call(fixture.watch(false));
    {
        let store = fixture.store.lock().unwrap();
        let mut settings = store.load_settings().unwrap();
        settings.doctrines = (0..12)
            .map(|i| crate::storage::Doctrine {
                id: uuid::Uuid::new_v4().to_string(),
                title: format!("Large {i}"),
                body: "x".repeat(100 * 1024),
            })
            .collect();
        store.save_settings(&settings).unwrap();
        let job = store.load_queue().unwrap().remove(0);
        let mut run = crate::review::host::request(
            &store,
            &job.work.as_ref().unwrap().id,
            true,
            crate::now_seconds().unwrap(),
        )
        .unwrap();
        run.operation.fail(
            &crate::review::Failure::permanent("Synthetic recoverable-by-human failure")
                .monitoring(),
            crate::now_seconds().unwrap(),
        );
        run.selection.agent.prompt = "retained".repeat(160_000);
        store.save_reviews(&[run]).unwrap();
    }
    let inspected = fixture.call(json!({"operation":"inspect"}));
    assert_eq!(inspected["ok"], true);
    assert!(serde_json::to_vec(&inspected).unwrap().len() < 8192);
    let accounts = fixture.call(json!({"operation":"accounts","kind":"repository"}));
    assert_eq!(accounts["result"]["values"][0]["account_id"], "101");
    assert_eq!(accounts["result"]["values"][0]["connection_generation"], 7);
    let mut offset = 0;
    let mut count = 0;
    let mut first = None;
    loop {
        let page =
            fixture.call(json!({"operation":"resources","kind":"doctrines","offset":offset}));
        assert_eq!(page["ok"], true);
        let values = page["result"]["values"].as_array().unwrap();
        if first.is_none() {
            first = values.first().cloned();
        }
        count += values.len();
        match page["result"]["next_offset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert_eq!(count, 12);
    let first = first.unwrap();
    let mut changed = first.clone();
    changed["title"] = json!("Edited large doctrine");
    assert_eq!(
        fixture.call(
            json!({"operation":"save","revision":inspected["result"]["revision"],
        "edit":{"kind":"doctrine","id":first["id"],"expected":first,"value":changed}})
        )["ok"],
        true
    );
    let queue = fixture.call(json!({"operation":"queue"}));
    assert_eq!(queue["ok"], true);
    let item = &queue["result"]["values"][0];
    assert!(item["reviews"][0]["candidate_key"].is_string());
    let mut offset = 0;
    let mut text = String::new();
    let mut digest = None;
    loop {
        let page = fixture
            .call(json!({"operation":"detail","item_id":item["id"],"offset":offset,"limit":65536}));
        assert_eq!(page["ok"], true);
        if let Some(old) = &digest {
            assert_eq!(&page["result"]["revision"], old);
        } else {
            digest = Some(page["result"]["revision"].clone());
        }
        text.push_str(page["result"]["text"].as_str().unwrap());
        match page["result"]["next_offset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert!(text.len() > 1024 * 1024);
    let detail: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(detail["item"]["id"], item["id"]);
    assert_eq!(
        fixture.call(
            json!({"operation":"retry_review","candidate_key":item["reviews"][0]["candidate_key"]})
        )["ok"],
        true
    );
}

#[test]
fn queue_summaries_surface_settled_findings_duck_reasons_and_observation_retries() {
    let fixture = Fixture::new();
    assert_eq!(fixture.call(fixture.watch(false))["ok"], true);
    let now = crate::now_seconds().unwrap();
    let duck = |ordinal: u32, failure: &str, reason: &str| {
        json!({"ordinal":ordinal,"input":{"kind":"pr","claim":"c","proposed_body":"b"},
            "outcome":{"outcome":"failure","failure":failure},"reason":reason,
            "limits":{"max_calls":8,"call_timeout_seconds":180,"primary_window_minutes":15},
            "agent":"Duck","model":"duck-model","state_id":"0:hash","duration_ms":5})
    };
    let assessment = |thread: &str, disposition: &str| {
        json!({"thread_id":thread,"disposition":disposition,"head_sha":"a".repeat(40),
            "base_sha":"b".repeat(40),"rationale":"r"})
    };
    {
        let store = fixture.store.lock().unwrap();
        let job = store.load_queue().unwrap().remove(0);
        let mut run =
            crate::review::host::request(&store, &job.work.unwrap().id, true, now).unwrap();
        run.operation.fail(
            &crate::review::Failure::tossed(crate::feedback::OBSERVATION_FAILED).monitoring(),
            now,
        );
        run.error = Some(crate::feedback::OBSERVATION_FAILED.into());
        run.attempt_ducks = vec![serde_json::from_value(duck(2, "failed", "stream_lost")).unwrap()];
        run.result = Some(
            serde_json::from_value(json!({
                "reviewed_base_sha":"b".repeat(40),
                "output":{"exit":{"exit":"lgtm","comment":"c","resolve_own_threads":["T5"],"action":"merge"}},
                "session_id":"fixture","model":"fixture-model","runtime_version":"fixture",
                "input_tokens":1,"output_tokens":1,"tool_calls":1,
                "ducks":[duck(1, "unverified", "thread_unread")],
                "finding_dispositions":[assessment("T1","author_wont_fix"),
                    assessment("T2","fixed"),assessment("T3","author_wont_fix"),
                    assessment("T5","no_longer_applies"),assessment("T6","signed_off")],
                "carried_findings":[assessment("T4","fixed")]
            }))
            .unwrap(),
        );
        run.packet = Some(
            serde_json::from_value(json!({"state_id":"0:hash","hash":"h","schema":1,
                "tool_set":[],"exits_offered":["lgtm"],
                "exits_denied":[{"exit":"approve_and_merge","reason":"CI is pending. Cannot merge yet."}]}))
            .unwrap(),
        );
        store.save_reviews(&[run]).unwrap();
    }
    let queue = fixture.call(json!({"operation":"queue"}));
    let mut native = queue["result"].clone();
    for item in native["values"].as_array_mut().unwrap() {
        for review in item["reviews"].as_array_mut().unwrap() {
            review["operation_id"] = json!("operation");
            review["next_attempt_at"] = json!(1_800_000_000);
        }
    }
    contract("native-queue.json", &native);
    let review = &queue["result"]["values"][0]["reviews"][0];
    assert_eq!(review["error"], crate::feedback::OBSERVATION_FAILED);
    assert!(review["next_attempt_at"]
        .as_i64()
        .is_some_and(|at| at >= now));
    assert_eq!(
        review["dispositions"],
        json!({"author_wont_fix":2,"carried_forward":1,"fixed":1,"no_longer_applies":1,"signed_off":1})
    );
    // Only a completed run has an exit; this one failed after its packet.
    assert!(review["exit"].is_null());
    assert_eq!(
        review["exits_denied"],
        json!([{"exit":"approve_and_merge","reason":"CI is pending. Cannot merge yet."}])
    );
    assert_eq!(
        review["duck_failures"],
        json!([{"ordinal":1,"failure":"unverified","reason":"thread_unread"},
            {"ordinal":2,"failure":"failed","reason":"stream_lost"}])
    );
}

#[test]
fn detail_of_an_automatically_pruned_pr_reads_its_tombstone() {
    let fixture = Fixture::new();
    assert_eq!(fixture.call(fixture.watch(false))["ok"], true);
    let now = crate::now_seconds().unwrap();
    {
        let store = fixture.store.lock().unwrap();
        let mut scope = store.load_queue_state().unwrap().tracked.remove(0);
        scope.lifecycle = crate::provider::Lifecycle::Merged;
        scope.terminal_observed = true;
        let receipt = serde_json::from_value(json!({"scope":scope,"enqueue_watermark":1,
            "items":["pruned-item"],"pass_ordinals":{},"reply_ordinals":{},
            "follow_up_keys":[],"owned":[],"cleaned_at":now}))
        .unwrap();
        crate::retention::save(
            &store,
            &crate::retention::Ledger {
                receipts: vec![receipt],
                pending: None,
            },
        )
        .unwrap();
    }
    let detail = fixture.call(json!({"operation":"detail","item_id":"pruned-item"}));
    let text: serde_json::Value =
        serde_json::from_str(detail["result"]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["item"]["state"], "pruned");
    assert_eq!(text["item"]["pruned_at"], now);
    assert_eq!(text["reviews"], json!([]));
    let missing = fixture.call(json!({"operation":"detail","item_id":"never-seen"}));
    assert_eq!(missing["ok"], false);
}

/// Native MCP output the JS adapter's schemas must accept
/// (`tests/control/contract.test.mjs`). `UPDATE_GOLDEN=1` rewrites it.
fn contract(name: &str, value: &Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/control/fixtures")
        .join(name);
    let text = serde_json::to_string_pretty(value).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap_or_default(),
        text,
        "{name} differs from native output; rerun with UPDATE_GOLDEN=1, then npm run test:skills"
    );
}

#[test]
fn native_enum_ids_match_the_mcp_contract() {
    use crate::{
        actions::packet::MergePending, exits::gates::MergeBlock, queue::Progress,
        review::findings::Disposition,
    };
    let progress = [
        Progress::Approving,
        Progress::Merging,
        Progress::MergePendingBehindBase,
        Progress::MergePendingConflicts,
        Progress::Publishing,
    ];
    // Exhaustive: a new variant fails to compile until listed above.
    for p in progress {
        match p {
            Progress::Approving
            | Progress::Merging
            | Progress::MergePendingBehindBase
            | Progress::MergePendingConflicts
            | Progress::Publishing => {}
        }
    }
    let dispositions = [
        Disposition::StillApplies,
        Disposition::Fixed,
        Disposition::Withdrawn,
        Disposition::Unverifiable,
        Disposition::AuthorWontFix,
        Disposition::SignedOff,
        Disposition::NoLongerApplies,
    ];
    for d in dispositions {
        match d {
            Disposition::StillApplies
            | Disposition::Fixed
            | Disposition::Withdrawn
            | Disposition::Unverifiable
            | Disposition::AuthorWontFix
            | Disposition::SignedOff
            | Disposition::NoLongerApplies => {}
        }
        assert_eq!(serde_json::to_value(d).unwrap(), d.id());
    }
    let blocks = [MergeBlock::BehindBase, MergeBlock::Conflicts];
    for b in blocks {
        match b {
            MergeBlock::BehindBase | MergeBlock::Conflicts => {}
        }
    }
    let exits: Vec<_> = crate::exits::Exit::ALL.iter().map(|e| e.id()).collect();
    for exit in crate::exits::Exit::ALL {
        assert_eq!(serde_json::to_value(exit).unwrap(), exit.id());
    }
    contract(
        "native-enums.json",
        &json!({
            "exit": exits,
            "progress": progress,
            "merge_block": blocks,
            "disposition": dispositions.iter().map(|d| d.id()).chain(["carried_forward"])
                .collect::<Vec<_>>(),
            "merge_pending": MergePending { reason: MergeBlock::Conflicts, ci_not_run: true },
        }),
    );
}
