mod support;

use pr_sniper_lib::github::metadata::{Lifecycle, PullRequest, RequestedTeam};
use pr_sniper_lib::github::provider::{
    Capabilities, CommentCapability, Connection, RemoteRepository,
};
use pr_sniper_lib::github::{ConnectionError, Identity};
use pr_sniper_lib::monitoring::{Monitor, PollResult};
use pr_sniper_lib::policy::{Policy, WatchedIdentity};
use pr_sniper_lib::storage::{Settings, Store};
use support::Fixture;

fn configured(store: &Store) -> Settings {
    store
        .save_defaults(Policy {
            watched_authors: vec![WatchedIdentity {
                id: "42".into(),
                login: "old-author-login".into(),
            }],
            automatic_agent_start: true,
            ..Policy::default()
        })
        .unwrap();
    store.add_repository("example/project").unwrap()
}

fn identity(id: &str, login: &str) -> Identity {
    Identity {
        id: id.into(),
        login: login.into(),
    }
}

fn candidate() -> PullRequest {
    PullRequest {
        id: "1031".into(),
        number: 31,
        title: "Review me".into(),
        author: Some(identity("42", "renamed-author")),
        requested_reviewers: vec![],
        requested_teams: vec![],
        state: Lifecycle::Open,
        draft: false,
        head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        base_sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        head_repository_id: Some("900".into()),
        base_repository_id: "900".into(),
        updated_at: "2026-09-20T20:00:00Z".into(),
        files: vec![],
    }
}

fn result(pulls: Vec<PullRequest>) -> PollResult {
    PollResult {
        connection: Connection {
            identity: identity("7", "signed-in-user"),
            repository: RemoteRepository {
                id: "900".into(),
                name: "example/project".into(),
            },
            capabilities: Capabilities {
                read: true,
                comment: CommentCapability::Unknown,
            },
        },
        pull_requests: pulls,
    }
}

fn poll(monitor: &mut Monitor, store: &Store, now: i64, result: PollResult) {
    let settings = store.load_settings().unwrap();
    let ticket = monitor.begin(&settings, now, true).unwrap().pop().unwrap();
    monitor.finish(store, ticket, Ok(result), now + 1).unwrap();
}

#[test]
fn author_login_changes_do_not_change_stable_eligibility_or_job_identity() {
    let fixture = Fixture::new();
    let store = fixture.store();
    configured(&store);
    let mut monitor = Monitor::default();

    poll(&mut monitor, &store, 1000, result(vec![candidate()]));

    let jobs = fixture.store().load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].provider, "github");
    assert_eq!(jobs[0].repository_id, "900");
    assert_eq!(jobs[0].pull_request_id, "1031");
    assert_eq!(jobs[0].head_sha, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert!(jobs[0].watched_author);
    assert!(!jobs[0].requested_reviewer);
    assert_eq!(jobs[0].waiting, "agent_not_implemented");
}

#[test]
fn lifecycle_and_unmatched_identity_never_admit_ineligible_work() {
    let fixture = Fixture::new();
    let store = fixture.store();
    configured(&store);
    let mut monitor = Monitor::default();
    let mut wrong_id = candidate();
    wrong_id.author = Some(identity("43", "old-author-login"));
    let mut closed = candidate();
    closed.state = Lifecycle::Closed;
    let mut merged = candidate();
    merged.state = Lifecycle::Merged;
    let mut draft = candidate();
    draft.draft = true;
    let mut deleted_author = candidate();
    deleted_author.author = None;
    let mut team_only = wrong_id.clone();
    team_only.requested_teams = vec![RequestedTeam {
        id: "7".into(),
        slug: "signed-in-user".into(),
    }];

    poll(
        &mut monitor,
        &store,
        1000,
        result(vec![
            wrong_id,
            closed,
            merged,
            draft,
            deleted_author,
            team_only,
        ]),
    );

    assert!(fixture.store().load_queue().unwrap().is_empty());
    assert_eq!(monitor.snapshot()[0].last_success, Some(1001));
}

