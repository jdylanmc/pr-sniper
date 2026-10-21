use super::*;

fn root() -> tempfile::TempDir {
    super::super::tests::root()
}
fn fail(point: &str) {
    super::super::tests::fail(point)
}
fn config(home: &Path) -> Value {
    serde_json::from_slice(&fs::read(home.join(".copilot/mcp-config.json")).unwrap()).unwrap()
}
fn write_config(home: &Path, value: &Value) {
    fs::create_dir_all(home.join(".copilot")).unwrap();
    fs::write(
        home.join(".copilot/mcp-config.json"),
        serde_json::to_vec(value).unwrap(),
    )
    .unwrap();
}

#[test]
fn setup_update_remove_owns_only_its_server_and_preserves_custom_configuration() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let foreign = json!({"command":"foreign","env":{"PRIVATE":"never expose"}});
    write_config(
        &home,
        &json!({"custom":{"x":1},"mcpServers":{"other":foreign}}),
    );
    assert_eq!(integration_at(&home).unwrap().state, "absent");
    let first = install_integration_at(&home, &home.join("app"), None).unwrap();
    assert_eq!(first.state, "installed");
    assert_eq!(first.mcp_registered, Some(true));
    assert_eq!(config(&home)["mcpServers"]["pr-sniper"]["type"], "local");
    assert_eq!(config(&home)["mcpServers"]["other"], foreign);
    assert_eq!(
        install_integration_at(&home, &home.join("new-app"), None)
            .unwrap_err()
            .code,
        "conflict"
    );
    let update =
        install_integration_at(&home, &home.join("new-app"), first.revision.as_deref()).unwrap();
    assert_ne!(first.revision, update.revision);
    let removed = remove_integration_at(&home, update.revision.as_deref().unwrap()).unwrap();
    assert_eq!(removed.state, "absent");
    assert_eq!(
        config(&home),
        json!({"custom":{"x":1},"mcpServers":{"other":foreign}})
    );
}

#[test]
fn foreign_or_customized_entry_is_never_adopted_or_removed() {
    for owned in [false, true] {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let before = if owned {
            install_integration_at(&home, &home.join("app"), None)
                .unwrap()
                .revision
        } else {
            None
        };
        let custom = json!({"mcpServers":{"pr-sniper":{"command":"custom"}}});
        write_config(&home, &custom);
        for result in [
            integration_at(&home),
            install_integration_at(&home, &home.join("app"), before.as_deref()),
            remove_integration_at(&home, before.as_deref().unwrap_or("unknown")),
        ] {
            assert_eq!(result.unwrap_err().code, "installation_conflict");
            assert_eq!(config(&home), custom);
        }
    }
}

#[test]
fn package_and_registration_boundaries_recover_and_removal_is_repeatable() {
    for update in [false, true] {
        for point in [
            "mcp-intent-written",
            "asset-0",
            "mcp-package-installed",
            "mcp-config-written",
            "mcp-owner-written",
        ] {
            if update && point == "mcp-config-written" {
                continue;
            }
            let root = root();
            let home = root.path().canonicalize().unwrap();
            let before = if update {
                install_integration_at(&home, &home.join("old-app"), None)
                    .unwrap()
                    .revision
            } else {
                None
            };
            fail(point);
            assert!(
                install_integration_at(&home, &home.join("new-app"), before.as_deref()).is_err(),
                "{point}"
            );
            let reopened = integration_at(&home).unwrap_or_else(|e| panic!("{point}: {e:?}"));
            let installed =
                install_integration_at(&home, &home.join("new-app"), reopened.revision.as_deref())
                    .unwrap();
            assert_eq!(installed.state, "installed");
            fail("mcp-package-removed");
            assert!(remove_integration_at(&home, installed.revision.as_deref().unwrap()).is_err());
            assert_eq!(integration_at(&home).unwrap().state, "absent");
            assert!(config(&home)["mcpServers"].get("pr-sniper").is_none());
        }
    }
}

#[test]
fn interrupted_registration_does_not_overwrite_external_edits() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    fail("mcp-package-installed");
    assert!(install_integration_at(&home, &home.join("app"), None).is_err());
    let custom = json!({"mcpServers":{"pr-sniper":{"command":"custom"}}});
    write_config(&home, &custom);
    assert_eq!(
        integration_at(&home).unwrap_err().code,
        "installation_conflict"
    );
    assert_eq!(config(&home), custom);
    assert_eq!(
        installation_at(&home.join(".copilot/skills/pr-sniper"))
            .unwrap()
            .state,
        "installed"
    );
}

