use chrono::TimeZone;
use pr_sniper_lib::{
    github::{
        metadata::{Lifecycle, PullRequest},
        provider::{
            Capabilities, CommentCapability, Connection, GithubClient, RemoteRepository, Response,
            Transport,
        },
        ConnectionError, Identity,
    },
    monitoring::{next_run, Monitor, PollResult},
    policy::{PolicyOverrides, Schedule, WatchedIdentity},
    storage::{ProviderId, Repository, Settings, Store},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const ACCOUNT_ID: &str = "22";
const REPOSITORY_ID: &str = "100";
const REPOSITORY_NAME: &str = "example/repo";
const HEAD_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn repository(enabled: bool) -> Repository {
    Repository {
        id: "00000000-0000-4000-8000-000000000001".into(),
        provider: ProviderId::Github,
        name: REPOSITORY_NAME.into(),
        enabled,
        provider_account_id: Some(ACCOUNT_ID.into()),
        legacy_installation_id: None,
        provider_repository_id: Some(REPOSITORY_ID.into()),
        overrides: PolicyOverrides::default(),
        review_preset: None,
        watched_authors: Vec::new(),
        assignments: Vec::new(),
    }
}

fn store() -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().to_path_buf());
    let mut settings = Settings::default();
    settings.repositories.push(repository(true));
    store.save_settings(&settings).unwrap();
    (root, store)
}

fn set_settings(store: &Store, settings: &Settings) {
    store.save_settings(settings).unwrap();
}

fn pull(
    id: &str,
    number: u64,
    author_id: &str,
    author_login: &str,
    requested_reviewers: &[(&str, &str)],
    head_sha: &str,
    updated_at: &str,
) -> PullRequest {
    PullRequest {
        id: id.into(),
        number,
        title: format!("PR {number}"),
        author: Some(Identity {
            id: author_id.into(),
            login: author_login.into(),
        }),
        requested_reviewers: requested_reviewers
            .iter()
            .map(|(id, login)| Identity {
                id: (*id).into(),
                login: (*login).into(),
            })
            .collect(),
        requested_teams: Vec::new(),
        state: Lifecycle::Open,
        draft: false,
        head_sha: head_sha.into(),
        base_sha: HEAD_A.into(),
        head_repository_id: Some(REPOSITORY_ID.into()),
        base_repository_id: REPOSITORY_ID.into(),
        updated_at: updated_at.into(),
        files: Vec::new(),
    }
}

fn poll_result(pulls: Vec<PullRequest>, login: &str) -> PollResult {
    PollResult {
        connection: Connection {
            identity: Identity {
                id: ACCOUNT_ID.into(),
                login: login.into(),
            },
            repository: RemoteRepository {
                id: REPOSITORY_ID.into(),
                name: REPOSITORY_NAME.into(),
            },
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Available,
            },
        },
        pull_requests: pulls,
    }
}

fn check(
    monitor: &mut Monitor,
    store: &Store,
    at: i64,
    pulls: Vec<PullRequest>,
    login: &str,
) -> Result<(), String> {
    let mut tickets = monitor.prepare_checks(store, at, true)?;
    assert_eq!(tickets.len(), 1);
    monitor.finish(
        store,
        tickets.remove(0),
        Ok(poll_result(pulls, login)),
        at + 1,
    )
}

#[test]
fn interval_cron_timezone_and_daylight_transitions_are_explicit() {
    let interval = Schedule::Interval {
        minutes: 5,
        timezone: "America/New_York".into(),
    };
    assert_eq!(next_run(&interval, 1_775_000_000), Ok(1_775_000_300));
    assert_eq!(
        next_run(
            &Schedule::Interval {
                minutes: 5,
                timezone: "local".into(),
            },
            1_775_000_000
        ),
        Err(ConnectionError::Configuration)
    );

    let zone = chrono_tz::America::New_York;
    let before_spring = zone
        .with_ymd_and_hms(2026, 3, 8, 1, 0, 0)
        .single()
        .unwrap()
        .timestamp();
    let next_spring = next_run(
        &Schedule::Cron {
            expression: "30 2 * * *".into(),
            timezone: "America/New_York".into(),
        },
        before_spring,
    )
    .unwrap();
    // A missing local cron time is advanced to the first valid instant.
    assert_eq!(
        next_spring,
        zone.with_ymd_and_hms(2026, 3, 8, 3, 0, 0)
            .single()
            .unwrap()
            .timestamp()
    );

    let before_fall = zone
        .with_ymd_and_hms(2026, 11, 1, 0, 30, 0)
        .single()
        .unwrap()
        .timestamp();
    let next_fall = next_run(
        &Schedule::Cron {
            expression: "30 1 * * *".into(),
            timezone: "America/New_York".into(),
        },
        before_fall,
    )
    .unwrap();
    assert_eq!(
        next_fall,
        zone.with_ymd_and_hms(2026, 11, 1, 1, 30, 0)
            .earliest()
            .unwrap()
            .timestamp()
    );
    let after_first_fall = zone
        .with_ymd_and_hms(2026, 11, 1, 1, 45, 0)
        .earliest()
        .unwrap()
        .timestamp();
    assert_eq!(
        next_run(
            &Schedule::Cron {
                expression: "30 1 * * *".into(),
                timezone: "America/New_York".into(),
            },
            after_first_fall
        )
        .unwrap(),
        zone.with_ymd_and_hms(2026, 11, 2, 1, 30, 0)
            .single()
            .unwrap()
            .timestamp()
    );
}