#[test]
fn reviewer_only_and_fork_work_require_trust_even_with_automatic_start() {
    let fixture = Fixture::new();
    let store = fixture.store();
    configured(&store);
    let mut reviewer_only = candidate();
    reviewer_only.author = None;
    reviewer_only.requested_reviewers = vec![identity("7", "renamed-reviewer")];
    let mut fork = candidate();
    fork.id = "1032".into();
    fork.number = 32;
    fork.head_repository_id = Some("901".into());
    let mut unknown_head = candidate();
    unknown_head.id = "1033".into();
    unknown_head.number = 33;
    unknown_head.head_repository_id = None;

    poll(
        &mut Monitor::default(),
        &store,
        1000,
        result(vec![reviewer_only, fork, unknown_head]),
    );

    let jobs = fixture.store().load_queue().unwrap();
    assert_eq!(jobs.len(), 3);
    assert!(!jobs[0].watched_author);
    assert!(jobs[0].requested_reviewer);
    assert_eq!(jobs[0].waiting, "trust_confirmation");
    assert_eq!(jobs[1].waiting, "trust_confirmation");
    assert_eq!(jobs[2].waiting, "trust_confirmation");
}

#[test]
fn disabled_reviewer_trigger_does_not_admit_a_nonwatched_author() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = configured(&store);
    settings.defaults.reviewer_assignment = false;
    store.save_settings(&settings).unwrap();
    let mut pull = candidate();
    pull.author = Some(identity("99", "outside-watchlist"));
    pull.requested_reviewers = vec![identity("7", "signed-in-user")];

    poll(&mut Monitor::default(), &store, 1000, result(vec![pull]));

    assert!(fixture.store().load_queue().unwrap().is_empty());
}

#[test]
fn both_triggers_repeated_polls_and_store_reopen_admit_one_job_per_head() {
    let fixture = Fixture::new();
    let store = fixture.store();
    configured(&store);
    let mut pull = candidate();
    pull.requested_reviewers = vec![identity("7", "signed-in-user")];
    poll(
        &mut Monitor::default(),
        &store,
        1000,
        result(vec![pull.clone()]),
    );
    drop(store);
    let reopened = fixture.store();
    let mut restarted = Monitor::default();
    poll(&mut restarted, &reopened, 1100, result(vec![pull.clone()]));
    let jobs = reopened.load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].watched_author && jobs[0].requested_reviewer);
    assert_eq!(jobs[0].detected_at, 1001);
    pull.head_sha = "cccccccccccccccccccccccccccccccccccccccc".into();

    poll(&mut restarted, &reopened, 1200, result(vec![pull]));

    let jobs = fixture.store().load_queue().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[1].head_sha, "cccccccccccccccccccccccccccccccccccccccc");
    assert_eq!(jobs[1].detected_at, 1201);
    assert!(fixture.path().join("state/queue.json").is_file());
}

#[test]
fn reviewer_removal_and_reassignment_do_not_duplicate_the_same_policy_head() {
    let fixture = Fixture::new();
    let store = fixture.store();
    configured(&store);
    let mut pull = candidate();
    pull.author = Some(identity("99", "outside-watchlist"));
    pull.requested_reviewers = vec![identity("7", "signed-in-user")];
    let mut monitor = Monitor::default();
    poll(&mut monitor, &store, 1000, result(vec![pull.clone()]));
    let mut removed = pull.clone();
    removed.requested_reviewers.clear();
    poll(&mut monitor, &store, 1100, result(vec![removed]));

    poll(&mut monitor, &store, 1200, result(vec![pull]));

    assert_eq!(fixture.store().load_queue().unwrap().len(), 1);
}

