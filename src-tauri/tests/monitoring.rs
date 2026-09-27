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
    monitoring::{
        next_run, AccountAvailability, Monitor, MonitoringError, PollResult,
        WAITING_BINDING_CHANGED, WAITING_HUMAN_START, WAITING_INELIGIBLE,
        WAITING_NO_LONGER_CURRENT, WAITING_POLICY_CHANGED, WAITING_REPOSITORY_DISABLED,
        WAITING_REPOSITORY_REMOVED, WAITING_SUPERSEDED,
    },
    policy::{PolicyOverrides, Schedule, WatchedIdentity},
    storage::{Agent, Assignment, ProviderId, Repository, Settings, Store},
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
    poll_result_for(ACCOUNT_ID, REPOSITORY_ID, REPOSITORY_NAME, pulls, login)
}

fn poll_result_for(
    account_id: &str,
    repository_id: &str,
    repository_name: &str,
    pulls: Vec<PullRequest>,
    login: &str,
) -> PollResult {
    PollResult {
        connection: Connection {
            identity: Identity {
                id: account_id.into(),
                login: login.into(),
            },
            repository: RemoteRepository {
                id: repository_id.into(),
                name: repository_name.into(),
            },
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Available,
            },
        },
        pull_requests: pulls,
    }
}

fn available_accounts(ids: &[(&str, &str)]) -> BTreeMap<String, AccountAvailability> {
    ids.iter()
        .map(|(id, login)| {
            (
                (*id).into(),
                AccountAvailability {
                    login: (*login).into(),
                    connected: true,
                },
            )
        })
        .collect()
}

fn check(
    monitor: &mut Monitor,
    store: &Store,
    at: i64,
    pulls: Vec<PullRequest>,
    login: &str,
) -> Result<(), MonitoringError> {
    let mut tickets = monitor
        .prepare_checks(store, at, true)
        .map_err(MonitoringError::Storage)?;
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
fn repeated_manual_checks_coalesce_behind_the_inflight_read() {
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
    let pending = monitor
        .prepare_checks(&store, 1_800_000_003, false)
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert!(monitor
        .prepare_checks(&store, 1_800_000_004, false)
        .unwrap()
        .is_empty());
}

#[test]
fn manual_check_preserves_the_upcoming_scheduled_occurrence() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut first = monitor.prepare_checks(&store, 1_800_000_000, true).unwrap();
    assert_eq!(first.len(), 1);
    let scheduled = monitor.snapshot()[0].next_run;
    monitor
        .finish(
            &store,
            first.remove(0),
            Ok(poll_result(Vec::new(), "current-login")),
            1_800_000_001,
        )
        .unwrap();
    let mut manual = monitor.prepare_checks(&store, 1_800_000_100, true).unwrap();
    assert_eq!(manual.len(), 1);
    monitor
        .finish(
            &store,
            manual.remove(0),
            Ok(poll_result(Vec::new(), "current-login")),
            1_800_000_101,
        )
        .unwrap();
    assert_eq!(monitor.snapshot()[0].next_run, scheduled);
}

#[test]
fn each_assignment_schedule_has_its_own_health_and_due_ticket() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    settings.agents.push(Agent {
        id: "00000000-0000-4000-8000-000000000020".into(),
        name: "Agent A".into(),
        model: "gpt-4o".into(),
        ai_account: None,
        doctrine: None,
        prompt: "review".into(),
        signature: "sig-a".into(),
    });
    settings.agents.push(Agent {
        id: "00000000-0000-4000-8000-000000000021".into(),
        name: "Agent B".into(),
        model: "gpt-4o".into(),
        ai_account: None,
        doctrine: None,
        prompt: "review".into(),
        signature: "sig-b".into(),
    });
    settings.repositories[0].assignments = vec![
        Assignment {
            id: "00000000-0000-4000-8000-000000000010".into(),
            agent_id: "00000000-0000-4000-8000-000000000020".into(),
            schedule: Schedule::Interval {
                minutes: 5,
                timezone: "America/New_York".into(),
            },
            comment: false,
            approve: false,
        },
        Assignment {
            id: "00000000-0000-4000-8000-000000000011".into(),
            agent_id: "00000000-0000-4000-8000-000000000021".into(),
            schedule: Schedule::Interval {
                minutes: 15,
                timezone: "America/New_York".into(),
            },
            comment: false,
            approve: false,
        },
    ];
    set_settings(&store, &settings);
    let mut monitor = Monitor::restore(&store).unwrap();
    let tickets = monitor.prepare_checks(&store, 1_800_000_000, true).unwrap();
    assert_eq!(tickets.len(), 1);
    assert!(monitor
        .snapshot()
        .iter()
        .any(|health| health.schedule_key.starts_with("interval:5:")));
    assert!(monitor
        .snapshot()
        .iter()
        .any(|health| health.schedule_key.starts_with("interval:15:")));
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
    assert_eq!(
        jobs.iter()
            .find(|job| job.head_sha == HEAD_A)
            .unwrap()
            .waiting,
        WAITING_SUPERSEDED
    );
    assert!(jobs
        .iter()
        .any(|job| job.head_sha == HEAD_B && job.waiting == "human_start"));
    assert_eq!(restarted.snapshot()[0].last_success, Some(1_800_000_201));
}

