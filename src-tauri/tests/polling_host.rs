mod support;

use pr_sniper_lib::github::provider::{GithubClient, Response, Transport};
use pr_sniper_lib::github::ConnectionError;
use pr_sniper_lib::monitoring::{poll, Monitor};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use support::Fixture;

#[derive(Clone)]
struct Session {
    account: Option<u64>,
    remote: u64,
    read: bool,
}

#[derive(Clone, Copy)]
enum Change {
    None,
    Account,
    SignOut,
    Remote,
    ReadPermission,
}

struct SessionTransport {
    captured: Session,
    current: Rc<RefCell<Session>>,
    changed: Rc<Cell<bool>>,
    change: Change,
    name: String,
    include_pull: bool,
}

impl Transport for SessionTransport {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        let body: Value = if path == "/user" {
            json!({"id": self.captured.account.unwrap(), "login": "fixture-user"})
        } else if path == format!("/repos/{}", self.name) {
            json!({
                "id": self.captured.remote, "full_name": self.name,
                "private": false, "archived": false, "disabled": false,
                "permissions": {"pull": self.captured.read}
            })
        } else if path == format!("/repos/{}/pulls?state=open&per_page=1", self.name) {
            json!([])
        } else if path
            == format!(
                "/repos/{}/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1",
                self.name
            )
        {
            if !self.changed.replace(true) {
                let mut current = self.current.borrow_mut();
                match self.change {
                    Change::None => (),
                    Change::Account => current.account = Some(8),
                    Change::SignOut => current.account = None,
                    Change::Remote => current.remote = 901,
                    Change::ReadPermission => current.read = false,
                }
            }
            if self.include_pull {
                json!([{
                    "id": 1031, "number": 31, "title": "Requested from the original account",
                    "user": {"id": 42, "login": "not-watched"},
                    "state": "open", "draft": false, "merged": false, "merged_at": null,
                    "head": {"sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "repo": {"id": 900}},
                    "base": {"sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "repo": {"id": 900}},
                    "updated_at": "2026-09-20T20:00:00Z",
                    "requested_reviewers": [{"id": 7, "login": "fixture-user"}],
                    "requested_teams": []
                }])
            } else {
                json!([])
            }
        } else {
            panic!("unexpected GET in actual host polling seam: {path}");
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&body).unwrap(),
        })
    }
}

fn stable_client(name: &str) -> GithubClient<SessionTransport> {
    let session = Session {
        account: Some(7),
        remote: if name == "example/second" { 901 } else { 900 },
        read: true,
    };
    GithubClient::new(SessionTransport {
        captured: session.clone(),
        current: Rc::new(RefCell::new(session)),
        changed: Rc::new(Cell::new(false)),
        change: Change::None,
        name: name.into(),
        include_pull: false,
    })
}

fn checkpoint_failure_releases_undispatched_checks(filename: &str) {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.add_repository("example/first").unwrap();
    store.add_repository("example/second").unwrap();
    let mut monitor = Monitor::default();
    assert!(monitor
        .prepare_checks(&store, 1000, false)
        .unwrap()
        .is_empty());
    let collision = fixture.path().join("state").join(filename);
    if collision.is_file() {
        std::fs::remove_file(&collision).unwrap();
    }
    std::fs::create_dir(&collision).unwrap();

    let prepared = monitor.prepare_checks(&store, 1900, false);

    assert!(
        prepared.is_err(),
        "checkpoint failure must not dispatch work"
    );
    std::fs::remove_dir(&collision).unwrap();
    let health = monitor.snapshot();
    assert_eq!(health.len(), 2);
    assert!(
        health.iter().all(|row| !row.in_flight),
        "{filename}: no provider owns the tickets dropped before dispatch"
    );
    assert!(health.iter().all(|row| row.last_success.is_none()));
    assert!(health.iter().all(|row| row.last_failure.is_some()));
    let manual = monitor.prepare_checks(&store, 1901, true).unwrap();
    assert_eq!(manual.len(), 2, "corrected storage must allow Check Now");
    for ticket in manual {
        let outcome = poll(&ticket, || Ok(stable_client(&ticket.name)));
        assert!(outcome.is_ok());
        monitor.finish(&store, ticket, outcome, 1902).unwrap();
    }
    let scheduled = monitor.prepare_checks(&store, 2801, false).unwrap();
    assert_eq!(scheduled.len(), 2, "the next real schedule must still run");
    for ticket in scheduled {
        let outcome = poll(&ticket, || Ok(stable_client(&ticket.name)));
        assert!(outcome.is_ok());
        monitor.finish(&store, ticket, outcome, 2802).unwrap();
    }
    assert!(monitor.snapshot().iter().all(|row| {
        !row.in_flight && row.last_success == Some(2802) && row.last_failure.is_none()
    }));
}

