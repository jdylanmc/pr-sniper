//! The app owns exactly one Copilot CLI MCP server entry. The `/pr-sniper`
//! skill is installed separately with the skills CLI and is only detected here.
use super::{revision, transport::private_directory, Error};
use crate::storage::private_fs;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

pub const SKILL_COMMAND: &str =
    "npx skills add jdylanmc/pr-sniper --skill pr-sniper -g -a github-copilot";
const NAME: &str = "pr-sniper";
const OWNER: &str = "copilot-mcp-owner.json";
/// Pre-release receipt from the retired app-installed skill package.
const LEGACY_OWNER: &str = "copilot-entry.json";
const INTENT: &str = "copilot-mcp-intent.json";
const LOCK: &str = "copilot-mcp.lock";
const MAX_CONFIG: u64 = 8 * 1024 * 1024;
const MAX_SKILL: u64 = 256 * 1024;

fn checkpoint(point: &str) -> Result<(), Error> {
    #[cfg(test)]
    tests::fault(point)?;
    #[cfg(not(test))]
    let _ = point;
    Ok(())
}

/// The app-bundled stdio server and the executable it launches.
#[derive(Debug, Clone)]
pub struct Server {
    pub script: PathBuf,
    pub executable: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Installation {
    pub state: &'static str,
    pub revision: Option<String>,
    pub mcp_registered: bool,
    pub skill_detected: bool,
    pub skill_command: &'static str,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    entry: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyOwner {
    target: PathBuf,
    entry: Value,
}

/// Receipt the retired installer wrote inside its skill package.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyManifest {
    #[serde(rename = "format", default)]
    _format: Option<u32>,
    #[serde(rename = "protocol")]
    _protocol: u32,
    #[serde(rename = "version")]
    _version: String,
    files: BTreeMap<String, String>,
}

const LEGACY_MANIFEST: &str = ".pr-sniper-install.json";
const LEGACY_DIRECTORIES: [&str; 2] = ["references", "scripts"];

/// Hashes of a real, link-free legacy package, or `None` when it has any
/// member the retired installer could not have written.
fn legacy_hashes(package: &Path) -> Option<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    for directory in std::iter::once(None).chain(LEGACY_DIRECTORIES.map(Some)) {
        let path = directory.map_or_else(|| package.to_path_buf(), |name| package.join(name));
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Err(error) if error.kind() == ErrorKind::NotFound && directory.is_some() => continue,
            _ => return None,
        }
        for entry in fs::read_dir(&path).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let metadata = entry.metadata().ok()?;
            if metadata.is_dir()
                && directory.is_none()
                && LEGACY_DIRECTORIES.contains(&name.as_str())
            {
                continue;
            }
            if !metadata.is_file() || metadata.len() > MAX_SKILL {
                return None;
            }
            let relative = directory.map_or(name.clone(), |parent| format!("{parent}/{name}"));
            found.insert(relative, revision(&fs::read(entry.path()).ok()?));
        }
    }
    Some(found)
}

/// Removes the retired app-installed skill package only while it is exactly
/// what that installer recorded. Customized, extended or linked packages stay.
fn retire_legacy_package(home: &Path, target: &Path) {
    let package = home.join(".copilot").join("skills").join(NAME);
    if target != package
        || [home.join(".copilot"), home.join(".copilot").join("skills")]
            .iter()
            .any(|parent| !fs::symlink_metadata(parent).is_ok_and(|m| m.is_dir()))
    {
        return;
    }
    let manifest_path = package.join(LEGACY_MANIFEST);
    let Some(manifest) = fs::read(&manifest_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LegacyManifest>(&bytes).ok())
    else {
        return;
    };
    let Some(mut found) = legacy_hashes(&package) else {
        return;
    };
    if found.remove(LEGACY_MANIFEST).is_none() || found != manifest.files {
        return;
    }
    for name in manifest.files.keys() {
        if fs::remove_file(package.join(name)).is_err() {
            return;
        }
    }
    let _ = fs::remove_file(&manifest_path);
    for directory in LEGACY_DIRECTORIES {
        let _ = fs::remove_dir(package.join(directory));
    }
    let _ = fs::remove_dir(&package);
}