#[test]
fn manual_and_scheduled_checks_share_one_non_overlapping_path() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut first = monitor.prepare_checks(&store, 1_800_000_000, true).unwrap();
    assert_eq!(first.len(), 1);
    assert!(monitor
        .prepare_checks(&store, 1_800_000_001, true)
        .unwrap()
        .is_empty());

    monitor
        .finish(
            &store,
            first.remove(0),
            Ok(poll_result(Vec::new(), "current-login")),
            1_800_000_002,
        )
        .unwrap();
    assert!(monitor
        .prepare_checks(&store, 1_800_000_003, false)
        .unwrap()
        .is_empty());
    assert_eq!(
        monitor
            .prepare_checks(&store, 1_800_000_003, true)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn shared_repository_bindings_are_serialized_across_accounts() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    let mut second = repository(true);
    second.id = "00000000-0000-4000-8000-000000000002".into();
    second.provider_account_id = Some("23".into());
    settings.repositories.push(second);
    set_settings(&store, &settings);

    let mut monitor = Monitor::restore(&store).unwrap();
    assert!(monitor
        .prepare_checks(&store, 1_800_000_000, false)
        .unwrap()
        .is_empty());
    let due = monitor.snapshot()[0].next_run;
    let mut tickets = monitor.prepare_checks(&store, due, false).unwrap();
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].provider_account_id, ACCOUNT_ID);
    monitor
        .finish(
            &store,
            tickets.remove(0),
            Ok(poll_result(Vec::new(), "first-account")),
            due + 1,
        )
        .unwrap();

    let tickets = monitor.prepare_checks(&store, due + 2, false).unwrap();
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].provider_account_id, "23");
}

#[test]
fn interrupted_attempt_recovers_as_one_honest_due_check() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    assert_eq!(
        monitor
            .prepare_checks(&store, 1_800_000_000, true)
            .unwrap()
            .len(),
        1
    );

    let mut restarted = Monitor::restore(&store).unwrap();
    let recovered = restarted.snapshot().remove(0);
    assert!(!recovered.in_flight);
    assert_eq!(recovered.last_failure.as_deref(), Some("interrupted"));
    assert_eq!(
        restarted
            .prepare_checks(&store, 1_800_000_001, false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn missed_schedule_occurrences_run_once_and_reschedule_from_now() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    assert!(monitor
        .prepare_checks(&store, 1_800_000_000, false)
        .unwrap()
        .is_empty());
    let next = monitor.snapshot()[0].next_run;
    assert!(next > 1_800_000_000);

    let missed_by = next + 10 * 24 * 60 * 60;
    let mut tickets = monitor.prepare_checks(&store, missed_by, false).unwrap();
    assert_eq!(tickets.len(), 1);
    monitor
        .finish(
            &store,
            tickets.remove(0),
            Ok(poll_result(Vec::new(), "current-login")),
            missed_by + 1,
        )
        .unwrap();
    assert!(monitor
        .prepare_checks(&store, missed_by + 2, false)
        .unwrap()
        .is_empty());
}

#[test]
fn author_reviewer_and_combined_triggers_filter_before_queueing() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].watched_authors = vec![WatchedIdentity {
        id: "11".into(),
        login: "old-author-login".into(),
    }];
    set_settings(&store, &settings);
    let mut monitor = Monitor::restore(&store).unwrap();
    let pulls = vec![
        pull(
            "1",
            1,
            "11",
            "new-author-login",
            &[],
            HEAD_A,
            "2026-09-25T10:00:00Z",
        ),
        pull(
            "2",
            2,
            "33",
            "reviewer-only-author",
            &[(ACCOUNT_ID, "current-login")],
            HEAD_A,
            "2026-09-25T10:01:00Z",
        ),
        pull(
            "3",
            3,
            "11",
            "new-author-login",
            &[(ACCOUNT_ID, "current-login")],
            HEAD_A,
            "2026-09-25T10:02:00Z",
        ),
        pull(
            "4",
            4,
            "44",
            "unrelated",
            &[],
            HEAD_A,
            "2026-09-25T10:03:00Z",
        ),
    ];
    check(
        &mut monitor,
        &store,
        1_800_000_000,
        pulls,
        "renamed-account",
    )
    .unwrap();
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 3);
    assert!(jobs[0].watched_author);
    assert!(!jobs[0].requested_reviewer);
    assert_eq!(jobs[0].author_login.as_deref(), Some("new-author-login"));
    assert_eq!(jobs[0].account_login, "renamed-account");
    assert_eq!(jobs[1].waiting, "trust_confirmation");
    assert!(jobs[2].watched_author && jobs[2].requested_reviewer);
}

