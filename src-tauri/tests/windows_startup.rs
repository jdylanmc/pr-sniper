#![cfg(windows)]

mod support;
use pr_sniper_lib::{
    startup::{LoginRegistration, RegistrationStatus},
    storage::Settings,
};
use std::{fs, path::PathBuf, ptr};
use support::Fixture;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY,
        REG_CREATED_NEW_KEY, REG_OPTION_NON_VOLATILE, REG_SZ,
    },
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

struct RegistrationFixture {
    files: Fixture,
    key: String,
    handle: HKEY,
    executable: PathBuf,
}

impl RegistrationFixture {
    fn new() -> Self {
        let files = Fixture::new();
        let executable = files.path().join("PR & Sniper.exe");
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let key = format!(r"Software\PR-Sniper-startup-test-{}", uuid::Uuid::new_v4());
        eprintln!("Owned startup fixture: HKCU\\{key}");
        let mut handle = ptr::null_mut();
        let mut disposition = 0;
        assert_eq!(
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    wide(&key).as_ptr(),
                    0,
                    ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    ptr::null(),
                    &mut handle,
                    &mut disposition,
                )
            },
            ERROR_SUCCESS
        );
        assert_eq!(
            disposition, REG_CREATED_NEW_KEY,
            "fixture must own a fresh key"
        );
        Self {
            files,
            key,
            handle,
            executable,
        }
    }

    fn registration(&self) -> LoginRegistration {
        LoginRegistration::new(self.key.clone(), "owned".into(), self.executable.clone())
    }

    fn seed(&self, name: &str, kind: u32, bytes: &[u8]) {
        assert_eq!(
            unsafe {
                RegSetValueExW(
                    self.handle,
                    wide(name).as_ptr(),
                    0,
                    kind,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                )
            },
            ERROR_SUCCESS
        );
    }

    fn read(&self, name: &str) -> Option<(u32, Vec<u8>)> {
        let mut kind = 0;
        let mut size = 4096;
        let mut bytes = vec![0; size as usize];
        let result = unsafe {
            RegQueryValueExW(
                self.handle,
                wide(name).as_ptr(),
                ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return None;
        }
        assert_eq!(result, ERROR_SUCCESS);
        bytes.truncate(size as usize);
        Some((kind, bytes))
    }
}

impl Drop for RegistrationFixture {
    fn drop(&mut self) {
        let closed = unsafe { RegCloseKey(self.handle) };
        let deleted = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(&self.key).as_ptr()) };
        let mut handle = ptr::null_mut();
        let remaining = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(&self.key).as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut handle,
            )
        };
        if !handle.is_null() {
            unsafe {
                RegCloseKey(handle);
            }
        }
        if closed != ERROR_SUCCESS || deleted != ERROR_SUCCESS || remaining != ERROR_FILE_NOT_FOUND
        {
            let message = format!("Owned registry cleanup failed for {}: close={closed}, delete={deleted}, read={remaining}", self.key);
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

#[test]
fn reading_missing_registration_neither_writes_a_value_nor_saves_intent() {
    let fixture = RegistrationFixture::new();
    assert_eq!(
        fixture.registration().status().unwrap(),
        RegistrationStatus::Absent
    );
    assert!(fixture.read("owned").is_none());
    assert!(!fixture.files.store().has_saved_settings());
    let missing_key = LoginRegistration::new(
        format!(r"{}\missing", fixture.key),
        "owned".into(),
        fixture.executable.clone(),
    );
    assert_eq!(missing_key.status().unwrap(), RegistrationStatus::Absent);
}

#[test]
fn explicit_opt_in_quotes_native_executable_and_restart_reads_saved_intent() {
    let fixture = RegistrationFixture::new();
    let store = fixture.files.store();
    store.add_repository("octo/hello-world").unwrap();
    let before = store.load_settings().unwrap();
    fixture.registration().set_enabled(&store, true).unwrap();
    let (kind, bytes) = fixture.read("owned").unwrap();
    assert_eq!(kind, REG_SZ);
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    assert_eq!(
        String::from_utf16(&units).unwrap(),
        format!("\"{}\"\0", fixture.executable.display())
    );
    let reopened = fixture.files.store().load_settings().unwrap();
    assert!(reopened.launch_at_login);
    assert_eq!(reopened.repositories, before.repositories);
    assert_eq!(reopened.defaults, before.defaults);
    assert_eq!(
        fixture.registration().status().unwrap(),
        RegistrationStatus::Registered
    );
    assert_eq!(
        serde_json::to_string(&RegistrationStatus::Registered).unwrap(),
        "\"registered\""
    );
}

#[test]
fn explicit_opt_out_only_removes_the_exact_owned_value() {
    let fixture = RegistrationFixture::new();
    fixture.seed("unrelated", REG_BINARY, b"untouched");
    fixture
        .registration()
        .set_enabled(&fixture.files.store(), true)
        .unwrap();
    fixture
        .registration()
        .set_enabled(&fixture.files.store(), false)
        .unwrap();
    assert_eq!(
        fixture.registration().status().unwrap(),
        RegistrationStatus::Absent
    );
    assert!(
        !fixture
            .files
            .store()
            .load_settings()
            .unwrap()
            .launch_at_login
    );
    assert_eq!(
        fixture.read("unrelated"),
        Some((REG_BINARY, b"untouched".to_vec()))
    );
}

#[test]
fn wrong_type_malformed_command_and_missing_executable_are_invalid() {
    let fixture = RegistrationFixture::new();
    let registration = fixture.registration();
    registration
        .set_enabled(&fixture.files.store(), true)
        .unwrap();
    let (_, bytes) = fixture.read("owned").unwrap();
    fixture.seed("owned", REG_BINARY, &bytes);
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Invalid);
    fixture.seed("owned", REG_SZ, b"malformed");
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Invalid);
    fixture.seed("owned", REG_SZ, &bytes);
    let moved = LoginRegistration::new(
        fixture.key.clone(),
        "owned".into(),
        std::env::current_exe().unwrap(),
    );
    assert_eq!(moved.status().unwrap(), RegistrationStatus::Invalid);
    fs::remove_file(&fixture.executable).unwrap();
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Invalid);
}

