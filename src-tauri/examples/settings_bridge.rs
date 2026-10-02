use pr_sniper_lib::storage::{Settings, Store};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

// Opt-in barriers for cross-process tests; normal commands still read stdin to EOF.
fn panel_probe(probe: Option<&str>, stage: &str) -> Result<(), String> {
    if probe != Some(stage) {
        return Ok(());
    }
    writeln!(io::stderr(), "fixture-panel:{stage}")
        .map_err(|_| "Cannot report panel fixture probe.")?;
    let mut release = String::new();
    io::stdin()
        .read_line(&mut release)
        .map_err(|_| "Cannot read panel fixture probe release.")?;
    if release.trim_end() != "continue" {
        return Err("Panel fixture probe was not released.".into());
    }
    Ok(())
}

// Only the process-per-command browser bridge serializes the native navigation
// session. The application retains this same state under a mutex in memory.
fn panel_dispatch(
    store: &Store,
    root: &Path,
    request: &Request,
    probe: Option<&str>,
) -> Result<Value, String> {
    use pr_sniper_lib::panel::{Route, Session, Tab};
    // Lock a stable sidecar, not the session inode replaced at commit. Never unlink
    // it while the root is live: waiting processes must share the same OS lock.
    std::fs::create_dir_all(root).map_err(|_| "Cannot lock panel fixture session.")?;
    let _ownership = {
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("fixture-panel-session.lock"))
            .map_err(|_| "Cannot lock panel fixture session.")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut reported = false;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err("Timed out waiting for panel fixture session.".into());
                    }
                    if probe.is_some() && !reported {
                        writeln!(io::stderr(), "fixture-panel:waiting")
                            .map_err(|_| "Cannot report panel fixture probe.")?;
                        reported = true;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(std::fs::TryLockError::Error(_)) => {
                    return Err("Cannot lock panel fixture session.".into());
                }
            }
        }
        lock
    };
    let path = root.join("fixture-panel-session.json");
    let mut session: Session = match std::fs::read(&path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Invalid panel fixture session.")?
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut session = Session::default();
            session.navigate(store, None)?;
            session.set_visible(true);
            session
        }
        Err(_) => return Err("Cannot read panel fixture session.".into()),
    };
    panel_probe(probe, "loaded")?;
    let route = match request.command.as_str() {
        "panel_navigate" => Some(
            serde_json::from_value(request.args["route"].clone())
                .map_err(|_| "Invalid panel route.")?,
        ),
        "open_settings" => Some(Route::tab(Tab::Settings)),
        "open_diagnostics" => Some(Route::utility(true)),
        "open_queue_item" => Some(Route::item(
            request.args["itemId"]
                .as_str()
                .ok_or("Item identity required.")?
                .into(),
        )),
        _ => None,
    };
    if route.is_some() || request.command == "fixture_show_panel" {
        session.navigate(store, route)?;
        session.set_visible(true);
    } else if request.command == "hide_panel" {
        session.set_visible(false);
    }
    let snapshot = session.snapshot(store);
    if request.command != "panel_snapshot" {
        let bytes = serde_json::to_vec(&session).map_err(|_| "Cannot encode panel fixture.")?;
        let mut file = tempfile::Builder::new()
            .prefix(".fixture-panel-session-")
            .tempfile_in(root)
            .map_err(|_| "Cannot save panel fixture session.")?;
        if probe == Some("staged") {
            let split = bytes.len() / 2;
            file.write_all(&bytes[..split])
                .map_err(|_| "Cannot save panel fixture session.")?;
            panel_probe(probe, "staged")?;
            file.write_all(&bytes[split..])
                .map_err(|_| "Cannot save panel fixture session.")?;
        } else {
            file.write_all(&bytes)
                .map_err(|_| "Cannot save panel fixture session.")?;
        }
        file.as_file()
            .sync_all()
            .map_err(|_| "Cannot save panel fixture session.")?;
        file.persist(&path)
            .map_err(|_| "Cannot save panel fixture session.")?;
    }
    if request.command == "open_queue_item" {
        if let Some(reason) = &snapshot.missing {
            return Err(reason.clone());
        }
    }
    serde_json::to_value(snapshot).map_err(|_| "Cannot encode panel snapshot.".into())
}

