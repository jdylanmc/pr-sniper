use super::*;
use std::cell::RefCell;

type Action = Box<dyn FnOnce()>;

thread_local! {
    static FAILURE: RefCell<Option<(String, bool)>> = const { RefCell::new(None) };
    static ACTION: RefCell<Option<(String, Action)>> = RefCell::new(None);
}

pub(super) fn fault(point: &str) -> Result<(), Error> {
    let action = ACTION.with(|value| {
        let hit = value
            .borrow()
            .as_ref()
            .is_some_and(|(name, _)| name == point);
        hit.then(|| value.borrow_mut().take().unwrap().1)
    });
    if let Some(action) = action {
        action();
    }
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
            return Err(io_error("injected registration interruption"));
        }
        Ok(())
    })
}

fn fail(point: &str) {
    FAILURE.with(|value| *value.borrow_mut() = Some((point.into(), false)));
}

fn crash(point: &str) {
    FAILURE.with(|value| *value.borrow_mut() = Some((point.into(), true)));
}

#[cfg(unix)]
fn during(point: &str, action: impl FnOnce() + 'static) {
    ACTION.with(|value| *value.borrow_mut() = Some((point.into(), Box::new(action))));
}

fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("mcp-registration-")
        .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .unwrap()
}

/// A fixture app with a bundled server script; never executed.
fn app(root: &Path, name: &str) -> Server {
    let resources = root.join(name).join("mcp");
    fs::create_dir_all(&resources).unwrap();
    let script = resources.join("mcp-server.mjs");
    fs::write(&script, "// fixture server; never executed").unwrap();
    Server {
        script,
        executable: root.join(name).join("pr-sniper"),
    }
}

fn config_path(home: &Path) -> PathBuf {
    home.join(".copilot").join("mcp-config.json")
}

fn config(home: &Path) -> Value {
    serde_json::from_slice(&fs::read(config_path(home)).unwrap()).unwrap()
}

fn write_config(home: &Path, value: &Value) {
    fs::create_dir_all(home.join(".copilot")).unwrap();
    fs::write(config_path(home), serde_json::to_vec(value).unwrap()).unwrap();
}

fn conflict_code(result: Result<impl std::fmt::Debug, Error>) -> String {
    result.unwrap_err().code
}

#[test]
fn registration_lifecycle_owns_only_its_server_entry() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let foreign = json!({"command":"foreign","env":{"PRIVATE":"never expose"}});
    let original = json!({"custom":{"x":1},"mcpServers":{"other":foreign}});
    write_config(&home, &original);
    let old = app(&home, "old");
    let absent = integration_at(&home, &old).unwrap();
    assert_eq!(absent.state, "absent");
    assert_eq!(absent.revision, None);
    assert!(!absent.mcp_registered);
    assert_eq!(absent.skill_command, SKILL_COMMAND);

    let first = install_integration_at(&home, &old, None).unwrap();
    assert_eq!(first.state, "installed");
    assert!(first.mcp_registered);
    let entry = &config(&home)["mcpServers"]["pr-sniper"];
    assert_eq!(entry["type"], "local");
    assert_eq!(entry["command"], "node");
    assert_eq!(
        entry["args"],
        json!([old.script, "--app", old.executable]),
        "the entry launches the app-bundled server for this app"
    );
    assert_eq!(config(&home)["mcpServers"]["other"], foreign);
    assert_eq!(config(&home)["custom"], json!({"x":1}));
    assert_eq!(
        conflict_code(install_integration_at(&home, &old, None)),
        "conflict"
    );

    // A moved or updated app sees its own entry as stale until repaired.
    let new = app(&home, "new");
    let stale = integration_at(&home, &new).unwrap();
    assert_eq!(stale.state, "incomplete");
    assert_eq!(stale.revision, first.revision);
    let updated = install_integration_at(&home, &new, stale.revision.as_deref()).unwrap();
    assert_eq!(updated.state, "installed");
    assert_ne!(updated.revision, first.revision);
    assert_eq!(
        conflict_code(remove_integration_at(
            &home,
            &new,
            first.revision.as_deref().unwrap()
        )),
        "conflict"
    );
    let removed = remove_integration_at(&home, &new, updated.revision.as_deref().unwrap()).unwrap();
    assert_eq!(removed.state, "absent");
    assert_eq!(config(&home), original);
}

