use super::{check_revision, installer, preset, revision, Error, RequestOverride, PROTOCOL};
use crate::{
    storage::{ResourceEdit, Store},
    Host,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tauri::Manager;

struct Request {
    request_id: String,
    issued_at: i64,
    command: Command,
}

#[allow(clippy::large_enum_variant)]
#[derive(Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Command {
    Describe,
    Inspect,
    Resources {
        kind: ResourceKind,
        #[serde(default)]
        offset: usize,
        limit: Option<usize>,
    },
    Accounts {
        kind: AccountKind,
        #[serde(default)]
        offset: usize,
        limit: Option<usize>,
    },
    Queue {
        #[serde(default)]
        offset: usize,
        limit: Option<usize>,
    },
    Health {
        #[serde(default)]
        offset: usize,
        limit: Option<usize>,
    },
    Detail {
        item_id: String,
        #[serde(default)]
        offset: usize,
        limit: Option<usize>,
    },
    Target {
        url: String,
    },
    Models {
        account_id: String,
    },
    Resolve {
        account_id: String,
        url: String,
    },
    Validate {
        edit: ResourceEdit,
    },
    Save {
        revision: String,
        edit: ResourceEdit,
        account_generation: Option<u64>,
        #[serde(default)]
        allow_grants: bool,
    },
    Preset {
        revision: String,
        account_id: String,
        model: String,
    },
    Watch {
        expected: crate::storage::Repository,
        url: String,
        account_generation: u64,
        #[serde(default)]
        review_again: bool,
        #[serde(default)]
        overrides: BTreeMap<String, RequestOverride>,
    },
    Pause {
        paused: bool,
    },
    CheckNow,
    Retry {
        operation_id: String,
    },
    RetryReview {
        candidate_key: String,
    },
    RetryState {
        item_id: String,
    },
    CancelReview {
        operation_id: String,
    },
    Publish {
        review_operation_id: String,
    },
    CancelPublication {
        publication_id: String,
    },
    CancelAction {
        id: String,
    },
    ReconcileAction {
        id: String,
    },
    RetryActionObservation {
        item_id: String,
    },
    FollowUp {
        id: String,
    },
    CancelFollowUp {
        id: String,
    },
    Clean {
        item_id: Option<String>,
    },
    DismissStale {
        item_id: String,
        expected_token: String,
    },
    Theme {
        theme: crate::storage::Theme,
    },
    LoginItem {
        enabled: bool,
    },
    Notifications {
        enabled: bool,
    },
    NotificationCategory {
        category: crate::notifications::Category,
        enabled: bool,
    },
    Installation,
    Install {
        host: String,
        expected: Option<String>,
    },
    Uninstall {
        host: String,
        expected: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResourceKind {
    Agents,
    Doctrines,
    Voices,
    Repositories,
    Preferences,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AccountKind {
    Repository,
    Copilot,
}

impl Command {
    fn mutating(&self) -> bool {
        !matches!(
            self,
            Self::Describe
                | Self::Inspect
                | Self::Resources { .. }
                | Self::Accounts { .. }
                | Self::Queue { .. }
                | Self::Health { .. }
                | Self::Detail { .. }
                | Self::Target { .. }
                | Self::Models { .. }
                | Self::Resolve { .. }
                | Self::Validate { .. }
                | Self::Installation
        )
    }
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    id: String,
    issued_at: i64,
    hash: String,
    response: Option<Result<Value, Error>>,
}

pub(super) fn storage(error: impl Serialize) -> Error {
    Error::new("storage", error)
}
pub(super) fn operation(error: impl Serialize) -> Error {
    Error::new("operation_failed", error)
}
pub(super) fn value(value: impl Serialize) -> Result<Value, Error> {
    serde_json::to_value(value).map_err(|_| Error::new("encoding", ()))
}

fn receipts(store: &Store) -> Result<Vec<Receipt>, Error> {
    match store.read_state("control-receipts.json") {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| storage("Invalid control receipts."))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(storage("Control receipts unavailable.")),
    }
}

pub(crate) fn begin_request(
    store: &Store,
    id: &str,
    issued_at: i64,
    hash: &str,
    now: i64,
) -> Result<Option<Result<Value, Error>>, Error> {
    if issued_at < now - 300 || issued_at > now + 30 {
        return Err(Error::new("expired_request", ()));
    }
    let mut receipts = receipts(store)?;
    receipts.retain(|r| r.issued_at >= now - 300);
    if let Some(previous) = receipts.iter().find(|r| r.id == id) {
        if previous.hash != hash {
            return Err(Error::new("request_id_conflict", ()));
        }
        return Ok(Some(previous.response.clone().unwrap_or_else(|| {
            Err(Error::new("outcome_unknown", json!({"request_id":id})))
        })));
    }
    if receipts.len() >= 256 {
        return Err(Error::new(
            "busy",
            "Receipt capacity; wait before submitting new mutations.",
        ));
    }
    receipts.push(Receipt {
        id: id.into(),
        issued_at,
        hash: hash.into(),
        response: None,
    });
    store
        .write_state("control-receipts.json", &receipts)
        .map_err(storage)?;
    Ok(None)
}

pub(crate) fn finish_request(
    store: &Store,
    id: &str,
    response: Result<Value, Error>,
) -> Result<(), Error> {
    let mut receipts = receipts(store)?;
    let receipt = receipts
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| Error::new("outcome_unknown", ()))?;
    receipt.response = Some(response);
    store
        .write_state("control-receipts.json", &receipts)
        .map_err(|_| Error::new("outcome_unknown", ()))
}

pub(crate) async fn execute(app: &tauri::AppHandle, input: Value) -> Result<Value, Error> {
    let host = app.state::<Host>();
    execute_request(&host.store, input, |command| execute_command(app, command)).await
}

pub(super) async fn execute_request<F, Fut>(
    store: &std::sync::Mutex<Store>,
    input: Value,
    dispatch: F,
) -> Result<Value, Error>
where
    F: FnOnce(Command) -> Fut,
    Fut: std::future::Future<Output = Result<Value, Error>>,
{
    // Parse the envelope separately: serde's flatten + deny_unknown_fields
    // combination cannot validate an internally tagged flattened enum.
    let mut fields = input
        .as_object()
        .cloned()
        .ok_or_else(|| Error::new("invalid_request", ()))?;
    let request_id = fields
        .remove("request_id")
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(|| Error::new("invalid_request", "request_id required"))?;
    let issued_at = fields
        .remove("issued_at")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::new("invalid_request", "issued_at required"))?;
    let supplied_fields: Vec<_> = fields.keys().cloned().collect();
    let command: Command = serde_json::from_value(Value::Object(fields))
        .map_err(|_| Error::new("invalid_request", ()))?;
    // Serde's internally tagged unit variants otherwise ignore extra fields.
    let supported_fields =
        serde_json::to_value(&command).map_err(|_| Error::new("encoding", ()))?;
    if supplied_fields
        .iter()
        .any(|field| supported_fields.get(field).is_none())
    {
        return Err(Error::new("invalid_request", ()));
    }
    let request = Request {
        request_id,
        issued_at,
        command,
    };
    uuid::Uuid::parse_str(&request.request_id)
        .map_err(|_| Error::new("invalid_request", "Invalid request_id."))?;
    let mutating = request.command.mutating();
    let hash = request_hash(input);
    if mutating {
        let now = crate::now_seconds().map_err(storage)?;
        let store = store.lock().map_err(|_| storage(()))?;
        if let Some(previous) =
            begin_request(&store, &request.request_id, request.issued_at, &hash, now)?
        {
            return previous.map(|value| replay_marker(value, true));
        }
    }
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        dispatch(request.command),
    )
    .await
    .unwrap_or_else(|_| {
        Err(Error::new(
            "outcome_unknown",
            json!({"request_id":request.request_id}),
        ))
    });
    let response = if mutating {
        response.map(|value| replay_marker(value, false))
    } else {
        response
    };
    if mutating {
        let store = store
            .lock()
            .map_err(|_| Error::new("outcome_unknown", ()))?;
        finish_request(&store, &request.request_id, response.clone())?;
    }
    response
}

