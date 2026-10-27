//! Azure DevOps polling end to end over the fake transport and token source:
//! admission, drafts, iterations as revisions, and parked failure causes.

use super::*;
use crate::monitoring::{FailureClass, Monitor, OperationState, PollResult, PollTicket};
use crate::storage::{ResourceEdit, Store};

const ID: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const AGENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const NOW: i64 = 1_800_000_000;

fn sha(n: u32) -> String {
    format!("{n:040x}")
}

fn git(path: &str) -> String {
    format!("{ORG}/{PROJECT}/_apis/git/repositories/{REPO}/{path}")
}

fn raw_pull(number: u64, author: &str, source: u32, draft: bool) -> Value {
    json!({
        "pullRequestId": number,
        "status": "active",
        "title": format!("Change {number}"),
        "createdBy": {"id": author, "displayName": "Someone"},
        "creationDate": "2026-10-20T10:00:00Z",
        "isDraft": draft,
        "repository": {"id": REPO},
        "lastMergeSourceCommit": {"commitId": sha(source)},
        "lastMergeTargetCommit": {"commitId": sha(900)},
    })
}

/// A monitored Azure DevOps repository that admits Blake's pull requests,
/// enabled through the same resource save as Settings.
fn monitored() -> (tempfile::TempDir, Store, Monitor) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().to_path_buf());
    let mut settings = store.load_settings().unwrap();
    settings.agents.push(
        serde_json::from_value(json!({
            "id": AGENT, "name": "Reviewer", "model": "fixture-model",
            "prompt": "Find defects.", "signature": "fixture",
            "ai_account": {"provider": "copilot", "account_id": "33"}
        }))
        .unwrap(),
    );
    let repository: crate::storage::Repository = serde_json::from_value(json!({
        "id": ID, "provider": "azure_devops", "name": "contoso/Web/app", "enabled": false,
        "cron": "*/15 * * * *",
        "provider_account_id": AVERY, "provider_repository_id": key().id(),
        "admission": {"mode": "filtered", "rules": [
            {"kind": "include_users", "enabled": true,
             "users": [{"id": BLAKE, "login": "Blake Example"}]}
        ]},
        "assignments": [{
            "id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc", "agent_id": AGENT,
            "schedule": {"kind": "interval", "minutes": 5, "timezone": "UTC"},
            "comment": false, "approve": false
        }]
    }))
    .unwrap();
    settings.repositories.push(repository.clone());
    store.save_settings(&settings).unwrap();
    let mut enabled = repository.clone();
    enabled.enabled = true;
    store
        .save_resource(ResourceEdit::Repository {
            id: ID.into(),
            expected: Some(Box::new(repository)),
            value: Some(Box::new(enabled)),
        })
        .unwrap();
    let monitor = Monitor::restore(&store).unwrap();
    (root, store, monitor)
}

fn open(
    tokens: Arc<FakeTokens>,
    wire: &Arc<FakeWire>,
    runtime: &tokio::runtime::Runtime,
) -> Result<Reader, ConnectionError> {
    Reader::open(
        client(tokens, Arc::clone(wire)),
        runtime.handle().clone(),
        &Identity {
            id: AVERY.into(),
            display_name: "Avery Example".into(),
            unique_name: None,
        },
    )
}

/// One poll of `ticket` through the provider read interfaces.
fn poll(
    tokens: Arc<FakeTokens>,
    wire: &Arc<FakeWire>,
    ticket: &PollTicket,
) -> Result<PollResult, ConnectionError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let reader = open(tokens, wire, &runtime)?;
    let (connection, pull_requests) = crate::poll_repository(&reader, ticket)?;
    Ok(PollResult {
        connection,
        pull_requests,
        states: Default::default(),
    })
}

fn signed_in_wire(listing: Value) -> Arc<FakeWire> {
    let wire = Arc::new(FakeWire::default());
    wire.json(
        &format!("{ORG}/_apis/connectionData"),
        identity_body(AVERY, "Avery Example"),
    )
    .json(
        &format!("{ORG}/Web/_apis/git/repositories/app?"),
        raw_repository(REPO, "app", "Web"),
    )
    .json(&git("pullrequests?"), listing);
    wire
}

fn monitor_accounts() -> std::collections::BTreeMap<String, crate::monitoring::AccountAvailability>
{
    let mut auth = crate::GithubAuth::new();
    auth.set_azure_devops_account(
        Some(Person {
            id: AVERY.into(),
            login: "Avery Example".into(),
        }),
        None,
    );
    auth.monitoring_accounts()
}