#[test]
fn failed_attempt_health_is_visible_and_persists_across_restart() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut tickets = monitor.prepare_checks(&store, 1_800_000_000, true).unwrap();
    let error = monitor
        .finish(
            &store,
            tickets.remove(0),
            Err(ConnectionError::Network),
            1_800_000_010,
        )
        .unwrap_err();
    assert!(!error.requires_host_report());

    let health = monitor.snapshot().remove(0);
    assert_eq!(health.last_attempt, Some(1_800_000_000));
    assert_eq!(health.last_success, None);
    assert_eq!(health.last_failure.as_deref(), Some("Network"));
    assert!(health.next_run > 1_800_000_010);

    let mut restarted = Monitor::restore(&store).unwrap();
    let restored = restarted.snapshot().remove(0);
    assert_eq!(restored.last_attempt, health.last_attempt);
    assert_eq!(restored.last_success, health.last_success);
    assert_eq!(restored.next_run, health.next_run);
    assert_eq!(restored.last_failure, health.last_failure);

    let mut retry = restarted
        .prepare_checks(&store, 1_800_000_020, true)
        .unwrap();
    restarted
        .finish(
            &store,
            retry.remove(0),
            Ok(poll_result(Vec::new(), "current-login")),
            1_800_000_021,
        )
        .unwrap();
    assert_eq!(restarted.snapshot()[0].last_failure, None);
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
    assert_eq!(store.load_queue().unwrap()[0].waiting, WAITING_SUPERSEDED);

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
    assert!(store
        .load_queue()
        .unwrap()
        .iter()
        .any(|job| job.head_sha == HEAD_B && job.waiting == "trust_confirmation"));
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
            "/repos/example/repo/pulls?state=open&sort=created&direction=asc&per_page=100&page=1" => (
                json!([open_pr(
                    1001,
                    31,
                    44,
                    HEAD_A,
                    "2026-09-25T10:00:00Z"
                )]),
                Some("<https://api.github.com/repos/example/repo/pulls?state=open&sort=created&direction=asc&per_page=100&page=2>; rel=\"next\""),
                None,
            ),
            "/repos/example/repo/pulls?state=open&sort=created&direction=asc&per_page=100&page=2" => {
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

fn store_with_detected_job() -> (tempfile::TempDir, Store, Monitor) {
    let (root, store) = store();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].watched_authors = vec![WatchedIdentity {
        id: "11".into(),
        login: "author".into(),
    }];
    set_settings(&store, &settings);
    let mut monitor = Monitor::restore(&store).unwrap();
    check(
        &mut monitor,
        &store,
        1_800_000_000,
        vec![pull(
            "1",
            1,
            "11",
            "author",
            &[],
            HEAD_A,
            "2026-09-25T10:00:00Z",
        )],
        "current-login",
    )
    .unwrap();
    (root, store, monitor)
}