/// A durable, already-authorized registration change. Recovery completes it
/// only while the real configuration file and the entry still match.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    config: PathBuf,
    before: Option<Value>,
    after: Option<Value>,
}

struct Guard(File);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn io_error(_: impl std::fmt::Debug) -> Error {
    Error::new("installation_io", ())
}

fn conflict() -> Error {
    Error::new(
        "installation_conflict",
        "Copilot entry, configuration target or ownership changed; preserve custom configuration before repair.",
    )
}

#[cfg(unix)]
fn unsafe_path() -> Error {
    Error::new(
        "installation_conflict",
        "Copilot configuration path must resolve to directories owned by this user.",
    )
}

#[derive(Debug, PartialEq, Eq)]
struct Resolved {
    copilot: PathBuf,
    config: PathBuf,
}

/// Resolve the existing prefix (following links) and append the missing rest.
#[cfg(unix)]
fn real(path: &Path) -> Result<PathBuf, Error> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        match fs::symlink_metadata(current) {
            Ok(_) => {
                // A dangling link fails here; it is never written through.
                let mut base = fs::canonicalize(current).map_err(|_| unsafe_path())?;
                for part in missing.iter().rev() {
                    base.push(part);
                }
                return Ok(base);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing.push(current.file_name().ok_or_else(unsafe_path)?.to_owned());
                current = current.parent().ok_or_else(unsafe_path)?;
            }
            Err(_) => return Err(unsafe_path()),
        }
    }
}

/// Every existing directory above a resolved target belongs to this user or
/// root, and others cannot rename entries in it (sticky directories excepted).
#[cfg(unix)]
fn trusted(path: &Path, uid: u32) -> Result<(), Error> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    for ancestor in path.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => return Err(unsafe_path()),
        };
        let mode = metadata.permissions().mode();
        if !metadata.is_dir()
            || (metadata.uid() != uid && metadata.uid() != 0)
            || (mode & 0o022 != 0 && mode & 0o1000 == 0)
        {
            return Err(unsafe_path());
        }
    }
    Ok(())
}

