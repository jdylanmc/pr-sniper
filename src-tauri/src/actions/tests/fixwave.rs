use super::*;

fn peer_approval() -> ProviderReview {
    ProviderReview {
        id: "144".into(),
        actor_id: "44".into(),
        head: "a".repeat(40),
        state: "APPROVED".into(),
        body: "Provider-confirmed peer review.".into(),
        submitted_at: Some("2026-09-30T00:00:00Z".into()),
    }
}

fn prepare_final(store: &Store, item: &str, observation: &Observation) -> FinalReview {
    let mut settings = store.load_settings().unwrap();
    settings.repositories[0].assignments[0].comment = false;
    store.save_settings(&settings).unwrap();
    synchronize(store, item, Ok(observation.clone()), NOW + 10).unwrap();
    let id = store.load_actions().unwrap().finals[0].id.clone();
    let execution = host::prepare_dispatch(store, &id, NOW + 11).unwrap();
    host::complete(store, &execution, Ok(output()), NOW + 12).unwrap();
    store.load_actions().unwrap().finals.remove(0)
}

#[test]
fn fixwave_peer_approval_allows_author_merge_through_real_provider_without_own_vote() {
    for author in ["11", "22"] {
        let (_root, store, item) = fixture(1, true, true);
        let mut current = observed();
        current.author_id = author.into();
        current.reviews.push(peer_approval());
        let run = prepare_final(&store, &item, &current);
        assert!(ready(&store, &run, &current, Action::Merge).is_ok());
        if author == "22" {
            assert!(ready(&store, &run, &current, Action::Approve).is_err());
        }
        let mut environment = env(&store);
        environment.wire.0.lock().unwrap().observation = current.clone();
        let client = GithubClient::new(environment.wire.clone());
        let connected = client.connect("example/repo", Some("22")).unwrap();
        let mut fetched = client.action_observation(&connected.repository, 1).unwrap();
        fetched.write_capability = connected.capabilities.comment == CommentCapability::Available;
        assert_eq!(fetched.reviews, current.reviews);
        synchronize(&store, &item, Ok(fetched), NOW + 19).unwrap();
        let effect = host::prepare_next_effect(&store, &item, NOW + 20)
            .unwrap()
            .unwrap();
        assert_eq!(effect.action, Action::Merge);
        host::execute_action(&mut environment, &effect).unwrap();
        let ledger = store.load_actions().unwrap();
        assert!(ledger
            .effects
            .iter()
            .all(|effect| effect.action == Action::Merge));
        assert_eq!(environment.wire.0.lock().unwrap().writes.len(), 1);
        assert_eq!(ledger.effects[0].state, EffectState::Confirmed);
        assert!(ledger.effects[0].receipt.is_some());
    }
}

#[test]
fn fixwave_absent_stale_dismissed_malformed_and_unconfirmed_peer_votes_block_merge() {
    for case in [
        "absent",
        "stale",
        "dismissed",
        "changes_requested",
        "pending",
        "time",
        "id",
        "author_vote",
    ] {
        let (_root, store, item) = fixture(1, true, true);
        let mut current = observed();
        let mut approval = peer_approval();
        match case {
            "stale" => approval.head = "c".repeat(40),
            "dismissed" => approval.state = "DISMISSED".into(),
            "changes_requested" => approval.state = "CHANGES_REQUESTED".into(),
            "pending" => approval.submitted_at = None,
            "time" => approval.submitted_at = Some("not-a-provider-time".into()),
            "id" => approval.id = "not-a-provider-id".into(),
            "author_vote" => approval.actor_id = current.author_id.clone(),
            _ => {}
        }
        if case != "absent" {
            current.reviews.push(approval);
        }
        if case == "changes_requested" {
            synchronize(&store, &item, Ok(current), NOW + 10).unwrap();
            assert!(store.load_actions().unwrap().finals.is_empty());
            assert!(store.load_actions().unwrap().effects.is_empty());
            continue;
        }
        let run = prepare_final(&store, &item, &current);
        assert!(
            ready(&store, &run, &current, Action::Merge).is_err(),
            "{case}"
        );
        assert!(store.load_actions().unwrap().effects.is_empty());
    }
}

#[test]
fn fixwave_peer_approval_change_after_final_cannot_authorize_merge() {
    let (_root, store, item) = fixture(1, true, true);
    let mut current = observed();
    current.reviews.push(peer_approval());
    let run = prepare_final(&store, &item, &current);
    assert!(ready(&store, &run, &current, Action::Merge).is_ok());
    current.reviews[0].state = "DISMISSED".into();
    assert!(prepare_effect(&store, &run.id, Action::Merge, &current, NOW + 20).is_err());
}

#[test]
fn fixwave_approve_only_still_votes_and_missing_approval_sequences_approve_then_merge() {
    for (merge, peer) in [(false, true), (true, false)] {
        let (_root, store, item) = fixture(1, true, merge);
        let mut current = observed();
        if peer {
            current.reviews.push(peer_approval());
        }
        prepare_final(&store, &item, &current);
        let mut environment = env(&store);
        environment.wire.0.lock().unwrap().observation = current;
        let approval = host::prepare_next_effect(&store, &item, NOW + 20)
            .unwrap()
            .unwrap();
        assert_eq!(approval.action, Action::Approve);
        host::execute_action(&mut environment, &approval).unwrap();
        let current = environment.wire.0.lock().unwrap().observation.clone();
        synchronize(&store, &item, Ok(current), NOW + 21).unwrap();
        let next = host::prepare_next_effect(&store, &item, NOW + 22).unwrap();
        if merge {
            let effect = next.unwrap();
            assert_eq!(effect.action, Action::Merge);
            environment.now = NOW + 22;
            host::execute_action(&mut environment, &effect).unwrap();
        } else {
            assert!(next.is_none());
        }
        let actions = store.load_actions().unwrap();
        assert_eq!(actions.effects.len(), if merge { 2 } else { 1 });
        assert!(actions
            .effects
            .iter()
            .all(|effect| effect.state == EffectState::Confirmed));
        assert_eq!(
            environment.wire.0.lock().unwrap().writes.len(),
            actions.effects.len()
        );
    }
}
