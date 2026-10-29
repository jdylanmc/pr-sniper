use super::*;
use crate::{monitoring::QueueJob, storage::Settings};
use serde_json::json;

const NOW: i64 = 1_800_000_000;

fn park(store: &Store, failures: &[OperationFailure]) {
    let repo_id = uuid::Uuid::new_v4().to_string();
    let agent_id = uuid::Uuid::new_v4().to_string();
    let assignment_id = uuid::Uuid::new_v4().to_string();
    let settings: Settings = serde_json::from_value(json!({
        "launch_at_login":false,
        "repositories":[{
            "id":repo_id,"provider":"github","name":"example/repo","enabled":true,
            "provider_account_id":"22","provider_repository_id":"100",
            "watched_authors":[{"id":"11","login":"author"}],
            "assignments":[{"id":assignment_id,"agent_id":agent_id,"schedule":{"kind":"interval","minutes":5,"timezone":"UTC"},"comment":false}]
        }],
        "agents":[{
            "id":agent_id,"name":"Reviewer","model":"model",
            "ai_account":{"provider":"copilot","account_id":"33"},
            "prompt":"Review correctness.","signature":"machine"
        }]
    }))
    .unwrap();
    store.save_settings(&settings).unwrap();
    let jobs: Vec<QueueJob> = (1..=failures.len())
        .map(|number| {
            serde_json::from_value(json!({
                "assignment_id":assignment_id,"provider":"github","account_id":"22","account_login":"owner",
                "configuration_id":repo_id,"repository_id":"100","repository_name":"example/repo",
                "pull_request_id":number.to_string(),"number":number,"title":"Review","head_sha":"a".repeat(40),
                "trigger_policy":"[[\"11\"],true]","author_id":"11","author_login":"author",
                "watched_author":true,"all_authors":false,"requested_reviewer":false,
                "waiting":"human_start","detected_at":100
            }))
            .unwrap()
        })
        .collect();
    store.save_queue(&jobs).unwrap();
    let mut runs = Vec::new();
    for candidate in crate::review::host::candidates(store).unwrap() {
        let failure = &failures[candidate.job.number as usize - 1];
        let mut run = crate::review::host::request(store, &candidate.key, true, NOW).unwrap();
        run.operation.state = OperationState::ManualRetry;
        run.operation.failure = Some(failure.clone());
        run.operation.next_attempt_at = None;
        runs.push(run);
    }
    store.save_reviews(&runs).unwrap();
    store
        .save_automation(&crate::capacity::Automation { paused: true })
        .unwrap();
}

fn flags(store: &Store) -> Vec<(u64, State, bool)> {
    let mut flags: Vec<_> = super::super::snapshot(store, vec![])
        .unwrap()
        .items
        .into_iter()
        .map(|item| (item.job.number, item.state, item.retry_all))
        .collect();
    flags.sort();
    flags
}

#[test]
fn retry_all_rearms_credentials_parks_while_paused_and_leaves_human_decisions() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    park(
        &store,
        &[
            OperationFailure::Credentials,
            OperationFailure::Network,
            OperationFailure::Permanent,
            OperationFailure::Panic,
        ],
    );
    assert_eq!(
        flags(&store),
        [
            (1, State::Failed, true),
            (2, State::Failed, true),
            (3, State::Failed, false),
            (4, State::Failed, false),
        ]
    );
    let first = plan(&store, vec![]).unwrap();
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.skipped, 2);
    assert_eq!(rearm(&store, NOW + 1).unwrap(), (2, 2));

    // Each re-armed review starts a fresh operation; the parked one stays as history.
    let mut queued: Vec<_> = store
        .load_reviews()
        .unwrap()
        .into_iter()
        .filter(|run| run.operation.state == OperationState::Queued)
        .map(|run| run.job.number)
        .collect();
    queued.sort();
    assert_eq!(queued, [1, 2]);
    assert_eq!(
        flags(&store),
        [
            (1, State::Queued, false),
            (2, State::Queued, false),
            (3, State::Failed, false),
            (4, State::Failed, false),
        ]
    );
    let again = plan(&store, vec![]).unwrap();
    assert!(again.items.is_empty());
    assert_eq!(again.skipped, 2);
}