#[test]
fn scans_retire_superseded_ineligible_and_unseen_jobs_without_erasing_history() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].watched_authors = vec![WatchedIdentity {
        id: "11".into(),
        login: "author".into(),
    }];
    set_settings(&store, &settings);
    let mut monitor = Monitor::restore(&store).unwrap();
    check(
        &mut monitor,
        &store,
        1_800_000_000,
        vec![
            pull("1", 1, "11", "author", &[], HEAD_A, "2026-09-25T10:00:00Z"),
            pull(
                "2",
                2,
                "33",
                "reviewer-only",
                &[(ACCOUNT_ID, "current-login")],
                HEAD_A,
                "2026-09-25T10:01:00Z",
            ),
            pull("3", 3, "11", "author", &[], HEAD_A, "2026-09-25T10:02:00Z"),
        ],
        "current-login",
    )
    .unwrap();

    check(
        &mut monitor,
        &store,
        1_800_000_100,
        vec![
            pull("1", 1, "11", "author", &[], HEAD_B, "2026-09-25T10:03:00Z"),
            pull(
                "2",
                2,
                "33",
                "reviewer-only",
                &[],
                HEAD_A,
                "2026-09-25T10:04:00Z",
            ),
        ],
        "current-login",
    )
    .unwrap();

    let jobs = store.load_queue().unwrap();
    assert_eq!(jobs.len(), 4);
    assert_eq!(
        jobs.iter()
            .find(|job| job.pull_request_id == "1" && job.head_sha == HEAD_A)
            .unwrap()
            .waiting,
        WAITING_SUPERSEDED
    );
    assert_eq!(
        jobs.iter()
            .find(|job| job.pull_request_id == "2")
            .unwrap()
            .waiting,
        WAITING_INELIGIBLE
    );
    assert_eq!(
        jobs.iter()
            .find(|job| job.pull_request_id == "3")
            .unwrap()
            .waiting,
        WAITING_NO_LONGER_CURRENT
    );
    assert!(jobs.iter().any(|job| job.pull_request_id == "1"
        && job.head_sha == HEAD_B
        && job.waiting == "human_start"));

    let mut reappeared = pull("3", 3, "11", "author", &[], HEAD_A, "2026-09-25T10:05:00Z");
    reappeared.title = "PR 3 reappeared".into();
    check(
        &mut monitor,
        &store,
        1_800_000_150,
        vec![reappeared],
        "current-login",
    )
    .unwrap();
    let reactivated = store.load_queue().unwrap();
    assert_eq!(reactivated.len(), 4);
    assert_eq!(
        reactivated
            .iter()
            .filter(|job| job.pull_request_id == "3" && job.head_sha == HEAD_A)
            .count(),
        1
    );
    assert_eq!(
        reactivated
            .iter()
            .find(|job| job.pull_request_id == "3" && job.head_sha == HEAD_A)
            .unwrap()
            .waiting,
        WAITING_HUMAN_START
    );
    assert_eq!(
        reactivated
            .iter()
            .find(|job| job.pull_request_id == "3" && job.head_sha == HEAD_A)
            .unwrap()
            .title,
        "PR 3 reappeared"
    );

    let before_failure = reactivated;
    let mut tickets = monitor.prepare_checks(&store, 1_800_000_200, true).unwrap();
    let error = monitor
        .finish(
            &store,
            tickets.remove(0),
            Err(ConnectionError::Network),
            1_800_000_201,
        )
        .unwrap_err();
    assert!(!error.requires_host_report());
    assert_eq!(store.load_queue().unwrap(), before_failure);
}

#[test]
fn configuration_changes_retire_actionable_jobs_before_a_provider_scan() {
    for (case, expected) in [
        ("policy", WAITING_POLICY_CHANGED),
        ("disabled", WAITING_REPOSITORY_DISABLED),
        ("removed", WAITING_REPOSITORY_REMOVED),
        ("replaced", WAITING_REPOSITORY_REMOVED),
        ("rebound", WAITING_BINDING_CHANGED),
    ] {
        let (_root, store, mut monitor) = store_with_detected_job();
        let mut settings = store.load_settings().unwrap();
        match case {
            "policy" => {
                settings.repositories[0].watched_authors = vec![WatchedIdentity {
                    id: "12".into(),
                    login: "different-author".into(),
                }];
            }
            "disabled" => settings.repositories[0].enabled = false,
            "removed" => settings.repositories.clear(),
            "replaced" => {
                settings.repositories[0].id = "00000000-0000-4000-8000-000000000099".into();
            }
            "rebound" => settings.repositories[0].provider_account_id = Some("23".into()),
            _ => unreachable!(),
        }
        set_settings(&store, &settings);
        let accounts = available_accounts(&[("22", "current-login"), ("23", "other-login")]);
        monitor
            .synchronize_configuration(&store, &accounts, 1_800_000_100)
            .unwrap();
        assert_eq!(
            store.load_queue().unwrap()[0].waiting,
            expected,
            "case {case}"
        );
    }
}

