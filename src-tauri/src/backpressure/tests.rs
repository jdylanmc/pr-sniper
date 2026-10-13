use super::*;

fn github() -> Scope {
    Scope::github("22")
}

#[test]
fn backoff_doubles_from_thirty_seconds_and_caps_at_fifteen_minutes() {
    let floors: Vec<_> = (0..8).map(|step| delay_seconds("op", step, None)).collect();
    for (step, delay) in floors.iter().enumerate() {
        let base = (30_i64 << step).min(CAP_SECONDS);
        assert!(
            *delay >= base.min(CAP_SECONDS) && *delay <= CAP_SECONDS,
            "{delay}"
        );
        assert!(*delay <= base + base / 10);
    }
    assert_eq!(delay_seconds("op", 6, None), CAP_SECONDS);
    assert_eq!(delay_seconds("op", 200, None), CAP_SECONDS);
}

#[test]
fn jitter_is_deterministic_per_operation() {
    assert_eq!(delay_seconds("a", 3, None), delay_seconds("a", 3, None));
    assert!((0..40)
        .any(|i| delay_seconds(&format!("op-{i}"), 3, None) != delay_seconds("op-0", 3, None)));
}

#[test]
fn a_stated_delay_wins_even_beyond_the_cap() {
    assert_eq!(delay_seconds("op", 0, Some(3_600)), 3_600);
    assert_eq!(delay_seconds("op", 5, Some(45)), 45);
    assert_eq!(delay_seconds("op", 0, Some(0)), 1);
}

#[test]
fn repeated_signals_step_up_and_a_success_clears_the_scope() {
    let mut state = Backpressure::default();
    let first = state.signal(github(), None, "op", 1_000);
    let second = state.signal(github(), None, "op", first);
    assert!(second - first > first - 1_000);
    assert_eq!(state.gates[0].step, 1);
    assert_eq!(state.until(&github(), first), Some(second));
    assert!(state.note_success(&github()));
    assert_eq!(state.until(&github(), first), None);
    let again = state.signal(github(), None, "op", 9_000);
    assert_eq!(state.gates[0].step, 0);
    assert!(again - 9_000 <= 33);
}

#[test]
fn scopes_are_independent() {
    let mut state = Backpressure::default();
    state.signal(github(), Some(300), "op", 100);
    assert_eq!(state.until(&github(), 100), Some(400));
    assert_eq!(state.until(&Scope::github("23"), 100), None);
    assert_eq!(state.until(&Scope::copilot("22"), 100), None);
    assert_eq!(state.until(&github(), 400), None);
}

#[test]
fn stale_gates_are_dropped_on_the_next_signal() {
    let mut state = Backpressure::default();
    state.signal(github(), Some(60), "op", 0);
    state.signal(Scope::copilot("7"), Some(60), "op", 100_000);
    assert_eq!(state.gates.len(), 1);
}

#[test]
fn gates_persist_across_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    assert_eq!(store.load_backpressure().unwrap(), Backpressure::default());
    let until = record(&store, github(), Some(120), "op", 500).unwrap();
    assert_eq!(until, 620);
    let restarted = Store::new(root.path().into());
    assert_eq!(
        restarted.load_backpressure().unwrap().until(&github(), 501),
        Some(620)
    );
    succeeded(&restarted, &github()).unwrap();
    assert!(restarted.load_backpressure().unwrap().gates.is_empty());
}

#[test]
fn an_invalid_file_blocks_work_instead_of_hiding_a_gate() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::new(root.path().into());
    store.write_state("backpressure.json", &"nope").unwrap();
    assert!(store.load_backpressure().is_err());
}
