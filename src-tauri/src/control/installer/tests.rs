use super::*;
use std::cell::RefCell;

thread_local! {
    static FAILURE: RefCell<Option<(String, bool)>> = const { RefCell::new(None) };
    static NONCE: RefCell<Option<uuid::Uuid>> = const { RefCell::new(None) };
}

pub(super) fn next_nonce() -> Option<uuid::Uuid> {
    NONCE.with(|nonce| nonce.borrow_mut().take())
}

pub(super) fn fault(point: &str) -> Result<(), Error> {
    FAILURE.with(|value| {
        let hit = value
            .borrow()
            .as_ref()
            .is_some_and(|(name, _)| name == point);
        if hit {
            let (_, crash) = value.borrow_mut().take().unwrap();
            if crash {
                std::process::exit(79);
            }
            return Err(io_error("injected package interruption"));
        }
        Ok(())
    })
}

fn fail(point: &str) {
    FAILURE.with(|value| *value.borrow_mut() = Some((point.into(), false)));
}

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("installer-transaction-")
        .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap()
}

#[test]
fn partial_first_install_reopens_without_an_unowned_live_directory() {
    let root = root();
    let physical = root.path().canonicalize().unwrap();
    let target = physical.join("skills/pr-sniper");
    fail("asset-0");
    assert!(install_at(&target, &physical.join("app"), None).is_err());
    let recovered = installation_at(&target).expect("interrupted install must be recoverable");
    let installed = install_at(
        &target,
        &physical.join("app"),
        recovered.revision.as_deref(),
    )
    .unwrap();
    assert_eq!(installed.state, "installed");
}

#[test]
fn partial_update_reopens_with_a_verified_package() {
    let root = root();
    let physical = root.path().canonicalize().unwrap();
    let target = physical.join("skills/pr-sniper");
    let before = install_at(&target, &physical.join("old-app"), None).unwrap();
    fail("asset-3");
    assert!(install_at(
        &target,
        &physical.join("new-app"),
        before.revision.as_deref()
    )
    .is_err());
    let recovered = installation_at(&target)
        .expect("interrupted update must retain or recover a verified package");
    assert_eq!(recovered.state, "installed");
    let installed = install_at(
        &target,
        &physical.join("new-app"),
        recovered.revision.as_deref(),
    )
    .unwrap();
    assert_eq!(installed.state, "installed");
}

fn assert_package(target: &Path, executable: &Path) {
    assert_eq!(inspect_at(target).unwrap().state, "installed");
    for (name, body) in ASSETS {
        assert_eq!(fs::read_to_string(target.join(name)).unwrap(), *body);
    }
    let launcher: serde_json::Value =
        serde_json::from_slice(&fs::read(target.join("launcher.json")).unwrap()).unwrap();
    assert_eq!(launcher["executable"], serde_json::json!(executable));
}

fn stage(parent: &Path, suffix: &str) -> std::path::PathBuf {
    fs::read_dir(transaction::workspace(&parent.join("pr-sniper")).unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".pr-sniper-package-")
                && p.file_name().unwrap().to_string_lossy().ends_with(suffix)
        })
        .unwrap()
}

const BOUNDARIES: &[&str] = &[
    "stage-created",
    "journal-written",
    "asset-0",
    "asset-1",
    "asset-2",
    "asset-3",
    "manifest-written",
    "stage-verified",
    "ready",
    "new-moved",
    "activated",
    "backup-removed",
    "journal-removed",
];

#[test]
fn injected_install_boundaries_reopen_to_absence_old_or_complete_new_package() {
    for update in [false, true] {
        for point in BOUNDARIES.iter().copied().chain(
            update
                .then_some(["old-moved", "backup-cleanup-file"])
                .into_iter()
                .flatten(),
        ) {
            let root = root();
            let physical = root.path().canonicalize().unwrap();
            let target = physical.join("skills/pr-sniper");
            let old = physical.join("old-app");
            let new = physical.join("new-app");
            let before = update.then(|| install_at(&target, &old, None).unwrap());
            fail(point);
            assert!(
                install_at(
                    &target,
                    &new,
                    before.as_ref().and_then(|i| i.revision.as_deref())
                )
                .is_err(),
                "{point}"
            );
            let recovered =
                installation_at(&target).unwrap_or_else(|e| panic!("{update} {point}: {e:?}"));
            if let Some(before) = &before {
                assert_eq!(recovered.state, "installed", "{point}");
                if recovered.revision == before.revision {
                    assert_package(&target, &old);
                } else {
                    assert_package(&target, &new);
                }
            } else if recovered.state == "installed" {
                assert_package(&target, &new);
            }
            let expected = recovered.revision.clone();
            let completed = install_at(&target, &new, expected.as_deref()).unwrap();
            assert_package(&target, &new);
            assert_eq!(
                remove_at(&target, completed.revision.as_deref().unwrap())
                    .unwrap()
                    .state,
                "absent"
            );
        }
    }
}