fn resolve(home: &Path) -> Result<Resolved, Error> {
    if !home.is_absolute() {
        return Err(Error::new("invalid_home", ()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // std has no getuid; the nearest existing home ancestor identifies the
        // user, as the control transport does.
        let uid = home
            .ancestors()
            .find_map(|path| fs::metadata(path).ok())
            .ok_or_else(unsafe_path)?
            .uid();
        let copilot = real(&home.join(".copilot"))?;
        trusted(&copilot, uid)?;
        // A linked mcp-config.json is preserved; its real target is replaced.
        let config = real(&copilot.join("mcp-config.json"))?;
        trusted(config.parent().ok_or_else(unsafe_path)?, uid)?;
        Ok(Resolved { copilot, config })
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let copilot = home.join(".copilot");
        let config = copilot.join("mcp-config.json");
        for ancestor in config.ancestors() {
            if fs::symlink_metadata(ancestor).is_ok_and(|metadata| {
                metadata.file_attributes()
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                    != 0
            }) {
                return Err(Error::new(
                    "installation_linked_unsupported",
                    "Use a real %USERPROFILE%\\.copilot folder and mcp-config.json file; links are unsupported on Windows.",
                ));
            }
        }
        Ok(Resolved { copilot, config })
    }
}

/// Read-only: a `pr-sniper` skill installed by the skills CLI (or by hand).
fn skill_detected(home: &Path) -> bool {
    [".copilot", ".agents"].iter().any(|base| {
        let file = home.join(base).join("skills").join(NAME).join("SKILL.md");
        let Ok(metadata) = fs::metadata(&file) else {
            return false;
        };
        if !metadata.is_file() || metadata.len() > MAX_SKILL {
            return false;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            return false;
        };
        let mut lines = text.lines();
        lines.next().map(str::trim_end) == Some("---")
            && lines
                .take_while(|line| line.trim_end() != "---")
                .any(|line| line.trim() == "name: pr-sniper")
    })
}

fn desired(server: &Server) -> Value {
    json!({
        "type":"local","command":"node",
        "args":[server.script, "--app", server.executable],
        "tools":["*"],"timeout":180000
    })
}

struct Scope {
    home: PathBuf,
    paths: Resolved,
    work: PathBuf,
}

impl Scope {
    fn new(home: &Path) -> Result<Self, Error> {
        let paths = resolve(home)?;
        let work = paths.copilot.join(".pr-sniper-installer");
        private_directory(&work).map_err(io_error)?;
        for name in [OWNER, INTENT] {
            private_fs::recover(&work, Some(name)).map_err(io_error)?;
        }
        let config = &paths.config;
        if let Some(parent) = config.parent().filter(|parent| parent.exists()) {
            private_fs::recover(parent, config.file_name().and_then(|name| name.to_str()))
                .map_err(io_error)?;
        }
        Ok(Self {
            home: home.into(),
            paths,
            work,
        })
    }
    /// A link retargeted since this scope resolved is a conflict.
    fn check(&self) -> Result<(), Error> {
        if resolve(&self.home)? != self.paths {
            return Err(conflict());
        }
        Ok(())
    }
    fn lock(&self) -> Result<Guard, Error> {
        let path = self.work.join(LOCK);
        let file = match private_fs::create_file(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                private_fs::existing_file(&path).map_err(|_| conflict())?;
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                    .map_err(io_error)?
            }
            Err(error) => return Err(io_error(error)),
        };
        file.try_lock()
            .map_err(|_| Error::new("installation_busy", ()))?;
        Ok(Guard(file))
    }
    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<Option<T>, Error> {
        let path = self.work.join(name);
        match private_fs::existing_file(&path) {
            Ok(()) => {
                if fs::metadata(&path).map_err(io_error)?.len() > super::MAX_FRAME as u64 {
                    return Err(conflict());
                }
                serde_json::from_slice(&fs::read(path).map_err(io_error)?)
                    .map(Some)
                    .map_err(|_| conflict())
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) => Err(conflict()),
        }
    }
    fn write(&self, name: &str, value: &impl Serialize) -> Result<(), Error> {
        private_fs::replace(
            &self.work.join(name),
            &serde_json::to_vec(value).map_err(io_error)?,
            "Copilot integration receipt",
        )
        .map_err(io_error)?;
        private_fs::sync_directory(&self.work).map_err(io_error)
    }
    fn clear(&self, name: &str) -> Result<(), Error> {
        let path = self.work.join(name);
        match private_fs::existing_file(&path) {
            Ok(()) => fs::remove_file(path).map_err(io_error)?,
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(conflict()),
        }
        private_fs::sync_directory(&self.work).map_err(io_error)
    }
    fn owner(&self) -> Result<Option<Value>, Error> {
        if let Some(owner) = self.read::<Owner>(OWNER)? {
            return Ok(Some(owner.entry));
        }
        Ok(self
            .read::<LegacyOwner>(LEGACY_OWNER)?
            .map(|owner| owner.entry))
    }
    fn config(&self) -> Result<(Option<Vec<u8>>, Value), Error> {
        let path = &self.paths.config;
        match private_fs::existing_file(path) {
            Ok(()) => {
                if fs::metadata(path).map_err(io_error)?.len() > MAX_CONFIG {
                    return Err(Error::new("mcp_config_too_large", ()));
                }
                let bytes = fs::read(path).map_err(io_error)?;
                let value: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::new("mcp_config_invalid", ()))?;
                if !value.is_object() || value.get("mcpServers").is_some_and(|s| !s.is_object()) {
                    return Err(Error::new("mcp_config_invalid", ()));
                }
                Ok((Some(bytes), value))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok((None, json!({"mcpServers":{}})))
            }
            Err(_) => Err(conflict()),
        }
    }
    fn entry(&self) -> Result<Option<Value>, Error> {
        Ok(self
            .config()?
            .1
            .get("mcpServers")
            .and_then(|s| s.get(NAME))
            .cloned())
    }
    fn set_entry(&self, before: Option<&Value>, after: Option<&Value>) -> Result<(), Error> {
        let (bytes, mut config) = self.config()?;
        let current = config.get("mcpServers").and_then(|s| s.get(NAME));
        if current == after {
            return Ok(());
        }
        if current != before {
            return Err(conflict());
        }
        let servers = config
            .as_object_mut()
            .unwrap()
            .entry("mcpServers")
            .or_insert(json!({}))
            .as_object_mut()
            .unwrap();
        if let Some(after) = after {
            servers.insert(NAME.into(), after.clone());
        } else {
            servers.remove(NAME);
        }
        checkpoint("mcp-config-merged")?;
        // Other servers are merged from the latest read, never from an old UI
        // draft or ownership receipt. Detect an external edit or retargeted
        // link immediately before replacing the real file.
        self.check()?;
        if self.config()?.0 != bytes {
            return Err(conflict());
        }
        let path = &self.paths.config;
        let parent = path.parent().ok_or_else(conflict)?;
        private_fs::replace(
            path,
            &serde_json::to_vec_pretty(&config).map_err(io_error)?,
            "Copilot MCP configuration",
        )
        .map_err(io_error)?;
        private_fs::sync_directory(parent).map_err(io_error)?;
        if self.entry()?.as_ref() != after {
            return Err(conflict());
        }
        checkpoint("mcp-config-written")
    }
    fn recover(&self) -> Result<(), Error> {
        let Some(intent) = self.read::<Intent>(INTENT)? else {
            return Ok(());
        };
        if intent.config != self.paths.config {
            return Err(conflict());
        }
        self.set_entry(intent.before.as_ref(), intent.after.as_ref())?;
        match intent.after {
            Some(entry) => self.write(OWNER, &Owner { entry })?,
            None => self.clear(OWNER)?,
        }
        if let Some(legacy) = self.read::<LegacyOwner>(LEGACY_OWNER)? {
            retire_legacy_package(&self.home, &legacy.target);
            self.clear(LEGACY_OWNER)?;
        }
        checkpoint("mcp-owner-written")?;
        self.clear(INTENT)
    }
    fn status(&self, server: &Server) -> Result<Installation, Error> {
        let entry = self.entry()?;
        let owner = self.owner()?;
        let state = match (&owner, &entry) {
            (None, None) => "absent",
            (None, Some(_)) => return Err(conflict()),
            (Some(owned), Some(entry)) if owned != entry => return Err(conflict()),
            (Some(_), Some(entry)) if *entry == desired(server) => "installed",
            // Owned but stale (moved app) or removed from the host file.
            _ => "incomplete",
        };
        Ok(Installation {
            state,
            revision: (state != "absent").then(|| revision(&json!({ "entry": entry }))),
            mcp_registered: entry.is_some(),
            skill_detected: skill_detected(&self.home),
            skill_command: SKILL_COMMAND,
        })
    }
    fn apply(&self, after: Option<Value>) -> Result<(), Error> {
        self.write(
            INTENT,
            &Intent {
                config: self.paths.config.clone(),
                before: self.entry()?,
                after,
            },
        )?;
        checkpoint("mcp-intent-written")?;
        self.recover()
    }
}

