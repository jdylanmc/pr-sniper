#![cfg(windows)]

// The same native adapter source, not an emulation or a replacement host.
#[path = "../src/notifications/windows_native.rs"]
#[allow(dead_code)]
mod native;

use native::{Apartment, Registration};

fn fixture() -> Registration {
    Registration {
        version: 1,
        profile: "5335c011-9951-48b9-a976-f1f824adbc11".into(),
        root: std::path::PathBuf::from(r"D:\owned test profile"),
        executable: std::path::PathBuf::from(r"D:\owned test app\PR Sniper.exe"),
        credential_service: Some("com.jdylanmc.pr-sniper.tests.notifications-contract".into()),
    }
}

#[test]
fn identity_round_trip_retains_exact_cold_start_profile() {
    let registration = fixture();
    assert_eq!(
        Registration::decode(&registration.encode().unwrap()).unwrap(),
        registration
    );
    assert!(registration
        .command()
        .unwrap()
        .starts_with("\"D:\\owned test app\\PR Sniper.exe\" --pr-sniper-notification-server "));
    assert!(registration
        .aumid()
        .starts_with("com.jdylanmc.pr-sniper.notify."));
    assert!(!registration.aumid().contains("owned test"));
    assert_eq!(registration.clsid(), fixture().clsid());
    let mut other = fixture();
    other.profile = uuid::Uuid::new_v4().to_string();
    assert_ne!(registration.clsid(), other.clsid());
    assert_ne!(registration.aumid(), other.aumid());
}

#[test]
fn rejects_foreign_notice_and_noncanonical_payloads() {
    let registration = fixture();
    let own = format!(
        "pr-sniper:{}:{}",
        registration.profile,
        uuid::Uuid::new_v4()
    );
    assert!(registration.notice_id(&own).is_ok());
    assert!(registration.notice_id("pr-sniper:foreign:notice").is_err());
    assert!(registration
        .notice_id(&format!("{own}?approve=true"))
        .is_err());
    assert!(Registration::decode("not json").is_err());
    assert!(Registration::decode(&"x".repeat(16001)).is_err());
    for path in [
        r"relative",
        "D:\\foo\" --other",
        "D:\\foo|bar",
        "D:\\foo\nbar",
    ] {
        let mut invalid = fixture();
        invalid.executable = path.into();
        assert!(invalid.encode().is_err());
    }
    let mut invalid = fixture();
    invalid.credential_service = Some("com.jdylanmc.pr-sniper".into());
    assert!(invalid.encode().is_err());
}

#[test]
fn xml_is_native_parseable_and_contains_only_supplied_private_text() {
    let _apartment = Apartment::new().unwrap();
    let xml = native::toast_xml("a<&\"'", "Private <title>", "No repo & no code");
    let document = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
    document
        .LoadXml(&windows::core::HSTRING::from(&xml))
        .unwrap();
    assert!(xml.contains("launch=\"a&lt;&amp;&quot;&apos;\""));
    assert!(xml.contains("Private &lt;title&gt;"));
    assert!(!xml.contains("<actions>"));
    assert!(!xml.contains("scenario="));
    assert!(!xml.contains("protocol"));
}

#[test]
fn com_factory_dispatches_exact_notice_and_rejects_foreign_activation() {
    use std::sync::{mpsc, Arc};
    use windows::{
        core::HSTRING,
        Win32::{
            System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER},
            UI::Notifications::INotificationActivationCallback,
        },
    };
    let _apartment = Apartment::new().unwrap();
    let mut registration = fixture();
    registration.profile = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel();
    let server = native::ActivationServer::start(
        registration.clone(),
        Arc::new(move |id| tx.send(id).map_err(|_| "closed".into())),
    )
    .unwrap();
    let callback: INotificationActivationCallback =
        unsafe { CoCreateInstance(&registration.clsid(), None, CLSCTX_LOCAL_SERVER) }.unwrap();
    let id = format!(
        "pr-sniper:{}:{}",
        registration.profile,
        uuid::Uuid::new_v4()
    );
    unsafe {
        callback
            .Activate(
                &HSTRING::from(registration.aumid()),
                &HSTRING::from(&id),
                &[],
            )
            .unwrap();
        assert!(callback
            .Activate(&HSTRING::from("foreign"), &HSTRING::from(&id), &[])
            .is_err());
        assert!(callback
            .Activate(
                &HSTRING::from(registration.aumid()),
                &HSTRING::from("merge"),
                &[]
            )
            .is_err());
    }
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap(),
        id
    );
    assert!(rx.try_recv().is_err());
    drop(callback);
    server.wait_released().unwrap();
    server.stop().unwrap();
    let unavailable: windows::core::Result<INotificationActivationCallback> =
        unsafe { CoCreateInstance(&registration.clsid(), None, CLSCTX_LOCAL_SERVER) };
    assert!(
        unavailable.is_err(),
        "Class object must be revoked on its owning apartment."
    );
}

