use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_DIAGNOSTICS_BYTES: u64 = 256 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticEvent {
    SessionStarted,
    WindowOpened,
    WindowHidden,
    SettingsSaved,
    QuitRequested,
    HostFailure,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub timestamp_secs: u64,
    pub event: DiagnosticEvent,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub launch_at_login: bool,
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn load_settings(&self) -> Result<Settings, String> {
        let path = self.root.join("config/settings.json");
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Settings::default()),
            Err(_) => return Err("Cannot read settings. Check local file permissions.".into()),
        };
        serde_json::from_slice(&bytes)
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.".into())
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        // Never silently replace unreadable or corrupt existing configuration.
        self.load_settings()?;
        let directory = self.root.join("config");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .map_err(|_| "Cannot create configuration directory.".to_string())?;
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|_| "Cannot encode settings.".to_string())?;
        let temporary = directory.join("settings.json.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| "Cannot write settings.".to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot flush settings.".to_string())?;
        fs::rename(temporary, directory.join("settings.json"))
            .map_err(|_| "Cannot replace settings.".to_string())?;
        Ok(())
    }

    pub fn record(&self, event: DiagnosticEvent) -> Result<(), String> {
        let directory = self.root.join("state");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .map_err(|_| "Cannot create diagnostics directory.".to_string())?;
        let diagnostic = Diagnostic {
            timestamp_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "System clock precedes the Unix epoch.".to_string())?
                .as_secs(),
            event,
        };
        let mut bytes = serde_json::to_vec(&diagnostic)
            .map_err(|_| "Cannot encode diagnostics.".to_string())?;
        bytes.push(b'\n');
        let path = directory.join("diagnostics.jsonl");
        match fs::metadata(&path) {
            Ok(metadata) if metadata.len() + bytes.len() as u64 > MAX_DIAGNOSTICS_BYTES => {
                fs::rename(&path, directory.join("diagnostics.previous.jsonl"))
                    .map_err(|_| "Cannot rotate diagnostics.".to_string())?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot inspect diagnostics.".into()),
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .and_then(|mut file| file.write_all(&bytes).and_then(|_| file.sync_data()))
            .map_err(|_| "Cannot write diagnostics.".into())
    }

    pub fn diagnostics(&self) -> Result<Vec<Diagnostic>, String> {
        let path = self.root.join("state/diagnostics.jsonl");
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err("Cannot read diagnostics.".into()),
        };
        if contents.len() as u64 > MAX_DIAGNOSTICS_BYTES {
            return Err("Diagnostics exceed the supported size.".into());
        }
        contents
            .lines()
            .map(|line| {
                serde_json::from_str(line).map_err(|_| "Diagnostics contain invalid data.".into())
            })
            .collect()
    }
}
