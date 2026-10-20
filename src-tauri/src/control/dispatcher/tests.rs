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
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

struct Fixture {
    _root: tempfile::TempDir,
    store: Mutex<Store>,
    monitor: Mutex<Monitor>,
    ai: Coordinator,
    auth: Mutex<GithubAuth>,
    copilot: Mutex<GithubAuth>,
    generations: Mutex<BTreeMap<String, u64>>,
    launched: Mutex<Vec<String>>,
    head: Mutex<String>,
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
            head: Mutex::new("a".repeat(40)),
        }
    }
    fn watch(&self, again: bool) -> Value {
        json!({"operation":"watch","expected":self.store.lock().unwrap().load_settings().unwrap().repositories[0],
            "url":"https://github.com/fixture/repo-1/pull/1","account_generation":7,"review_again":again})
    }
    fn call(&self, mut input: Value) -> Value {
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
                    protocol: 1,
                    port,
                    token: "a".repeat(64),
                    app_version: "fixture".into(),
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
                            |command| async {
                                assert!(
                                    handles(&command),
                                    "Command must use the native shared route"
                                );
                                dispatch(self, command)
                            },
                        ))
                })
                .unwrap();
            });
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let bytes =
                serde_json::to_vec(&json!({"protocol":1,"token":"a".repeat(64),"request":input}))
                    .unwrap();
            stream.write_all(b"SNIPER1\n").unwrap();
            stream
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&bytes).unwrap();
            let mut size = [0; 4];
            stream.read_exact(&mut size).unwrap();
            let size = u32::from_be_bytes(size) as usize;
            assert!(size <= super::super::MAX_FRAME);
            let mut bytes = vec![0; size];
            stream.read_exact(&mut bytes).unwrap();
            server.join().unwrap();
            serde_json::from_slice(&bytes).unwrap()
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
    ) -> Result<(crate::github::intake::ResolvedTarget, u64), Error> {
        assert_eq!(account, "101");
        Ok((
            crate::github::intake::ResolvedTarget {
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
    fn login_status(&self) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn quitting(&self) -> bool {
        false
    }
}

#[test]
fn unit_commands_reject_unknown_fields_before_dispatch() {
    let fixture = Fixture::new();
    let response = fixture.call(json!({"operation":"inspect","unexpected":true}));
    assert_eq!(response["error"]["code"], "invalid_request");
    assert!(fixture.launched.lock().unwrap().is_empty());
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
fn large_profile_has_bounded_inspection_accounts_resources_details_and_native_edit() {
    let fixture = Fixture::new();
    fixture.call(fixture.watch(false));
    let mut run = fixture.complete();
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