#[test]
fn delayed_reads_cannot_admit_after_disable_remove_retarget_or_policy_change() {
    for change in ["disable", "remove", "retarget", "watchlist", "reviewer"] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let settings = configured(&store);
        let id = &settings.repositories[0].id;
        let mut monitor = Monitor::default();
        let ticket = monitor.begin(&settings, 1000, true).unwrap().pop().unwrap();
        match change {
            "disable" => {
                store
                    .update_repository(id, "example/project", false)
                    .unwrap();
            }
            "remove" => {
                store.remove_repository(id).unwrap();
            }
            "retarget" => {
                store.update_repository(id, "example/other", true).unwrap();
            }
            "watchlist" => {
                let mut policy = settings.defaults;
                policy.watched_authors.clear();
                store.save_defaults(policy).unwrap();
            }
            "reviewer" => {
                let mut policy = settings.defaults;
                policy.reviewer_assignment = false;
                store.save_defaults(policy).unwrap();
            }
            _ => unreachable!(),
        }

        monitor
            .finish(&store, ticket, Ok(result(vec![candidate()])), 1001)
            .unwrap();

        assert!(fixture.store().load_queue().unwrap().is_empty(), "{change}");
        assert_eq!(monitor.snapshot()[0].last_success, None, "{change}");
        assert!(monitor.snapshot()[0].last_failure.is_some(), "{change}");
    }
}

#[test]
fn missing_verified_read_capability_cannot_become_a_successful_admission() {
    let fixture = Fixture::new();
    let store = fixture.store();
    configured(&store);
    let mut unverified = result(vec![candidate()]);
    unverified.connection.capabilities.read = false;
    let mut monitor = Monitor::default();

    poll(&mut monitor, &store, 1000, unverified);

    assert!(fixture.store().load_queue().unwrap().is_empty());
    assert_eq!(monitor.snapshot()[0].last_success, None);
    assert_eq!(
        monitor.snapshot()[0].last_failure,
        Some(ConnectionError::MissingReadPermission)
    );
}

#[test]
fn failed_poll_keeps_prior_success_and_records_the_actual_safe_failure() {
    let failures = [
        ConnectionError::SignedOut,
        ConnectionError::WrongIdentity,
        ConnectionError::MissingReadPermission,
        ConnectionError::RateLimited,
        ConnectionError::Network,
        ConnectionError::IncompleteRead,
        ConnectionError::RevisionChanged,
        ConnectionError::RepositoryChanged,
        ConnectionError::Configuration,
    ];
    for failure in failures {
        let fixture = Fixture::new();
        let store = fixture.store();
        let settings = configured(&store);
        let mut monitor = Monitor::default();
        poll(&mut monitor, &store, 1000, result(vec![]));
        let ticket = monitor.begin(&settings, 1100, true).unwrap().pop().unwrap();

        monitor.finish(&store, ticket, Err(failure), 1101).unwrap();

        let health = &monitor.snapshot()[0];
        assert_eq!(health.last_attempt, Some(1100), "{failure:?}");
        assert_eq!(health.last_success, Some(1001), "{failure:?}");
        assert_eq!(health.last_failure, Some(failure));
        assert!(!health.in_flight);
        assert!(fixture.store().load_queue().unwrap().is_empty());
    }
}

#[test]
fn successful_poll_supplies_the_newest_update_cursor_to_the_next_check() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = configured(&store);
    let mut newer = candidate();
    newer.id = "1032".into();
    newer.number = 32;
    newer.updated_at = "2026-09-20T20:02:00Z".into();
    let mut monitor = Monitor::default();
    poll(&mut monitor, &store, 1000, result(vec![newer, candidate()]));

    let ticket = monitor.begin(&settings, 1100, true).unwrap().pop().unwrap();

    assert_eq!(
        ticket.updated_after.as_deref(),
        Some("2026-09-20T20:02:00Z")
    );
    monitor
        .finish(&store, ticket, Err(ConnectionError::IncompleteRead), 1101)
        .unwrap();
    let retry = monitor.begin(&settings, 1200, true).unwrap().pop().unwrap();
    assert_eq!(retry.updated_after.as_deref(), Some("2026-09-20T20:02:00Z"));
}