#[test]
fn identical_update_recovers_after_its_directory_was_activated() {
    let root = root();
    let root = root.path().canonicalize().unwrap();
    let target = root.join("skills/pr-sniper");
    let before = install_at(&target, &root.join("app"), None).unwrap();
    fail("new-moved");
    assert!(install_at(&target, &root.join("app"), before.revision.as_deref()).is_err());
    assert_eq!(installation_at(&target).unwrap().revision, before.revision);
    assert_package(&target, &root.join("app"));
}

#[test]
fn recovery_cleanup_can_itself_be_interrupted() {
    let root = root();
    let root = root.path().canonicalize().unwrap();
    let target = root.join("skills/pr-sniper");
    fail("asset-2");
    assert!(install_at(&target, &root.join("app"), None).is_err());
    fail("stage-cleanup-file");
    assert!(installation_at(&target).is_err());
    assert_eq!(installation_at(&target).unwrap().state, "absent");
}

#[test]
fn remove_recovery_is_truthful_after_each_directory_and_cleanup_boundary() {
    for point in [
        "ready",
        "old-moved",
        "activated",
        "backup-cleanup-file",
        "backup-removed",
        "journal-removed",
    ] {
        let root = root();
        let root = root.path().canonicalize().unwrap();
        let target = root.join("skills/pr-sniper");
        let installed = install_at(&target, &root.join("app"), None).unwrap();
        fail(point);
        assert!(
            remove_at(&target, installed.revision.as_deref().unwrap()).is_err(),
            "{point}"
        );
        assert_eq!(installation_at(&target).unwrap().state, "absent", "{point}");
        assert!(remove_at(&target, installed.revision.as_deref().unwrap()).is_err());
        assert!(!target.exists());
    }
}

#[test]
fn stale_expected_revision_is_not_reinterpreted_after_recovered_activation() {
    let root = root();
    let root = root.path().canonicalize().unwrap();
    let target = root.join("skills/pr-sniper");
    let old = install_at(&target, &root.join("old"), None).unwrap();
    fail("old-moved");
    assert!(install_at(&target, &root.join("new"), old.revision.as_deref()).is_err());
    assert!(
        matches!(install_at(&target, &root.join("third"), old.revision.as_deref()),
        Err(Error { code, .. }) if code == "conflict")
    );
    assert_package(&target, &root.join("new"));
}

#[test]
fn customized_live_or_transaction_content_is_never_deleted() {
    for location in ["live", "staging", "backup", "staging-after-old-move"] {
        let root = root();
        let root = root.path().canonicalize().unwrap();
        let target = root.join("skills/pr-sniper");
        let old = install_at(&target, &root.join("old"), None).unwrap();
        let customized = match location {
            "live" => target.join("SKILL.md"),
            "staging" => {
                fail("asset-0");
                assert!(install_at(&target, &root.join("new"), old.revision.as_deref()).is_err());
                stage(target.parent().unwrap(), ".new").join("SKILL.md")
            }
            "backup" => {
                fail("activated");
                assert!(install_at(&target, &root.join("new"), old.revision.as_deref()).is_err());
                stage(target.parent().unwrap(), ".old").join("user-notes.txt")
            }
            _ => {
                fail("old-moved");
                assert!(install_at(&target, &root.join("new"), old.revision.as_deref()).is_err());
                stage(target.parent().unwrap(), ".new").join("SKILL.md")
            }
        };
        fs::write(&customized, "human customization").unwrap();
        assert!(installation_at(&target).is_err());
        assert!(install_at(&target, &root.join("new"), old.revision.as_deref()).is_err());
        assert!(remove_at(&target, old.revision.as_deref().unwrap()).is_err());
        assert_eq!(
            fs::read_to_string(&customized).unwrap(),
            "human customization"
        );
        if location == "staging-after-old-move" || location == "staging" {
            assert_package(&target, &root.join("old"));
        }
        if location == "backup" {
            assert_package(&target, &root.join("new"));
        }
    }
}

