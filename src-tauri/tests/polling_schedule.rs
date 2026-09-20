mod support;

use pr_sniper_lib::github::ConnectionError;
use pr_sniper_lib::monitoring::Monitor;
use pr_sniper_lib::policy::{PolicyOverrides, Schedule};
use support::Fixture;

#[test]
fn cron_uses_the_configured_zone_not_the_hosts_local_zone() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.add_repository("example/project").unwrap();
    settings.defaults.schedule = Schedule::Cron {
        expression: "0 9 * * *".into(),
        timezone: "America/New_York".into(),
    };
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::default();

    let tickets = monitor.begin(&settings, 1_768_485_540, false).unwrap();

    assert!(tickets.is_empty());
    assert_eq!(monitor.snapshot()[0].next_run, 1_768_485_600);
    assert_eq!(
        monitor
            .begin(&settings, 1_768_485_600, false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn cron_repository_does_not_starve_an_independent_interval_repository() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store.add_repository("example/cron").unwrap();
    let mut settings = store.add_repository("example/interval").unwrap();
    settings.repositories[0].overrides = PolicyOverrides {
        schedule: Some(Schedule::Cron {
            expression: "0 9 * * *".into(),
            timezone: "America/New_York".into(),
        }),
        ..PolicyOverrides::default()
    };
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::default();

    let tickets = monitor.begin(&settings, 1_768_485_540, true).unwrap();

    assert_eq!(tickets.len(), 2);
    assert_eq!(tickets[0].name, "example/cron");
    assert_eq!(tickets[1].name, "example/interval");
}

#[test]
fn check_now_preserves_cadence_and_suppresses_an_overlapping_scheduled_check() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = store.add_repository("example/project").unwrap();
    let mut monitor = Monitor::default();
    assert!(monitor.begin(&settings, 1000, false).unwrap().is_empty());
    assert_eq!(monitor.snapshot()[0].next_run, 1900);

    let ticket = monitor.begin(&settings, 1100, true).unwrap().pop().unwrap();

    assert_eq!(monitor.snapshot()[0].last_attempt, Some(1100));
    assert_eq!(monitor.snapshot()[0].next_run, 1900);
    assert!(monitor.begin(&settings, 1900, false).unwrap().is_empty());
    assert!(monitor.begin(&settings, 1900, true).unwrap().is_empty());
    monitor
        .finish(&store, ticket, Err(ConnectionError::Network), 1910)
        .unwrap();
    assert!(!monitor.snapshot()[0].in_flight);
    assert_eq!(monitor.snapshot()[0].last_success, None);
    assert_eq!(
        monitor.snapshot()[0].last_failure,
        Some(ConnectionError::Network)
    );
    let persisted: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.path().join("state/polling.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(persisted[0]["last_attempt"], 1100);
    assert_eq!(persisted[0]["last_success"], serde_json::Value::Null);
    assert_eq!(persisted[0]["last_failure"], "network");
}

#[test]
fn waking_after_many_missed_intervals_creates_only_one_check() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let settings = store.add_repository("example/project").unwrap();
    let mut monitor = Monitor::default();
    monitor.begin(&settings, 1000, false).unwrap();

    let mut tickets = monitor.begin(&settings, 10_000, false).unwrap();

    assert_eq!(tickets.len(), 1);
    assert_eq!(monitor.snapshot()[0].next_run, 10_900);
    monitor
        .finish(
            &store,
            tickets.pop().unwrap(),
            Err(ConnectionError::RateLimited),
            10_001,
        )
        .unwrap();
    assert!(monitor.begin(&settings, 10_001, false).unwrap().is_empty());
}
