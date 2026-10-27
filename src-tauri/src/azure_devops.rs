//! Azure DevOps authentication delegated to the Azure CLI (ADR 0026).

pub mod az_cli;
pub mod connection;
pub mod host;
pub mod read;
pub mod setup;
pub mod tenant;
pub mod token_source;

/// Azure DevOps first-party resource (application) ID.
pub const RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
