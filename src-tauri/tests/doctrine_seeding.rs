use pr_sniper_lib::storage::{Doctrine, Store};
use std::{collections::HashSet, fs, path::Path};

mod support;
use support::Fixture;

fn canonical_doctrines() -> Vec<Doctrine> {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.agents/skills/doctrine/doctrines");
    let mut doctrines: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".doctrine.md"))
        .map(|path| {
            let title = path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .strip_suffix(".doctrine.md")
                .unwrap()
                .to_string();
            let source = fs::read_to_string(path).unwrap();
            let mut lines = source.lines();
            assert_eq!(lines.next(), Some("---"));
            let metadata: Vec<_> = lines.by_ref().take_while(|line| *line != "---").collect();
            assert!(metadata.contains(&format!("name: {title}").as_str()));
            let heading = lines.find(|line| !line.trim().is_empty()).unwrap();
            assert!(heading.starts_with("# "));
            let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            assert!(!body.is_empty());
            Doctrine { title, body }
        })
        .collect();
    doctrines.sort_by(|a, b| a.title.cmp(&b.title));
    assert_eq!(doctrines.len(), 23);
    doctrines
}

#[test]
fn fresh_settings_persist_the_exact_complete_canonical_catalog() {
    let fixture = Fixture::new();
    let settings = fixture.store().load_settings().unwrap();
    assert_eq!(settings.doctrines, canonical_doctrines());
    assert_eq!(
        settings
            .doctrines
            .iter()
            .map(|doctrine| &doctrine.title)
            .collect::<HashSet<_>>()
            .len(),
        23
    );
    assert!(fixture.store().has_saved_settings());
    let persisted = fs::read(fixture.path().join("config/settings.json")).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&persisted).unwrap()["doctrines"],
        serde_json::to_value(&settings.doctrines).unwrap()
    );
    assert!(!settings.launch_at_login);
    assert!(!settings.defaults.automatic_agent_start);
    assert!(!settings.defaults.automatic_comment_publication);
    assert!(settings.agents.is_empty());
    assert!(settings.repositories.is_empty());
    let restarted = Store::new(fixture.path().into());
    assert_eq!(restarted.load_settings().unwrap(), settings);
    assert_eq!(
        fs::read(fixture.path().join("config/settings.json")).unwrap(),
        persisted,
        "reopening must not rewrite the initialized catalog"
    );
}

#[test]
fn saving_another_section_first_retains_all_doctrines() {
    let fixture = Fixture::new();
    let saved = fixture.store().add_repository("fixture/project").unwrap();
    assert_eq!(saved.doctrines, canonical_doctrines());
    assert_eq!(fixture.store().load_settings().unwrap(), saved);
}

#[test]
fn edited_created_deleted_and_explicitly_empty_libraries_survive_restart() {
    let fixture = Fixture::new();
    let original = fixture.store().load_settings().unwrap();
    let mut edited = original.clone();
    edited.doctrines[0] = Doctrine {
        title: "renamed-principle".into(),
        body: "User-edited principles, not the shipped text.".into(),
    };
    edited.doctrines.remove(1);
    edited.doctrines.push(Doctrine {
        title: "custom-principle".into(),
        body: "User-created principles.".into(),
    });
    let saved = fixture
        .store()
        .save_preferences(edited.clone(), &original)
        .unwrap();
    assert_eq!(fixture.store().load_settings().unwrap(), edited);
    edited.doctrines.clear();
    fixture
        .store()
        .save_preferences(edited.clone(), &saved)
        .unwrap();
    let path = fixture.path().join("config/settings.json");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["doctrines"], serde_json::json!([]));
    assert_eq!(fixture.store().load_settings().unwrap(), edited);
    let saved = fixture.store().add_repository("fixture/project").unwrap();
    assert!(saved.doctrines.is_empty());
    assert!(fixture
        .store()
        .load_settings()
        .unwrap()
        .doctrines
        .is_empty());
}

#[test]
fn existing_custom_empty_and_legacy_missing_libraries_are_not_reseeded() {
    for library in [
        "",
        r#","doctrines":[]"#,
        r#","doctrines":[{"title":"mine","body":"Keep this exact custom body."}]"#,
    ] {
        let fixture = Fixture::new();
        let path = fixture.path().join("config/settings.json");
        fs::create_dir(path.parent().unwrap()).unwrap();
        let original = format!(r#"{{"launch_at_login":true{library}}}"#);
        fs::write(&path, &original).unwrap();
        let settings = fixture.store().load_settings().unwrap();
        assert!(settings.launch_at_login);
        assert_eq!(
            settings.doctrines.len(),
            usize::from(library.contains("mine"))
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fixture
            .store()
            .save_preferences(settings.clone(), &settings)
            .unwrap();
        assert_eq!(fixture.store().load_settings().unwrap(), settings);
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(value["doctrines"].is_array());
    }
}

#[test]
fn initialization_failure_is_an_error_not_a_saved_snapshot_and_can_be_retried() {
    let fixture = Fixture::new();
    let blocked = fixture.path().join("config/settings.json.tmp");
    fs::create_dir_all(&blocked).unwrap();
    let error = fixture.store().load_settings().unwrap_err();
    assert_eq!(error, "Cannot write settings.");
    assert!(!fixture.store().has_saved_settings());
    fs::remove_dir(blocked).unwrap();
    assert_eq!(
        fixture.store().load_settings().unwrap().doctrines,
        canonical_doctrines()
    );
    assert!(fixture.store().has_saved_settings());
}

#[test]
fn initialized_catalog_does_not_bypass_stale_draft_protection() {
    let fixture = Fixture::new();
    let expected = fixture.store().load_settings().unwrap();
    let mut external = expected.clone();
    external.doctrines[0].body = "Changed in another window.".into();
    fixture
        .store()
        .save_preferences(external.clone(), &expected)
        .unwrap();
    let mut stale = expected.clone();
    stale.root_folder = Some("/fixture/root".into());
    assert!(fixture
        .store()
        .save_preferences(stale, &expected)
        .unwrap_err()
        .contains("Settings changed in another window"));
    assert_eq!(fixture.store().load_settings().unwrap(), external);
}
