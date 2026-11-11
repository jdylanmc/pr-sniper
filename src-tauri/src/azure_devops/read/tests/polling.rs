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
    monitored_with(json!({"mode": "filtered", "rules": [
        {"kind": "include_users", "enabled": true,
         "users": [{"id": BLAKE, "login": "Blake Example"}]}
    ]}))
}

/// A monitored Azure DevOps repository with `admission`.
fn monitored_with(admission: Value) -> (tempfile::TempDir, Store, Monitor) {
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
        "admission": admission,
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
    let (connection, mut polled) = crate::poll_repository(&reader, ticket, ticket.prepared_at)?;
    let (facts, held) = crate::path_facts(&reader, &connection, &mut polled.pulls, ticket)?;
    Ok(PollResult {
        facts,
        listing: crate::hold(polled.cursor, held),
        connection,
        pull_requests: polled.pulls,
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
    // its own host and its AI account are.
    let accounts = monitor_accounts();
    let wanted = crate::connectivity::wanted_scopes(&store, &accounts).unwrap();
    assert!(wanted.iter().all(|scope| matches!(
        scope,
        crate::backpressure::Scope::Copilot { .. } | crate::backpressure::Scope::AzureDevops { .. }
    )));
    assert!(wanted.contains(&crate::backpressure::Scope::azure_devops(
        &jobs[0].account_id
    )));

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

#[test]
fn the_paths_gate_reads_iteration_changes_and_keeps_out_other_prs() {
    let (_root, store, mut monitor) = monitored();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].paths = vec!["src/".into(), "!src/gen/".into()];
    store.save_settings(&settings).unwrap();
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = signed_in_wire(json!({"value": [
        raw_pull(1, BLAKE, 1, false),
        raw_pull(2, BLAKE, 2, false),
        raw_pull(3, BLAKE, 3, false),
    ]}));
    let edit =
        |path: &str| json!({"item": {"path": path, "objectId": sha(50)}, "changeType": "edit"});
    for (number, path) in [(1, "/src/a.rs"), (2, "/docs/a.md"), (3, "/src/gen/a.rs")] {
        wire.json(
            &git(&format!("pullRequests/{number}/iterations?")),
            json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(number)}}]}),
        )
        .json(
            &git(&format!("pullRequests/{number}/iterations/1/changes?")),
            json!({"changeEntries": [edit(path)]}),
        );
    }
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW).unwrap();
    let numbers: Vec<_> = store
        .load_queue()
        .unwrap()
        .iter()
        .map(|j| j.number)
        .collect();
    assert_eq!(numbers, [1], "only the PR with an admitted path is queued");
}

#[test]
fn one_pull_request_with_malformed_changes_waits_and_the_rest_are_admitted() {
    let (_root, store, mut monitor) = monitored();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].paths = vec!["src/".into()];
    store.save_settings(&settings).unwrap();
    let listing = json!({"value": [
        raw_pull(1, BLAKE, 1, false),
        raw_pull(2, BLAKE, 2, false),
        raw_pull(3, BLAKE, 3, false),
    ]});
    let edit = json!({"item": {"path": "/src/a.rs", "objectId": sha(50)}, "changeType": "edit"});
    // An entry with neither an item path nor an earlier one names no file.
    let unnamed = json!({"item": {"path": null, "objectId": sha(51)}, "changeType": "edit"});
    let wire_for = |second: &Value, listing: &Value| {
        let wire = signed_in_wire(listing.clone());
        for number in 1..=3u32 {
            let changes = if number == 2 { second } else { &edit };
            wire.json(
                &git(&format!("pullRequests/{number}/iterations?")),
                json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(number)}}]}),
            )
            .json(
                &git(&format!("pullRequests/{number}/iterations/1/changes?")),
                json!({"changeEntries": [changes]}),
            );
        }
        wire
    };
    let numbers = || {
        let mut numbers: Vec<_> = store
            .load_queue()
            .unwrap()
            .iter()
            .map(|j| j.number)
            .collect();
        numbers.sort();
        numbers
    };

    let ticket = next_ticket(&mut monitor, &store, NOW);
    let result = poll(
        Arc::new(FakeTokens::default()),
        &wire_for(&unnamed, &listing),
        &ticket,
    );
    monitor.finish(&store, ticket, result, NOW).unwrap();
    assert_eq!(numbers(), [1, 3]);
    let health = monitor.snapshot().remove(0);
    assert!(health.last_success.is_some());
    assert_eq!(health.last_failure, None);

    // Readable again, and read by ID though the listing has moved past it:
    // the held-out pull request is admitted on the next poll.
    let ticket = next_ticket(&mut monitor, &store, NOW + 900);
    let moved_on = json!({"value": [raw_pull(1, BLAKE, 1, false), raw_pull(3, BLAKE, 3, false)]});
    let wire = wire_for(&edit, &moved_on);
    wire.json(&git("pullrequests/2?"), raw_pull(2, BLAKE, 2, false));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW + 900).unwrap();
    assert_eq!(numbers(), [1, 2, 3]);
}