#[test]
fn uncreated_ordinary_home_beyond_max_path_supports_the_lifecycle() {
    let root = root();
    // Like the browser fixture: an ordinary (non-verbatim) absolute home that
    // does not exist yet, with paths beyond Windows MAX_PATH.
    let home = root
        .path()
        .join("long-profile-".repeat(12))
        .join("skill-host");
    let server = app(root.path(), "app");
    let first = install_integration_at(&home, &server, None).unwrap();
    assert_eq!(first.state, "installed");
    assert_eq!(
        config(&home)["mcpServers"]["pr-sniper"]["args"][0],
        json!(server.script)
    );
    let removed =
        remove_integration_at(&home, &server, first.revision.as_deref().unwrap()).unwrap();
    assert_eq!(removed.state, "absent");
}

#[test]
fn removed_entry_is_incomplete_and_needs_explicit_repair() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let server = app(&home, "app");
    install_integration_at(&home, &server, None).unwrap();
    write_config(
        &home,
        &json!({"mcpServers":{"other":{"command":"preserve"}}}),
    );
    let status = integration_at(&home, &server).unwrap();
    assert_eq!(status.state, "incomplete");
    assert!(!status.mcp_registered);
    // Launch repair never restores a registration the human removed.
    assert!(!repair_integration_at(&home, &server).unwrap());
    assert!(config(&home)["mcpServers"].get("pr-sniper").is_none());
    let repaired = install_integration_at(&home, &server, status.revision.as_deref()).unwrap();
    assert_eq!(repaired.state, "installed");
    assert_eq!(config(&home)["mcpServers"]["other"]["command"], "preserve");
}

#[test]
fn launch_repair_follows_only_an_owned_unmodified_entry() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let old = app(&home, "old");
    let new = app(&home, "new");
    assert!(!repair_integration_at(&home, &new).unwrap());
    assert!(!config_path(&home).exists());

    install_integration_at(&home, &old, None).unwrap();
    assert!(!repair_integration_at(&home, &old).unwrap());
    assert!(repair_integration_at(&home, &new).unwrap());
    assert_eq!(integration_at(&home, &new).unwrap().state, "installed");
    assert_eq!(
        config(&home)["mcpServers"]["pr-sniper"]["args"][2],
        json!(new.executable)
    );

    let custom = json!({"mcpServers":{"pr-sniper":{"command":"custom"}}});
    write_config(&home, &custom);
    assert_eq!(
        conflict_code(repair_integration_at(&home, &old)),
        "installation_conflict"
    );
    assert_eq!(config(&home), custom);
}

#[test]
fn foreign_or_customized_entry_is_never_adopted_or_removed() {
    for owned in [false, true] {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let server = app(&home, "app");
        let before = if owned {
            install_integration_at(&home, &server, None)
                .unwrap()
                .revision
        } else {
            None
        };
        let custom = json!({"mcpServers":{"pr-sniper":{"command":"custom"}}});
        write_config(&home, &custom);
        for result in [
            integration_at(&home, &server),
            install_integration_at(&home, &server, before.as_deref()),
            remove_integration_at(&home, &server, before.as_deref().unwrap_or("unknown")),
        ] {
            assert_eq!(conflict_code(result), "installation_conflict");
            assert_eq!(config(&home), custom);
        }
    }
}

