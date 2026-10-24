//! The app owns exactly one Copilot CLI MCP server entry. The `/pr-sniper`
//! skill is installed separately with the skills CLI and is only detected here.
use super::{revision, transport::private_directory, Error};
use crate::storage::{file_identity, private_fs};
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
const WORK: &str = ".pr-sniper-installer";
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
    /// Only an installed app may write its paths into the registration.
    pub installed: bool,
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

/// A regular file or directory, never a symlink or Windows reparse point.
fn plain(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return false;
        }
    }
    !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir())
}

/// One scanned legacy member: its stable identity and content hash.
struct Member {
    identity: file_identity::Identity,
    hash: String,
}

/// A plain file's identity and hash, or `None` if it is linked, oversized or
/// changed while it was read.
fn member(path: &Path) -> Option<Member> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !plain(&metadata) || !metadata.is_file() || metadata.len() > MAX_SKILL {
        return None;
    }
    let identity = file_identity::identity(path).ok()??;
    let bytes = fs::read(path).ok()?;
    let after = fs::symlink_metadata(path).ok()?;
    (plain(&after) && file_identity::identity(path).ok()?.as_ref() == Some(&identity)).then(|| {
        Member {
            identity,
            hash: revision(&bytes),
        }
    })
}

/// Members of a real, link-free legacy package, or `None` when it has any
/// member the retired installer could not have written.
fn legacy_members(package: &Path) -> Option<BTreeMap<String, Member>> {
    let mut found = BTreeMap::new();
    for directory in std::iter::once(None).chain(LEGACY_DIRECTORIES.map(Some)) {
        let path = directory.map_or_else(|| package.to_path_buf(), |name| package.join(name));
        match fs::symlink_metadata(&path) {
            Ok(metadata) if plain(&metadata) && metadata.is_dir() => {}
            Err(error) if error.kind() == ErrorKind::NotFound && directory.is_some() => continue,
            _ => return None,
        }
        for entry in fs::read_dir(&path).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let metadata = fs::symlink_metadata(entry.path()).ok()?;
            if !plain(&metadata) {
                return None;
            }
            if metadata.is_dir()
                && directory.is_none()
                && LEGACY_DIRECTORIES.contains(&name.as_str())
            {
                continue;
            }
            let relative = directory.map_or(name.clone(), |parent| format!("{parent}/{name}"));
            found.insert(relative, member(&entry.path())?);
        }
    }
    Some(found)
}

const QUARANTINE: &str = "legacy-skill-";

/// A quarantined legacy package that did not verify, kept intact for the user.
fn quarantined(path: &Path) -> Error {
    Error::new("legacy_skill_quarantined", json!({ "path": path }))
}

/// Retires the old app-installed skill package only while it is exactly what
/// that installer recorded. Customized, extended or linked packages stay in
/// place. A verified package is moved whole into the installer's private area,
/// re-verified there by identity and hash, and only then deleted; anything
/// that changed is kept there and reported.
fn retire_legacy_package(home: &Path, target: &Path, work: &Path) -> Result<(), Error> {
    let package = home.join(".copilot").join("skills").join(NAME);
    if target != package
        || [home.join(".copilot"), home.join(".copilot").join("skills")]
            .iter()
            .any(|parent| !fs::symlink_metadata(parent).is_ok_and(|m| plain(&m) && m.is_dir()))
    {
        return Ok(());
    }
    let Some(found) = legacy_members(&package) else {
        return Ok(());
    };
    let Some(receipt) = found.get(LEGACY_MANIFEST) else {
        return Ok(());
    };
    let Some(manifest) = fs::read(package.join(LEGACY_MANIFEST))
        .ok()
        .filter(|bytes| revision(bytes) == receipt.hash)
        .and_then(|bytes| serde_json::from_slice::<LegacyManifest>(&bytes).ok())
    else {
        return Ok(());
    };
    if found.len() != manifest.files.len() + 1
        || manifest
            .files
            .iter()
            .any(|(name, hash)| found.get(name).map(|member| &member.hash) != Some(hash))
    {
        return Ok(());
    }
    checkpoint("legacy-package-scanned")?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let quarantine = work.join(format!("{QUARANTINE}{}-{unique}", std::process::id()));
    if fs::symlink_metadata(&quarantine).is_ok() || fs::rename(&package, &quarantine).is_err() {
        // Nothing moved; the package stays where the user can see it.
        return Ok(());
    }
    checkpoint("legacy-package-quarantined").map_err(|_| quarantined(&quarantine))?;
    let verified = legacy_members(&quarantine).is_some_and(|now| {
        now.len() == found.len()
            && found.iter().all(|(name, scanned)| {
                now.get(name).is_some_and(|member| {
                    member.identity == scanned.identity && member.hash == scanned.hash
                })
            })
    });
    if !verified {
        return Err(quarantined(&quarantine));
    }
    for name in found.keys() {
        fs::remove_file(quarantine.join(name)).map_err(|_| quarantined(&quarantine))?;
    }
    for directory in LEGACY_DIRECTORIES {
        match fs::remove_dir(quarantine.join(directory)) {
            Err(error) if error.kind() != ErrorKind::NotFound => {
                return Err(quarantined(&quarantine))
            }
            _ => {}
        }
    }
    fs::remove_dir(&quarantine).map_err(|_| quarantined(&quarantine))
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
    parent: (u64, u128),
}

