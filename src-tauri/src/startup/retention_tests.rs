use super::{LoginRegistration, RegistrationStatus};
use crate::storage::{private_fs::recovery_fault, Settings, Store};
use std::fs;

struct RegistrationFixture {
    root: tempfile::TempDir,
    registration: LoginRegistration,
    #[cfg(windows)]
    key: Vec<u16>,
}

impl RegistrationFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("fixture-executable");
        fs::write(&executable, b"isolated file; never executed").unwrap();
        #[cfg(target_os = "macos")]
        let registration = {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            LoginRegistration::new(root.path().join("fixture.plist"), executable)
        };
        #[cfg(windows)]
        let (registration, key) = {
            use windows_sys::Win32::{
                Foundation::ERROR_SUCCESS,
                System::Registry::{
                    RegCloseKey, RegCreateKeyExW, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
                    KEY_SET_VALUE, REG_CREATED_NEW_KEY, REG_OPTION_NON_VOLATILE,
                },
            };
            let name = format!(
                r"Software\PR-Sniper-retention-r6-test-{}",
                uuid::Uuid::new_v4()
            );
            let key: Vec<u16> = name.encode_utf16().chain([0]).collect();
            let mut handle = std::ptr::null_mut();
            let mut disposition = 0;
            assert_eq!(
                unsafe {
                    RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        key.as_ptr(),
                        0,
                        std::ptr::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_QUERY_VALUE | KEY_SET_VALUE,
                        std::ptr::null(),
                        &mut handle,
                        &mut disposition,
                    )
                },
                ERROR_SUCCESS
            );
            assert_eq!(
                disposition, REG_CREATED_NEW_KEY,
                "Only an owned fixture key is permitted"
            );
            assert_eq!(unsafe { RegCloseKey(handle) }, ERROR_SUCCESS);
            (
                LoginRegistration::new(name, "owned".into(), executable),
                key,
            )
        };
        Self {
            root,
            registration,
            #[cfg(windows)]
            key,
        }
    }
}

#[cfg(windows)]
impl Drop for RegistrationFixture {
    fn drop(&mut self) {
        use windows_sys::Win32::{
            Foundation::ERROR_SUCCESS,
            System::Registry::{RegDeleteKeyW, HKEY_CURRENT_USER},
        };
        assert_eq!(
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, self.key.as_ptr()) },
            ERROR_SUCCESS
        );
    }
}

#[test]
fn r6_native_login_caller_keeps_committed_registration_and_rolls_back_only_failed_save() {
    // Actual platform caller, but only a temp plist or fresh non-Run registry
    // key. No for_app, launchd, Startup Apps, or executable invocation.
    for point in ["sync", "unlink"] {
        let fixture = RegistrationFixture::new();
        let store = Store::new(fixture.root.path().join("storage"));
        store.load_settings().unwrap();
        store.load_settings().unwrap();
        let path = fixture.root.path().join("storage/config/settings.json");
        recovery_fault::arm(path.clone(), point);
        fixture.registration.set_enabled(&store, true).unwrap();
        assert_eq!(
            fixture.registration.status().unwrap(),
            RegistrationStatus::Registered
        );
        assert!(
            serde_json::from_slice::<Settings>(&fs::read(&path).unwrap())
                .unwrap()
                .launch_at_login
        );
        assert!(store
            .load_settings()
            .unwrap_err()
            .contains("recover staged settings"));
        assert_eq!(
            fixture.registration.status().unwrap(),
            RegistrationStatus::Registered
        );
        assert!(store.load_settings().unwrap().launch_at_login);
        assert!(fs::read_dir(path.parent().unwrap()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_str()
                .unwrap()
                .contains(".pr-sniper-write-")
        }));

        let obstruction = path.with_file_name("settings.json.tmp");
        fs::write(&obstruction, b"foreign occupant").unwrap();
        assert!(fixture.registration.set_enabled(&store, false).is_err());
        assert_eq!(
            fixture.registration.status().unwrap(),
            RegistrationStatus::Registered
        );
        assert!(store.load_settings().unwrap().launch_at_login);
        assert_eq!(fs::read(&obstruction).unwrap(), b"foreign occupant");
        fs::remove_file(obstruction).unwrap();
        fixture.registration.set_enabled(&store, false).unwrap();
        assert_eq!(
            fixture.registration.status().unwrap(),
            RegistrationStatus::Absent
        );
        assert!(!store.load_settings().unwrap().launch_at_login);
    }
}