#[test]
fn interrupted_registration_boundaries_complete_the_authorized_intent() {
    for operation in ["install", "update", "remove"] {
        for point in [
            "mcp-intent-written",
            "mcp-config-merged",
            "mcp-config-written",
            "mcp-owner-written",
        ] {
            let root = root();
            let home = root.path().canonicalize().unwrap();
            write_config(
                &home,
                &json!({"mcpServers":{"other":{"command":"untouched"}}}),
            );
            let old = app(&home, "old");
            let new = app(&home, "new");
            let before = (operation != "install")
                .then(|| install_integration_at(&home, &old, None).unwrap());
            fail(point);
            let result = match operation {
                "remove" => remove_integration_at(
                    &home,
                    &old,
                    before.as_ref().unwrap().revision.as_deref().unwrap(),
                ),
                _ => install_integration_at(
                    &home,
                    &new,
                    before.as_ref().and_then(|b| b.revision.as_deref()),
                ),
            };
            assert!(result.is_err(), "{operation} {point}");
            let reopened = integration_at(&home, &new)
                .unwrap_or_else(|error| panic!("{operation} {point}: {error:?}"));
            assert_eq!(
                reopened.state,
                if operation == "remove" {
                    "absent"
                } else {
                    "installed"
                },
                "{operation} {point}"
            );
            assert_eq!(config(&home)["mcpServers"]["other"]["command"], "untouched");
        }
    }
}

#[test]
fn interrupted_intent_does_not_overwrite_external_edits() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let server = app(&home, "app");
    fail("mcp-intent-written");
    assert!(install_integration_at(&home, &server, None).is_err());
    let custom = json!({"mcpServers":{"pr-sniper":{"command":"custom"}}});
    write_config(&home, &custom);
    assert_eq!(
        conflict_code(integration_at(&home, &server)),
        "installation_conflict"
    );
    assert_eq!(config(&home), custom);
}

#[test]
fn malformed_config_is_preserved() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let server = app(&home, "app");
    fs::create_dir_all(home.join(".copilot")).unwrap();
    fs::write(config_path(&home), "invalid").unwrap();
    assert_eq!(
        conflict_code(install_integration_at(&home, &server, None)),
        "mcp_config_invalid"
    );
    assert_eq!(fs::read_to_string(config_path(&home)).unwrap(), "invalid");
}

#[test]
fn missing_bundled_server_is_never_registered() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let mut server = app(&home, "app");
    server.script = home.join("missing.mjs");
    assert_eq!(
        conflict_code(install_integration_at(&home, &server, None)),
        "invalid_executable"
    );
    assert!(!config_path(&home).exists());
}

#[test]
fn skill_detection_is_read_only_and_follows_links() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let server = app(&home, "app");
    assert!(!integration_at(&home, &server).unwrap().skill_detected);
    let skill = "---\nname: pr-sniper\ndescription: fixture\n---\n# PR Sniper\n";
    let other = home.join(".agents/skills/pr-sniper");
    fs::create_dir_all(&other).unwrap();
    fs::write(
        other.join("SKILL.md"),
        "---\nname: other\n---\nname: pr-sniper\n",
    )
    .unwrap();
    assert!(!integration_at(&home, &server).unwrap().skill_detected);
    fs::write(other.join("SKILL.md"), skill).unwrap();
    assert!(integration_at(&home, &server).unwrap().skill_detected);
    fs::remove_dir_all(home.join(".agents")).unwrap();
    #[cfg(unix)]
    {
        // A git-managed skills folder linked as ~/.copilot/skills.
        let repository = home.join("agent-skills/skills/pr-sniper");
        fs::create_dir_all(&repository).unwrap();
        fs::write(repository.join("SKILL.md"), skill).unwrap();
        std::os::unix::fs::symlink(
            home.join("agent-skills/skills"),
            home.join(".copilot/skills"),
        )
        .unwrap();
        let before = fs::read_dir(home.join("agent-skills/skills"))
            .unwrap()
            .count();
        assert!(integration_at(&home, &server).unwrap().skill_detected);
        install_integration_at(&home, &server, None).unwrap();
        assert_eq!(
            fs::read_dir(home.join("agent-skills/skills"))
                .unwrap()
                .count(),
            before
        );
        assert!(fs::symlink_metadata(home.join(".copilot/skills"))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}

#[test]
fn legacy_owned_entry_is_migrated_and_legacy_package_preserved() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let package = home.join(".copilot/skills/pr-sniper");
    fs::create_dir_all(package.join("scripts")).unwrap();
    fs::write(package.join(".pr-sniper-install.json"), "{}").unwrap();
    fs::write(package.join("SKILL.md"), "---\nname: pr-sniper\n---\n").unwrap();
    let entry = json!({"type":"local","command":"node",
        "args":[package.join("scripts/mcp-server.mjs")],"tools":["*"],"timeout":180000});
    write_config(&home, &json!({"mcpServers":{"pr-sniper":entry}}));
    let work = home.join(".copilot/.pr-sniper-installer");
    crate::control::transport::private_directory(&work).unwrap();
    fs::write(
        work.join(LEGACY_OWNER),
        serde_json::to_vec(&json!({"target":package,"entry":entry})).unwrap(),
    )
    .unwrap();
    let server = app(&home, "app");
    let status = integration_at(&home, &server).unwrap();
    assert_eq!(status.state, "incomplete");
    assert!(status.skill_detected);
    let installed = install_integration_at(&home, &server, status.revision.as_deref()).unwrap();
    assert_eq!(installed.state, "installed");
    assert!(!work.join(LEGACY_OWNER).exists());
    assert_eq!(
        fs::read_to_string(package.join(".pr-sniper-install.json")).unwrap(),
        "{}"
    );
}