#[test]
fn foreign_staging_backup_and_journal_occupants_are_preserved() {
    for suffix in [".new", ".old", ".json", ".preserved"] {
        let root = root();
        let root = root.path().canonicalize().unwrap();
        let target = root.join("skills/pr-sniper");
        let before = install_at(&target, &root.join("old"), None).unwrap();
        let nonce = uuid::Uuid::new_v4();
        let occupant = transaction::workspace(&target)
            .unwrap()
            .join(format!(".pr-sniper-package-{nonce}{suffix}"));
        let file = if suffix == ".json" {
            occupant.clone()
        } else {
            fs::create_dir(&occupant).unwrap();
            occupant.join("foreign.txt")
        };
        fs::write(&file, "foreign bytes").unwrap();
        NONCE.with(|value| *value.borrow_mut() = Some(nonce));
        assert!(install_at(&target, &root.join("new"), before.revision.as_deref()).is_err());
        NONCE.with(|value| value.borrow_mut().take());
        assert_eq!(fs::read_to_string(&file).unwrap(), "foreign bytes");
        assert_package(&target, &root.join("old"));
    }
}

#[test]
fn linked_managed_directory_is_refused_without_following_or_removing_it() {
    let root = root();
    let root = root.path().canonicalize().unwrap();
    let target = root.join("skills/pr-sniper");
    let before = install_at(&target, &root.join("old"), None).unwrap();
    let external = root.join("external-references");
    fs::rename(target.join("references"), &external).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external, target.join("references")).unwrap();
    #[cfg(windows)]
    crate::windows_permissions::junction(&external, &target.join("references"));
    let original = fs::read(external.join("control.md")).unwrap();
    assert!(installation_at(&target).is_err());
    assert!(install_at(&target, &root.join("new"), before.revision.as_deref()).is_err());
    assert!(remove_at(&target, before.revision.as_deref().unwrap()).is_err());
    assert_eq!(fs::read(external.join("control.md")).unwrap(), original);
    #[cfg(unix)]
    fs::remove_file(target.join("references")).unwrap();
    #[cfg(windows)]
    fs::remove_dir(target.join("references")).unwrap();
}

#[test]
fn process_interruption_reopens_first_install_update_and_removal() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for action in ["first", "update", "remove"] {
        let points: &[&str] = match action {
            "first" => &[
                "stage-created",
                "asset-0",
                "ready",
                "new-moved",
                "activated",
                "journal-removed",
            ],
            "update" => &[
                "asset-3",
                "ready",
                "old-moved",
                "new-moved",
                "activated",
                "backup-cleanup-file",
            ],
            _ => &[
                "ready",
                "old-moved",
                "activated",
                "backup-cleanup-file",
                "journal-removed",
            ],
        };
        for point in points {
            let root = root();
            let root = root.path().canonicalize().unwrap();
            let target = root.join("skills/pr-sniper");
            if action != "first" {
                install_at(&target, &root.join("old-app"), None).unwrap();
            }
            let owner = uuid::Uuid::new_v4().to_string();
            fs::write(root.join("test-owner"), &owner).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "control::installer::tests::crash_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("PR_SNIPER_INSTALL_TEST_ROOT", &root)
                .env("PR_SNIPER_INSTALL_TEST_OWNER", owner)
                .env("PR_SNIPER_INSTALL_TEST_POINT", point)
                .env("PR_SNIPER_INSTALL_TEST_ACTION", action)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
                panic!("Crash child timed out");
            }
            let result = child.wait_with_output().unwrap();
            assert_eq!(
                result.status.code(),
                Some(79),
                "{action} {point}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let recovered =
                installation_at(&target).unwrap_or_else(|e| panic!("{action} {point}: {e:?}"));
            if action == "remove" {
                assert_eq!(recovered.state, "absent");
            } else {
                let installed = install_at(
                    &target,
                    &root.join("new-app"),
                    recovered.revision.as_deref(),
                )
                .unwrap();
                assert_package(&target, &root.join("new-app"));
                remove_at(&target, installed.revision.as_deref().unwrap()).unwrap();
            }
        }
    }
}

