//! Versioned local control plane. The native host remains the only writer.
mod dispatcher;
pub(crate) mod installer;
mod operations;
mod preset;
#[cfg(test)]
mod tests;
mod transport;
pub(crate) mod work;

pub use installer::{
    install_at, install_integration_at, installation_at, integration_at, remove_at,
    remove_integration_at, Installation,
};
pub use transport::{serve_connection, Endpoint, MAX_FRAME, PROTOCOL};
pub use work::{prioritize, request_again, RequestOverride, WatchResult};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub code: String,
    pub detail: Value,
}

impl Error {
    pub(crate) fn new(code: &str, detail: impl Serialize) -> Self {
        Self {
            code: code.into(),
            detail: serde_json::to_value(detail).unwrap_or(Value::Null),
        }
    }
}

pub(crate) fn revision(value: &impl Serialize) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

pub(crate) fn check_revision(expected: &str, value: &impl Serialize) -> Result<(), Error> {
    if expected != revision(value) {
        return Err(Error::new(
            "conflict",
            "Read the current configuration before saving.",
        ));
    }
    Ok(())
}

pub(crate) fn start(app: &tauri::AppHandle, root: &Path) -> Result<(), String> {
    let server = transport::Server::bind(&root.join("control"))?;
    let app = app.clone();
    std::thread::Builder::new()
        .name("local-control".into())
        .spawn(move || {
            server.run(
                || {
                    app.state::<crate::Host>()
                        .quitting
                        .load(std::sync::atomic::Ordering::SeqCst)
                },
                |request| tauri::async_runtime::block_on(operations::execute(&app, request)),
            );
        })
        .map_err(|_| "Local control could not start.".to_string())?;
    Ok(())
}
