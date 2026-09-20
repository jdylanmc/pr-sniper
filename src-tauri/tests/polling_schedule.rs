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
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.path().join("state/polling.json")).unwrap())
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

#[test]
fn spring_gap_runs_fixed_wall_time_at_the_first_valid_instant() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.add_repository("example/project").unwrap();
    settings.defaults.schedule = Schedule::Cron {
        expression: "30 2 * * *".into(),
        timezone: "America/New_York".into(),
    };
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::default();

    monitor.begin(&settings, 1_772_953_140, false).unwrap();

    // 2026-03-08 01:59 EST -> 03:00 EDT; local 02:30 does not exist.
    assert_eq!(monitor.snapshot()[0].next_run, 1_772_953_200);
}

#[test]
fn fall_fold_runs_a_fixed_wall_time_only_on_its_first_occurrence() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.add_repository("example/project").unwrap();
    settings.defaults.schedule = Schedule::Cron {
        expression: "30 1 * * *".into(),
        timezone: "America/New_York".into(),
    };
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::default();
    monitor.begin(&settings, 1_793_510_940, false).unwrap();
    assert_eq!(monitor.snapshot()[0].next_run, 1_793_511_000);

    let tickets = monitor.begin(&settings, 1_793_511_000, false).unwrap();

    assert_eq!(tickets.len(), 1);
    // Next fixed 01:30 is November 2 at 06:30 UTC, not the repeated November 1 hour.
    assert_eq!(monitor.snapshot()[0].next_run, 1_793_601_000);
}

#[test]
fn wildcard_minutes_follow_chronology_through_the_repeated_fall_hour() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut settings = store.add_repository("example/project").unwrap();
    settings.defaults.schedule = Schedule::Cron {
        expression: "* 1 * * *".into(),
        timezone: "America/New_York".into(),
    };
    store.save_settings(&settings).unwrap();
    let mut monitor = Monitor::default();

    monitor.begin(&settings, 1_793_512_740, false).unwrap();

    // 01:59 EDT is followed by 01:00 EST, at 06:00 UTC.
    assert_eq!(monitor.snapshot()[0].next_run, 1_793_512_800);
}