#[test]
fn failed_settings_save_restores_exact_previous_type_and_bytes() {
    let fixture = RegistrationFixture::new();
    let store = fixture.files.store();
    store.save_settings(&Settings::default()).unwrap();
    fs::create_dir(fixture.files.path().join("config/settings.json.tmp")).unwrap();
    fixture.seed("owned", REG_BINARY, b"previous malformed value");
    let unrelated: Vec<_> = wide("unrelated bytes")
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    fixture.seed("unrelated", REG_SZ, &unrelated);
    assert!(fixture.registration().set_enabled(&store, true).is_err());
    assert_eq!(
        fixture.read("owned"),
        Some((REG_BINARY, b"previous malformed value".to_vec()))
    );
    assert_eq!(fixture.read("unrelated"), Some((REG_SZ, unrelated)));
    assert!(!store.load_settings().unwrap().launch_at_login);
}

#[test]
fn failed_settings_save_removes_a_new_value_and_restores_a_disabled_value() {
    let fixture = RegistrationFixture::new();
    let registration = fixture.registration();
    let store = fixture.files.store();
    store.save_settings(&Settings::default()).unwrap();
    let stage = fixture.files.path().join("config/settings.json.tmp");
    fs::create_dir(&stage).unwrap();
    assert!(registration.set_enabled(&store, true).is_err());
    assert_eq!(registration.status().unwrap(), RegistrationStatus::Absent);
    fs::remove_dir(&stage).unwrap();
    registration.set_enabled(&store, true).unwrap();
    let prior = fixture.read("owned");
    let prior_settings = fs::read(fixture.files.path().join("config/settings.json")).unwrap();
    fs::create_dir(stage).unwrap();
    assert!(registration.set_enabled(&store, false).is_err());
    assert_eq!(fixture.read("owned"), prior);
    assert_eq!(
        fs::read(fixture.files.path().join("config/settings.json")).unwrap(),
        prior_settings
    );
}

#[test]
fn invalid_native_location_fails_without_changing_saved_intent() {
    let fixture = RegistrationFixture::new();
    let store = fixture.files.store();
    store.save_settings(&Settings::default()).unwrap();
    let registration = LoginRegistration::new(
        format!("{}\0invalid", fixture.key),
        "owned".into(),
        fixture.executable.clone(),
    );
    assert!(registration.status().is_err());
    assert!(registration.set_enabled(&store, true).is_err());
    assert!(!store.load_settings().unwrap().launch_at_login);
    assert!(fixture.read("owned").is_none());
}

#[test]
fn denied_native_key_access_is_not_absence_and_does_not_change_intent() {
    let fixture = RegistrationFixture::new();
    let store = fixture.files.store();
    store.save_settings(&Settings::default()).unwrap();
    fixture.seed("owned", REG_BINARY, b"preserved registration");
    struct RestoreAccess<'a>(&'a RegistrationFixture, String);
    impl Drop for RestoreAccess<'_> {
        fn drop(&mut self) {
            let output = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    r#"
                    $ErrorActionPreference = 'Stop'
                    $rights = [System.Security.AccessControl.RegistryRights]::ChangePermissions
                    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(
                        $env:PR_SNIPER_REGISTRY_FIXTURE,
                        [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, $rights)
                    try {
                        $acl = [System.Security.AccessControl.RegistrySecurity]::new()
                        $acl.SetSecurityDescriptorSddlForm($env:PR_SNIPER_REGISTRY_ACL, 'Access')
                        $key.SetAccessControl($acl)
                    } finally { $key.Dispose() }
                "#,
                ])
                .env("PR_SNIPER_REGISTRY_FIXTURE", &self.0.key)
                .env("PR_SNIPER_REGISTRY_ACL", &self.1)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "restore owned registry ACL: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", r#"
            $ErrorActionPreference = 'Stop'
            $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($env:PR_SNIPER_REGISTRY_FIXTURE, $true)
            try {
                $acl = $key.GetAccessControl()
                $acl.GetSecurityDescriptorSddlForm('Access')
                $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
                $rule = [System.Security.AccessControl.RegistryAccessRule]::new(
                    $sid, 'QueryValues,SetValue', 'None', 'None', 'Deny')
                $acl.AddAccessRule($rule)
                $key.SetAccessControl($acl)
            } finally { $key.Dispose() }
        "#])
        .env("PR_SNIPER_REGISTRY_FIXTURE", &fixture.key)
        .output().unwrap();
    assert!(
        output.status.success(),
        "deny owned registry access: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let guard = RestoreAccess(
        &fixture,
        String::from_utf8(output.stdout).unwrap().trim().into(),
    );
    let registration = fixture.registration();
    assert_eq!(
        registration.status().unwrap_err(),
        "Cannot access the launch-at-login registration."
    );
    assert!(registration.set_enabled(&store, true).is_err());
    assert!(!store.load_settings().unwrap().launch_at_login);
    drop(guard);
    assert_eq!(
        fixture.read("owned"),
        Some((REG_BINARY, b"preserved registration".to_vec()))
    );
}