#[test]
fn a_held_pull_request_past_the_by_id_budget_stays_held() {
    let (_root, store, mut monitor) = monitored();
    let mut ticket = next_ticket(&mut monitor, &store, NOW);
    // 200 tracked PRs missing from the listing fill the by-ID budget, read
    // after number 2: held PR 2 is not read this poll.
    ticket.tracked = (1000..1200).map(|n| (n.to_string(), n)).collect();
    ticket.listing = Some(crate::provider::ListingCursor {
        tracked_after: 2,
        held: vec![("2".into(), 2)],
        ..Default::default()
    });
    let wire = signed_in_wire(json!({"value": []}));
    for n in 1000..1200 {
        wire.json(
            &git(&format!("pullrequests/{n}?")),
            raw_pull(n, CASEY, 7, false),
        );
    }
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket).unwrap();
    assert!(!wire
        .urls()
        .iter()
        .any(|url| url.contains("/pullrequests/2?")));
    let listing = result.listing.unwrap();
    assert_eq!(listing.held, [("2".to_string(), 2)]);

    // Its turn: read by ID, facts read, no longer held.
    ticket.listing = Some(listing);
    ticket.tracked.clear();
    let wire = signed_in_wire(json!({"value": []}));
    wire.json(&git("pullrequests/2?"), raw_pull(2, BLAKE, 2, false));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket).unwrap();
    assert_eq!(result.pull_requests.len(), 1);
    assert!(result.listing.is_none_or(|listing| listing.held.is_empty()));
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
fn an_azure_devops_outage_suspends_polls_without_spending_attempts_then_resumes() {
    use crate::backpressure::Scope;
    use crate::connectivity::{record, Observation};
    let (_root, store, mut monitor) = monitored();
    let ado = Scope::azure_devops(AVERY);
    let ticket = next_ticket(&mut monitor, &store, NOW);
    // The poll loses its connection; the Azure DevOps host is confirmed down.
    record(&store, ado.clone(), Observation::Unreachable, NOW + 1).unwrap();
    let tokens = Arc::new(FakeTokens {
        fail: Some(TokenError::Network),
        ..FakeTokens::default()
    });
    let result = poll(tokens, &Arc::new(FakeWire::default()), &ticket);
    assert_eq!(result.as_ref().err(), Some(&ConnectionError::Network));
    let _ = monitor.finish(&store, ticket, result, NOW + 2);
    let operation = |monitor: &Monitor| {
        monitor
            .snapshot()
            .into_iter()
            .find_map(|health| health.operation)
            .unwrap()
    };
    let suspended = operation(&monitor);
    assert!(!suspended.parked());
    assert_eq!(suspended.attempt_count, 0);
    assert!(suspended.offline_since.is_some());
    // GitHub reachability neither holds nor frees it.
    record(
        &store,
        Scope::github(AVERY),
        Observation::Reachable,
        NOW + 30,
    )
    .unwrap();
    assert!(monitor
        .prepare_checks(&store, NOW + 600, true)
        .unwrap()
        .is_empty());
    // Recovery: the next check after a reachable probe polls again.
    record(&store, ado, Observation::Reachable, NOW + 700).unwrap();
    let ticket = next_ticket(&mut monitor, &store, NOW + 701);
    assert_eq!(ticket.provider, crate::storage::ProviderId::AzureDevops);
    assert_eq!(operation(&monitor).offline_since, None);
    assert!(!operation(&monitor).parked());
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

fn connected_wire() -> Arc<FakeWire> {
    let wire = Arc::new(FakeWire::default());
    wire.json(
        &format!("{ORG}/_apis/connectionData"),
        identity_body(AVERY, "Avery Example"),
    )
    .json(
        &format!("{ORG}/Web/_apis/git/repositories/app?"),
        raw_repository(REPO, "app", "Web"),
    );
    wire
}

fn creator(id: &str) -> String {
    git(&format!("pullrequests?searchCriteria.creatorId={id}"))
}

fn reviewer(id: &str) -> String {
    git(&format!("pullrequests?searchCriteria.reviewerId={id}"))
}

/// The whole active listing of a very large repository: `count` drafts,
/// newest first, in pages of 100.
fn huge_listing(wire: &FakeWire, count: u64, ready: &[(u64, &str)]) {
    let numbers: Vec<u64> = (1..=count).rev().collect();
    for chunk in numbers.chunks(100) {
        let page: Vec<Value> = chunk
            .iter()
            .map(|&n| match ready.iter().find(|(r, _)| *r == n) {
                Some((_, author)) => raw_pull(n, author, n as u32, false),
                None => raw_pull(n, CASEY, n as u32, true),
            })
            .collect();
        wire.json(
            &git("pullrequests?searchCriteria.status=active"),
            json!({ "value": page }),
        );
    }
}

fn queued(store: &Store) -> Vec<u64> {
    let mut numbers: Vec<_> = store
        .load_queue()
        .unwrap()
        .iter()
        .filter(|job| !crate::monitoring::retired(job))
        .map(|job| job.number)
        .collect();
    numbers.sort();
    numbers
}

fn health_failure(monitor: &Monitor) -> Option<String> {
    monitor
        .snapshot()
        .into_iter()
        .find_map(|health| health.last_failure)
}

#[test]
fn include_users_over_a_very_large_repository_lists_only_their_pull_requests() {
    let (_root, store, mut monitor) = monitored();
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = connected_wire();
    // More than 5,000 active pull requests; reading them all is never needed.
    huge_listing(&wire, 5_100, &[(4_242, BLAKE)]);
    wire.json(
        &creator(BLAKE),
        json!({"value": [raw_pull(4_242, BLAKE, 42, false)]}),
    );
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW).unwrap();
    assert_eq!(queued(&store), [4_242]);
    assert_eq!(health_failure(&monitor), None);
    let listings: Vec<_> = wire
        .urls()
        .into_iter()
        .filter(|url| url.contains("/pullrequests?"))
        .collect();
    assert_eq!(listings.len(), 1, "{listings:?}");
    assert!(listings[0].contains(&format!("searchCriteria.creatorId={BLAKE}")));
}