#[test]
fn restored_monitor_keeps_verified_identity_cursor_and_durable_deduplication() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = configured(&store);
    poll(
        &mut Monitor::default(),
        &store,
        1000,
        result(vec![candidate()]),
    );
    drop(store);
    let reopened = fixture.store();
    let mut monitor = Monitor::restore(&reopened).unwrap();

    let ticket = monitor.begin(&settings, 1100, true).unwrap().pop().unwrap();

    assert_eq!(
        ticket.updated_after.as_deref(),
        Some("2026-09-20T20:00:00Z")
    );
    assert_eq!(ticket.expected_account_id.as_deref(), Some("7"));
    assert_eq!(ticket.expected_repository_id.as_deref(), Some("900"));
    monitor
        .finish(&reopened, ticket, Ok(result(vec![candidate()])), 1101)
        .unwrap();
    assert_eq!(fixture.store().load_queue().unwrap().len(), 1);
}

#[test]
fn watchlist_order_and_display_labels_do_not_change_trigger_policy_identity() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = configured(&store);
    settings.defaults.watched_authors.push(WatchedIdentity {
        id: "55".into(),
        login: "another-author".into(),
    });
    store.save_settings(&settings).unwrap();
    poll(
        &mut Monitor::default(),
        &store,
        1000,
        result(vec![candidate()]),
    );
    settings.defaults.watched_authors.reverse();
    settings.defaults.watched_authors[1].login = "new-display-label".into();
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::restore(&fixture.store()).unwrap();

    let ticket = monitor.begin(&settings, 1100, true).unwrap().pop().unwrap();

    assert_eq!(
        ticket.updated_after.as_deref(),
        Some("2026-09-20T20:00:00Z")
    );
    monitor
        .finish(&store, ticket, Ok(result(vec![candidate()])), 1101)
        .unwrap();
    assert_eq!(fixture.store().load_queue().unwrap().len(), 1);
}

#[test]
fn changed_trigger_policy_invalidates_the_cursor_and_creates_a_distinct_job() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = configured(&store);
    poll(
        &mut Monitor::default(),
        &store,
        1000,
        result(vec![candidate()]),
    );
    settings.defaults.reviewer_assignment = false;
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::restore(&fixture.store()).unwrap();

    let ticket = monitor.begin(&settings, 1100, true).unwrap().pop().unwrap();

    assert_eq!(ticket.updated_after, None);
    monitor
        .finish(&store, ticket, Ok(result(vec![candidate()])), 1101)
        .unwrap();
    let jobs = fixture.store().load_queue().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_ne!(jobs[0].trigger_policy, jobs[1].trigger_policy);
}

#[test]
fn local_repository_readdition_preserves_remote_identity_deduplication() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = configured(&store);
    poll(
        &mut Monitor::default(),
        &store,
        1000,
        result(vec![candidate()]),
    );
    store
        .remove_repository(&settings.repositories[0].id)
        .unwrap();
    let readded = store.add_repository("example/project").unwrap();
    assert_ne!(settings.repositories[0].id, readded.repositories[0].id);

    poll(
        &mut Monitor::restore(&fixture.store()).unwrap(),
        &store,
        1100,
        result(vec![candidate()]),
    );

    let jobs = fixture.store().load_queue().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].repository_id, "900");
}

#[test]
fn retargeting_resets_remote_verification_and_never_reuses_another_repositories_job() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = configured(&store);
    let mut monitor = Monitor::default();
    poll(&mut monitor, &store, 1000, result(vec![candidate()]));
    let changed = store
        .update_repository(&settings.repositories[0].id, "example/other", true)
        .unwrap();
    let mut other = result(vec![candidate()]);
    other.connection.repository = RemoteRepository {
        id: "901".into(),
        name: "example/other".into(),
    };
    other.pull_requests[0].base_repository_id = "901".into();
    other.pull_requests[0].head_repository_id = Some("901".into());

    let ticket = monitor.begin(&changed, 1100, true).unwrap().pop().unwrap();

    assert_eq!(ticket.updated_after, None);
    assert_eq!(ticket.expected_repository_id, None);
    monitor.finish(&store, ticket, Ok(other), 1101).unwrap();
    let jobs = fixture.store().load_queue().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].repository_id, "900");
    assert_eq!(jobs[1].repository_id, "901");
}

