use super::{revision, transport::private_directory, Error, PROTOCOL};
use crate::storage::private_fs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};
mod integration;
#[cfg(test)]
mod tests;
mod transaction;
pub use integration::{install_integration_at, integration_at, remove_integration_at};

fn checkpoint(point: &str) -> Result<(), Error> {
    #[cfg(test)]
    tests::fault(point)?;
    #[cfg(not(test))]
    let _ = point;
    Ok(())
}

const ASSETS: &[(&str, &str)] = &[
    (
        "SKILL.md",
        include_str!("../../../skills/pr-sniper/SKILL.md"),
    ),
    (
        "references/control.md",
        include_str!("../../../skills/pr-sniper/references/control.md"),
    ),
    (
        "scripts/pr-sniper.mjs",
        include_str!("../../../skills/pr-sniper/scripts/pr-sniper.mjs"),
    ),
    (
        "scripts/control-client.mjs",
        include_str!("../../../skills/pr-sniper/scripts/control-client.mjs"),
    ),
    (
        "scripts/mcp-server.mjs",
        include_str!("../../../skills/pr-sniper/scripts/mcp-server.mjs"),
    ),
    (
        "scripts/mcp-server.mjs.LEGAL.txt",
        include_str!("../../../skills/pr-sniper/scripts/mcp-server.mjs.LEGAL.txt"),
    ),
    (
        "THIRD_PARTY_NOTICES.txt",
        include_str!("../../../skills/pr-sniper/THIRD_PARTY_NOTICES.txt"),
    ),
];

fn legacy_format() -> u32 {
    1
}
fn asset_names(format: u32) -> Option<std::collections::BTreeSet<&'static str>> {
    match format {
        1 => Some(
            [
                "SKILL.md",
                "references/control.md",
                "scripts/pr-sniper.mjs",
                "launcher.json",
            ]
            .into_iter()
            .collect(),
        ),
        2 => Some(
            ASSETS
                .iter()
                .map(|(name, _)| *name)
                .chain(std::iter::once("launcher.json"))
                .collect(),
        ),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default = "legacy_format")]
    format: u32,
    protocol: u32,
    version: String,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct Installation {
    pub state: &'static str,
    pub revision: Option<String>,
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_registered: Option<bool>,
}

fn io_error(_: impl std::fmt::Debug) -> Error {
    Error::new("installation_io", ())
}

fn verify_parents(directory: &Path) -> Result<(), Error> {
    for ancestor in directory.ancestors().take_while(|p| p.parent().is_some()) {
        match private_fs::existing_directory(ancestor) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(Error::new(
                    "installation_conflict",
                    "Linked or non-directory path.",
                ))
            }
        }
    }
    Ok(())
}

pub fn installation_at(directory: &Path) -> Result<Installation, Error> {
    transaction::locked(directory, false, || {
        transaction::recover(directory)?;
        inspect_at(directory)
    })
}

fn inspect_at(directory: &Path) -> Result<Installation, Error> {
    verify_parents(directory)?;
    if !directory.exists() {
        return Ok(Installation {
            state: "absent",
            revision: None,
            version: None,
            mcp_registered: None,
        });
    }
    let manifest_path = directory.join(".pr-sniper-install.json");
    private_fs::existing_file(&manifest_path)
        .map_err(|_| Error::new("installation_conflict", "Unowned skill directory."))?;
    if fs::metadata(&manifest_path).map_err(io_error)?.len() > super::MAX_FRAME as u64 {
        return Err(Error::new(
            "installation_conflict",
            "Invalid installation receipt.",
        ));
    }
    let bytes = fs::read(&manifest_path).map_err(io_error)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("installation_conflict", "Invalid installation receipt."))?;
    let names = asset_names(manifest.format)
        .ok_or_else(|| Error::new("installation_conflict", "Unsupported package format."))?;
    if manifest
        .files
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>()
        != names
    {
        return Err(Error::new(
            "installation_conflict",
            "Unexpected installation members.",
        ));
    }
    let mut found = file_hashes(directory)?;
    found.remove(".pr-sniper-install.json");
    if found != manifest.files || manifest.protocol != PROTOCOL {
        return Err(Error::new(
            "installation_conflict",
            "Customized or incompatible skill; preserve it before replacement.",
        ));
    }
    Ok(Installation {
        state: "installed",
        revision: Some(revision(&bytes)),
        version: Some(manifest.version),
        mcp_registered: None,
    })
}