/// Stable identity of the real directory holding the configuration file.
/// Failing to read it is a conflict, never a match.
fn parent_identity(config: &Path) -> Result<(u64, u128), Error> {
    file_identity::directory_identity(config.parent().ok_or_else(conflict)?).map_err(|_| conflict())
}

impl Scope {
    fn new(home: &Path) -> Result<Self, Error> {
        let paths = resolve(home)?;
        let work = paths.copilot.join(WORK);
        private_directory(&work).map_err(io_error)?;
        // Committed reads skip stage housekeeping: `private_fs::replace`
        // recovers its own stages before each write, so a read-only
        // `~/.copilot` still reports status.
        let parent = parent_identity(&paths.config)?;
        Ok(Self {
            home: home.into(),
            paths,
            work,
            parent,
        })
    }
    /// A link retargeted, or a real directory replaced, since this scope
    /// resolved is a conflict.
    fn check(&self) -> Result<(), Error> {
        if resolve(&self.home)? != self.paths || parent_identity(&self.paths.config)? != self.parent
        {
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
        read_config(&self.paths.config)
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
        checkpoint("mcp-config-checked")?;
        // Bind the write to the directory that was checked: a swapped folder
        // (including on Windows, where links are refused) is a conflict.
        if parent_identity(&self.paths.config)? != self.parent {
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
        // A link retargeted during the replacement means the public path never
        // received the write; never report it as registered.
        self.check()?;
        if self.entry()?.as_ref() != after {
            return Err(conflict());
        }
        checkpoint("mcp-config-written")
    }
    /// A kept legacy quarantine is reported on every read and write.
    fn quarantine(&self) -> Result<(), Error> {
        for entry in fs::read_dir(&self.work).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if entry.file_name().to_string_lossy().starts_with(QUARANTINE) {
                return Err(quarantined(&entry.path()));
            }
        }
        Ok(())
    }
    fn recover(&self) -> Result<(), Error> {
        self.quarantine()?;
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
            // An unverified quarantine keeps the legacy receipt and the intent
            // so nothing is forgotten; it is reported until the user resolves it.
            retire_legacy_package(&self.home, &legacy.target, &self.work)?;
            self.clear(LEGACY_OWNER)?;
        }
        checkpoint("mcp-owner-written")?;
        self.clear(INTENT)
    }
    fn status(&self, server: &Server) -> Result<Installation, Error> {
        installation(&self.home, server, self.owner()?, self.entry()?)
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

fn read_config(path: &Path) -> Result<(Option<Vec<u8>>, Value), Error> {
    match private_fs::existing_file(path) {
        Ok(()) => {
            if fs::metadata(path).map_err(io_error)?.len() > MAX_CONFIG {
                return Err(Error::new("mcp_config_too_large", ()));
            }
            let bytes = fs::read(path).map_err(io_error)?;
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| Error::new("mcp_config_invalid", ()))?;
            if !value.is_object() || value.get("mcpServers").is_some_and(|s| !s.is_object()) {
                return Err(Error::new("mcp_config_invalid", ()));
            }
            Ok((Some(bytes), value))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok((None, json!({"mcpServers":{}}))),
        Err(_) => Err(conflict()),
    }
}

fn installation(
    home: &Path,
    server: &Server,
    owner: Option<Value>,
    entry: Option<Value>,
) -> Result<Installation, Error> {
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
        skill_detected: skill_detected(home),
        skill_command: SKILL_COMMAND,
    })
}

