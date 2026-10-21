//! One owned Copilot entry plus the verified skill package. No other server
//! configuration is exposed, adopted or reset.
use super::*;
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::ErrorKind,
    path::PathBuf,
};

const NAME: &str = "pr-sniper";
const OWNER: &str = "copilot-entry.json";
const INTENT: &str = "copilot-integration-intent.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    target: PathBuf,
    entry: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    target: PathBuf,
    before_package: Option<String>,
    after_package: Option<String>,
    before_entry: Option<Value>,
    after_entry: Option<Value>,
}
struct Guard(File);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
struct Scope {
    target: PathBuf,
    work: PathBuf,
    config: PathBuf,
}
impl Scope {
    fn new(home: &Path) -> Result<Self, Error> {
        let target = home.join(".copilot/skills/pr-sniper");
        verify_parents(&target)?;
        let work = transaction::workspace(&target)?;
        verify_parents(&work)?;
        private_directory(&work).map_err(io_error)?;
        private_fs::recover(&work, Some(OWNER)).map_err(io_error)?;
        private_fs::recover(&work, Some(INTENT)).map_err(io_error)?;
        private_fs::recover(&home.join(".copilot"), Some("mcp-config.json")).map_err(io_error)?;
        Ok(Self {
            target,
            work,
            config: home.join(".copilot/mcp-config.json"),
        })
    }
    fn lock(&self) -> Result<Guard, Error> {
        let path = self.work.join("copilot-integration.lock");
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
                if fs::metadata(&path).map_err(io_error)?.len() > super::super::MAX_FRAME as u64 {
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
    fn config(&self) -> Result<(Option<Vec<u8>>, Value), Error> {
        verify_parents(self.config.parent().unwrap())?;
        match private_fs::existing_file(&self.config) {
            Ok(()) => {
                if fs::metadata(&self.config).map_err(io_error)?.len() > 8 * 1024 * 1024 {
                    return Err(Error::new("mcp_config_too_large", ()));
                }
                let bytes = fs::read(&self.config).map_err(io_error)?;
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
        // Other servers are merged from the latest read, never from an old UI
        // draft or ownership receipt. Detect an external edit before replacement.
        if self.config()?.0 != bytes {
            return Err(conflict());
        }
        private_fs::replace(
            &self.config,
            &serde_json::to_vec_pretty(&config).map_err(io_error)?,
            "Copilot MCP configuration",
        )
        .map_err(io_error)?;
        private_fs::sync_directory(self.config.parent().unwrap()).map_err(io_error)?;
        if self.entry()?.as_ref() != after {
            return Err(conflict());
        }
        checkpoint("mcp-config-written")
    }
    fn recover(&self) -> Result<(), Error> {
        let Some(intent) = self.read::<Intent>(INTENT)? else {
            return Ok(());
        };
        if intent.target != self.target {
            return Err(conflict());
        }
        let package = installation_at(&self.target)?;
        if let Some(after) = &intent.after_package {
            if package.revision.as_ref() == Some(after) {
                self.set_entry(intent.before_entry.as_ref(), intent.after_entry.as_ref())?;
                let entry = intent.after_entry.clone().ok_or_else(conflict)?;
                self.write(
                    OWNER,
                    &Owner {
                        target: self.target.clone(),
                        entry,
                    },
                )?;
                checkpoint("mcp-owner-written")?;
                self.clear(INTENT)
            } else if package.revision == intent.before_package
                && self.entry()? == intent.before_entry
            {
                // Package staging rolled back before activation. No host change
                // occurred; a fresh explicit setup can safely try again.
                self.clear(INTENT)
            } else {
                Err(conflict())
            }
        } else {
            if package.revision != intent.before_package && package.revision.is_some() {
                return Err(conflict());
            }
            self.set_entry(intent.before_entry.as_ref(), None)?;
            if let Some(revision) = package.revision {
                remove_at(&self.target, &revision)?;
            }
            checkpoint("mcp-package-removed")?;
            self.clear(OWNER)?;
            self.clear(INTENT)
        }
    }
    fn status(&self) -> Result<Installation, Error> {
        let package = installation_at(&self.target)?;
        let entry = self.entry()?;
        let owner = self.read::<Owner>(OWNER)?;
        if let Some(owner) = &owner {
            if owner.target != self.target
                || entry.as_ref().is_some_and(|entry| entry != &owner.entry)
            {
                return Err(conflict());
            }
        } else if entry.is_some() {
            return Err(conflict());
        }
        let registered = entry.is_some() && owner.is_some();
        let has_mcp = if package.revision.is_some() {
            let manifest: Manifest = serde_json::from_slice(
                &fs::read(self.target.join(".pr-sniper-install.json")).map_err(io_error)?,
            )
            .map_err(|_| conflict())?;
            manifest.format == 2
        } else {
            false
        };
        let absent = package.revision.is_none() && !registered;
        Ok(Installation {
            state: if absent {
                "absent"
            } else if has_mcp && registered {
                "installed"
            } else {
                "incomplete"
            },
            revision: (!absent)
                .then(|| revision(&json!({"package":package.revision,"entry":entry}))),
            version: package.version,
            mcp_registered: Some(registered),
        })
    }
}
fn conflict() -> Error {
    Error::new("installation_conflict","Copilot entry or integration ownership changed; preserve custom configuration before repair.")
}
fn desired(target: &Path) -> Value {
    json!({"type":"local","command":"node","args":[target.join("scripts/mcp-server.mjs")],"tools":["*"],"timeout":180000})
}

pub fn integration_at(home: &Path) -> Result<Installation, Error> {
    let scope = Scope::new(home)?;
    let _guard = scope.lock()?;
    scope.recover()?;
    scope.status()
}
pub fn install_integration_at(
    home: &Path,
    executable: &Path,
    expected: Option<&str>,
) -> Result<Installation, Error> {
    if !home.is_absolute() || !executable.is_absolute() {
        return Err(Error::new("invalid_executable", ()));
    }
    let scope = Scope::new(home)?;
    let _guard = scope.lock()?;
    scope.recover()?;
    let status = scope.status()?;
    if status.revision.as_deref() != expected {
        return Err(Error::new("conflict", ()));
    }
    let before = installation_at(&scope.target)?.revision;
    let intent = Intent {
        target: scope.target.clone(),
        before_package: before.clone(),
        after_package: Some(planned_revision(executable)?),
        before_entry: scope.entry()?,
        after_entry: Some(desired(&scope.target)),
    };
    scope.write(INTENT, &intent)?;
    checkpoint("mcp-intent-written")?;
    install_at(&scope.target, executable, before.as_deref())?;
    checkpoint("mcp-package-installed")?;
    scope.recover()?;
    scope.status()
}
pub fn remove_integration_at(home: &Path, expected: &str) -> Result<Installation, Error> {
    let scope = Scope::new(home)?;
    let _guard = scope.lock()?;
    scope.recover()?;
    let status = scope.status()?;
    if status.revision.as_deref() != Some(expected) {
        return Err(Error::new("conflict", ()));
    }
    scope.write(
        INTENT,
        &Intent {
            target: scope.target.clone(),
            before_package: installation_at(&scope.target)?.revision,
            after_package: None,
            before_entry: scope.entry()?,
            after_entry: None,
        },
    )?;
    checkpoint("mcp-intent-written")?;
    scope.recover()?;
    scope.status()
}

#[cfg(test)]
mod tests;