fn replay_marker(mut value: Value, replayed: bool) -> Value {
    if let Some(fields) = value.as_object_mut() {
        fields.insert("replayed".into(), Value::Bool(replayed));
        value
    } else {
        json!({"value":value,"replayed":replayed})
    }
}

fn request_hash(mut input: Value) -> String {
    fn canonical(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                fields.sort_keys();
                for child in fields.values_mut() {
                    canonical(child);
                }
            }
            Value::Array(values) => {
                for child in values {
                    canonical(child);
                }
            }
            _ => {}
        }
    }
    canonical(&mut input);
    revision(&input)
}

pub(crate) fn save_checked(
    store: &Store,
    expected: &str,
    edit: ResourceEdit,
    allow_grants: bool,
) -> Result<crate::storage::Settings, Error> {
    let current = store.load_settings().map_err(storage)?;
    check_revision(expected, &current)?;
    let next = store
        .validate_resource(edit.clone())
        .map_err(|e| Error::new("validation", e))?;
    if !allow_grants && expands_grants(&current, &next) {
        return Err(Error::new("explicit_grant_required", ()));
    }
    store
        .save_resource(edit)
        .map_err(|e| Error::new("validation", e))
}

fn expands_grants(old: &crate::storage::Settings, new: &crate::storage::Settings) -> bool {
    new.repositories.iter().any(|repo| {
        repo.assignments.iter().any(|assignment| {
            let before = old
                .repositories
                .iter()
                .find(|r| r.id == repo.id)
                .and_then(|r| {
                    r.assignments
                        .iter()
                        .find(|a| a.id == assignment.id)
                        .map(|a| (r, a))
                });
            let authority = repo.assignment_authority(assignment);
            let previous = before.map(|(r, a)| r.assignment_authority(a));
            let granted = |f: fn(&crate::storage::AssignmentAuthority) -> bool| {
                f(&authority) && !previous.as_ref().is_some_and(f)
            };
            granted(|a| a.comment)
                || granted(|a| a.reply)
                || granted(|a| a.approve)
                || granted(|a| a.merge)
                || assignment.actions.as_ref().is_some_and(|a| a.include_self)
                    && !before
                        .is_some_and(|(_, a)| a.actions.as_ref().is_some_and(|a| a.include_self))
                || before.is_some_and(|(r, a)| {
                    repo.assignment_tools(assignment)
                        .iter()
                        .any(|t| !r.assignment_tools(a).contains(t))
                })
        })
    })
}