fn next_ticket(monitor: &mut Monitor, store: &Store, now: i64) -> PollTicket {
    let mut tickets = monitor.prepare_checks(store, now, true).unwrap();
    assert_eq!(tickets.len(), 1);
    tickets.remove(0)
}

#[test]
fn admits_picked_teammates_ignores_drafts_and_takes_each_iteration_as_a_revision() {
    let (_root, store, mut monitor) = monitored();
    let ticket = next_ticket(&mut monitor, &store, NOW);
    assert_eq!(ticket.provider, crate::storage::ProviderId::AzureDevops);
    let wire = signed_in_wire(json!({"value": [
        raw_pull(1, BLAKE, 1, false),
        raw_pull(2, CASEY, 2, false),
        raw_pull(3, BLAKE, 3, true),
    ]}));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW).unwrap();
    let jobs = store.load_queue().unwrap();
    assert_eq!(
        jobs.iter()
            .map(|job| (job.number, job.provider.as_str(), job.head_sha.clone()))
            .collect::<Vec<_>>(),
        [(1, "azure_devops", sha(1))]
    );
    assert_eq!(jobs[0].author_id.as_deref(), Some(BLAKE));
    assert_eq!(jobs[0].repository_id, key().id());
    // Admitted, and the provider can run its review.
    let work = crate::capacity::candidates(&store, NOW).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].reason, None);
    // GitHub reachability is never probed for, or holds, Azure DevOps work;
    // only its AI account is.
    let accounts = monitor_accounts();
    assert!(crate::connectivity::wanted_scopes(&store, &accounts)
        .unwrap()
        .iter()
        .all(|scope| matches!(scope, crate::backpressure::Scope::Copilot { .. })));

    // A push makes a new iteration: a new revision supersedes the old work.
    let ticket = next_ticket(&mut monitor, &store, NOW + 900);
    let wire = signed_in_wire(json!({"value": [raw_pull(1, BLAKE, 4, false)]}));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW + 900).unwrap();
    let tracked = store.load_queue_state().unwrap().tracked;
    assert_eq!(tracked.len(), 1);
    assert_eq!(
        (tracked[0].iteration, tracked[0].head_sha.clone()),
        (2, sha(4))
    );
    let jobs = store.load_queue().unwrap();
    let current: Vec<_> = jobs
        .iter()
        .filter(|job| !crate::monitoring::retired(job))
        .map(|job| job.head_sha.clone())
        .collect();
    assert_eq!(current, [sha(4)]);
    assert!(jobs
        .iter()
        .any(|job| job.head_sha == sha(1) && crate::monitoring::retired(job)));

    // Ready for review: the draft is admitted once it leaves draft.
    let ticket = next_ticket(&mut monitor, &store, NOW + 1800);
    let wire = signed_in_wire(json!({"value": [
        raw_pull(1, BLAKE, 4, false),
        raw_pull(3, BLAKE, 3, false),
    ]}));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW + 1800).unwrap();
    let mut numbers: Vec<_> = store
        .load_queue()
        .unwrap()
        .iter()
        .filter(|job| !crate::monitoring::retired(job))
        .map(|job| job.number)
        .collect();
    numbers.sort();
    assert_eq!(numbers, [1, 3]);
}

/// The cause a repository poll parks with after failing every attempt it
/// gets: on a token, or on the listing after a good token.
fn parked_class(token: Option<TokenError>, listing: Option<Response>) -> Option<FailureClass> {
    let (_root, store, mut monitor) = monitored();
    let mut now = NOW;
    for _ in 0..crate::monitoring::MAX_FAILED_ATTEMPTS {
        let ticket = next_ticket(&mut monitor, &store, now);
        let wire = Arc::new(FakeWire::default());
        if let Some(response) = listing.clone() {
            wire.json(
                &format!("{ORG}/_apis/connectionData"),
                identity_body(AVERY, "Avery Example"),
            )
            .json(
                &format!("{ORG}/Web/_apis/git/repositories/app?"),
                raw_repository(REPO, "app", "Web"),
            )
            .on(&git("pullrequests?"), Ok(response));
        }
        let tokens = Arc::new(FakeTokens {
            fail: token,
            ..FakeTokens::default()
        });
        let result = poll(tokens, &wire, &ticket);
        assert!(result.is_err());
        monitor.finish(&store, ticket, result, now).unwrap_err();
        let operation = monitor
            .snapshot()
            .into_iter()
            .find_map(|health| health.operation)
            .unwrap();
        if operation.parked() {
            return operation.failure_class();
        }
        now = operation.next_attempt_at.unwrap();
    }
    None
}