#[test]
fn schedule_reconciliation_uses_only_exact_legacy_or_assignment_keys() {
    let (_root, store) = store();
    let accounts = available_accounts(&[(ACCOUNT_ID, "current-login")]);
    let mut monitor = Monitor::restore(&store).unwrap();
    monitor
        .synchronize_configuration(&store, &accounts, 1_800_000_000)
        .unwrap();
    assert_eq!(
        monitor.snapshot()[0].repository_id,
        "00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(monitor.snapshot().len(), 1);
    assert!(monitor.snapshot()[0].assignment_id.is_none());

    let mut settings = store.load_settings().unwrap();
    settings.agents = vec![
        Agent {
            id: "00000000-0000-4000-8000-000000000020".into(),
            name: "Agent A".into(),
            model: "gpt-4o".into(),
            ai_account: None,
            doctrine: None,
            prompt: "review".into(),
            signature: "sig-a".into(),
        },
        Agent {
            id: "00000000-0000-4000-8000-000000000021".into(),
            name: "Agent B".into(),
            model: "gpt-4o".into(),
            ai_account: None,
            doctrine: None,
            prompt: "review".into(),
            signature: "sig-b".into(),
        },
    ];
    settings.repositories[0].assignments = vec![
        Assignment {
            id: "00000000-0000-4000-8000-000000000010".into(),
            agent_id: settings.agents[0].id.clone(),
            schedule: Schedule::Interval {
                minutes: 5,
                timezone: "UTC".into(),
            },
            comment: false,
            approve: false,
        },
        Assignment {
            id: "00000000-0000-4000-8000-000000000011".into(),
            agent_id: settings.agents[1].id.clone(),
            schedule: Schedule::Interval {
                minutes: 15,
                timezone: "UTC".into(),
            },
            comment: false,
            approve: false,
        },
    ];
    set_settings(&store, &settings);
    monitor
        .synchronize_configuration(&store, &accounts, 1_800_000_010)
        .unwrap();
    let health = monitor.snapshot();
    assert_eq!(health.len(), 2);
    assert!(health.iter().all(|item| item.assignment_id.is_some()));
    assert!(health
        .iter()
        .any(|item| item.agent_name.as_deref() == Some("Agent A")));

    settings.repositories[0].assignments.remove(0);
    set_settings(&store, &settings);
    monitor
        .synchronize_configuration(&store, &accounts, 1_800_000_020)
        .unwrap();
    let health = monitor.snapshot();
    assert_eq!(health.len(), 1);
    assert_eq!(
        health[0].assignment_id.as_deref(),
        Some("00000000-0000-4000-8000-000000000011")
    );

    settings.repositories[0].assignments.clear();
    set_settings(&store, &settings);
    monitor
        .synchronize_configuration(&store, &accounts, 1_800_000_030)
        .unwrap();
    assert_eq!(monitor.snapshot().len(), 1);
    assert!(monitor.snapshot()[0].assignment_id.is_none());
}

#[test]
fn removed_inflight_schedule_keeps_exclusion_until_the_read_finishes() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    settings.agents.push(Agent {
        id: "00000000-0000-4000-8000-000000000020".into(),
        name: "Agent A".into(),
        model: "gpt-4o".into(),
        ai_account: None,
        doctrine: None,
        prompt: "review".into(),
        signature: "sig".into(),
    });
    settings.repositories[0].assignments.push(Assignment {
        id: "00000000-0000-4000-8000-000000000010".into(),
        agent_id: settings.agents[0].id.clone(),
        schedule: Schedule::Interval {
            minutes: 5,
            timezone: "UTC".into(),
        },
        comment: false,
        approve: false,
    });
    set_settings(&store, &settings);
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut tickets = monitor.prepare_checks(&store, 1_800_000_000, true).unwrap();
    let ticket = tickets.remove(0);

    let mut replacement = repository(true);
    replacement.id = "00000000-0000-4000-8000-000000000002".into();
    settings.repositories = vec![replacement];
    set_settings(&store, &settings);
    assert!(monitor
        .prepare_checks(&store, 1_800_000_001, true)
        .unwrap()
        .is_empty());
    assert!(monitor
        .finish(
            &store,
            ticket,
            Ok(poll_result(Vec::new(), "current-login")),
            1_800_000_002,
        )
        .is_err());
    assert_eq!(monitor.snapshot().len(), 1);
    assert_eq!(
        monitor.snapshot()[0].repository_id,
        "00000000-0000-4000-8000-000000000002"
    );
    assert_eq!(
        monitor
            .prepare_checks(&store, 1_800_000_003, true)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn disable_reenable_and_rebind_reset_health_and_cursor_honestly() {
    let (_root, store) = store();
    let mut monitor = Monitor::restore(&store).unwrap();
    check(
        &mut monitor,
        &store,
        1_800_000_000,
        Vec::new(),
        "current-login",
    )
    .unwrap();
    assert!(!store.load_monitoring_state().unwrap().cursors.is_empty());

    let accounts = available_accounts(&[("22", "current-login"), ("23", "other-login")]);
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].enabled = false;
    set_settings(&store, &settings);
    monitor
        .synchronize_configuration(&store, &accounts, 1_800_000_010)
        .unwrap();
    let disabled = monitor.snapshot().remove(0);
    assert!(!disabled.enabled);
    assert!(!disabled.schedule_available);
    assert_eq!(disabled.next_run, 0);

    settings.repositories[0].enabled = true;
    set_settings(&store, &settings);
    monitor
        .synchronize_configuration(&store, &accounts, 1_800_000_020)
        .unwrap();
    assert!(monitor.snapshot()[0].schedule_available);

    settings.repositories[0].provider_account_id = Some("23".into());
    set_settings(&store, &settings);
    monitor
        .synchronize_configuration(&store, &accounts, 1_800_000_030)
        .unwrap();
    let rebound = monitor.snapshot().remove(0);
    assert_eq!(rebound.provider_account_id.as_deref(), Some("23"));
    assert_eq!(rebound.account_login.as_deref(), Some("other-login"));
    assert_eq!(rebound.last_success, None);
    assert!(store.load_monitoring_state().unwrap().cursors.is_empty());
}

#[test]
fn one_manual_request_drains_every_account_and_assignment_for_a_shared_repository() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    settings.agents = (0..3)
        .map(|index| Agent {
            id: format!("00000000-0000-4000-8000-00000000002{index}"),
            name: format!("Agent {index}"),
            model: "gpt-4o".into(),
            ai_account: None,
            doctrine: None,
            prompt: "review".into(),
            signature: "sig".into(),
        })
        .collect();
    settings.repositories[0].assignments = vec![
        Assignment {
            id: "00000000-0000-4000-8000-000000000010".into(),
            agent_id: settings.agents[0].id.clone(),
            schedule: Schedule::Interval {
                minutes: 5,
                timezone: "UTC".into(),
            },
            comment: false,
            approve: false,
        },
        Assignment {
            id: "00000000-0000-4000-8000-000000000011".into(),
            agent_id: settings.agents[1].id.clone(),
            schedule: Schedule::Interval {
                minutes: 10,
                timezone: "UTC".into(),
            },
            comment: false,
            approve: false,
        },
    ];
    let mut second = repository(true);
    second.id = "00000000-0000-4000-8000-000000000002".into();
    second.provider_account_id = Some("23".into());
    second.assignments.push(Assignment {
        id: "00000000-0000-4000-8000-000000000012".into(),
        agent_id: settings.agents[2].id.clone(),
        schedule: Schedule::Interval {
            minutes: 15,
            timezone: "UTC".into(),
        },
        comment: false,
        approve: false,
    });
    settings.repositories.push(second);
    set_settings(&store, &settings);
    let accounts = available_accounts(&[("22", "account-a"), ("23", "account-b")]);
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut tickets = monitor
        .prepare_checks_with_accounts(&store, &accounts, 1_800_000_000, true)
        .unwrap();
    assert_eq!(tickets.len(), 1);
    assert_eq!(
        monitor
            .snapshot()
            .iter()
            .filter(|health| health.manual_pending)
            .count(),
        2
    );

    let mut sequence = Vec::new();
    loop {
        let ticket = tickets.remove(0);
        sequence.push((
            ticket.provider_account_id.clone(),
            ticket.health_key.clone(),
        ));
        monitor
            .finish_with_accounts(
                &store,
                &accounts,
                ticket.clone(),
                Ok(poll_result_for(
                    &ticket.provider_account_id,
                    &ticket.provider_repository_id,
                    &ticket.name,
                    Vec::new(),
                    accounts[&ticket.provider_account_id].login.as_str(),
                )),
                1_800_000_001 + sequence.len() as i64,
            )
            .unwrap();
        tickets = monitor
            .prepare_checks_with_accounts(
                &store,
                &accounts,
                1_800_000_010 + sequence.len() as i64,
                false,
            )
            .unwrap();
        if tickets.is_empty() {
            break;
        }
        assert_eq!(tickets.len(), 1);
    }
    assert_eq!(
        sequence,
        vec![
            (
                "22".into(),
                "00000000-0000-4000-8000-000000000001:assignment:00000000-0000-4000-8000-000000000010"
                    .into()
            ),
            (
                "22".into(),
                "00000000-0000-4000-8000-000000000001:assignment:00000000-0000-4000-8000-000000000011"
                    .into()
            ),
            (
                "23".into(),
                "00000000-0000-4000-8000-000000000002:assignment:00000000-0000-4000-8000-000000000012"
                    .into()
            ),
        ]
    );
    assert!(monitor
        .snapshot()
        .iter()
        .all(|health| !health.manual_pending && !health.in_flight));
}

