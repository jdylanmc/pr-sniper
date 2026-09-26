use pr_sniper_lib::storage::Settings;
use serde_json::json;

mod support;
use support::Fixture;

#[test]
fn sidebar_save_preserves_hidden_policy_and_rejects_stale_drafts() {
    let fixture = Fixture::new();
    let original = fixture.store().load_settings().unwrap();
    let mut edited: Settings =
        serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
    edited.defaults.automatic_agent_start = true;
    fixture.store().save_preferences(edited, &original).unwrap();
    let saved = fixture.store().load_settings().unwrap();
    assert!(saved.defaults.automatic_agent_start);
    assert!(!saved.defaults.automatic_comment_publication);
    assert_eq!(
        saved.defaults.reviewer_assignment,
        original.defaults.reviewer_assignment
    );
    assert!(fixture
        .store()
        .save_preferences(original.clone(), &saved)
        .is_ok());
    assert!(fixture.store().save_preferences(saved, &original).is_ok());
    let stale = fixture
        .store()
        .save_preferences(original.clone(), &original);
    assert!(stale.unwrap_err().contains("changed"));
}

#[test]
fn presets_survive_reopen_and_only_update_selected_prompts() {
    let fixture = Fixture::new();
    let original = fixture.store().load_settings().unwrap();
    let preset = "a17a5695-0e09-489b-bcb2-1da94dc3b8aa";
    let mut value = serde_json::to_value(&original).unwrap();
    value["presets"] =
        json!([{"id": preset, "name": "API review", "body": "Find breaking API changes."}]);
    value["default_review_preset"] = json!(preset);
    let edited = serde_json::from_value(value).unwrap();
    fixture.store().save_preferences(edited, &original).unwrap();
    let saved = fixture.store().load_settings().unwrap();
    assert_eq!(saved.defaults.prompt, "Find breaking API changes.");
    assert_eq!(saved.presets[0].name, "API review");
}

#[test]
fn preset_import_rejects_unknown_fields_credentials_and_duplicate_names() {
    for presets in [
        json!([{"id": "a17a5695-0e09-489b-bcb2-1da94dc3b8aa", "name": "API", "body": "Review", "command": "do not execute"}]),
        json!([{"id": "a17a5695-0e09-489b-bcb2-1da94dc3b8aa", "name": "API", "body": "ghp_synthetic"}]),
        json!([
            {"id": "a17a5695-0e09-489b-bcb2-1da94dc3b8aa", "name": "API", "body": "Review"},
            {"id": "f89d0c4a-7f42-45ad-8a99-372866d7ab34", "name": "api", "body": "Review"}
        ]),
    ] {
        let fixture = Fixture::new();
        let baseline = fixture.store().load_settings().unwrap();
        let path = fixture.path().join("config/settings.json");
        let before = std::fs::read(&path).unwrap();
        let mut value = serde_json::to_value(&baseline).unwrap();
        value["presets"] = presets;
        if let Ok(settings) = serde_json::from_value::<Settings>(value) {
            assert!(fixture
                .store()
                .save_preferences(settings, &baseline)
                .is_err());
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