fn dispatch(store: &Store, request: Request) -> Result<Value, String> {
    match request.command.as_str() {
        "diagnostics" => serde_json::to_value(store.diagnostics()?)
            .map_err(|_| "Cannot encode diagnostics.".into()),
        "seed_action_observation" => {
            let item = request.args["itemId"]
                .as_str()
                .ok_or("Iteration identity required.")?;
            let observation = serde_json::from_value(request.args["observation"].clone())
                .map_err(|_| "Invalid action-observation fixture.")?;
            pr_sniper_lib::actions::synchronize(store, item, Ok(observation), 1_800_000_100)?;
            serde_json::to_value(store.load_actions()?)
                .map_err(|_| "Cannot encode action state.".into())
        }
        "start_final_review" => {
            pr_sniper_lib::actions::request_final(
                store,
                request.args["id"]
                    .as_str()
                    .ok_or("Final identity required.")?,
                request.args["confirmTrust"]
                    .as_bool()
                    .ok_or("Trust flag required.")?,
                1_800_000_110,
            )?;
            Ok(Value::Null)
        }
        "retry_action_observation" => {
            pr_sniper_lib::actions::request_observation_retry(
                store,
                request.args["itemId"]
                    .as_str()
                    .ok_or("Iteration identity required.")?,
                1_800_000_110,
            )?;
            Ok(Value::Null)
        }
        "reconcile_provider_action" => {
            pr_sniper_lib::actions::request_reconciliation(
                store,
                request.args["id"]
                    .as_str()
                    .ok_or("Action identity required.")?,
                1_800_000_110,
            )?;
            Ok(Value::Null)
        }
        "cancel_provider_action" => {
            pr_sniper_lib::actions::cancel_effect(
                store,
                request.args["id"]
                    .as_str()
                    .ok_or("Action identity required.")?,
            )?;
            Ok(Value::Null)
        }
        "fixture_capacity_snapshot" => {
            // Synthetic reservations only: no worker, provider session or inference is launched.
            let coordinator = pr_sniper_lib::capacity::Coordinator::default();
            let candidates = pr_sniper_lib::capacity::candidates(store, 1_800_000_000)?;
            for id in request.args["activeIds"]
                .as_array()
                .ok_or("Synthetic active IDs required.")?
            {
                let id = id.as_str().ok_or("Invalid synthetic work identity.")?;
                let work = candidates
                    .iter()
                    .find(|w| w.key.id == id)
                    .ok_or("Synthetic work identity is not a native candidate.")?;
                if let Some(cancelled) = coordinator.reserve(store, work, |cancelled| {
                    Ok((format!("fixture-{id}"), cancelled))
                })? {
                    if request.args["stoppingIds"]
                        .as_array()
                        .is_some_and(|ids| ids.iter().any(|value| value.as_str() == Some(id)))
                    {
                        cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
            serde_json::to_value(coordinator.snapshot(store, 1_800_000_000)?)
                .map_err(|_| "Cannot encode synthetic capacity state.".into())
        }
        "automation_snapshot" => serde_json::to_value(
            pr_sniper_lib::capacity::Coordinator::default().snapshot(store, 1_800_000_000)?,
        )
        .map_err(|_| "Cannot encode automation state.".into()),
        "set_automation_paused" => {
            store.save_automation(&pr_sniper_lib::capacity::Automation {
                paused: request.args["paused"]
                    .as_bool()
                    .ok_or("Pause flag required.")?,
            })?;
            Ok(Value::Null)
        }
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
        "seed_queue_state" => {
            let jobs: Vec<_> = serde_json::from_value(request.args["jobs"].clone())
                .map_err(|_| "Invalid queue fixture.")?;
            let reviews: Vec<_> = serde_json::from_value(request.args["reviews"].clone())
                .map_err(|_| "Invalid review fixture.")?;
            let publications: Vec<_> = serde_json::from_value(request.args["publications"].clone())
                .map_err(|_| "Invalid publication fixture.")?;
            let follow_ups = pr_sniper_lib::follow_up::decode_with_origins(
                &serde_json::to_vec(&request.args["follow_ups"])
                    .map_err(|_| "Invalid conversation fixture.")?,
                &publications,
            )
            .map_err(|_| "Invalid follow-up fixture.")?;
            let mut queue = store.load_queue_state()?;
            queue.jobs = jobs;
            if let Some(tracked) = request.args.get("tracked") {
                queue.tracked = serde_json::from_value(tracked.clone())
                    .map_err(|_| "Invalid tracked PR fixture.")?;
            }
            store.save_queue_state(&queue)?;
            if let Some(monitoring) = request.args.get("monitoring") {
                store.save_monitoring_state(
                    &serde_json::from_value(monitoring.clone())
                        .map_err(|_| "Invalid monitoring fixture.")?,
                )?;
            }
            store.save_reviews(&reviews)?;
            store.save_publications(&publications)?;
            store.save_follow_ups(&follow_ups)?;
            if let Some(actions) = request.args.get("actions") {
                store.save_actions(
                    &serde_json::from_value(actions.clone())
                        .map_err(|_| "Invalid action fixture.")?,
                )?;
            }
            if let Some(feedback) = request.args.get("feedback") {
                store.save_feedback(
                    &serde_json::from_value(feedback.clone())
                        .map_err(|_| "Invalid feedback fixture.")?,
                )?;
            }
            Ok(Value::Null)
        }
        "monitoring_snapshot" => serde_json::to_value(pr_sniper_lib::queue::snapshot(
            store,
            store
                .load_monitoring_state()?
                .health
                .into_values()
                .collect(),
        )?)
        .map_err(|_| "Cannot encode queue.".into()),
        "queue_destination" => Ok(json!(pr_sniper_lib::queue::destination(
            store,
            request.args["itemId"]
                .as_str()
                .ok_or("Item identity required.")?,
            request.args["file"].as_str(),
        )?
        .as_str())),
        "queue_selection" => Ok(json!(store.load_queue_selection()?)),
        "notification_snapshot" => serde_json::to_value(pr_sniper_lib::notifications::snapshot(
            store,
            Ok(pr_sniper_lib::notifications::Permission {
                authorization: "authorized".into(),
                alerts_enabled: Some(true),
                center_enabled: Some(true),
            }),
        )?)
        .map_err(|_| "Cannot encode notification test state.".into()),
        "set_notifications_enabled" => {
            let mut ledger = store.load_notifications()?;
            ledger.enabled = request.args["enabled"]
                .as_bool()
                .ok_or("Enabled flag required.")?;
            store.save_notifications(&ledger)?;
            Ok(Value::Null)
        }
        "seed_notifications" => {
            let ledger = serde_json::from_value(request.args)
                .map_err(|_| "Invalid notification fixture.")?;
            store.save_notifications(&ledger)?;
            Ok(Value::Null)
        }
        "observe_notifications" => {
            let snapshot = pr_sniper_lib::queue::snapshot(store, vec![])?;
            let mut ledger = store.load_notifications()?;
            ledger.observe(
                &pr_sniper_lib::notifications::frames(&snapshot),
                1_800_000_000,
            )?;
            store.save_notifications(&ledger)?;
            Ok(json!(ledger))
        }
        "test_notification" => {
            let destination = if let Some(item_id) = request.args["itemId"].as_str() {
                pr_sniper_lib::queue::destination(store, item_id, None)?;
                pr_sniper_lib::notifications::Destination::QueueItem {
                    item_id: item_id.into(),
                }
            } else {
                pr_sniper_lib::notifications::Destination::Settings
            };
            let mut ledger = store.load_notifications()?;
            let id = ledger.enqueue_test(destination, 1_800_000_000)?;
            store.save_notifications(&ledger)?;
            Ok(json!(id))
        }
        "notification_destination" => Ok(json!(pr_sniper_lib::notifications::destination(
            store,
            request.args["id"]
                .as_str()
                .ok_or("Notification ID required.")?,
        )?)),
        "select_queue_item" => {
            pr_sniper_lib::queue::select(store, request.args["itemId"].as_str())?;
            Ok(Value::Null)
        }
        "save_preferences" => {
            let settings = serde_json::from_value(request.args["settings"].clone())
                .map_err(|_| "Unsupported settings configuration.")?;
            let expected = serde_json::from_value(request.args["expected"].clone())
                .map_err(|_| "Unsupported settings snapshot.")?;
            recorded_settings(store, store.save_preferences(settings, &expected)?)
        }
        "saved_resources" => serde_json::to_value(store.saved_resources()?)
            .map_err(|_| "Cannot encode saved resources.".into()),
        "validate_resource" => {
            let edit = serde_json::from_value(request.args["edit"].clone())
                .map_err(|_| "Unsupported resource edit.")?;
            serde_json::to_value(store.validate_resource(edit)?.readiness())
                .map_err(|_| "Cannot encode resource readiness.".into())
        }
        "save_resource" => {
            let edit = serde_json::from_value(request.args["edit"].clone())
                .map_err(|_| "Unsupported resource edit.")?;
            recorded_settings(store, store.save_resource(edit)?)
        }
        "monitoring_activation_status" => {
            let repository_id = request.args["repositoryId"]
                .as_str()
                .ok_or("Repository ID is required.")?;
            let settings = store.load_settings()?;
            let monitor = pr_sniper_lib::monitoring::Monitor::restore(store)?;
            serde_json::to_value(monitor.activation_status(&settings, repository_id))
                .map_err(|_| "Cannot encode monitoring scope status.".into())
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
    let probe_arg = std::env::args().nth(2);
    let probe = match probe_arg.as_deref() {
        None => None,
        Some("--panel-probe=loaded") => Some("loaded"),
        Some("--panel-probe=staged") => Some("staged"),
        Some(_) => return Err("Unsupported panel fixture probe".into()),
    };
    let mut input = String::new();
    if probe.is_some() {
        io::stdin().read_line(&mut input)?;
    } else {
        io::stdin().read_to_string(&mut input)?;
    }
    let request: Request = serde_json::from_str(&input)?;
    let store = Store::new(root.clone());
    let result = if matches!(
        request.command.as_str(),
        "panel_snapshot"
            | "panel_navigate"
            | "hide_panel"
            | "open_settings"
            | "open_diagnostics"
            | "open_queue_item"
            | "fixture_show_panel"
    ) {
        panel_dispatch(&store, &root, &request, probe)
    } else if probe.is_some() {
        Err("Panel fixture probes require a panel command.".into())
    } else {
        dispatch(&store, request)
    };
    let response = match result {
        Ok(value) => json!({ "ok": value }),
        Err(message) => json!({ "error": message }),
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}
