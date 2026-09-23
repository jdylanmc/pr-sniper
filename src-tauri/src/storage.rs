use crate::policy::{Policy, PolicyOverrides};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_DIAGNOSTICS_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticEvent {
    SessionStarted,
    WindowOpened,
    WindowHidden,
    SettingsSaved,
    QuitRequested,
    HostFailure,
    GithubConnectionChecked,
    GithubConnectionFailed,
    GithubMetadataRead,
    GithubReadFailed,
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
    #[serde(default)]
    pub defaults: Policy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<Repository>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_folder: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<ReviewPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_review_preset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPreset {
    pub id: String,
    pub name: String,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct SavedSettings {
    pub settings: Settings,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub id: String,
    pub provider: Provider,
    pub name: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "PolicyOverrides::is_empty")]
    pub overrides: PolicyOverrides,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_preset: Option<String>,
}

impl Settings {
    fn validate(&self) -> Result<(), String> {
        self.defaults.validate()?;
        if self
            .root_folder
            .as_ref()
            .is_some_and(|path| !std::path::Path::new(path).is_absolute())
        {
            return Err("Choose an absolute local root folder.".into());
        }
        let mut preset_ids = HashSet::new();
        let mut preset_names = HashSet::new();
        for preset in &self.presets {
            if uuid::Uuid::parse_str(&preset.id).is_err()
                || !preset_ids.insert(&preset.id)
                || preset.name.trim().is_empty()
                || preset.name.chars().count() > 80
                || !preset_names.insert(preset.name.trim().to_lowercase())
                || preset.body.chars().count() > 12000
            {
                return Err("Presets need unique identities and names (1-80 characters), and instructions up to 12,000 characters.".into());
            }
            crate::policy::validate_configuration_text(&preset.name)?;
            crate::policy::validate_configuration_text(&preset.body)?;
        }
        let validate_preset = |id: &Option<String>| -> Result<(), String> {
            if id.as_ref().is_some_and(|id| !preset_ids.contains(id)) {
                return Err("The selected review preset no longer exists. Choose a local preset or custom instructions.".into());
            }
            Ok(())
        };
        validate_preset(&self.default_review_preset)?;
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for repository in &self.repositories {
            validate_preset(&repository.review_preset)?;
            if uuid::Uuid::parse_str(&repository.id).is_err() || !ids.insert(&repository.id) {
                return Err("Repository identities must be valid and unique.".into());
            }
            if canonical_repository(&repository.name)? != repository.name
                || !names.insert(&repository.name)
            {
                return Err("Repository names must be canonical and unique.".into());
            }
            if repository.installation_id.is_some() != repository.provider_repository_id.is_some()
                || repository
                    .installation_id
                    .as_ref()
                    .is_some_and(|id| !is_decimal_id(id))
                || repository
                    .provider_repository_id
                    .as_ref()
                    .is_some_and(|id| !is_decimal_id(id))
            {
                return Err(
                    "Connected repositories require stable installation and repository identities."
                        .into(),
                );
            }
            repository.overrides.effective(&self.defaults).validate()?;
        }
        Ok(())
    }

    pub fn effective_policy(&self, id: &str) -> Option<Policy> {
        self.repositories
            .iter()
            .find(|repository| repository.id == id)
            .map(|repository| repository.overrides.effective(&self.defaults))
    }
}

fn is_decimal_id(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) && value != "0"
}

