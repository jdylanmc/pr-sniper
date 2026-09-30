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