#[test]
fn narrowed_listings_keep_exclude_users_and_first_match_order() {
    for (rules, admitted) in [
        // Exclude before include: Blake is rejected although included.
        (
            json!([
                {"kind": "exclude_users", "enabled": true,
                 "users": [{"id": BLAKE, "login": "Blake Example"}]},
                {"kind": "include_users", "enabled": true,
                 "users": [{"id": BLAKE, "login": "Blake Example"},
                           {"id": CASEY, "login": "Casey Example"}]}
            ]),
            vec![2],
        ),
        // Include before exclude: the include admits first.
        (
            json!([
                {"kind": "include_users", "enabled": true,
                 "users": [{"id": BLAKE, "login": "Blake Example"},
                           {"id": CASEY, "login": "Casey Example"}]},
                {"kind": "exclude_users", "enabled": true,
                 "users": [{"id": BLAKE, "login": "Blake Example"}]}
            ]),
            vec![1, 2],
        ),
    ] {
        let (_root, store, mut monitor) =
            monitored_with(json!({"mode": "filtered", "rules": rules}));
        let ticket = next_ticket(&mut monitor, &store, NOW);
        let wire = connected_wire();
        wire.json(
            &creator(BLAKE),
            json!({"value": [raw_pull(1, BLAKE, 1, false)]}),
        )
        .json(
            &creator(CASEY),
            json!({"value": [raw_pull(2, CASEY, 2, false)]}),
        );
        let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
        monitor.finish(&store, ticket, result, NOW).unwrap();
        assert_eq!(queued(&store), admitted);
        assert!(wire
            .urls()
            .iter()
            .filter(|url| url.contains("/pullrequests?"))
            .all(|url| url.contains("searchCriteria.creatorId=")));
    }
}