pub fn canonical_repository(input: &str) -> Result<String, String> {
    let lower = input.trim().to_ascii_lowercase();
    let path = lower.strip_prefix("https://github.com/").unwrap_or(&lower);
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let parts: Vec<_> = path.split('/').collect();
    let valid = parts.len() == 2
        && !parts[0].is_empty()
        && parts[0].len() <= 39
        && !parts[0].starts_with('-')
        && !parts[0].ends_with('-')
        && parts[0]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !parts[1].is_empty()
        && parts[1].len() <= 100
        && parts[1] != "."
        && parts[1] != ".."
        && parts[1]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    if !valid {
        return Err("Enter owner/repository or an HTTPS github.com repository URL (no credentials, query or fragment).".into());
    }
    Ok(path.to_string())
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn has_saved_settings(&self) -> bool {
        self.root.join("config/settings.json").exists()
    }

    pub fn save_preferences(
        &self,
        mut settings: Settings,
        expected: &Settings,
    ) -> Result<Settings, String> {
        let current = self.load_settings()?;
        if &current != expected {
            return Err("Settings changed in another window. Reload Settings before saving; your draft has not been written.".into());
        }
        if settings.launch_at_login != current.launch_at_login {
            return Err("Change launch at login using the separate startup control.".into());
        }
        settings.validate()?;
        if let Some(id) = &settings.default_review_preset {
            settings.defaults.prompt = settings
                .presets
                .iter()
                .find(|p| &p.id == id)
                .unwrap()
                .body
                .clone();
        }
        for repository in &mut settings.repositories {
            if let Some(id) = &repository.review_preset {
                repository.overrides.prompt = Some(
                    settings
                        .presets
                        .iter()
                        .find(|p| &p.id == id)
                        .unwrap()
                        .body
                        .clone(),
                );
            }
        }
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn load_settings(&self) -> Result<Settings, String> {
        let path = self.root.join("config/settings.json");
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Settings::default()),
            Err(_) => return Err("Cannot read settings. Check local file permissions.".into()),
        };
        let settings: Settings = serde_json::from_slice(&bytes)
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        settings
            .validate()
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
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

    pub fn add_repository(&self, repository: &str) -> Result<Settings, String> {
        let name = canonical_repository(repository)?;
        let mut settings = self.load_settings()?;
        if settings.repositories.iter().any(|repo| repo.name == name) {
            return Err("This GitHub repository is already configured.".into());
        }
        settings.repositories.push(Repository {
            id: uuid::Uuid::new_v4().to_string(),
            provider: Provider::Github,
            name,
            enabled: true,
            installation_id: None,
            provider_repository_id: None,
            overrides: PolicyOverrides::default(),
            review_preset: None,
        });
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn update_repository(
        &self,
        id: &str,
        repository: &str,
        enabled: bool,
    ) -> Result<Settings, String> {
        let name = canonical_repository(repository)?;
        let mut settings = self.load_settings()?;
        if settings
            .repositories
            .iter()
            .any(|repo| repo.id != id && repo.name == name)
        {
            return Err("This GitHub repository is already configured.".into());
        }
        let repo = settings
            .repositories
            .iter_mut()
            .find(|repo| repo.id == id)
            .ok_or("Repository no longer exists. Reload Settings.")?;
        repo.name = name;
        repo.installation_id = None;
        repo.provider_repository_id = None;
        repo.enabled = enabled;
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn remove_repository(&self, id: &str) -> Result<Settings, String> {
        let mut settings = self.load_settings()?;
        let index = settings
            .repositories
            .iter()
            .position(|repo| repo.id == id)
            .ok_or("Repository no longer exists. Reload Settings.")?;
        settings.repositories.remove(index);
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn save_defaults(&self, policy: Policy) -> Result<Settings, String> {
        let mut settings = self.load_settings()?;
        if settings.defaults.prompt != policy.prompt {
            settings.default_review_preset = None;
        }
        settings.defaults = policy;
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn save_repository_policy(
        &self,
        id: &str,
        overrides: PolicyOverrides,
    ) -> Result<Settings, String> {
        let mut settings = self.load_settings()?;
        let repository = settings
            .repositories
            .iter_mut()
            .find(|repository| repository.id == id)
            .ok_or("Repository no longer exists. Reload Settings.")?;
        if repository.overrides.prompt != overrides.prompt {
            repository.review_preset = None;
        }
        repository.overrides = overrides;
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn finish_settings_save(&self, settings: Settings) -> SavedSettings {
        let warning = self.record(DiagnosticEvent::SettingsSaved).err().map(|_| {
            "Settings saved, but host diagnostics could not be recorded. Check local storage permissions."
                .to_string()
        });
        SavedSettings { settings, warning }
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
