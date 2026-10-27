//! Settings › Azure DevOps commands. Install and sign-in start only here,
//! from an explicit Settings action; no background path reaches them.

use super::connection::HttpConnectionTransport;
use super::read::{Client, HttpTransport, Reader, Transport};
use super::setup::{AccountView, Backends, Binding, Platform, Program, Setup, SetupView};
use super::tenant::HttpHeadTransport;
use super::token_source::system_clock;
use crate::cli_process::SystemRunner;
use crate::provider::{ConnectionError, Person};
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

/// Azure DevOps REST transport for repository reads.
pub struct ReadTransport(Arc<dyn Transport>);

pub fn read_transport() -> Result<ReadTransport, String> {
    Ok(ReadTransport(Arc::new(
        HttpTransport::new().map_err(|_| "HTTP client unavailable.")?,
    )))
}

/// The bound Azure DevOps identity ID, if any.
pub fn bound_identity(app: &tauri::AppHandle) -> Option<String> {
    app.try_state::<Arc<Setup>>()?
        .binding()
        .identity
        .map(|identity| identity.id)
}

/// Repository reads as `account_id`, the bound and signed-in identity. Runs
/// off the async runtime (a blocking thread); never interactive.
pub fn read_session(
    app: &tauri::AppHandle,
    account_id: &str,
) -> Result<(Person, Reader), ConnectionError> {
    let setup = app
        .try_state::<Arc<Setup>>()
        .ok_or(ConnectionError::Configuration)?
        .inner()
        .clone();
    let transport = Arc::clone(
        &app.try_state::<ReadTransport>()
            .ok_or(ConnectionError::Configuration)?
            .0,
    );
    let runtime = tauri::async_runtime::handle().inner().clone();
    let connected = runtime.block_on(setup.connected())?;
    if connected.identity.id != account_id {
        return Err(ConnectionError::WrongIdentity);
    }
    let client = Client::new(
        setup.token_source(),
        transport,
        connected.tenant,
        &connected.organization,
    );
    let reader = Reader::open(client, runtime, &connected.identity)?;
    Ok((reader.identity().clone(), reader))
}

/// Mirrors the setup into the repository account that monitoring polls as:
/// its bound identity, and whether it is signed in. A connected identity
/// resumes work a credential failure parked.
pub fn sync_account(app: &tauri::AppHandle, setup: &Setup, view: &SetupView) {
    let identity = setup.binding().identity.map(|identity| Person {
        id: identity.id,
        login: identity.display_name,
    });
    let connected = match &view.account {
        AccountView::Connected { identity: signed } => {
            Some(identity.as_ref().is_some_and(|bound| bound.id == signed.id))
        }
        AccountView::SignedOut
        | AccountView::InteractionRequired
        | AccountView::WrongIdentity { .. } => Some(false),
        _ => None,
    };
    if let Some(host) = app.try_state::<crate::Host>() {
        if let Ok(mut auth) = host.github_auth.lock() {
            auth.set_azure_devops_account(identity, connected);
        }
    }
}

type SetupState<'a> = State<'a, Arc<Setup>>;

fn synced(app: &tauri::AppHandle, setup: &Setup, view: SetupView) -> SetupView {
    sync_account(app, setup, &view);
    view
}

#[tauri::command]
pub fn azure_devops_setup_state(app: tauri::AppHandle, setup: SetupState<'_>) -> SetupView {
    synced(&app, &setup, setup.view())
}

#[tauri::command]
pub async fn azure_devops_check(
    app: tauri::AppHandle,
    setup: SetupState<'_>,
) -> Result<SetupView, String> {
    Ok(synced(&app, &setup, setup.check().await))
}

#[tauri::command]
pub async fn azure_devops_set_organization(
    app: tauri::AppHandle,
    setup: SetupState<'_>,
    organization: String,
) -> Result<SetupView, String> {
    let view = setup.set_organization(&organization).await?;
    Ok(synced(&app, &setup, view))
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
    app: tauri::AppHandle,
    setup: SetupState<'_>,
) -> Result<SetupView, String> {
    let view = setup.use_signed_in_account().await?;
    Ok(synced(&app, &setup, view))
}