fn validate(server: &Server) -> Result<(), Error> {
    if !server.executable.is_absolute()
        || !server.script.is_absolute()
        || !fs::metadata(&server.script).is_ok_and(|metadata| metadata.is_file())
    {
        return Err(Error::new("invalid_executable", ()));
    }
    Ok(())
}

pub fn integration_at(home: &Path, server: &Server) -> Result<Installation, Error> {
    let scope = Scope::new(home)?;
    let _guard = scope.lock()?;
    scope.recover()?;
    scope.status(server)
}

pub fn install_integration_at(
    home: &Path,
    server: &Server,
    expected: Option<&str>,
) -> Result<Installation, Error> {
    validate(server)?;
    let scope = Scope::new(home)?;
    let _guard = scope.lock()?;
    scope.recover()?;
    if scope.status(server)?.revision.as_deref() != expected {
        return Err(Error::new("conflict", ()));
    }
    scope.apply(Some(desired(server)))?;
    scope.status(server)
}

pub fn remove_integration_at(
    home: &Path,
    server: &Server,
    expected: &str,
) -> Result<Installation, Error> {
    let scope = Scope::new(home)?;
    let _guard = scope.lock()?;
    scope.recover()?;
    if scope.status(server)?.revision.as_deref() != Some(expected) {
        return Err(Error::new("conflict", ()));
    }
    scope.apply(None)?;
    scope.status(server)
}