#[test]
fn package_without_registration_requires_explicit_setup() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    install_at(
        &home.join(".copilot/skills/pr-sniper"),
        &home.join("app"),
        None,
    )
    .unwrap();
    let status = integration_at(&home).unwrap();
    assert_eq!(status.state, "incomplete");
    assert!(!home.join(".copilot/mcp-config.json").exists());
    assert_eq!(
        install_integration_at(&home, &home.join("app"), status.revision.as_deref())
            .unwrap()
            .state,
        "installed"
    );
}

#[test]
fn malformed_config_and_linked_assets_are_preserved() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    fs::create_dir_all(home.join(".copilot")).unwrap();
    fs::write(home.join(".copilot/mcp-config.json"), "invalid").unwrap();
    assert_eq!(
        install_integration_at(&home, &home.join("app"), None)
            .unwrap_err()
            .code,
        "mcp_config_invalid"
    );
    assert_eq!(
        fs::read_to_string(home.join(".copilot/mcp-config.json")).unwrap(),
        "invalid"
    );
    #[cfg(unix)]
    {
        let outside = home.join("custom.json");
        fs::write(&outside, "{}").unwrap();
        fs::remove_file(home.join(".copilot/mcp-config.json")).unwrap();
        std::os::unix::fs::symlink(&outside, home.join(".copilot/mcp-config.json")).unwrap();
        assert!(install_integration_at(&home, &home.join("app"), None).is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "{}");
    }
}

#[test]
fn legacy_verified_package_migrates_only_on_explicit_setup() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let target = home.join(".copilot/skills/pr-sniper");
    install_at(&target, &home.join("app"), None).unwrap();
    let manifest_path = target.join(".pr-sniper-install.json");
    let mut manifest: Manifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let legacy = asset_names(1).unwrap();
    for name in manifest
        .files
        .keys()
        .filter(|name| !legacy.contains(&name.as_str()))
    {
        fs::remove_file(target.join(name)).unwrap();
    }
    manifest
        .files
        .retain(|name, _| legacy.contains(&name.as_str()));
    manifest.format = 1;
    let mut legacy_manifest = serde_json::to_value(&manifest).unwrap();
    legacy_manifest.as_object_mut().unwrap().remove("format");
    fs::write(manifest_path, serde_json::to_vec(&legacy_manifest).unwrap()).unwrap();
    let status = integration_at(&home).unwrap();
    assert_eq!(status.state, "incomplete");
    assert!(!home.join(".copilot/mcp-config.json").exists());
    fail("old-moved");
    assert!(install_integration_at(&home, &home.join("app"), status.revision.as_deref()).is_err());
    assert_eq!(integration_at(&home).unwrap().state, "installed");
    assert!(target.join("scripts/mcp-server.mjs").is_file());
}

#[test]
fn integration_process_child() {
    let Some(root) = std::env::var_os("PR_SNIPER_MCP_INSTALL_TEST_ROOT") else {
        return;
    };
    let home = PathBuf::from(root);
    let owner = std::env::var("PR_SNIPER_MCP_INSTALL_TEST_OWNER").unwrap();
    assert_eq!(fs::read_to_string(home.join("test-owner")).unwrap(), owner);
    let before = integration_at(&home).unwrap();
    let point = std::env::var("PR_SNIPER_MCP_INSTALL_TEST_POINT").unwrap();
    super::super::tests::crash(&point);
    install_integration_at(&home, &home.join("new-app"), before.revision.as_deref()).unwrap();
    panic!("expected injected process interruption");
}

#[test]
fn real_process_interruption_recovers_registered_package_and_foreign_entries() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for update in [false, true] {
        for point in [
            "mcp-intent-written",
            "mcp-package-installed",
            "mcp-owner-written",
        ] {
            let root = root();
            let home = root.path().canonicalize().unwrap();
            write_config(
                &home,
                &json!({"mcpServers":{"other":{"command":"untouched"}}}),
            );
            if update {
                install_integration_at(&home, &home.join("old-app"), None).unwrap();
            }
            let owner = uuid::Uuid::new_v4().to_string();
            fs::write(home.join("test-owner"), &owner).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "control::installer::integration::tests::integration_process_child",
                    "--nocapture",
                ])
                .env("PR_SNIPER_MCP_INSTALL_TEST_ROOT", &home)
                .env("PR_SNIPER_MCP_INSTALL_TEST_OWNER", owner)
                .env("PR_SNIPER_MCP_INSTALL_TEST_POINT", point)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(25);
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("MCP installer child timed out");
            }
            let output = child.wait_with_output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(79),
                "{point}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let status = integration_at(&home).unwrap();
            let installed =
                install_integration_at(&home, &home.join("new-app"), status.revision.as_deref())
                    .unwrap();
            assert_eq!(installed.state, "installed");
            remove_integration_at(&home, installed.revision.as_deref().unwrap()).unwrap();
            assert_eq!(
                config(&home),
                json!({"mcpServers":{"other":{"command":"untouched"}}})
            );
        }
    }
}
