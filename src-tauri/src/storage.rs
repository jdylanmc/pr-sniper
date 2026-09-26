use crate::policy::{Policy, PolicyOverrides, Schedule, WatchedIdentity};
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    // An explicitly empty library is saved, never confused with uninitialized data.
    #[serde(default)]
    pub doctrines: Vec<Doctrine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<Agent>,
}

/// A named review principle. `title` is both the display label and the
/// identity (its slug) -- there is no separate id field. Plain text body,
/// ~500 words recommended but never enforced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Doctrine {
    pub title: String,
    pub body: String,
}

/// A reusable review profile: model + optional doctrine + prompt +
/// signature. Referenced by id from repository assignments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_account: Option<AiAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doctrine: Option<String>,
    pub prompt: String,
    pub signature: String,
}

/// Stable connection reference, never a credential or mutable login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiAccount {
    pub provider: String,
    pub account_id: String,
}

/// One agent running on one repository: its own timer and permissions.
/// `approve` is stored but never executed -- provider approval submission
/// remains an explicit MVP non-goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub id: String,
    pub agent_id: String,
    pub schedule: Schedule,
    pub comment: bool,
    #[serde(default)]
    pub approve: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Github,
    AzureDevops,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountId {
    pub provider: ProviderId,
    pub account_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRepositoryId {
    pub provider: ProviderId,
    pub repository_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryAccountBinding {
    pub account: ProviderAccountId,
    pub repository: ProviderRepositoryId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub id: String,
    pub provider: ProviderId,
    pub name: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_account_id: Option<String>,
    #[serde(default, rename = "installation_id", skip_serializing)]
    pub legacy_installation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "PolicyOverrides::is_empty")]
    pub overrides: PolicyOverrides,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_preset: Option<String>,
    /// Optional per-repository watchlist; exact GitHub login + stable
    /// numeric id, no wildcards. Shared across this repository's assignments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watched_authors: Vec<WatchedIdentity>,
    /// N agents, each with its own timer and permissions, running on this
    /// repository.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<Assignment>,
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
        let mut doctrine_titles = HashSet::new();
        for doctrine in &self.doctrines {
            if doctrine.title.trim().is_empty()
                || !doctrine_titles.insert(doctrine.title.trim().to_lowercase())
            {
                return Err("Doctrines need a unique, nonempty title.".into());
            }
            crate::policy::validate_configuration_text(&doctrine.title)?;
            crate::policy::validate_configuration_text(&doctrine.body)?;
        }
        let mut agent_ids = HashSet::new();
        let mut agent_names = HashSet::new();
        for agent in &self.agents {
            if uuid::Uuid::parse_str(&agent.id).is_err()
                || !agent_ids.insert(&agent.id)
                || agent.name.trim().is_empty()
                || !agent_names.insert(agent.name.trim().to_lowercase())
                || agent.model.trim().is_empty()
                || agent.signature.trim().is_empty()
            {
                return Err(
                    "Agents need a unique identity, a unique name, a model, and a signature."
                        .into(),
                );
            }
            if agent
                .doctrine
                .as_ref()
                .is_some_and(|title| !doctrine_titles.contains(&title.trim().to_lowercase()))
            {
                return Err(
                    "The selected doctrine no longer exists. Choose a local doctrine or none."
                        .into(),
                );
            }
            crate::policy::validate_configuration_text(&agent.name)?;
            crate::policy::validate_configuration_text(&agent.model)?;
            crate::policy::validate_configuration_text(&agent.prompt)?;
            crate::policy::validate_configuration_text(&agent.signature)?;
            if let Some(account) = &agent.ai_account {
                if account.provider != "copilot"
                    || !is_provider_id(&ProviderId::Github, &account.account_id)
                {
                    return Err(
                        "Choose a supported AI provider and stable account identity.".into(),
                    );
                }
            }
        }
        let mut ids = HashSet::new();
        let mut bindings = HashSet::new();
        for repository in &self.repositories {
            validate_preset(&repository.review_preset)?;
            if uuid::Uuid::parse_str(&repository.id).is_err() || !ids.insert(&repository.id) {
                return Err("Repository identities must be valid and unique.".into());
            }
            if canonical_provider_repository(&repository.provider, &repository.name)?
                != repository.name
            {
                return Err("Repository names must be canonical.".into());
            }
            if repository.provider_account_id.is_some()
                && repository.provider_repository_id.is_none()
                || repository
                    .provider_account_id
                    .as_ref()
                    .is_some_and(|id| !is_provider_id(&repository.provider, id))
                || repository
                    .provider_repository_id
                    .as_ref()
                    .is_some_and(|id| !is_provider_id(&repository.provider, id))
            {
                return Err(
                    "Connected repositories require stable provider, account and repository identities.".into(),
                );
            }
            let binding = if repository.provider_account_id.is_some()
                && repository.provider_repository_id.is_some()
            {
                (
                    repository.provider.clone(),
                    repository.provider_account_id.clone(),
                    repository.provider_repository_id.clone(),
                    None,
                )
            } else {
                (
                    repository.provider.clone(),
                    None,
                    None,
                    Some(repository.name.clone()),
                )
            };
            if !bindings.insert(binding) {
                return Err("Repository bindings must be unique.".into());
            }
            repository.overrides.effective(&self.defaults).validate()?;
            let mut watched_ids = HashSet::new();
            for identity in &repository.watched_authors {
                let valid_id = identity
                    .id
                    .parse::<u64>()
                    .ok()
                    .filter(|id| *id > 0 && id.to_string() == identity.id);
                if valid_id.is_none() || !watched_ids.insert(&identity.id) {
                    return Err(
                        "Watched GitHub account IDs must be unique positive decimal numbers."
                            .into(),
                    );
                }
                if identity.login.trim().is_empty()
                    || identity.login.chars().any(char::is_whitespace)
                {
                    return Err("Each watched GitHub identity needs a nonempty login display label without whitespace.".into());
                }
            }
            let mut assignment_ids = HashSet::new();
            for assignment in &repository.assignments {
                if uuid::Uuid::parse_str(&assignment.id).is_err()
                    || !assignment_ids.insert(&assignment.id)
                {
                    return Err("Assignments need a unique identity.".into());
                }
                if !agent_ids.contains(&assignment.agent_id) {
                    return Err("The selected agent no longer exists. Choose a local agent.".into());
                }
                assignment.schedule.validate()?;
            }
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

impl Repository {
    pub fn account_binding(&self) -> Option<RepositoryAccountBinding> {
        Some(RepositoryAccountBinding {
            account: ProviderAccountId {
                provider: self.provider.clone(),
                account_id: self.provider_account_id.clone()?,
            },
            repository: ProviderRepositoryId {
                provider: self.provider.clone(),
                repository_id: self.provider_repository_id.clone()?,
            },
        })
    }
}

fn is_provider_id(provider: &ProviderId, value: &str) -> bool {
    match provider {
        ProviderId::Github => is_decimal_id(value),
        ProviderId::AzureDevops => !value.is_empty() && value.len() <= 256,
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

fn canonical_provider_repository(provider: &ProviderId, input: &str) -> Result<String, String> {
    match provider {
        ProviderId::Github => canonical_repository(input),
        ProviderId::AzureDevops => {
            let value = input.trim();
            if value.is_empty()
                || value.len() > 512
                || value.chars().any(char::is_control)
                || value.contains('@')
            {
                return Err("Enter a stable Azure DevOps repository identity.".into());
            }
            Ok(value.to_string())
        }
    }
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn load_queue(&self) -> Result<Vec<crate::monitoring::QueueJob>, String> {
        match fs::read(self.root.join("state/queue.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Review queue is invalid; no polling result was saved.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => Err("Cannot read the review queue. Check local file permissions.".into()),
        }
    }

    pub fn save_queue(&self, jobs: &[crate::monitoring::QueueJob]) -> Result<(), String> {
        self.write_state("queue.json", jobs)
    }

    pub fn load_monitoring_state(&self) -> Result<crate::monitoring::MonitoringState, String> {
        match fs::read(self.root.join("state/monitoring.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Monitoring state is invalid; polling cannot resume.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok(crate::monitoring::MonitoringState::default())
            }
            Err(_) => Err("Cannot read monitoring state. Check local file permissions.".into()),
        }
    }

    pub fn save_monitoring_state(
        &self,
        state: &crate::monitoring::MonitoringState,
    ) -> Result<(), String> {
        self.write_state("monitoring.json", state)
    }

    fn write_state<T: Serialize + ?Sized>(&self, name: &str, value: &T) -> Result<(), String> {
        let directory = self.root.join("state");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .map_err(|_| "Cannot create monitoring state directory.".to_string())?;
        let bytes =
            serde_json::to_vec_pretty(value).map_err(|_| "Cannot encode monitoring state.")?;
        let temporary = directory.join(format!("{name}.tmp"));
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| "Cannot write monitoring state.")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot flush monitoring state.")?;
        fs::rename(temporary, directory.join(name))
            .map_err(|_| "Cannot replace monitoring state.".into())
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
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let settings = Settings {
                    doctrines: crate::doctrine_seeds::doctrines(),
                    ..Settings::default()
                };
                settings.validate()?;
                self.write_settings(&settings)?;
                return Ok(settings);
            }
            Err(_) => return Err("Cannot read settings. Check local file permissions.".into()),
        };
        let mut settings: Settings = serde_json::from_slice(&bytes)
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        let migrated = settings
            .repositories
            .iter_mut()
            .fold(false, |changed, repository| {
                if matches!(repository.provider, ProviderId::Github)
                    && repository.legacy_installation_id.take().is_some()
                {
                    repository.provider_account_id = None;
                    repository.provider_repository_id = None;
                    true
                } else {
                    changed
                }
            });
        settings
            .validate()
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        if migrated {
            self.write_settings(&settings)?;
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        // Never silently replace unreadable or corrupt existing configuration.
        self.load_settings()?;
        self.write_settings(settings)
    }

    fn write_settings(&self, settings: &Settings) -> Result<(), String> {
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
        if settings.repositories.iter().any(|repo| {
            repo.provider == ProviderId::Github
                && repo.name == name
                && repo.provider_account_id.is_none()
        }) {
            return Err("This GitHub repository is already configured.".into());
        }
        settings.repositories.push(Repository {
            id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderId::Github,
            name,
            enabled: true,
            provider_account_id: None,
            legacy_installation_id: None,
            provider_repository_id: None,
            overrides: PolicyOverrides::default(),
            review_preset: None,
            watched_authors: Vec::new(),
            assignments: Vec::new(),
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
        if settings.repositories.iter().any(|repo| {
            repo.id != id
                && repo.provider == ProviderId::Github
                && repo.name == name
                && repo.provider_account_id.is_none()
        }) {
            return Err("This GitHub repository is already configured.".into());
        }
        let repo = settings
            .repositories
            .iter_mut()
            .find(|repo| repo.id == id)
            .ok_or("Repository no longer exists. Reload Settings.")?;
        repo.name = name;
        repo.provider_account_id = None;
        repo.legacy_installation_id = None;
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
