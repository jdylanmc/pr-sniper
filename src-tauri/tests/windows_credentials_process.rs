#![cfg(windows)]

use pr_sniper_lib::github::{
    oauth::TokenPair,
    token_store::{CredentialStore, ProviderAccountId, RotationSafeStore},
    windows_credentials::WindowsCredentialStore,
};
use std::{
    os::windows::io::AsRawHandle,
    process::{Command, Stdio},
    ptr,
    sync::Barrier,
    time::{Duration, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_NOT_FOUND, WAIT_OBJECT_0},
    Security::Credentials::{CredFree, CredReadW, CRED_TYPE_GENERIC},
    System::Threading::WaitForSingleObject,
};

const SERVICE_ENV: &str = "PR_SNIPER_CREDENTIAL_TEST_SERVICE";
const ROLE_ENV: &str = "PR_SNIPER_CREDENTIAL_TEST_ROLE";
const SERVICE_PREFIX: &str = "com.jdylanmc.pr-sniper.tests.accounts-";

fn cases(service: &str) -> [(ProviderAccountId, String); 2] {
    let hex = |value: &str| {
        value
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    ["101", "202"].map(|id| {
        (
            ProviderAccountId::github(id),
            format!(
                "com.jdylanmc.pr-sniper:{}:{}",
                hex(service),
                hex(&format!("account:github:{id}"))
            ),
        )
    })
}

fn native_present(target: &str) -> Result<bool, String> {
    let target: Vec<_> = target.encode_utf16().chain([0]).collect();
    let mut credential = ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
        let code = unsafe { GetLastError() };
        return if code == ERROR_NOT_FOUND {
            Ok(false)
        } else {
            Err(format!("CredReadW code={code}"))
        };
    }
    if credential.is_null() {
        return Err("CredReadW returned no allocation".into());
    }
    unsafe { CredFree(credential.cast()) };
    Ok(true)
}

fn pair(access: &str, refresh: &str) -> TokenPair {
    TokenPair::new_at(
        access,
        refresh,
        UNIX_EPOCH + Duration::from_secs(1000),
        Duration::from_secs(60),
        Duration::from_secs(120),
    )
}

#[test]
#[ignore = "invoked only by the exact-owned fresh-process lifecycle test"]
fn credential_process_worker() {
    let service = std::env::var(SERVICE_ENV).expect("parent-owned namespace");
    let nonce = service
        .strip_prefix(SERVICE_PREFIX)
        .expect("test-owned service prefix");
    assert_eq!(uuid::Uuid::parse_str(nonce).unwrap().to_string(), nonce);
    let role = std::env::var(ROLE_ENV).expect("parent-selected phase");
    let cases = cases(&service);
    match role.as_str() {
        "produce" => {
            for (_, target) in &cases {
                assert!(
                    !native_present(target).unwrap(),
                    "refuse an occupied fixture target"
                );
            }
            let store = RotationSafeStore::new(WindowsCredentialStore::with_service(&service));
            let barrier = Barrier::new(2);
            std::thread::scope(|scope| {
                for (key, _) in &cases {
                    let store = &store;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        store.save(key, &pair("before", "refresh-before")).unwrap();
                        store
                            .rotate(key, |_| Ok(pair("after", "refresh-after")))
                            .unwrap();
                        assert_eq!(
                            store.load(key).unwrap().unwrap(),
                            pair("after", "refresh-after")
                        );
                    });
                }
            });
            // An independent exact-target oracle confirms these claims really
            // identify the records just created, not a stale target encoding.
            for (_, target) in &cases {
                assert!(native_present(target).unwrap());
            }
            std::thread::scope(|scope| {
                for (key, _) in &cases {
                    let store = &store;
                    scope.spawn(move || {
                        store.delete(key).unwrap();
                        assert!(store.load(key).unwrap().is_none());
                    });
                }
            });
        }
        "verify" => {
            for (_, target) in &cases {
                assert!(
                    !native_present(target).unwrap(),
                    "residual exact fixture target: {target}"
                );
            }
        }
        _ => panic!("unknown credential fixture phase"),
    }
    println!("credential-process-{role}-complete");
}

fn run_child(service: &str, role: &str) -> Result<(), String> {
    let mut child = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
        .args([
            "--exact",
            "credential_process_worker",
            "--ignored",
            "--nocapture",
        ])
        .env(SERVICE_ENV, service)
        .env(ROLE_ENV, role)
        .env("RUST_BACKTRACE", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    eprintln!(
        "[credential-process-fixture] stage=spawn role={role} pid={}",
        child.id()
    );
    let waited = unsafe { WaitForSingleObject(child.as_raw_handle(), 10_000) };
    if waited != WAIT_OBJECT_0 {
        let killed = child.kill();
        let reaped = child.wait();
        return Err(format!("{role} did not exit within ten seconds; wait_status={waited}; kill={killed:?}; reap={reaped:?}"));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout)
            .contains(&format!("credential-process-{role}-complete"))
    {
        return Err(format!(
            "{role} failed: {}; {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

#[test]
fn native_credentials_are_absent_after_producer_exit() {
    let service = format!("{SERVICE_PREFIX}{}", uuid::Uuid::new_v4());
    let cases = cases(&service);
    for (_, target) in &cases {
        eprintln!("[credential-fixture] stage=claim target={target}");
        assert!(
            !native_present(target).unwrap(),
            "refuse an occupied fixture target"
        );
    }

    let produced = run_child(&service, "produce");
    // This fresh process verifies before the controller performs any cleanup.
    let verified = run_child(&service, "verify");
    eprintln!(
        "[credential-process-fixture] stage=fresh_verification outcome={}",
        if verified.is_ok() { "absent" } else { "failed" }
    );

    let store = WindowsCredentialStore::with_service(&service);
    let mut cleanup = Vec::new();
    for (key, target) in &cases {
        if let Err(error) = store.delete(key) {
            cleanup.push(format!("{target}: delete {error:?}"));
        }
        match native_present(target) {
            Ok(false) => {}
            Ok(true) => cleanup.push(format!("{target}: still present")),
            Err(error) => cleanup.push(format!("{target}: {error}")),
        }
    }
    eprintln!(
        "[credential-fixture] stage=cleanup outcome={} service={service} records=2",
        if cleanup.is_empty() {
            "absent"
        } else {
            "failed"
        }
    );
    assert!(
        produced.is_ok() && verified.is_ok() && cleanup.is_empty(),
        "producer={produced:?}; fresh verification={verified:?}; cleanup={cleanup:?}"
    );
}