#[test]
fn crash_child() {
    let Some(root) = std::env::var_os("PR_SNIPER_INSTALL_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root).canonicalize().unwrap();
    let target_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .canonicalize()
        .unwrap();
    assert!(
        root.starts_with(target_root)
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("installer-transaction-")
    );
    assert_eq!(
        fs::read_to_string(root.join("test-owner")).unwrap(),
        std::env::var("PR_SNIPER_INSTALL_TEST_OWNER").unwrap()
    );
    let target = root.join("skills/pr-sniper");
    let before = installation_at(&target).unwrap();
    if let Ok(filename) = std::env::var("PR_SNIPER_INSTALL_TEST_FILE") {
        assert!(["scripts/pr-sniper.mjs", ".pr-sniper-install.json"].contains(&filename.as_str()));
        let nonce =
            uuid::Uuid::parse_str(&std::env::var("PR_SNIPER_INSTALL_TEST_NONCE").unwrap()).unwrap();
        NONCE.with(|value| *value.borrow_mut() = Some(nonce));
        let file = transaction::workspace(&target)
            .unwrap()
            .join(format!(".pr-sniper-package-{nonce}.new"))
            .join(filename);
        crate::storage::private_fs::crash_test::arm(
            file,
            std::env::var("PR_SNIPER_INSTALL_TEST_POINT").unwrap(),
            1,
        );
    } else {
        FAILURE.with(|value| {
            *value.borrow_mut() =
                Some((std::env::var("PR_SNIPER_INSTALL_TEST_POINT").unwrap(), true))
        });
    }
    if std::env::var("PR_SNIPER_INSTALL_TEST_ACTION").unwrap() == "remove" {
        remove_at(&target, before.revision.as_deref().unwrap()).unwrap();
    } else {
        install_at(&target, &root.join("new-app"), before.revision.as_deref()).unwrap();
    }
    panic!("Requested interruption boundary was not reached");
}

#[test]
fn actual_partial_file_write_and_activation_journal_failures_recover() {
    for update in [false, true] {
        for filename in ["scripts/pr-sniper.mjs", ".pr-sniper-install.json"] {
            for failure in ["claim", "payload"] {
                let root = root();
                let root = root.path().canonicalize().unwrap();
                let target = root.join("skills/pr-sniper");
                let before = update.then(|| install_at(&target, &root.join("old"), None).unwrap());
                let nonce = uuid::Uuid::new_v4();
                NONCE.with(|value| *value.borrow_mut() = Some(nonce));
                crate::storage::private_fs::recovery_fault::arm(
                    transaction::workspace(&target)
                        .unwrap()
                        .join(format!(".pr-sniper-package-{nonce}.new"))
                        .join(filename),
                    failure,
                );
                assert!(install_at(
                    &target,
                    &root.join("new"),
                    before.as_ref().and_then(|r| r.revision.as_deref())
                )
                .is_err());
                let recovered = installation_at(&target).unwrap();
                if update {
                    assert_eq!(recovered.revision, before.unwrap().revision);
                    assert_package(&target, &root.join("old"));
                } else {
                    assert_eq!(recovered.state, "absent");
                }
                let installed =
                    install_at(&target, &root.join("new"), recovered.revision.as_deref()).unwrap();
                let updated =
                    install_at(&target, &root.join("newer"), installed.revision.as_deref())
                        .unwrap();
                assert_eq!(
                    remove_at(&target, updated.revision.as_deref().unwrap())
                        .unwrap()
                        .state,
                    "absent"
                );
            }
        }
    }
    let root = root();
    let root = root.path().canonicalize().unwrap();
    let target = root.join("skills/pr-sniper");
    let before = install_at(&target, &root.join("old"), None).unwrap();
    fail("old-moved");
    assert!(install_at(&target, &root.join("new"), before.revision.as_deref()).is_err());
    let journal = stage(target.parent().unwrap(), ".json");
    crate::storage::private_fs::recovery_fault::arm(journal, "payload");
    assert!(installation_at(&target).is_err());
    assert_package(&target, &root.join("new"));
    assert_eq!(installation_at(&target).unwrap().state, "installed");
    assert_package(&target, &root.join("new"));
}

#[test]
fn links_inserted_after_interruption_are_preserved_and_never_activated() {
    for linked in ["staging", "target"] {
        let root = root();
        let root = root.path().canonicalize().unwrap();
        let target = root.join("skills/pr-sniper");
        let old = install_at(&target, &root.join("old"), None).unwrap();
        fail("old-moved");
        assert!(install_at(&target, &root.join("new"), old.revision.as_deref()).is_err());
        let external = root.join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("untouched.txt"), "human file").unwrap();
        let link = if linked == "target" {
            target.clone()
        } else {
            let stage = stage(target.parent().unwrap(), ".new");
            fs::remove_file(stage.join("references/control.md")).unwrap();
            fs::remove_dir(stage.join("references")).unwrap();
            stage.join("references")
        };
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, &link).unwrap();
        #[cfg(windows)]
        crate::windows_permissions::junction(&external, &link);
        assert!(installation_at(&target).is_err());
        assert!(fs::symlink_metadata(&link).is_ok());
        assert_eq!(
            fs::read_to_string(external.join("untouched.txt")).unwrap(),
            "human file"
        );
        if linked == "staging" {
            assert_package(&target, &root.join("old"));
        }
        #[cfg(unix)]
        fs::remove_file(link).unwrap();
        #[cfg(windows)]
        fs::remove_dir(link).unwrap();
    }
}