#[test]
fn failed_cursor_temporary_write_releases_all_undispatched_checks() {
    checkpoint_failure_releases_undispatched_checks("poll-cursors.json.tmp");
}

#[test]
fn failed_cursor_rename_releases_all_undispatched_checks() {
    checkpoint_failure_releases_undispatched_checks("poll-cursors.json");
}

#[test]
fn failed_health_temporary_write_releases_all_undispatched_checks() {
    checkpoint_failure_releases_undispatched_checks("polling.json.tmp");
}

#[test]
fn failed_health_rename_releases_all_undispatched_checks() {
    checkpoint_failure_releases_undispatched_checks("polling.json");
}

fn delayed_poll_rechecks_the_current_session(change: Change, expected: ConnectionError) {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.add_repository("example/project").unwrap();
    let mut monitor = Monitor::default();
    let ticket = monitor
        .prepare_checks(&store, 1000, true)
        .unwrap()
        .pop()
        .unwrap();
    let current = Rc::new(RefCell::new(Session {
        account: Some(7),
        remote: 900,
        read: true,
    }));
    let changed = Rc::new(Cell::new(false));

    let outcome = poll(&ticket, || {
        let captured = current.borrow().clone();
        if captured.account.is_none() {
            return Err(ConnectionError::SignedOut);
        }
        Ok(GithubClient::new(SessionTransport {
            captured,
            current: Rc::clone(&current),
            changed: Rc::clone(&changed),
            change,
            name: ticket.name.clone(),
            include_pull: true,
        }))
    });

    assert!(changed.get(), "the session must change during the real GET");
    assert_eq!(outcome.as_ref().err().copied(), Some(expected));
    monitor.finish(&store, ticket, outcome, 1001).unwrap();
    assert!(fixture.store().load_queue().unwrap().is_empty());
    assert_eq!(monitor.snapshot()[0].last_success, None);
    assert_eq!(monitor.snapshot()[0].last_failure, Some(expected));
    let settings = store.load_settings().unwrap();
    let mut restarted = Monitor::restore(&fixture.store()).unwrap();
    let next = restarted
        .begin(&settings, 1100, true)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(next.updated_after, None);
    assert_eq!(next.expected_account_id, None);
}

#[test]
fn account_switch_during_read_rejects_the_old_requested_reviewer() {
    delayed_poll_rechecks_the_current_session(Change::Account, ConnectionError::WrongIdentity);
}

#[test]
fn signout_during_read_rejects_a_still_valid_captured_token() {
    delayed_poll_rechecks_the_current_session(Change::SignOut, ConnectionError::SignedOut);
}

#[test]
fn remote_replacement_during_read_cannot_advance_success_or_cursor() {
    delayed_poll_rechecks_the_current_session(Change::Remote, ConnectionError::RepositoryChanged);
}

#[test]
fn read_permission_loss_during_read_cannot_admit_old_results() {
    delayed_poll_rechecks_the_current_session(
        Change::ReadPermission,
        ConnectionError::MissingReadPermission,
    );
}
