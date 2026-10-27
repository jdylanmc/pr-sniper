//! Settings › Azure DevOps commands. Install and sign-in start only here,
//! from an explicit Settings action; no background path reaches them.

use super::connection::HttpConnectionTransport;
use super::setup::{Backends, Binding, Platform, Program, Setup, SetupView};
use super::tenant::HttpHeadTransport;
use super::token_source::system_clock;
use crate::cli_process::SystemRunner;
use crate::storage::Store;
use std::sync::Arc;
use tauri::{Manager, State};

/// Non-secret binding in `state/`.
pub const BINDING_FILE: &str = "azure-devops.json";

pub fn load(store: &Store) -> Binding {
    store
        .read_state(BINDING_FILE)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn locate(program: Program) -> Option<std::path::PathBuf> {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    match program {
        Program::Az => super::az_cli::discover(),
        Program::Homebrew => crate::process_path::find(&["brew"], &paths, &[]),
        Program::Winget => crate::process_path::find(&["winget.exe"], &paths, &[]),
    }
}

pub fn create(app: tauri::AppHandle, binding: Binding) -> Result<Arc<Setup>, String> {
    let head = HttpHeadTransport::new().map_err(|_| "HTTP client unavailable.")?;
    let connection = HttpConnectionTransport::new().map_err(|_| "HTTP client unavailable.")?;
    Ok(Setup::new(
        Backends {
            runner: Arc::new(SystemRunner),
            head: Arc::new(head),
            connection: Arc::new(connection),
            clock: system_clock(),
            platform: Platform::current(),
            locate: Arc::new(locate),
            open_page: Arc::new(|url| webbrowser::open(url).is_ok()),
            persist: Arc::new(move |binding| {
                let host = app.state::<crate::Host>();
                let store = host.store.lock().map_err(|_| ())?;
                store.write_state(BINDING_FILE, binding).map_err(|_| ())
            }),
        },
        binding,
    ))
}

type SetupState<'a> = State<'a, Arc<Setup>>;

#[tauri::command]
pub fn azure_devops_setup_state(setup: SetupState<'_>) -> SetupView {
    setup.view()
}

#[tauri::command]
pub async fn azure_devops_check(setup: SetupState<'_>) -> Result<SetupView, String> {
    Ok(setup.check().await)
}

#[tauri::command]
pub async fn azure_devops_set_organization(
    setup: SetupState<'_>,
    organization: String,
) -> Result<SetupView, String> {
    Ok(setup.set_organization(&organization).await?)
}

#[tauri::command]
pub async fn azure_devops_install(setup: SetupState<'_>) -> Result<SetupView, String> {
    Ok(setup.install()?)
}

#[tauri::command]
pub async fn azure_devops_sign_in(setup: SetupState<'_>) -> Result<SetupView, String> {
    Ok(setup.sign_in()?)
}

#[tauri::command]
pub async fn azure_devops_cancel(setup: SetupState<'_>) -> Result<SetupView, String> {
    Ok(setup.cancel().await)
}

#[tauri::command]
pub async fn azure_devops_use_signed_in_account(
    setup: SetupState<'_>,
) -> Result<SetupView, String> {
    Ok(setup.use_signed_in_account().await?)
}