#[test]
fn nested_bootstrap_process_interruptions_preserve_artifacts_and_allow_future_operations() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for update in [false, true] {
        for filename in ["scripts/pr-sniper.mjs", ".pr-sniper-install.json"] {
            for boundary in ["bootstrap-stage", "owner-created", "owner-partial"] {
                let owner = root();
                let root = owner.path().canonicalize().unwrap();
                let target = root.join("skills/pr-sniper");
                let before =
                    update.then(|| install_at(&target, &root.join("old-app"), None).unwrap());
                let nonce = uuid::Uuid::new_v4();
                let staging = transaction::workspace(&target)
                    .unwrap()
                    .join(format!(".pr-sniper-package-{nonce}.new"));
                let file = staging.join(filename);
                let ready = file
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("crash-ready.json");
                let token = uuid::Uuid::new_v4().to_string();
                fs::write(root.join("test-owner"), &token).unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "control::installer::tests::crash_child",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("PR_SNIPER_INSTALL_TEST_ROOT", &root)
                    .env("PR_SNIPER_INSTALL_TEST_OWNER", token)
                    .env("PR_SNIPER_INSTALL_TEST_ACTION", "update")
                    .env("PR_SNIPER_INSTALL_TEST_POINT", boundary)
                    .env("PR_SNIPER_INSTALL_TEST_FILE", filename)
                    .env("PR_SNIPER_INSTALL_TEST_NONCE", nonce.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let child_pid = child.id();
                let deadline = Instant::now() + Duration::from_secs(20);
                while !ready.exists()
                    && child.try_wait().unwrap().is_none()
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                let reached = ready.exists();
                if child.try_wait().unwrap().is_none() {
                    child.kill().unwrap();
                }
                let outcome = child.wait_with_output().unwrap();
                assert!(
                    reached,
                    "{filename} {boundary}: {}",
                    String::from_utf8_lossy(&outcome.stderr)
                );
                let observed: serde_json::Value =
                    serde_json::from_slice(&fs::read(&ready).unwrap()).unwrap();
                assert_eq!(observed["point"], boundary);
                assert_eq!(observed["pid"], child_pid);
                let payload = std::path::PathBuf::from(observed["stage"].as_str().unwrap());
                assert!(payload.starts_with(&staging));
                // Remove only the crash harness's own readiness signal, not any
                // per-file bootstrap payload or ownership artifact.
                fs::remove_file(&ready).unwrap();
                let mut preserved = vec![(
                    payload.strip_prefix(&staging).unwrap().to_path_buf(),
                    fs::read(&payload).unwrap(),
                )];
                let claim = payload.with_extension("owner");
                if claim.exists() {
                    preserved.push((
                        claim.strip_prefix(&staging).unwrap().to_path_buf(),
                        fs::read(&claim).unwrap(),
                    ));
                }
                assert!(transaction::workspace(&target)
                    .unwrap()
                    .join(format!(".pr-sniper-package-{nonce}.json"))
                    .exists());
                let recovered = installation_at(&target).unwrap();
                assert_eq!(
                    recovered.revision,
                    before.as_ref().and_then(|value| value.revision.clone())
                );
                let quarantine = transaction::workspace(&target)
                    .unwrap()
                    .join(format!(".pr-sniper-package-{nonce}.preserved"));
                assert!(!staging.exists());
                for (relative, bytes) in &preserved {
                    assert_eq!(&fs::read(quarantine.join(relative)).unwrap(), bytes);
                }
                let installed = install_at(
                    &target,
                    &root.join("new-app"),
                    recovered.revision.as_deref(),
                )
                .unwrap();
                assert_package(&target, &root.join("new-app"));
                let updated = install_at(
                    &target,
                    &root.join("newer-app"),
                    installed.revision.as_deref(),
                )
                .unwrap();
                assert_package(&target, &root.join("newer-app"));
                assert_eq!(
                    remove_at(&target, updated.revision.as_deref().unwrap())
                        .unwrap()
                        .state,
                    "absent"
                );
                for (relative, bytes) in &preserved {
                    assert_eq!(&fs::read(quarantine.join(relative)).unwrap(), bytes);
                }
            }
        }
    }
}