/// Read-only status while no installer state exists. Before any opt-in the
/// app creates nothing under `~/.copilot`; with receipts, intents or a
/// quarantine present, `None` selects the locked, recovering path.
fn observe(home: &Path, server: &Server) -> Result<Option<Installation>, Error> {
    let paths = resolve(home)?;
    if has_state(&paths)? {
        return Ok(None);
    }
    let entry = read_config(&paths.config)?
        .1
        .get("mcpServers")
        .and_then(|s| s.get(NAME))
        .cloned();
    installation(home, server, None, entry).map(Some)
}

/// Receipts, intents, a quarantine or an interrupted write exist: only an
/// earlier explicit opt-in creates anything besides the lock.
fn has_state(paths: &Resolved) -> Result<bool, Error> {
    let work = paths.copilot.join(WORK);
    match fs::symlink_metadata(&work) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(_) => Ok(true),
        Ok(metadata) if !metadata.is_dir() => Ok(true),
        Ok(_) => {
            for entry in fs::read_dir(&work).map_err(io_error)? {
                if entry.map_err(io_error)?.file_name() != LOCK {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// Launch: with no installer state nothing is published or written. With
/// state, `publish` refreshes the bundled server before an owned entry is
/// re-pointed.
pub fn launch_repair_at(
    home: &Path,
    server: &Server,
    publish: impl FnOnce() -> Result<(), Error>,
) -> Result<bool, Error> {
    if !server.installed || !has_state(&resolve(home)?)? {
        return Ok(false);
    }
    publish()?;
    repair_integration_at(home, server)
}

fn not_installed() -> Error {
    Error::new("app_not_installed", ())
}

fn validate(server: &Server) -> Result<(), Error> {
    if !server.installed {
        return Err(not_installed());
    }
    if !server.executable.is_absolute()
        || !server.script.is_absolute()
        || !fs::metadata(&server.script).is_ok_and(|metadata| metadata.is_file())
    {
        return Err(Error::new("invalid_executable", ()));
    }
    Ok(())
}

pub fn integration_at(home: &Path, server: &Server) -> Result<Installation, Error> {
    if let Some(status) = observe(home, server)? {
        return Ok(status);
    }
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
    if !server.installed {
        return Err(not_installed());
    }
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
    if !server.installed {
        return Ok(false);
    }
    validate(server)?;
    // Nothing owned: never create installer state on launch.
    if observe(home, server)?.is_some() {
        return Ok(false);
    }
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
    Ok(bundled_script(root))
}

/// Where `materialize` publishes the server; computing it writes nothing.
#[cfg(windows)]
pub fn bundled_script(root: &Path) -> PathBuf {
    root.join("mcp").join("mcp-server.mjs")
}

/// Expected registration for this app. Never writes; see [`publish`].
fn server(app: &tauri::AppHandle, home: &Path) -> Result<Server, Error> {
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
    let script = bundled_script(&app.path().app_local_data_dir().map_err(io_error)?);
    let installed = crate::startup::installed_executable(&executable, home);
    Ok(Server {
        script,
        executable,
        installed,
    })
}

/// Windows publishes the embedded server only for explicit actions or
/// existing installer state; macOS ships it signed in the bundle.
fn publish(app: &tauri::AppHandle) -> Result<(), Error> {
    #[cfg(windows)]
    {
        use tauri::Manager;
        materialize(&app.path().app_local_data_dir().map_err(io_error)?)?;
    }
    #[cfg(not(windows))]
    let _ = app;
    Ok(())
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
    let result =
        home(app).and_then(|home| launch_repair_at(&home, &server(app, &home)?, || publish(app)));
    if result.is_err() {
        eprintln!("PR Sniper: Copilot MCP registration repair skipped");
    }
}

#[tauri::command]
pub(crate) fn control_installation(app: tauri::AppHandle) -> Result<Installation, Error> {
    let home = home(&app)?;
    integration_at(&home, &server(&app, &home)?)
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
    let server = server(&app, &home)?;
    if server.installed {
        publish(&app)?;
    }
    install_integration_at(&home, &server, expected.as_deref())
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
    remove_integration_at(&home, &server(&app, &home)?, &expected)
}
