use super::{revision, transport::private_directory, Error, PROTOCOL};
use crate::storage::private_fs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

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
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    protocol: u32,
    version: String,
    files: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct Installation {
    pub state: &'static str,
    pub revision: Option<String>,
    pub version: Option<String>,
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
    verify_parents(directory)?;
    if !directory.exists() {
        return Ok(Installation {
            state: "absent",
            revision: None,
            version: None,
        });
    }
    let manifest_path = directory.join(".pr-sniper-install.json");
    private_fs::existing_file(&manifest_path)
        .map_err(|_| Error::new("installation_conflict", "Unowned skill directory."))?;
    let bytes = fs::read(&manifest_path).map_err(io_error)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("installation_conflict", "Invalid installation receipt."))?;
    let names: std::collections::BTreeSet<_> = ASSETS
        .iter()
        .map(|(name, _)| *name)
        .chain(std::iter::once("launcher.json"))
        .collect();
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
    let mut found = BTreeMap::new();
    fn walk(
        root: &Path,
        directory: &Path,
        found: &mut BTreeMap<String, String>,
    ) -> Result<(), Error> {
        private_fs::recover(directory, None).map_err(io_error)?;
        for entry in fs::read_dir(directory).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            let name = path
                .strip_prefix(root)
                .map_err(io_error)?
                .to_string_lossy()
                .replace('\\', "/");
            if name == ".pr-sniper-install.json" {
                continue;
            }
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
    })
}

pub fn install_at(
    directory: &Path,
    executable: &Path,
    expected: Option<&str>,
) -> Result<Installation, Error> {
    let current = installation_at(directory)?;
    if current.revision.as_deref() != expected {
        return Err(Error::new("conflict", ()));
    }
    if !executable.is_absolute() {
        return Err(Error::new("invalid_executable", ()));
    }
    private_directory(directory).map_err(io_error)?;
    let launcher = serde_json::to_string(&serde_json::json!({
        "protocol": PROTOCOL, "executable": executable
    }))
    .map_err(io_error)?;
    let mut files = BTreeMap::new();
    for (name, content) in ASSETS
        .iter()
        .copied()
        .chain(std::iter::once(("launcher.json", launcher.as_str())))
    {
        let file = directory.join(name);
        private_fs::directory(file.parent().unwrap()).map_err(io_error)?;
        private_fs::replace(&file, content.as_bytes(), "skill asset").map_err(io_error)?;
        files.insert(name.to_string(), revision(&content.as_bytes()));
    }
    let manifest = Manifest {
        protocol: PROTOCOL,
        version: env!("CARGO_PKG_VERSION").into(),
        files,
    };
    private_fs::replace(
        &directory.join(".pr-sniper-install.json"),
        &serde_json::to_vec(&manifest).map_err(io_error)?,
        "skill receipt",
    )
    .map_err(io_error)?;
    installation_at(directory)
}

pub fn remove_at(directory: &Path, expected: &str) -> Result<Installation, Error> {
    let current = installation_at(directory)?;
    if current.revision.as_deref() != Some(expected) {
        return Err(Error::new("conflict", ()));
    }
    // Only the exact verified, unmodified package is removed.
    fs::remove_dir_all(directory).map_err(io_error)?;
    installation_at(directory)
}

#[tauri::command]
pub(crate) fn control_installation(app: tauri::AppHandle) -> Result<Installation, Error> {
    use tauri::Manager;
    installation_at(
        &app.path()
            .home_dir()
            .map_err(io_error)?
            .join(".copilot/skills/pr-sniper"),
    )
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
    install_at(
        &app.path()
            .home_dir()
            .map_err(io_error)?
            .join(".copilot/skills/pr-sniper"),
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
    remove_at(
        &app.path()
            .home_dir()
            .map_err(io_error)?
            .join(".copilot/skills/pr-sniper"),
        &expected,
    )
}
