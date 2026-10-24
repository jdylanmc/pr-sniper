use super::retention_tests::RegistrationFixture;
use super::{installed_under, RegistrationStatus};
use crate::storage::Store;
use std::{fs, path::PathBuf};

/// A registration left by a build that no longer exists (e.g. an old dev path).
fn stale(fixture: &RegistrationFixture) {
    let old = fixture.root.path().join("old-build");
    fs::write(&old, b"isolated file; never executed").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&old, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fixture.for_executable(old.clone()).register().unwrap();
    fs::remove_file(old).unwrap();
}

#[test]
fn launch_repair_repoints_only_an_enabled_invalid_registration_from_an_installed_app() {
    let fixture = RegistrationFixture::new();
    let registration = &fixture.registration;
    assert!(!registration.repair_on_launch(true, true).unwrap());
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Absent);

    stale(&fixture);
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Invalid);
    for (enabled, installed) in [(false, true), (true, false), (false, false)] {
        assert!(!registration.repair_on_launch(enabled, installed).unwrap());
        assert_eq!(registration.status().unwrap(), RegistrationStatus::Invalid);
    }
    assert!(registration.repair_on_launch(true, true).unwrap());
    assert_eq!(
        registration.status().unwrap(),
        RegistrationStatus::Registered
    );
    assert!(!registration.repair_on_launch(true, true).unwrap());
}

#[test]
fn explicit_repair_matches_the_saved_preference() {
    for enabled in [true, false] {
        let fixture = RegistrationFixture::new();
        let store = Store::new(fixture.root.path().join("storage"));
        store.load_settings().unwrap();
        stale(&fixture);
        // Settings Repair reapplies the saved preference: re-register or
        // remove the stale entry, never register while the preference is Off.
        fixture.registration.set_enabled(&store, enabled).unwrap();
        assert_eq!(
            fixture.registration.status().unwrap(),
            if enabled {
                RegistrationStatus::Registered
            } else {
                RegistrationStatus::Absent
            }
        );
        assert_eq!(store.load_settings().unwrap().launch_at_login, enabled);
    }
}

#[test]
fn only_installed_locations_may_repair_at_launch() {
    #[cfg(target_os = "macos")]
    {
        let roots = [
            PathBuf::from("/Applications"),
            PathBuf::from("/Users/fixture/Applications"),
        ];
        for (path, installed) in [
            ("/Applications/PR Sniper.app/Contents/MacOS/pr-sniper", true),
            (
                "/Users/fixture/Applications/PR Sniper.app/Contents/MacOS/pr-sniper",
                true,
            ),
            (
                "/Users/fixture/orca/pr-sniper/src-tauri/target/release/bundle/macos/PR Sniper.app/Contents/MacOS/pr-sniper",
                false,
            ),
            ("/Users/fixture/src-tauri/target/debug/pr-sniper", false),
            ("/Applications/Nested/PR Sniper.app/Contents/MacOS/pr-sniper", false),
            ("/Applications/PR Sniper/Contents/MacOS/pr-sniper", false),
        ] {
            assert_eq!(installed_under(&PathBuf::from(path), &roots), installed, "{path}");
        }
    }
    #[cfg(windows)]
    {
        let roots = [PathBuf::from(r"C:\Users\fixture\AppData\Local\PR Sniper")];
        for (path, installed) in [
            (
                r"C:\Users\fixture\AppData\Local\PR Sniper\pr-sniper.exe",
                true,
            ),
            (
                r"c:\users\FIXTURE\appdata\local\pr sniper\pr-sniper.exe",
                true,
            ),
            (
                r"C:\src\pr-sniper\src-tauri\target\release\pr-sniper.exe",
                false,
            ),
            (
                r"C:\Users\fixture\AppData\Local\PR Sniper\old\pr-sniper.exe",
                false,
            ),
        ] {
            assert_eq!(
                installed_under(&PathBuf::from(path), &roots),
                installed,
                "{path}"
            );
        }
    }
}