#[test]
fn interrupted_bootstrap_quarantine_reopens_without_deleting_preserved_content() {
    let owner = root();
    let root = owner.path().canonicalize().unwrap();
    let target = root.join("skills/pr-sniper");
    let nonce = uuid::Uuid::new_v4();
    NONCE.with(|value| *value.borrow_mut() = Some(nonce));
    crate::storage::private_fs::recovery_fault::arm(
        transaction::workspace(&target).unwrap().join(format!(
            ".pr-sniper-package-{nonce}.new/scripts/pr-sniper.mjs"
        )),
        "claim",
    );
    assert!(install_at(&target, &root.join("app"), None).is_err());
    fail("bootstrap-preserved");
    assert!(installation_at(&target).is_err());
    let quarantine = transaction::workspace(&target)
        .unwrap()
        .join(format!(".pr-sniper-package-{nonce}.preserved"));
    assert!(quarantine.exists());
    assert_eq!(installation_at(&target).unwrap().state, "absent");
    assert!(quarantine.exists());
    install_at(&target, &root.join("app"), None).unwrap();
    assert!(quarantine.exists());
}

#[test]
fn bootstrap_ambiguity_does_not_hide_custom_or_linked_content() {
    for change in ["custom", "foreign", "linked"] {
        let owner = root();
        let root = owner.path().canonicalize().unwrap();
        let target = root.join("skills/pr-sniper");
        let installed = install_at(&target, &root.join("old"), None).unwrap();
        let nonce = uuid::Uuid::new_v4();
        NONCE.with(|value| *value.borrow_mut() = Some(nonce));
        let staging = transaction::workspace(&target)
            .unwrap()
            .join(format!(".pr-sniper-package-{nonce}.new"));
        crate::storage::private_fs::recovery_fault::arm(
            staging.join("scripts/pr-sniper.mjs"),
            "claim",
        );
        assert!(install_at(&target, &root.join("new"), installed.revision.as_deref()).is_err());
        let protected = match change {
            "custom" => {
                fs::write(staging.join("SKILL.md"), "human text").unwrap();
                staging.join("SKILL.md")
            }
            "foreign" => {
                fs::write(staging.join("notes.txt"), "human text").unwrap();
                staging.join("notes.txt")
            }
            _ => {
                let external = root.join("external");
                fs::rename(staging.join("references"), &external).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(&external, staging.join("references")).unwrap();
                #[cfg(windows)]
                crate::windows_permissions::junction(&external, &staging.join("references"));
                external.join("control.md")
            }
        };
        let bytes = fs::read(&protected).unwrap();
        assert!(installation_at(&target).is_err());
        assert!(staging.exists());
        assert_eq!(fs::read(protected).unwrap(), bytes);
        assert_package(&target, &root.join("old"));
        if change == "linked" {
            #[cfg(unix)]
            fs::remove_file(staging.join("references")).unwrap();
            #[cfg(windows)]
            fs::remove_dir(staging.join("references")).unwrap();
        }
    }
}
