use super::*;
use std::path::PathBuf;

fn claimed(root: &Path) -> (PathBuf, PathBuf) {
    directory(root).unwrap();
    let nonce = uuid::Uuid::new_v4().to_string();
    let stage = root.join(format!("state.json{WRITE_MARKER}{nonce}.stage"));
    let marker = root.join(format!("state.json{WRITE_MARKER}{nonce}.owner"));
    create_stage(&stage)
        .unwrap()
        .write_all(b"abandoned bulky payload")
        .unwrap();
    create_stage(&marker)
        .unwrap()
        .write_all(ownership("state.json", &nonce).as_bytes())
        .unwrap();
    (stage, marker)
}

#[test]
fn recovery_discards_only_complete_owned_claims_not_suffix_matches() {
    let root = tempfile::tempdir().unwrap();
    let (stage, marker) = claimed(root.path());
    let unowned = root.path().join("state.json.tmp");
    std::fs::write(&unowned, b"unowned occupied stage").unwrap();
    recover(root.path(), None).unwrap();
    assert!(!stage.exists());
    assert!(!marker.exists());
    let (other_stage, other_marker) = claimed(root.path());
    std::fs::write(&other_marker, b"not an ownership record").unwrap();
    assert!(recover(root.path(), None).is_err());
    assert_eq!(std::fs::read(&unowned).unwrap(), b"unowned occupied stage");
    assert_eq!(
        std::fs::read(&other_stage).unwrap(),
        b"abandoned bulky payload"
    );
    assert!(replace(&root.path().join("state.json"), b"new", "fixture").is_err());
    assert_eq!(std::fs::read(unowned).unwrap(), b"unowned occupied stage");
}

#[test]
fn recovery_repeats_after_payload_discard_or_successful_rename() {
    let root = tempfile::tempdir().unwrap();
    for renamed in [false, true] {
        let (stage, marker) = claimed(root.path());
        if renamed {
            std::fs::rename(&stage, root.path().join("state.json")).unwrap();
        } else {
            std::fs::remove_file(&stage).unwrap();
        }
        recover(root.path(), None).unwrap();
        recover(root.path(), None).unwrap();
        assert!(!marker.exists());
        if renamed {
            assert_eq!(
                std::fs::read(root.path().join("state.json")).unwrap(),
                b"abandoned bulky payload"
            );
        }
    }
}
#[test]
fn recovery_cannot_reclaim_a_live_writers_claim() {
    let root = tempfile::tempdir().unwrap();
    let (stage, marker) = claimed(root.path());
    let owner = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&marker)
        .unwrap();
    owner.lock().unwrap();
    assert!(recover(root.path(), None).is_err());
    assert!(stage.exists());
    assert!(marker.exists());
    drop(owner);
    recover(root.path(), None).unwrap();
    assert!(!stage.exists());
    assert!(!marker.exists());
}

fn claims(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains(WRITE_MARKER)
        })
        .collect()
}

#[test]
fn r6_rename_commits_before_deferred_sync_or_unlink_and_recovery_remains_explicit() {
    for point in ["sync", "unlink"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.json");
        recovery_fault::arm(path.clone(), point);
        replace(&path, b"committed", "fixture").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"committed");
        let owners = claims(root.path());
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].extension().unwrap(), "owner");
        assert!(recover(root.path(), None).is_err(), "{point}");
        assert_eq!(fs::read(&path).unwrap(), b"committed");
        assert_eq!(
            claims(root.path()),
            owners,
            "Recovery retains ownership on {point} failure"
        );
        recover(root.path(), None).unwrap();
        assert!(claims(root.path()).is_empty());
        replace(&path, b"next commit", "fixture").unwrap();
        recover(root.path(), None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"next commit");
    }
}

