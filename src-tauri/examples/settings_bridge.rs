use pr_sniper_lib::storage::{Settings, Store};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    command: String,
    #[serde(default)]
    args: Value,
}

fn recorded_settings(store: &Store, settings: Settings) -> Result<Value, String> {
    serde_json::to_value(store.finish_settings_save(settings))
        .map_err(|_| "Cannot encode settings.".into())
}

fn dispatch(store: &Store, request: Request) -> Result<Value, String> {
    match request.command.as_str() {
        "seed_settings" => {
            let settings: Settings = serde_json::from_value(request.args)
                .map_err(|_| "Invalid settings test fixture.".to_string())?;
            store.save_settings(&settings)?;
            Ok(Value::Null)
        }
        "snapshot" => {
            let (settings, error) = match store.load_settings() {
                Ok(settings) => (Some(settings), None),
                Err(error) => (None, Some(error)),
            };
            Ok(json!({
                "settings": settings,
                "login_registration": "absent",
                "isolated": true,
                "error": error,
                "version": env!("CARGO_PKG_VERSION")
                ,"settings_persisted": store.has_saved_settings()
            }))
        }
        "save_preferences" => {
            let settings = serde_json::from_value(request.args["settings"].clone())
                .map_err(|_| "Unsupported settings configuration.")?;
            let expected = serde_json::from_value(request.args["expected"].clone())
                .map_err(|_| "Unsupported settings snapshot.")?;
            recorded_settings(store, store.save_preferences(settings, &expected)?)
        }
        "canonical_repository_name" => {
            let repository = request.args["repository"]
                .as_str()
                .ok_or("Repository required.")?;
            let scratch = pr_sniper_lib::storage::canonical_repository(repository)?;
            Ok(json!(scratch))
        }
        "discover_repositories" => {
            let root = request.args["root"]
                .as_str()
                .ok_or("Root folder required.")?;
            serde_json::to_value(pr_sniper_lib::discovery::discover(std::path::Path::new(
                root,
            ))?)
            .map_err(|_| "Cannot encode discovery.".into())
        }
        "save_repository" => {
            let repository = request.args["repository"]
                .as_str()
                .ok_or("Repository name is required.")?;
            recorded_settings(store, store.add_repository(repository)?)
        }
        "update_repository" => {
            let id = request.args["id"]
                .as_str()
                .ok_or("Repository ID is required.")?;
            let name = request.args["repository"]
                .as_str()
                .ok_or("Repository name is required.")?;
            let enabled = request.args["enabled"]
                .as_bool()
                .ok_or("Enabled state is required.")?;
            recorded_settings(store, store.update_repository(id, name, enabled)?)
        }
        "remove_repository" => {
            let id = request.args["id"]
                .as_str()
                .ok_or("Repository ID is required.")?;
            recorded_settings(store, store.remove_repository(id)?)
        }
        "save_defaults" => {
            let policy = serde_json::from_value(request.args["policy"].clone())
                .map_err(|_| "Unsupported policy configuration.")?;
            recorded_settings(store, store.save_defaults(policy)?)
        }
        "save_repository_policy" => {
            let id = request.args["id"]
                .as_str()
                .ok_or("Repository ID is required.")?;
            let overrides = serde_json::from_value(request.args["overrides"].clone())
                .map_err(|_| "Unsupported policy override configuration.")?;
            recorded_settings(store, store.save_repository_policy(id, overrides)?)
        }
        command => Err(format!("Unsupported settings test command: {command}")),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("Missing test data root")?);
    if !root.is_absolute() {
        return Err("Test data root must be absolute".into());
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Request = serde_json::from_str(&input)?;
    let response = match dispatch(&Store::new(root), request) {
        Ok(value) => json!({ "ok": value }),
        Err(message) => json!({ "error": message }),
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}