async fn validate_model(
    app: &tauri::AppHandle,
    account_id: &str,
    model: &str,
    intelligence: Option<&crate::storage::AgentIntelligence>,
) -> Result<(), Error> {
    let models = crate::copilot::list_copilot_models(
        app.clone(),
        account_id.into(),
        uuid::Uuid::new_v4().to_string(),
    )
    .await
    .map_err(|e| Error::new("account_or_models_unavailable", e))?;
    let models = serde_json::to_value(models).map_err(|_| storage(()))?;
    let selected = models
        .as_array()
        .and_then(|models| {
            models.iter().find(|m| {
                m["id"].as_str() == Some(model)
                    && m.get("policy")
                        .and_then(|p| p.get("state"))
                        .and_then(Value::as_str)
                        .is_none_or(|s| matches!(s, "enabled" | "unconfigured"))
            })
        })
        .ok_or_else(|| Error::new("model_unavailable", ()))?;
    if let Some(choice) = intelligence {
        if let Some(effort) = &choice.reasoning_effort {
            if selected.pointer("/capabilities/supports/reasoningEffort")
                == Some(&Value::Bool(false))
                || !selected["supportedReasoningEfforts"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(effort)))
            {
                return Err(Error::new("effort_unavailable", ()));
            }
        }
        if let Some(tier) = &choice.context_tier {
            let supported = selected["supportedContextTiers"]
                .as_array()
                .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(tier)))
                || match tier.as_str() {
                    "default" => {
                        selected
                            .pointer("/billing/tokenPrices/maxPromptTokens")
                            .is_some()
                            || selected
                                .pointer("/billing/tokenPrices/contextMax")
                                .is_some()
                    }
                    "long_context" => selected
                        .pointer("/billing/tokenPrices/longContext")
                        .is_some(),
                    _ => false,
                };
            if !supported || !["default", "long_context"].contains(&tier.as_str()) {
                return Err(Error::new("context_unavailable", ()));
            }
        }
    }
    Ok(())
}