#[test]
fn closed_draft_and_unassigned_revisions_never_enqueue() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut closed = pull("1", 1, "11", "author", &[], HEAD_A, "2026-09-25T10:00:00Z");
    closed.state = Lifecycle::Closed;
    let mut draft = pull("2", 2, "11", "author", &[], HEAD_A, "2026-09-25T10:01:00Z");
    draft.draft = true;
    let unrelated = pull("3", 3, "11", "author", &[], HEAD_A, "2026-09-25T10:02:00Z");
    check(
        &mut monitor,
        &store,
        1_800_000_000,
        vec![closed, draft, unrelated],
        "current-login",
    )
    .unwrap();
    assert!(store.load_queue().unwrap().is_empty());
}

#[test]
fn polling_deduplicates_repeats_but_admits_new_heads_and_survives_restart() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].watched_authors = vec![WatchedIdentity {
        id: "11".into(),
        login: "author".into(),
    }];
    set_settings(&store, &settings);
    let original = pull("1", 1, "11", "author", &[], HEAD_A, "2026-09-25T10:00:00Z");
    let mut monitor = Monitor::restore(&store).unwrap();
    check(
        &mut monitor,
        &store,
        1_800_000_000,
        vec![original.clone()],
        "old-account-login",
    )
    .unwrap();

    let mut restarted = Monitor::restore(&store).unwrap();
    check(
        &mut restarted,
        &store,
        1_800_000_100,
        vec![original],
        "new-account-login",
    )
    .unwrap();
    assert_eq!(store.load_queue().unwrap().len(), 1);
    assert_eq!(
        store.load_queue().unwrap()[0].account_login,
        "new-account-login"
    );

    let changed_head = pull("1", 1, "11", "author", &[], HEAD_B, "2026-09-25T10:01:00Z");
    check(
        &mut restarted,
        &store,
        1_800_000_200,
        vec![changed_head],
        "new-account-login",
    )
    .unwrap();
    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().any(|job| job.head_sha == HEAD_A));
    assert!(jobs.iter().any(|job| job.head_sha == HEAD_B));
    assert_eq!(restarted.snapshot()[0].last_success, Some(1_800_000_201));
}

#[test]
fn failed_attempt_health_is_visible_and_persists_across_restart() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut tickets = monitor.prepare_checks(&store, 1_800_000_000, true).unwrap();
    assert!(monitor
        .finish(
            &store,
            tickets.remove(0),
            Err(ConnectionError::Network),
            1_800_000_010,
        )
        .is_err());

    let health = monitor.snapshot().remove(0);
    assert_eq!(health.last_attempt, Some(1_800_000_000));
    assert_eq!(health.last_success, None);
    assert_eq!(health.last_failure.as_deref(), Some("Network"));
    assert!(health.next_run > 1_800_000_010);

    let restarted = Monitor::restore(&store).unwrap();
    let restored = restarted.snapshot().remove(0);
    assert_eq!(restored.last_attempt, health.last_attempt);
    assert_eq!(restored.last_success, health.last_success);
    assert_eq!(restored.next_run, health.next_run);
    assert_eq!(restored.last_failure, health.last_failure);
}

#[test]
fn reviewer_removal_and_reassignment_only_admit_current_eligibility() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    let reviewer_only = pull(
        "1",
        1,
        "33",
        "author",
        &[(ACCOUNT_ID, "current-login")],
        HEAD_A,
        "2026-09-25T10:00:00Z",
    );
    check(
        &mut monitor,
        &store,
        1_800_000_000,
        vec![reviewer_only],
        "current-login",
    )
    .unwrap();
    assert_eq!(store.load_queue().unwrap().len(), 1);
    assert_eq!(store.load_queue().unwrap()[0].waiting, "trust_confirmation");

    let removed = pull("1", 1, "33", "author", &[], HEAD_B, "2026-09-25T10:01:00Z");
    check(
        &mut monitor,
        &store,
        1_800_000_100,
        vec![removed],
        "current-login",
    )
    .unwrap();
    assert_eq!(store.load_queue().unwrap().len(), 1);

    let reassigned = pull(
        "1",
        1,
        "33",
        "author",
        &[(ACCOUNT_ID, "current-login")],
        HEAD_B,
        "2026-09-25T10:02:00Z",
    );
    check(
        &mut monitor,
        &store,
        1_800_000_200,
        vec![reassigned],
        "current-login",
    )
    .unwrap();
    assert_eq!(store.load_queue().unwrap().len(), 2);
}