fn legacy_package(home: &Path, extra: Option<&str>) -> (PathBuf, PathBuf) {
    let package = home.join(".copilot/skills/pr-sniper");
    fs::create_dir_all(package.join("scripts")).unwrap();
    let files = [
        ("SKILL.md", "---\nname: pr-sniper\n---\n"),
        ("scripts/pr-sniper.mjs", "old"),
    ];
    let mut hashes = serde_json::Map::new();
    for (name, text) in files {
        fs::write(package.join(name), text).unwrap();
        hashes.insert(name.into(), json!(revision(&text.as_bytes().to_vec())));
    }
    fs::write(
        package.join(".pr-sniper-install.json"),
        serde_json::to_vec(&json!({"format":2,"protocol":1,"version":"0.5.0","files":hashes}))
            .unwrap(),
    )
    .unwrap();
    if let Some(name) = extra {
        fs::write(package.join(name), "mine").unwrap();
    }
    let entry = json!({"type":"local","command":"node",
        "args":[package.join("scripts/mcp-server.mjs")],"tools":["*"],"timeout":180000});
    write_config(home, &json!({"mcpServers":{"pr-sniper":entry}}));
    let work = home.join(".copilot/.pr-sniper-installer");
    crate::control::transport::private_directory(&work).unwrap();
    fs::write(
        work.join(LEGACY_OWNER),
        serde_json::to_vec(&json!({"target":package,"entry":entry})).unwrap(),
    )
    .unwrap();
    (package, work)
}

#[test]
fn unmodified_legacy_package_is_retired_on_repair() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let (package, work) = legacy_package(&home, None);
    let server = app(&home, "app");
    let status = integration_at(&home, &server).unwrap();
    assert!(package.exists());
    install_integration_at(&home, &server, status.revision.as_deref()).unwrap();
    assert!(!package.exists());
    assert!(home.join(".copilot/skills").is_dir());
    assert!(!work.join(LEGACY_OWNER).exists());
}

#[test]
fn customized_legacy_package_is_preserved_on_repair() {
    for extra in ["notes.md", "scripts/local.mjs"] {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let (package, work) = legacy_package(&home, Some(extra));
        let server = app(&home, "app");
        let status = integration_at(&home, &server).unwrap();
        install_integration_at(&home, &server, status.revision.as_deref()).unwrap();
        assert_eq!(fs::read_to_string(package.join(extra)).unwrap(), "mine");
        assert!(package.join("SKILL.md").exists());
        assert!(package.join(".pr-sniper-install.json").exists());
        assert!(!work.join(LEGACY_OWNER).exists());
    }
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let (package, _) = legacy_package(&home, None);
    fs::write(package.join("SKILL.md"), "edited").unwrap();
    let server = app(&home, "app");
    let status = integration_at(&home, &server).unwrap();
    install_integration_at(&home, &server, status.revision.as_deref()).unwrap();
    assert_eq!(
        fs::read_to_string(package.join("SKILL.md")).unwrap(),
        "edited"
    );
}