#[test]
fn cancelling_manual_pending_work_does_not_start_another_read() {
    let (_root, store) = store();
    let mut settings = store.load_settings().unwrap();
    let mut second = repository(true);
    second.id = "00000000-0000-4000-8000-000000000002".into();
    second.provider_account_id = Some("23".into());
    settings.repositories.push(second);
    set_settings(&store, &settings);
    let accounts = available_accounts(&[("22", "account-a"), ("23", "account-b")]);
    let mut monitor = Monitor::restore(&store).unwrap();
    let mut tickets = monitor
        .prepare_checks_with_accounts(&store, &accounts, 1_800_000_000, true)
        .unwrap();
    assert_eq!(tickets.len(), 1);
    monitor.cancel_pending_checks(&store).unwrap();
    let ticket = tickets.remove(0);
    monitor
        .finish_with_accounts(
            &store,
            &accounts,
            ticket.clone(),
            Ok(poll_result_for(
                &ticket.provider_account_id,
                &ticket.provider_repository_id,
                &ticket.name,
                Vec::new(),
                "account-a",
            )),
            1_800_000_001,
        )
        .unwrap();
    assert!(monitor
        .prepare_checks_with_accounts(&store, &accounts, 1_800_000_002, false)
        .unwrap()
        .is_empty());
}