#[test]
fn assigned_lists_by_reviewer_and_a_tracked_pull_request_stays_monitored_after_it_leaves() {
    let (_root, store, mut monitor) = monitored_with(json!({"mode": "filtered", "rules": [
        {"kind": "assigned", "enabled": true}
    ]}));
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = connected_wire();
    let mut assigned = raw_pull(7, BLAKE, 7, false);
    assigned["reviewers"] = json!([{"id": AVERY, "displayName": "Avery Example"}]);
    wire.json(&reviewer(AVERY), json!({"value": [assigned]}));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW).unwrap();
    assert_eq!(queued(&store), [7]);
    assert!(wire
        .urls()
        .iter()
        .filter(|url| url.contains("/pullrequests?"))
        .all(|url| url.contains(&format!("searchCriteria.reviewerId={AVERY}"))));

    // Avery is removed as reviewer and the author pushes: the narrowed query
    // no longer lists it, but the tracked pull request is read on its own.
    let ticket = next_ticket(&mut monitor, &store, NOW + 900);
    assert_eq!(ticket.tracked, [("7".to_string(), 7)]);
    let wire = connected_wire();
    wire.json(&reviewer(AVERY), json!({"value": []}))
        .json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 8, false));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW + 900).unwrap();
    let tracked = store.load_queue_state().unwrap().tracked;
    assert_eq!(
        (
            tracked[0].number,
            tracked[0].iteration,
            tracked[0].head_sha.clone()
        ),
        (7, 2, sha(8))
    );
}

#[test]
fn all_mode_over_a_very_large_repository_admits_by_batch_and_never_fails() {
    let (_root, store, mut monitor) = monitored_with(json!({"mode": "all", "rules": []}));
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = connected_wire();
    // 5,100 active pull requests; 5,000 is read in the first batch, 30 later.
    huge_listing(&wire, 5_100, &[(5_000, BLAKE), (30, BLAKE)]);
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    let cursor = result.as_ref().unwrap().listing.clone().unwrap();
    assert_eq!(cursor.skip, 1_000);
    monitor.finish(&store, ticket, result, NOW).unwrap();
    // Admitted before the pass completes, with no repository failure.
    assert_eq!(queued(&store), [5_000]);
    assert_eq!(health_failure(&monitor), None);
    assert!(monitor
        .snapshot()
        .into_iter()
        .all(|health| health.operation.is_none_or(|op| !op.parked())));
    // The next poll resumes where this one stopped.
    let ticket = next_ticket(&mut monitor, &store, NOW + 900);
    assert_eq!(ticket.listing, Some(cursor));
}