#[test]
fn r6_precommit_failures_keep_old_data_and_recover_without_bulky_or_unprovable_payloads() {
    for point in ["claim", "payload"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.json");
        replace(&path, b"old committed data", "fixture").unwrap();
        recover(root.path(), None).unwrap();
        recovery_fault::arm(path.clone(), point);
        assert!(replace(&path, b"new bulky payload", "fixture").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old committed data");
        recover(root.path(), None).unwrap();
        for remnant in claims(root.path()) {
            if remnant.extension().unwrap() == "stage" {
                assert_eq!(fs::metadata(remnant).unwrap().len(), 0);
            }
        }
        replace(&path, b"safe retry", "fixture").unwrap();
        recover(root.path(), None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"safe retry");
    }
}

#[test]
fn r6_resource_save_reports_commit_and_keeps_the_next_expected_snapshot_truthful() {
    use crate::storage::{ResourceEdit, Settings, Store};
    for point in ["sync", "unlink"] {
        let root = tempfile::tempdir().unwrap();
        let store = Store::new(root.path().into());
        store.load_settings().unwrap();
        let before = store.load_settings().unwrap();
        let path = root.path().join("config/settings.json");
        let mut value = before.global_preferences();
        value.capacity += 1;
        recovery_fault::arm(path.clone(), point);
        let saved = store
            .save_resource(ResourceEdit::Preferences {
                expected: before.global_preferences(),
                value,
            })
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Settings>(&fs::read(&path).unwrap()).unwrap(),
            saved
        );
        assert!(store
            .load_settings()
            .unwrap_err()
            .contains("recover staged settings"));
        assert_eq!(
            serde_json::from_slice::<Settings>(&fs::read(&path).unwrap()).unwrap(),
            saved
        );
        assert_eq!(store.load_settings().unwrap(), saved);
        assert!(claims(path.parent().unwrap()).is_empty());
        assert!(store
            .save_resource(ResourceEdit::Preferences {
                expected: before.global_preferences(),
                value: before.global_preferences(),
            })
            .unwrap_err()
            .contains("Resource changed"));
        store
            .save_resource(ResourceEdit::Preferences {
                expected: saved.global_preferences(),
                value: saved.global_preferences(),
            })
            .unwrap();
        assert_eq!(store.load_settings().unwrap(), saved);
    }
}

#[cfg(unix)]
#[test]
fn recovery_rejects_links_and_nonprivate_claims_without_touching_their_targets() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    for kind in ["stage-link", "claim-link", "hardlink", "permissions"] {
        let root = tempfile::tempdir().unwrap();
        let (stage, marker) = claimed(root.path());
        let outside = root.path().join("unrelated.json");
        let original = b"unrelated saved data";
        std::fs::write(&outside, original).unwrap();
        match kind {
            "stage-link" => {
                std::fs::remove_file(&stage).unwrap();
                symlink(&outside, &stage).unwrap();
            }
            "claim-link" => {
                std::fs::remove_file(&marker).unwrap();
                symlink(&outside, &marker).unwrap();
            }
            "hardlink" => {
                std::fs::remove_file(&stage).unwrap();
                std::fs::hard_link(&outside, &stage).unwrap();
            }
            _ => std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap(),
        }
        assert!(recover(root.path(), None).is_err(), "{kind}");
        assert_eq!(std::fs::read(outside).unwrap(), original);
        assert!(std::fs::symlink_metadata(stage).is_ok());
        assert!(std::fs::symlink_metadata(marker).is_ok());
    }
}

#[cfg(windows)]
#[path = "../../../tests/support/windows_permissions.rs"]
mod windows_permissions;

#[cfg(windows)]
#[test]
fn windows_claim_and_payload_are_private_and_denied_claims_stay_denied() {
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    let (stage, marker) = claimed(&private);
    windows_permissions::assert_private(&stage, false);
    windows_permissions::assert_private(&marker, false);
    let denial = windows_permissions::DeniedAccess::read_data(&marker);
    assert!(recover(&private, None).is_err());
    assert!(std::fs::read(&marker).is_err());
    assert!(stage.exists());
    drop(denial);
    recover(&private, None).unwrap();
    assert!(!stage.exists());
    assert!(!marker.exists());
}
