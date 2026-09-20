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

fn dispatch(store: &Store, request: Request) -> Result<Value, String> {
    match request.command.as_str() {
        "seed_settings" => {
            let settings: Settings = serde_json::from_value(request.args)
                .map_err(|_| "Invalid settings test fixture.".to_string())?;
            store.save_settings(&settings)?;
            Ok(Value::Null)
        }
        "snapshot" => Ok(json!({
            "settings": store.load_settings()?,
            "login_registration": "absent",
            "isolated": true,
            "error": null,
            "version": env!("CARGO_PKG_VERSION")
        })),
        "save_repository" => {
            let repository = request.args["repository"]
                .as_str()
                .ok_or("Repository name is required.")?;
            serde_json::to_value(store.add_repository(repository)?)
                .map_err(|_| "Cannot encode settings.".into())
        }
        // GREEN connects these IPC arms to the same Store operations as the app.
        "update_repository" | "remove_repository" => {
            Err("Repository editing and removal are not implemented.".into())
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
