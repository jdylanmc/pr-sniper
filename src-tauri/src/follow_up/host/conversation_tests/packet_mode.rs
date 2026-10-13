use super::*;

#[test]
fn packet_mode_primary_admits_no_legacy_mention_or_thread_follow_up_and_legacy_still_does() {
    let (_root, store, origin, thread) = fixture(7);
    let mut settings = store.load_settings().unwrap();
    let selected = settings.repositories[0].assignments[1].clone();
    settings.repositories[0].primary_assignment_id = Some(selected.id.clone());
    settings.repositories[0].assignments[0].actions = None;
    settings.repositories[0].assignments[1].tools = Some(vec!["get_thread".into()]);
    store.save_settings(&settings).unwrap();
    let comment = mention("501", "@actor please explain the contract.");
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![comment.clone()],
        NOW + 10,
    );
    observe(
        &store,
        &origin,
        &thread,
        'a',
        vec![comment.clone()],
        NOW + 20,
    );
    assert!(store.load_follow_ups().unwrap().is_empty());
    let ledger = store.load_feedback().unwrap();
    assert!(ledger.mentions[0]
        .blocked
        .as_deref()
        .unwrap()
        .contains("Packet-mode"));
    assert!(ledger.mentions[0].follow_up_id.is_none());

    // The same primary without a tool set is the legacy path and still admits.
    settings.repositories[0].assignments[1].tools = None;
    store.save_settings(&settings).unwrap();
    observe(&store, &origin, &thread, 'a', vec![comment], NOW + 30);
    assert_eq!(store.load_follow_ups().unwrap().len(), 1);
}