#[test]
fn failures_park_with_the_mapped_cause() {
    use FailureClass::{Credentials, Network, Other, Permission};
    for (token, expected) in [
        (TokenError::NotLoggedIn, Credentials),
        (TokenError::NoAccountInTenant, Credentials),
        (
            TokenError::InteractionRequired { aadsts: 50076 },
            Credentials,
        ),
        (TokenError::AzMissing, Credentials),
        (TokenError::TenantAccessBlocked, Permission),
        (TokenError::Network, Network),
        (TokenError::Timeout, Network),
        (
            TokenError::Other {
                exit_code: Some(1),
                aadsts: None,
            },
            Other,
        ),
        (TokenError::MalformedOutput, Other),
    ] {
        assert_eq!(parked_class(Some(token), None), Some(expected), "{token:?}");
    }
    assert_eq!(
        parked_class(None, Some(status(403, None))),
        Some(Permission)
    );
}

#[test]
fn a_credential_failure_waits_for_sign_in_without_another_attempt() {
    let (_root, store, mut monitor) = monitored();
    let mut auth = crate::GithubAuth::new();
    let avery = Person {
        id: AVERY.into(),
        login: "Avery Example".into(),
    };
    auth.set_azure_devops_account(Some(avery.clone()), None);
    let accounts = auth.monitoring_accounts();
    let ticket = monitor
        .prepare_checks_with_accounts(&store, &accounts, NOW, true)
        .unwrap()
        .remove(0);
    let tokens = Arc::new(FakeTokens {
        fail: Some(TokenError::NotLoggedIn),
        ..FakeTokens::default()
    });
    let result = poll(tokens, &Arc::new(FakeWire::default()), &ticket);
    assert_eq!(result.as_ref().err(), Some(&ConnectionError::SignedOut));
    assert!(auth.publish_azure_devops_failure(AVERY, ConnectionError::SignedOut));
    assert!(auth.repository_account_allowed(AVERY).is_err());
    let accounts = auth.monitoring_accounts();
    monitor
        .finish_with_accounts(&store, &accounts, ticket, result, NOW)
        .unwrap_err();
    let parked = |monitor: &Monitor| {
        monitor
            .snapshot()
            .into_iter()
            .find_map(|health| health.operation)
            .unwrap()
    };
    assert_eq!(parked(&monitor).state, OperationState::Failed);
    assert_eq!(
        parked(&monitor).failure_class(),
        Some(FailureClass::Credentials)
    );

    // Neither later occurrences nor Check Now start another poll (no Azure
    // CLI run) while the identity is signed out.
    for later in [NOW + 900, NOW + 3 * 3600] {
        monitor.auto_resume(&store, &accounts, later).unwrap();
        assert!(monitor
            .prepare_checks_with_accounts(&store, &accounts, later, true)
            .unwrap()
            .is_empty());
    }
    // Replaying Settings without a fresh probe (the failure dropped it)
    // tells nothing new and keeps the hold.
    let stale = crate::azure_devops::host::signed_in(
        &crate::azure_devops::setup::AccountView::Unavailable,
        Some(&avery),
    );
    assert_eq!(stale, None);
    auth.set_azure_devops_account(Some(avery.clone()), stale);
    assert!(!auth.monitoring_accounts()[AVERY].connected);
    // A failure that is not about credentials never holds the account.
    assert!(!auth.publish_azure_devops_failure(AVERY, ConnectionError::Network));
    assert_eq!(
        auth.repository_account_allowed(AVERY),
        Err(ConnectionError::SignedOut)
    );

    // A fresh probe sees the identity connected again: the parked poll resumes.
    let fresh = crate::azure_devops::host::signed_in(
        &crate::azure_devops::setup::AccountView::Connected {
            identity: Identity {
                id: AVERY.into(),
                display_name: "Avery Example".into(),
                unique_name: None,
            },
        },
        Some(&avery),
    );
    auth.set_azure_devops_account(Some(avery), fresh);
    let accounts = auth.monitoring_accounts();
    assert!(accounts[AVERY].connected);
    monitor
        .auto_resume(&store, &accounts, NOW + 4 * 3600)
        .unwrap();
    assert_eq!(
        monitor
            .prepare_checks_with_accounts(&store, &accounts, NOW + 4 * 3600, true)
            .unwrap()
            .len(),
        1
    );
}