#[test]
fn persisted_legacy_health_migrates_identity_fields_without_losing_history() {
    let value = json!({
        "repository_id": "configuration",
        "name": "example/repo",
        "schedule_key": "interval:5:UTC",
        "provider_repository_id": "100",
        "enabled": true,
        "last_attempt": 10,
        "last_success": 11,
        "next_run": 12,
        "schedule_available": true,
        "last_failure": null,
        "in_flight": false
    });
    let health: pr_sniper_lib::monitoring::ScheduleHealth = serde_json::from_value(value).unwrap();
    assert_eq!(health.last_success, Some(11));
    assert_eq!(health.provider_account_id, None);
    assert_eq!(health.assignment_id, None);
    assert!(!health.manual_pending);

    let job: pr_sniper_lib::monitoring::QueueJob = serde_json::from_value(json!({
        "provider": "github",
        "account_id": "22",
        "account_login": "current-login",
        "repository_id": "100",
        "repository_name": "example/repo",
        "pull_request_id": "1",
        "number": 1,
        "title": "PR 1",
        "head_sha": HEAD_A,
        "trigger_policy": "legacy-policy",
        "author_id": "11",
        "author_login": "author",
        "watched_author": true,
        "requested_reviewer": false,
        "waiting": "human_start",
        "detected_at": 10
    }))
    .unwrap();
    assert!(job.configuration_id.is_empty());
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
    let requests = requests.lock().unwrap();
    let poll_requests: Vec<_> = requests
        .iter()
        .filter(|path| path.contains("per_page=100"))
        .cloned()
        .collect();
    assert_eq!(
        poll_requests,
        vec![
            "/repos/example/repo/pulls?state=open&sort=created&direction=asc&per_page=100&page=1",
            "/repos/example/repo/pulls?state=open&sort=created&direction=asc&per_page=100&page=2",
        ]
    );
}