#[test]
fn a_failing_listing_never_fails_the_check_of_a_tracked_pull_request() {
    let (_root, store, mut monitor) = monitored();
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = connected_wire();
    wire.json(
        &creator(BLAKE),
        json!({"value": [raw_pull(1, BLAKE, 1, false)]}),
    );
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW).unwrap();
    assert_eq!(queued(&store), [1]);

    for (attempt, failure) in [
        Ok(status(500, None)),
        Err(NetworkError),
        Ok(Response {
            status: 200,
            oversize: true,
            ..Response::default()
        }),
    ]
    .into_iter()
    .enumerate()
    {
        let now = NOW + 900 * (attempt as i64 + 1);
        let ticket = next_ticket(&mut monitor, &store, now);
        let wire = connected_wire();
        for _ in 0..3 {
            wire.on(&creator(BLAKE), failure.clone());
        }
        wire.json(
            &git("pullrequests/1?"),
            raw_pull(1, BLAKE, 10 + attempt as u32, false),
        );
        let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
        assert!(result.is_ok(), "{attempt}: {:?}", result.as_ref().err());
        monitor.finish(&store, ticket, result, now).unwrap();
        assert_eq!(health_failure(&monitor), None);
        let tracked = store.load_queue_state().unwrap().tracked;
        assert_eq!(tracked[0].head_sha, sha(10 + attempt as u32));
    }
}

#[test]
fn a_closed_tracked_pull_request_the_scope_cannot_list_is_probed_for_a_reopen() {
    let (_root, store, mut monitor) = monitored_with(json!({"mode": "filtered", "rules": [
        {"kind": "assigned", "enabled": true}
    ]}));
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = connected_wire();
    let mut assigned = raw_pull(7, BLAKE, 7, false);
    assigned["reviewers"] = json!([{"id": AVERY, "displayName": "Avery Example"}]);
    wire.json(&reviewer(AVERY), json!({"value": [assigned]}));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW).unwrap();

    // Abandoned, after Avery left the reviewers: observed closed by ID.
    let ticket = next_ticket(&mut monitor, &store, NOW + 900);
    let wire = connected_wire();
    let mut abandoned = raw_pull(7, BLAKE, 7, false);
    abandoned["status"] = json!("abandoned");
    abandoned["closedDate"] = json!("2026-10-21T09:00:00Z");
    wire.json(&reviewer(AVERY), json!({"value": []}))
        .json(&git("pullrequests/7?"), abandoned);
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW + 900).unwrap();
    let tracked = store.load_queue_state().unwrap().tracked;
    assert_eq!(tracked[0].lifecycle, crate::provider::Lifecycle::Closed);

    // Reactivated with a push: no listing query finds it; the probe does.
    let ticket = next_ticket(&mut monitor, &store, NOW + 1800);
    assert!(ticket.tracked.is_empty());
    assert_eq!(ticket.closed, [("7".to_string(), 7)]);
    let wire = connected_wire();
    wire.json(&reviewer(AVERY), json!({"value": []}))
        .json(&git("pullrequests/7?"), raw_pull(7, BLAKE, 9, false));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW + 1800).unwrap();
    let tracked = store.load_queue_state().unwrap().tracked;
    assert_eq!(
        (tracked[0].lifecycle.clone(), tracked[0].head_sha.clone()),
        (crate::provider::Lifecycle::Open, sha(9))
    );
    assert!(store
        .load_queue()
        .unwrap()
        .iter()
        .any(|job| job.number == 7 && job.head_sha == sha(9) && !crate::monitoring::retired(job)));
}