async fn execute_command(app: &tauri::AppHandle, command: Command) -> Result<Value, Error> {
    if matches!(command, Command::Inspect | Command::Accounts { .. }) {
        crate::copilot::copilot_auth_state(app.clone())
            .await
            .map_err(operation)?;
    }
    if super::dispatcher::handles(&command) {
        let app = app.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let host = app.state::<Host>();
            super::dispatcher::dispatch(
                &super::dispatcher::Native {
                    app: &app,
                    host: &host,
                },
                command,
            )
        })
        .await
        .map_err(|_| Error::new("outcome_unknown", ()))?;
    }
    match command {
        Command::Accounts { .. }
        | Command::Queue { .. }
        | Command::Health { .. }
        | Command::Detail { .. }
        | Command::RetryReview { .. }
        | Command::Retry { .. }
        | Command::Inspect
        | Command::Resources { .. }
        | Command::Target { .. }
        | Command::Watch { .. } => unreachable!("shared dispatcher"),
        Command::Describe => Ok(json!({
            "protocol": PROTOCOL, "app_version":env!("CARGO_PKG_VERSION"),
            "host":"copilot_cli", "contract":include_str!("../../../skills/pr-sniper/references/control.md"),
            "tools":crate::storage::PACKET_TOOLS
        })),
        Command::Models { account_id } => value(
            crate::copilot::list_copilot_models(
                app.clone(),
                account_id,
                uuid::Uuid::new_v4().to_string(),
            )
            .await
            .map_err(operation)?,
        ),
        Command::Resolve { account_id, url } => value(
            crate::resolve_provider_repository(
                app.clone(),
                crate::storage::ProviderId::Github,
                account_id,
                url,
            )
            .await
            .map_err(operation)?,
        ),
        Command::Validate { edit } => {
            value(crate::validate_resource(app.state::<Host>(), edit).map_err(operation)?)
        }
        Command::Save {
            revision: expected,
            edit,
            account_generation,
            allow_grants,
        } => {
            if let ResourceEdit::Agent {
                expected,
                value: Some(agent),
                ..
            } = &edit
            {
                if expected.as_ref().is_none_or(|old| {
                    old.model != agent.model
                        || old.ai_account != agent.ai_account
                        || old.intelligence != agent.intelligence
                }) {
                    let account = agent
                        .ai_account
                        .as_ref()
                        .ok_or_else(|| Error::new("account_required", ()))?;
                    if account.provider != "copilot" {
                        return Err(Error::new("unsupported_provider", ()));
                    }
                    validate_model(
                        app,
                        &account.account_id,
                        &agent.model,
                        agent.intelligence.as_ref(),
                    )
                    .await?;
                }
            }
            let host = app.state::<Host>();
            let settings = {
                let generations = host.github_generations.lock().map_err(|_| storage(()))?;
                let auth = host.github_auth.lock().map_err(|_| storage(()))?;
                crate::validate_repository_save_account(
                    &auth,
                    &generations,
                    &edit,
                    account_generation,
                )
                .map_err(operation)?;
                let store = host.store.lock().map_err(|_| storage(()))?;
                save_checked(&store, &expected, edit, allow_grants)?
            };
            let saved = crate::finish_committed_settings(
                &host.github_generations,
                &host.github_auth,
                &host.store,
                &host.monitor,
                settings,
            );
            Ok(
                json!({"state":"saved", "revision": revision(&saved.settings), "warning": saved.warning}),
            )
        }
        Command::Preset {
            revision,
            account_id,
            model,
        } => {
            validate_model(app, &account_id, &model, None).await?;
            let host = app.state::<Host>();
            let store = host.store.lock().map_err(|_| storage(()))?;
            preset::apply(&store, &revision, &account_id, &model)
        }
        Command::Pause { paused } => {
            crate::capacity::set_automation_paused(app.clone(), paused).map_err(operation)?;
            Ok(json!({"state":"saved","paused":paused}))
        }
        Command::CheckNow => {
            crate::check_now(app.clone()).await.map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::RetryState { item_id } => {
            crate::review::host::retry_packet_state(app.clone(), item_id)
                .await
                .map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::CancelReview { operation_id } => {
            crate::review::host::cancel_review(app.clone(), operation_id)
                .await
                .map_err(operation)?;
            Ok(json!({"state":"stopping"}))
        }
        Command::Publish {
            review_operation_id,
        } => {
            crate::publication::host::publish_review(app.clone(), review_operation_id)
                .await
                .map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::CancelPublication { publication_id } => {
            crate::publication::host::cancel_publication(app.clone(), publication_id)
                .await
                .map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::CancelAction { id } => {
            crate::actions::host::cancel_provider_action(app.state::<Host>(), id)
                .map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::ReconcileAction { id } => {
            crate::actions::host::reconcile_provider_action(app.clone(), id).map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::RetryActionObservation { item_id } => {
            crate::actions::host::retry_action_observation(app.clone(), item_id)
                .map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::FollowUp { id } => {
            crate::follow_up::host::start_follow_up(app.clone(), id)
                .await
                .map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::CancelFollowUp { id } => {
            crate::follow_up::host::cancel_follow_up(app.clone(), id)
                .await
                .map_err(operation)?;
            Ok(json!({"state":"requested"}))
        }
        Command::Clean { item_id } => value(
            crate::clean_reviewed(app.clone(), item_id)
                .await
                .map_err(operation)?,
        ),
        Command::DismissStale {
            item_id,
            expected_token,
        } => {
            crate::dismiss_stale_review(app.clone(), item_id, expected_token)
                .await
                .map_err(operation)?;
            Ok(json!({"state":"saved"}))
        }
        Command::Theme { theme } => {
            crate::save_theme(app.state::<Host>(), theme).map_err(operation)?;
            Ok(json!({"state":"saved"}))
        }
        Command::LoginItem { enabled } => {
            crate::save_login(app.state::<Host>(), enabled).map_err(operation)?;
            Ok(json!({"state":"saved"}))
        }
        Command::Notifications { enabled } => value(
            crate::notifications::host::set_notifications_enabled(app.clone(), enabled)
                .await
                .map_err(operation)?,
        ),
        Command::NotificationCategory { category, enabled } => value(
            crate::notifications::host::set_notification_category(app.clone(), category, enabled)
                .await
                .map_err(operation)?,
        ),
        Command::Installation => value(installer::control_installation(app.clone())?),
        Command::Install { host, expected } => {
            value(installer::control_install(app.clone(), host, expected)?)
        }
        Command::Uninstall { host, expected } => {
            value(installer::control_uninstall(app.clone(), host, expected)?)
        }
    }
}