/// Launch repair: re-point only an owned, unmodified, still-present entry at
/// this app. Never creates, adopts or restores a removed registration.
pub fn repair_integration_at(home: &Path, server: &Server) -> Result<bool, Error> {
    validate(server)?;
    let scope = Scope::new(home)?;
    let _guard = scope.lock()?;
    scope.recover()?;
    let status = scope.status(server)?;
    if status.state != "incomplete" || !status.mcp_registered {
        return Ok(false);
    }
    scope.apply(Some(desired(server)))?;
    Ok(true)
}

#[cfg(windows)]
const BUNDLE: &[(&str, &[u8])] = &[
    (
        "mcp-server.mjs",
        include_bytes!("../../resources/mcp/mcp-server.mjs"),
    ),
    (
        "mcp-server.mjs.LEGAL.txt",
        include_bytes!("../../resources/mcp/mcp-server.mjs.LEGAL.txt"),
    ),
    (
        "THIRD_PARTY_NOTICES.txt",
        include_bytes!("../../resources/mcp/THIRD_PARTY_NOTICES.txt"),
    ),
];

/// The Windows installer owns only its executable and uninstaller, so the
/// embedded server is published into the app's private data directory.
#[cfg(windows)]
pub fn materialize(root: &Path) -> Result<PathBuf, Error> {
    let directory = root.join("mcp");
    private_fs::directory(&directory).map_err(io_error)?;
    for (name, bytes) in BUNDLE {
        let path = directory.join(name);
        if fs::read(&path).ok().as_deref() != Some(*bytes) {
            private_fs::replace(&path, bytes, "bundled MCP server").map_err(io_error)?;
        }
    }
    Ok(directory.join("mcp-server.mjs"))
}

fn server(app: &tauri::AppHandle) -> Result<Server, Error> {
    use tauri::Manager;
    let executable = std::env::current_exe().map_err(io_error)?;
    #[cfg(target_os = "macos")]
    let (executable, script) = (
        executable.canonicalize().map_err(io_error)?,
        app.path()
            .resource_dir()
            .map_err(io_error)?
            .join("mcp")
            .join("mcp-server.mjs")
            .canonicalize()
            .map_err(|_| Error::new("mcp_server_unavailable", ()))?,
    );
    #[cfg(windows)]
    let script = materialize(&app.path().app_local_data_dir().map_err(io_error)?)?;
    Ok(Server { script, executable })
}

fn home(app: &tauri::AppHandle) -> Result<PathBuf, Error> {
    use tauri::Manager;
    if app.state::<crate::Host>().isolated {
        return Err(Error::new("isolated_installation", ()));
    }
    app.path().home_dir().map_err(io_error)
}

/// Startup: follow a moved or updated installed app; dev builds never rewrite.
pub(crate) fn repair_on_launch(app: &tauri::AppHandle) {
    let result = home(app).and_then(|home| {
        let server = server(app)?;
        if !crate::startup::installed_executable(&server.executable, &home) {
            return Ok(false);
        }
        repair_integration_at(&home, &server)
    });
    if result.is_err() {
        eprintln!("PR Sniper: Copilot MCP registration repair skipped");
    }
}

#[tauri::command]
pub(crate) fn control_installation(app: tauri::AppHandle) -> Result<Installation, Error> {
    integration_at(&home(&app)?, &server(&app)?)
}

#[tauri::command]
pub(crate) fn control_install(
    app: tauri::AppHandle,
    host: String,
    expected: Option<String>,
) -> Result<Installation, Error> {
    let home = home(&app)?;
    if host != "copilot_cli" {
        return Err(Error::new("unsupported_host", ()));
    }
    install_integration_at(&home, &server(&app)?, expected.as_deref())
}

#[tauri::command]
pub(crate) fn control_uninstall(
    app: tauri::AppHandle,
    host: String,
    expected: String,
) -> Result<Installation, Error> {
    let home = home(&app)?;
    if host != "copilot_cli" {
        return Err(Error::new("unsupported_host", ()));
    }
    remove_integration_at(&home, &server(&app)?, &expected)
}