#[test]
fn path_facts_of_untracked_pull_requests_gone_from_a_whole_pass_are_evicted_across_batches() {
    let (_root, store, mut monitor) = monitored();
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].paths = vec!["src/".into()];
    store.save_settings(&settings).unwrap();
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = connected_wire();
    wire.json(
        &creator(BLAKE),
        json!({"value": [
            raw_pull(1, BLAKE, 1, false),
            raw_pull(2, BLAKE, 2, false),
            raw_pull(3, BLAKE, 3, false),
        ]}),
    );
    // PR 1 is gated out (untracked); 2 and 3 are admitted (tracked).
    for (number, path) in [(1, "/docs/a.md"), (2, "/src/a.rs"), (3, "/src/b.rs")] {
        wire.json(
            &git(&format!("pullRequests/{number}/iterations?")),
            json!({"value": [{"id": 1, "sourceRefCommit": {"commitId": sha(number)}}]}),
        )
        .json(
            &git(&format!("pullRequests/{number}/iterations/1/changes?")),
            json!({"changeEntries": [{"item": {"path": path, "objectId": sha(50)}, "changeType": "edit"}]}),
        );
    }
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    let connected = result.as_ref().unwrap().connection.clone();
    monitor.finish(&store, ticket, result, NOW).unwrap();
    assert_eq!(queued(&store), [2, 3]);
    let facts = |store: &Store| -> Vec<String> {
        store
            .load_monitoring_state()
            .unwrap()
            .path_facts
            .get(ID)
            .map(|facts| facts.keys().cloned().collect())
            .unwrap_or_default()
    };
    assert_eq!(facts(&store), ["1", "2", "3"]);

    // A pass over several batches lists only PR 3: tracked PR 2 is not
    // reached by ID in it.
    let open = |number: u64| {
        let raw: crate::azure_devops::read::pulls::RawPullRequest =
            serde_json::from_value(raw_pull(number, BLAKE, number as u32, false)).unwrap();
        raw.into_pull_request(&key()).unwrap()
    };
    let mut batch = |now: i64, passes: u64, skip: u64| {
        let ticket = next_ticket(&mut monitor, &store, now);
        let result = PollResult {
            connection: connected.clone(),
            pull_requests: vec![open(3)],
            states: Default::default(),
            facts: Default::default(),
            listing: Some(crate::provider::ListingCursor {
                skip,
                passes,
                ..Default::default()
            }),
        };
        monitor.finish(&store, ticket, Ok(result), now).unwrap();
    };
    batch(NOW + 900, 1, 100);
    assert_eq!(facts(&store), ["1", "2", "3"], "kept until the pass ends");
    batch(NOW + 1800, 2, 0);
    assert_eq!(facts(&store), ["2", "3"], "tracked PRs keep theirs");
}

#[test]
fn a_throttled_listing_waits_as_backpressure_and_resumes_from_the_committed_cursor() {
    let (_root, store, mut monitor) = monitored_with(json!({"mode": "all", "rules": []}));
    let ticket = next_ticket(&mut monitor, &store, NOW);
    let wire = connected_wire();
    let page = |from: u64| {
        json!({"value": (from..from + 100).rev()
            .map(|n| raw_pull(n, BLAKE, n as u32, false))
            .collect::<Vec<_>>()})
    };
    let listing = git("pullrequests?searchCriteria.status=active");
    wire.json(&listing, page(2_901))
        .json(&listing, page(2_801))
        .on(&listing, Ok(status(429, None)));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    assert_eq!(result.as_ref().err(), Some(&ConnectionError::RateLimited));
    let _ = monitor.finish(&store, ticket, result, NOW);
    // A wait, never a failure; nothing committed and no poll of the account
    // before the shared backpressure gate opens.
    assert!(monitor
        .snapshot()
        .into_iter()
        .all(|health| health.operation.is_none_or(|op| !op.parked())));
    assert!(queued(&store).is_empty());
    assert!(monitor
        .prepare_checks(&store, NOW + 5, true)
        .unwrap()
        .is_empty());

    // After the wait the batch is read again from the last committed
    // cursor: a PR on the second page is admitted.
    let ticket = next_ticket(&mut monitor, &store, NOW + 3_600);
    assert_eq!(ticket.listing, None);
    let wire = connected_wire();
    wire.json(&listing, page(2_901))
        .json(&listing, page(2_801))
        .json(&listing, json!({"value": []}));
    let result = poll(Arc::new(FakeTokens::default()), &wire, &ticket);
    monitor.finish(&store, ticket, result, NOW + 3_600).unwrap();
    assert!(queued(&store).contains(&2_850));
}