#[test]
fn disabling_then_reenabling_a_live_monitor_requires_a_complete_read() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = configured(&store);
    let id = &settings.repositories[0].id;
    let mut monitor = Monitor::default();
    poll(&mut monitor, &store, 1000, result(vec![candidate()]));
    let disabled = store
        .update_repository(id, "example/project", false)
        .unwrap();
    assert!(monitor.begin(&disabled, 1050, false).unwrap().is_empty());
    let enabled = store
        .update_repository(id, "example/project", true)
        .unwrap();

    let ticket = monitor.begin(&enabled, 1100, true).unwrap().pop().unwrap();

    assert_eq!(ticket.updated_after, None);
}

#[test]
fn host_checkpoint_must_preserve_cursor_invalidation_across_restart() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = configured(&store);
    let id = &settings.repositories[0].id;
    let mut monitor = Monitor::default();
    poll(&mut monitor, &store, 1000, result(vec![candidate()]));
    let disabled = store
        .update_repository(id, "example/project", false)
        .unwrap();
    assert!(monitor.begin(&disabled, 1050, false).unwrap().is_empty());
    monitor.checkpoint(&store).unwrap();
    drop(monitor);
    let mut restarted = Monitor::restore(&fixture.store()).unwrap();
    let enabled = store
        .update_repository(id, "example/project", true)
        .unwrap();

    let ticket = restarted
        .begin(&enabled, 1100, true)
        .unwrap()
        .pop()
        .unwrap();

    assert_eq!(ticket.updated_after, None);
}

fn assert_same_remote_exclusion(lifecycle: &str) {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = configured(&store);
    let old_id = &settings.repositories[0].id;
    let mut monitor = Monitor::default();
    let old_ticket = monitor.begin(&settings, 1000, true).unwrap().pop().unwrap();
    match lifecycle {
        "remove" => {
            store.remove_repository(old_id).unwrap();
        }
        "retarget" => {
            store
                .update_repository(old_id, "example/retargeted", true)
                .unwrap();
        }
        _ => unreachable!(),
    }
    store.add_repository("example/unrelated").unwrap();
    let current = store.add_repository("example/project").unwrap();

    let mut tickets = monitor.begin(&current, 1100, true).unwrap();

    assert_eq!(
        tickets
            .iter()
            .map(|ticket| ticket.name.as_str())
            .collect::<Vec<_>>(),
        vec!["example/unrelated"],
        "{lifecycle}: the old example/project read is still outstanding"
    );
    let mut unrelated = result(vec![]);
    unrelated.connection.repository = RemoteRepository {
        id: "902".into(),
        name: "example/unrelated".into(),
    };
    let unrelated_connection = unrelated.connection.clone();
    monitor
        .finish(&store, tickets.pop().unwrap(), Ok(unrelated), 1101)
        .unwrap();
    assert!(monitor.begin(&current, 1102, false).unwrap().is_empty());
    let mut scheduled = monitor.begin(&current, 2000, false).unwrap();
    assert_eq!(
        scheduled
            .iter()
            .map(|ticket| ticket.name.as_str())
            .collect::<Vec<_>>(),
        vec!["example/unrelated"],
        "{lifecycle}: a due tick must not overlap the old remote read"
    );
    monitor
        .finish(
            &store,
            scheduled.pop().unwrap(),
            Ok(PollResult {
                connection: unrelated_connection,
                pull_requests: vec![],
            }),
            2001,
        )
        .unwrap();
    monitor
        .finish(&store, old_ticket, Ok(result(vec![candidate()])), 2002)
        .unwrap();
    assert!(fixture.store().load_queue().unwrap().is_empty());
    let after_release = monitor.begin(&current, 2003, true).unwrap();
    assert_eq!(
        after_release
            .iter()
            .filter(|ticket| ticket.name == "example/project")
            .count(),
        1,
        "{lifecycle}: new configuration must become runnable after the old read ends"
    );
}

#[test]
fn remove_and_readd_cannot_overlap_an_existing_read_for_the_same_remote() {
    assert_same_remote_exclusion("remove");
}

#[test]
fn retarget_and_readd_cannot_overlap_an_existing_read_for_the_same_remote() {
    assert_same_remote_exclusion("retarget");
}