#[test]
fn disabling_or_removing_a_repository_before_completion_cannot_queue_work() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut tickets = monitor.prepare_checks(&store, 1_800_000_000, true).unwrap();
    let ticket = tickets.remove(0);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].enabled = false;
    set_settings(&store, &settings);
    assert!(monitor
        .finish(
            &store,
            ticket,
            Ok(poll_result(
                vec![pull(
                    "1",
                    1,
                    "22",
                    "author",
                    &[(ACCOUNT_ID, "current-login")],
                    HEAD_A,
                    "2026-09-25T10:00:00Z",
                )],
                "current-login",
            )),
            1_800_000_001,
        )
        .is_err());
    assert!(store.load_queue().unwrap().is_empty());

    settings.repositories.clear();
    set_settings(&store, &settings);
    assert!(monitor
        .prepare_checks(&store, 1_800_000_002, true)
        .unwrap()
        .is_empty());
}

fn open_pr(id: u64, number: u64, author_id: u64, head_sha: &str, updated_at: &str) -> Value {
    json!({
        "id": id,
        "number": number,
        "title": format!("PR {number}"),
        "user": {"id": author_id, "login": format!("author-{author_id}")},
        "state": "open",
        "draft": false,
        "merged": false,
        "head": {
            "sha": head_sha,
            "repo": {"id": 100}
        },
        "base": {
            "sha": HEAD_A,
            "repo": {"id": 100}
        },
        "requested_reviewers": [{"id": 22, "login": "renamed-reviewer"}],
        "requested_teams": [],
        "updated_at": updated_at
    })
}

#[derive(Clone)]
struct ReadOnlyGithubFixture {
    requests: Arc<Mutex<Vec<String>>>,
}

impl Transport for ReadOnlyGithubFixture {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        self.requests.lock().unwrap().push(path.into());
        let (body, link, scope) = match path {
            "/user" => (json!({"id": 22, "login": "renamed-account"}), None, None),
            "/repos/example/repo" => (
                json!({
                    "id": 100,
                    "full_name": "example/repo",
                    "private": false,
                    "archived": false,
                    "disabled": false,
                    "permissions": {"pull": true}
                }),
                None,
                Some("repo"),
            ),
            "/repos/example/repo/pulls?state=open&per_page=1" => (json!([]), None, None),
            "/repos/example/repo/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1" => (
                json!([open_pr(
                    1001,
                    31,
                    44,
                    HEAD_A,
                    "2026-09-25T10:00:00Z"
                )]),
                Some("<https://api.github.com/repos/example/repo/pulls?state=open&sort=updated&direction=desc&per_page=100&page=2>; rel=\"next\""),
                None,
            ),
            "/repos/example/repo/pulls?state=open&sort=updated&direction=desc&per_page=100&page=2" => {
                let mut pull = open_pr(
                    1002,
                    32,
                    45,
                    HEAD_B,
                    "2026-09-25T10:01:00Z"
                );
                pull.as_object_mut().unwrap().remove("merged");
                (json!([pull]), None, None)
            }
            _ => {
                return Err(ConnectionError::InvalidResponse);
            }
        };
        let mut headers = BTreeMap::new();
        if let Some(link) = link {
            headers.insert("link".into(), link.into());
        }
        if let Some(scope) = scope {
            headers.insert("x-oauth-scopes".into(), scope.into());
        }
        Ok(Response {
            status: 200,
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

#[test]
fn account_bound_polling_reads_every_page_using_only_get_requests() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = GithubClient::new(ReadOnlyGithubFixture {
        requests: requests.clone(),
    });
    let connection = client.connect(REPOSITORY_NAME, Some(ACCOUNT_ID)).unwrap();
    assert_eq!(connection.identity.id, ACCOUNT_ID);
    assert_eq!(connection.identity.login, "renamed-account");
    assert_eq!(connection.repository.id, REPOSITORY_ID);

    let pulls = client.poll_pull_requests(&connection.repository).unwrap();
    assert_eq!(pulls.len(), 2);
    assert_eq!(pulls[0].requested_reviewers[0].id, ACCOUNT_ID);
    assert_eq!(pulls[1].head_sha, HEAD_B);
    assert!(pulls.iter().all(|pull| pull.files.is_empty()));
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.contains("per_page=100"))
            .count(),
        2
    );
}