#[cfg(unix)]
mod linked {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    /// A dotfiles-style real configuration file outside ~/.copilot.
    fn linked_config(home: &Path, name: &str, value: &Value) -> PathBuf {
        let dotfiles = home.join(name);
        fs::create_dir_all(&dotfiles).unwrap();
        let real = dotfiles.join("copilot-mcp.json");
        fs::write(&real, serde_json::to_vec(value).unwrap()).unwrap();
        real
    }

    fn link(target: &Path, link: &Path) {
        let _ = fs::remove_file(link);
        symlink(target, link).unwrap();
    }

    fn read(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn linked_config_file_is_replaced_at_its_real_target_and_link_preserved() {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let original = json!({"mcpServers":{"other":{"command":"keep"}}});
        let real = linked_config(&home, "dotfiles", &original);
        fs::create_dir_all(home.join(".copilot")).unwrap();
        link(&real, &config_path(&home));
        let server = app(&home, "app");
        let installed = install_integration_at(&home, &server, None).unwrap();
        assert_eq!(installed.state, "installed");
        assert_eq!(fs::read_link(config_path(&home)).unwrap(), real);
        assert_eq!(read(&real)["mcpServers"]["other"]["command"], "keep");
        assert_eq!(read(&real)["mcpServers"]["pr-sniper"]["type"], "local");
        let removed =
            remove_integration_at(&home, &server, installed.revision.as_deref().unwrap()).unwrap();
        assert_eq!(removed.state, "absent");
        assert_eq!(read(&real), original);
        assert_eq!(fs::read_link(config_path(&home)).unwrap(), real);
    }

    #[test]
    fn linked_copilot_directory_keeps_all_writes_in_its_real_target() {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let real = home.join("synced/copilot");
        fs::create_dir_all(&real).unwrap();
        symlink(&real, home.join(".copilot")).unwrap();
        let server = app(&home, "app");
        let installed = install_integration_at(&home, &server, None).unwrap();
        assert_eq!(installed.state, "installed");
        assert!(real.join("mcp-config.json").is_file());
        assert!(real.join(".pr-sniper-installer").is_dir());
        assert!(fs::symlink_metadata(home.join(".copilot"))
            .unwrap()
            .file_type()
            .is_symlink());
        remove_integration_at(&home, &server, installed.revision.as_deref().unwrap()).unwrap();
    }

    #[test]
    fn retargeted_config_link_after_interruption_conflicts_until_restored() {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let first = linked_config(&home, "first", &json!({"mcpServers":{}}));
        let second = linked_config(&home, "second", &json!({"mcpServers":{}}));
        fs::create_dir_all(home.join(".copilot")).unwrap();
        link(&first, &config_path(&home));
        let server = app(&home, "app");
        fail("mcp-intent-written");
        assert!(install_integration_at(&home, &server, None).is_err());
        link(&second, &config_path(&home));
        for result in [
            integration_at(&home, &server),
            install_integration_at(&home, &server, None),
        ] {
            assert_eq!(conflict_code(result), "installation_conflict");
        }
        assert_eq!(read(&first), json!({"mcpServers":{}}));
        assert_eq!(read(&second), json!({"mcpServers":{}}));
        link(&first, &config_path(&home));
        assert_eq!(integration_at(&home, &server).unwrap().state, "installed");
        assert_eq!(read(&first)["mcpServers"]["pr-sniper"]["type"], "local");
        assert_eq!(read(&second), json!({"mcpServers":{}}));
    }

    #[test]
    fn retargeted_config_link_during_a_write_is_never_written_through() {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let first = linked_config(&home, "first", &json!({"mcpServers":{}}));
        let second = linked_config(&home, "second", &json!({"mcpServers":{}}));
        fs::create_dir_all(home.join(".copilot")).unwrap();
        link(&first, &config_path(&home));
        let server = app(&home, "app");
        let path = config_path(&home);
        let retarget = second.clone();
        during("mcp-config-merged", move || link(&retarget, &path));
        assert_eq!(
            conflict_code(install_integration_at(&home, &server, None)),
            "installation_conflict"
        );
        assert_eq!(read(&first), json!({"mcpServers":{}}));
        assert_eq!(read(&second), json!({"mcpServers":{}}));
    }

    #[test]
    fn dangling_or_untrusted_links_are_refused_without_writes() {
        let root = root();
        let home = root.path().canonicalize().unwrap();
        let server = app(&home, "app");
        fs::create_dir_all(home.join(".copilot")).unwrap();
        let missing = home.join("missing/copilot-mcp.json");
        link(&missing, &config_path(&home));
        assert_eq!(
            conflict_code(install_integration_at(&home, &server, None)),
            "installation_conflict"
        );
        assert!(!missing.exists());

        let shared = linked_config(&home, "shared", &json!({"mcpServers":{}}));
        fs::set_permissions(home.join("shared"), fs::Permissions::from_mode(0o777)).unwrap();
        link(&shared, &config_path(&home));
        assert_eq!(
            conflict_code(install_integration_at(&home, &server, None)),
            "installation_conflict"
        );
        assert_eq!(read(&shared), json!({"mcpServers":{}}));
        fs::set_permissions(home.join("shared"), fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn linked_copilot_folder_is_refused_with_an_actionable_code() {
    let root = root();
    let home = root.path().canonicalize().unwrap();
    let real = home.join("synced");
    fs::create_dir_all(&real).unwrap();
    crate::windows_permissions::junction(&real, &home.join(".copilot"));
    let server = app(&home, "app");
    assert_eq!(
        conflict_code(install_integration_at(&home, &server, None)),
        "installation_linked_unsupported"
    );
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    fs::remove_dir(home.join(".copilot")).unwrap();
}

#[test]
fn integration_process_child() {
    let Some(root) = std::env::var_os("PR_SNIPER_MCP_INSTALL_TEST_ROOT") else {
        return;
    };
    let home = PathBuf::from(root);
    let owner = std::env::var("PR_SNIPER_MCP_INSTALL_TEST_OWNER").unwrap();
    assert_eq!(fs::read_to_string(home.join("test-owner")).unwrap(), owner);
    let server = app(&home, "new");
    let before = integration_at(&home, &server).unwrap();
    crash(&std::env::var("PR_SNIPER_MCP_INSTALL_TEST_POINT").unwrap());
    install_integration_at(&home, &server, before.revision.as_deref()).unwrap();
    panic!("expected injected process interruption");
}

#[test]
fn real_process_interruption_recovers_registration_and_foreign_entries() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for update in [false, true] {
        for point in [
            "mcp-intent-written",
            "mcp-config-written",
            "mcp-owner-written",
        ] {
            let root = root();
            let home = root.path().canonicalize().unwrap();
            write_config(
                &home,
                &json!({"mcpServers":{"other":{"command":"untouched"}}}),
            );
            if update {
                install_integration_at(&home, &app(&home, "old"), None).unwrap();
            }
            let owner = uuid::Uuid::new_v4().to_string();
            fs::write(home.join("test-owner"), &owner).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "control::installer::tests::integration_process_child",
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
                panic!("MCP registration child timed out");
            }
            let output = child.wait_with_output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(79),
                "{point}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let server = app(&home, "new");
            let status = integration_at(&home, &server).unwrap();
            assert_eq!(status.state, "installed", "{point}");
            remove_integration_at(&home, &server, status.revision.as_deref().unwrap()).unwrap();
            assert_eq!(
                config(&home),
                json!({"mcpServers":{"other":{"command":"untouched"}}})
            );
        }
    }
}