fn file_hashes(directory: &Path) -> Result<BTreeMap<String, String>, Error> {
    private_fs::existing_directory(directory).map_err(io_error)?;
    let mut found = BTreeMap::new();
    fn walk(
        root: &Path,
        directory: &Path,
        found: &mut BTreeMap<String, String>,
    ) -> Result<(), Error> {
        private_fs::recover(directory, None).map_err(io_error)?;
        for entry in fs::read_dir(directory).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            let relative = path.strip_prefix(root).map_err(io_error)?.to_string_lossy();
            #[cfg(windows)]
            let name = relative.replace('\\', "/");
            #[cfg(not(windows))]
            let name = relative.into_owned();
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if metadata.is_dir() {
                private_fs::existing_directory(&path).map_err(io_error)?;
                if !["references", "scripts"].contains(&name.as_str()) {
                    return Err(Error::new(
                        "installation_conflict",
                        "Additional skill directory.",
                    ));
                }
                walk(root, &path, found)?;
            } else {
                private_fs::existing_file(&path).map_err(io_error)?;
                if metadata.len() > super::MAX_FRAME as u64 {
                    return Err(Error::new("installation_conflict", ()));
                }
                found.insert(name, revision(&fs::read(&path).map_err(io_error)?));
            }
        }
        Ok(())
    }
    walk(directory, directory, &mut found)?;
    Ok(found)
}

pub fn install_at(
    directory: &Path,
    executable: &Path,
    expected: Option<&str>,
) -> Result<Installation, Error> {
    transaction::locked(directory, true, || {
        transaction::recover(directory)?;
        let current = inspect_at(directory)?;
        if current.revision.as_deref() != expected {
            return Err(Error::new("conflict", ()));
        }
        if !executable.is_absolute() {
            return Err(Error::new("invalid_executable", ()));
        }
        if current.state == "installed" {
            private_directory(directory).map_err(io_error)?;
        }
        let launcher = serde_json::to_string(&serde_json::json!({
            "protocol": PROTOCOL, "executable": executable
        }))
        .map_err(io_error)?;
        let contents: Vec<_> = ASSETS
            .iter()
            .copied()
            .chain(std::iter::once(("launcher.json", launcher.as_str())))
            .map(|(name, content)| (name.to_owned(), content.to_owned()))
            .collect();
        transaction::install(directory, &contents, expected)?;
        inspect_at(directory)
    })
}

pub fn remove_at(directory: &Path, expected: &str) -> Result<Installation, Error> {
    transaction::locked(directory, false, || {
        transaction::recover(directory)?;
        let current = inspect_at(directory)?;
        if current.revision.as_deref() != Some(expected) {
            return Err(Error::new("conflict", ()));
        }
        transaction::remove(directory, expected)?;
        inspect_at(directory)
    })
}

#[tauri::command]
pub(crate) fn control_installation(app: tauri::AppHandle) -> Result<Installation, Error> {
    use tauri::Manager;
    if app.state::<crate::Host>().isolated {
        return Err(Error::new("isolated_installation", ()));
    }
    integration_at(&app.path().home_dir().map_err(io_error)?)
}

#[tauri::command]
pub(crate) fn control_install(
    app: tauri::AppHandle,
    host: String,
    expected: Option<String>,
) -> Result<Installation, Error> {
    use tauri::Manager;
    if app.state::<crate::Host>().isolated {
        return Err(Error::new("isolated_installation", ()));
    }
    if host != "copilot_cli" {
        return Err(Error::new("unsupported_host", ()));
    }
    install_integration_at(
        &app.path().home_dir().map_err(io_error)?,
        &std::env::current_exe().map_err(io_error)?,
        expected.as_deref(),
    )
}

#[tauri::command]
pub(crate) fn control_uninstall(
    app: tauri::AppHandle,
    host: String,
    expected: String,
) -> Result<Installation, Error> {
    use tauri::Manager;
    if app.state::<crate::Host>().isolated {
        return Err(Error::new("isolated_installation", ()));
    }
    if host != "copilot_cli" {
        return Err(Error::new("unsupported_host", ()));
    }
    remove_integration_at(&app.path().home_dir().map_err(io_error)?, &expected)
}

fn planned_revision(executable: &Path) -> Result<String, Error> {
    let launcher =
        serde_json::to_string(&serde_json::json!({"protocol":PROTOCOL,"executable":executable}))
            .map_err(io_error)?;
    let files = ASSETS
        .iter()
        .copied()
        .chain(std::iter::once(("launcher.json", launcher.as_str())))
        .map(|(name, body)| (name.to_string(), revision(&body.as_bytes())))
        .collect();
    let manifest = Manifest {
        format: 2,
        protocol: PROTOCOL,
        version: env!("CARGO_PKG_VERSION").into(),
        files,
    };
    Ok(revision(&serde_json::to_vec(&manifest).map_err(io_error)?))
}