#[test]
fn permission_is_aggregate_and_unknown_values_fail_closed() {
    use windows::UI::Notifications::NotificationSetting as Setting;
    assert_eq!(
        native::authorization(Setting::Enabled),
        "authorized_aggregate"
    );
    assert_eq!(
        native::authorization(Setting::DisabledForApplication),
        "denied"
    );
    assert_eq!(
        native::authorization(Setting::DisabledForUser),
        "disabled_for_user"
    );
    assert_eq!(
        native::authorization(Setting::DisabledByGroupPolicy),
        "disabled_by_policy"
    );
    assert_eq!(
        native::authorization(Setting::DisabledByManifest),
        "disabled_by_manifest"
    );
    assert_eq!(native::authorization(Setting(99)), "unknown");
}

#[test]
fn invalid_native_xml_is_a_definite_pre_submission_failure() {
    let _apartment = Apartment::new().unwrap();
    let registration = fixture();
    let id = format!(
        "pr-sniper:{}:{}",
        registration.profile,
        uuid::Uuid::new_v4()
    );
    let (error, uncertain) = native::send(&registration, &id, "\u{1}", "Private text").unwrap_err();
    assert!(error.contains("XML preparation"));
    assert!(!uncertain);
}

#[test]
fn cold_relay_rejects_second_activation_while_first_handoff_is_held() {
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;
    use windows::{
        core::HSTRING,
        Win32::{
            System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER},
            UI::Notifications::INotificationActivationCallback,
        },
    };
    let _apartment = Apartment::new().unwrap();
    let mut registration = fixture();
    registration.profile = uuid::Uuid::new_v4().to_string();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let release_rx = Mutex::new(release_rx);
    let (forwarded_tx, forwarded_rx) = mpsc::channel();
    let relay_registration = registration.clone();
    let relay = std::thread::spawn(move || {
        native::run_relay(
            relay_registration,
            Arc::new(|_| Ok(())),
            Arc::new(move |id| {
                entered_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                forwarded_tx.send(id).unwrap();
                Ok(())
            }),
            Duration::from_secs(5),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let callback: INotificationActivationCallback = loop {
        match unsafe { CoCreateInstance(&registration.clsid(), None, CLSCTX_LOCAL_SERVER) } {
            Ok(callback) => break callback,
            Err(error) => {
                assert!(std::time::Instant::now() < deadline, "{error:?}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    let first_id = format!(
        "pr-sniper:{}:{}",
        registration.profile,
        uuid::Uuid::new_v4()
    );
    let second_id = format!(
        "pr-sniper:{}:{}",
        registration.profile,
        uuid::Uuid::new_v4()
    );
    let clsid = registration.clsid();
    let appid = registration.aumid();
    let first_sent = first_id.clone();
    let first = std::thread::spawn(move || {
        let _apartment = Apartment::new().unwrap();
        let first_callback: INotificationActivationCallback =
            unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }.unwrap();
        unsafe { first_callback.Activate(&HSTRING::from(appid), &HSTRING::from(first_sent), &[]) }
    });
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let second = unsafe {
        callback.Activate(
            &HSTRING::from(registration.aumid()),
            &HSTRING::from(second_id),
            &[],
        )
    };
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    drop(callback);
    relay.join().unwrap().unwrap();
    assert_eq!(forwarded_rx.try_iter().collect::<Vec<_>>(), vec![first_id]);
    assert!(
        second.is_err(),
        "A second callback must not acknowledge an ID that the relay never forwards."
    );
}

#[test]
fn startup_queue_serializes_delayed_producer_with_ready_drain() {
    use std::sync::{mpsc, Arc, Mutex};
    let queue = Arc::new(Mutex::new(native::StartupQueue::new()));
    let expected = (fixture(), "saved-notice".to_string());
    let request = expected.clone();
    let producer_queue = queue.clone();
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let producer = std::thread::spawn(move || {
        // The not-ready decision and append share this guard in production.
        let mut state = producer_queue.lock().unwrap();
        observed_tx.send(()).unwrap();
        resume_rx.recv().unwrap();
        state.forward(request).unwrap()
    });
    observed_rx.recv().unwrap();
    let ready_queue = queue.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let ready = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        ready_queue.lock().unwrap().ready().unwrap()
    });
    ready_rx.recv().unwrap();
    resume_tx.send(()).unwrap();
    assert!(producer.join().unwrap().is_none());
    assert_eq!(ready.join().unwrap(), vec![expected.clone()]);
    assert!(queue.lock().unwrap().ready().unwrap().is_empty());
    // Opposite serialization: readiness wins before the producer takes the lock.
    assert_eq!(
        queue.lock().unwrap().forward(expected.clone()).unwrap(),
        Some(expected)
    );
    assert!(queue.lock().unwrap().ready().unwrap().is_empty());
}

#[test]
fn startup_queue_bounds_admission_and_closes_ready_and_pending_states() {
    let mut queue = native::StartupQueue::new();
    for i in 0..16 {
        assert!(queue.forward((fixture(), i.to_string())).unwrap().is_none());
    }
    assert!(queue.forward((fixture(), "overflow".into())).is_err());
    let cancelled = queue.shutdown();
    assert_eq!(
        cancelled
            .iter()
            .map(|(_, id)| id.clone())
            .collect::<Vec<_>>(),
        (0..16).map(|i| i.to_string()).collect::<Vec<_>>()
    );
    assert!(queue.ready().is_err());
    assert!(queue.forward((fixture(), "after shutdown".into())).is_err());
    let mut ready = native::StartupQueue::new();
    assert!(ready.ready().unwrap().is_empty());
    assert!(ready.shutdown().is_empty());
    assert!(ready
        .forward((fixture(), "after ready shutdown".into()))
        .is_err());
}

fn wait_factory(registration: &Registration) -> windows::Win32::System::Com::IClassFactory {
    use windows::Win32::System::Com::{CoGetClassObject, CLSCTX_LOCAL_SERVER};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match unsafe { CoGetClassObject(&registration.clsid(), CLSCTX_LOCAL_SERVER, None) } {
            Ok(factory) => return factory,
            Err(error) => {
                assert!(std::time::Instant::now() < deadline, "{error:?}");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

fn wait_revoked(registration: &Registration) {
    use windows::Win32::System::Com::{CoGetClassObject, IClassFactory, CLSCTX_LOCAL_SERVER};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let result: windows::core::Result<IClassFactory> =
            unsafe { CoGetClassObject(&registration.clsid(), CLSCTX_LOCAL_SERVER, None) };
        if result.is_err() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "COM class was not revoked."
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn cold_relay_success_and_handoff_error_wait_for_objects_and_server_locks() {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    use windows::{
        core::HSTRING,
        Win32::{
            System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER},
            UI::Notifications::INotificationActivationCallback,
        },
    };
    let _apartment = Apartment::new().unwrap();
    for fail_handoff in [false, true] {
        let mut registration = fixture();
        registration.profile = uuid::Uuid::new_v4().to_string();
        let relay_registration = registration.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let relay = std::thread::spawn(move || {
            let result = native::run_relay(
                relay_registration,
                Arc::new(|_| Ok(())),
                Arc::new(move |_| {
                    if fail_handoff {
                        Err("Owned handoff failed.".into())
                    } else {
                        Ok(())
                    }
                }),
                Duration::from_secs(3),
            );
            done_tx.send(result).unwrap();
        });
        let factory = wait_factory(&registration);
        unsafe {
            factory.LockServer(true).unwrap();
        }
        let callback: INotificationActivationCallback =
            unsafe { CoCreateInstance(&registration.clsid(), None, CLSCTX_LOCAL_SERVER) }.unwrap();
        let id = format!(
            "pr-sniper:{}:{}",
            registration.profile,
            uuid::Uuid::new_v4()
        );
        let activate = unsafe {
            callback.Activate(
                &HSTRING::from(registration.aumid()),
                &HSTRING::from(&id),
                &[],
            )
        };
        assert_eq!(activate.is_err(), fail_handoff);
        wait_revoked(&registration);
        assert!(
            done_rx.try_recv().is_err(),
            "Retained callback must keep release fence pending."
        );
        assert!(unsafe {
            callback.Activate(
                &HSTRING::from(registration.aumid()),
                &HSTRING::from(&id),
                &[],
            )
        }
        .is_err());
        assert!(
            unsafe { factory.LockServer(true) }.is_err(),
            "Closed factory must reject new locks."
        );
        drop(callback);
        assert!(
            done_rx.try_recv().is_err(),
            "Retained server lock must keep release fence pending."
        );
        unsafe {
            factory.LockServer(false).unwrap();
        }
        drop(factory);
        let result = done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(result.is_err(), fail_handoff);
        if fail_handoff {
            assert!(result.unwrap_err().contains("Owned handoff failed"));
        }
        relay.join().unwrap();
    }
}

#[test]
fn cold_relay_timeout_revokes_and_reports_bounded_retained_lock_failure() {
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};
    let _apartment = Apartment::new().unwrap();
    let mut registration = fixture();
    registration.profile = uuid::Uuid::new_v4().to_string();
    let relay_registration = registration.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let relay = std::thread::spawn(move || {
        done_tx
            .send(native::run_relay(
                relay_registration,
                Arc::new(|_| Ok(())),
                Arc::new(|_| panic!("No activation was supplied")),
                Duration::from_millis(300),
            ))
            .unwrap();
    });
    let factory = wait_factory(&registration);
    unsafe {
        factory.LockServer(true).unwrap();
    }
    wait_revoked(&registration);
    let start = Instant::now();
    let result = done_rx
        .recv_timeout(Duration::from_secs(7))
        .unwrap()
        .unwrap_err();
    assert!(result.contains("before timeout"));
    assert!(result.contains("retained the notification COM callback"));
    assert!(start.elapsed() < Duration::from_secs(7));
    unsafe {
        factory.LockServer(false).unwrap();
    }
    drop(factory);
    relay.join().unwrap();
}

#[test]
#[ignore = "Explicit opt-in native test: creates one unique test-owned Start Menu shortcut and HKCU COM class, then verifies exact cleanup."]
fn owned_registration_preserves_foreign_install_and_cleans_exact_identity() {
    let _apartment = Apartment::new().unwrap();
    let mut registration = fixture();
    registration.profile = uuid::Uuid::new_v4().to_string();
    registration.executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    registration.root = std::env::current_dir().unwrap().canonicalize().unwrap();
    registration.credential_service = Some(format!(
        "com.jdylanmc.pr-sniper.tests.notifications-{}",
        registration.profile
    ));
    let shortcut = registration.shortcut().unwrap();
    println!(
        "OWNED aumid={} HKCU\\{} shortcut={}",
        registration.aumid(),
        registration.key(),
        shortcut.display()
    );
    assert!(!registration.registered().unwrap());
    let result = std::panic::catch_unwind(|| {
        registration.install().unwrap();
        assert!(registration.registered().unwrap());
        registration.install().unwrap();
        let original = std::fs::read(&shortcut).unwrap();
        let mut foreign = registration.clone();
        foreign.executable = registration
            .executable
            .with_file_name("another-install.exe");
        assert!(foreign.install().is_err());
        assert!(foreign.uninstall().is_err());
        assert_eq!(std::fs::read(&shortcut).unwrap(), original);
        assert!(registration.registered().unwrap());
    });
    let cleanup = registration.uninstall();
    assert!(cleanup.is_ok(), "Exact owned cleanup failed: {cleanup:?}");
    assert!(!registration.registered().unwrap());
    assert!(!shortcut.try_exists().unwrap());
    println!("CLEANED exact HKCU class and shortcut; absence verified.");
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
