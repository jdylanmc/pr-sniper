use crate::admission::AdmissionConfig;
use crate::policy::{Policy, PolicyOverrides, Schedule, WatchedIdentity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_DIAGNOSTICS_BYTES: u64 = 256 * 1024;

#[path = "storage/diagnostics.rs"]
pub mod diagnostics;
#[path = "storage/private_fs.rs"]
pub(crate) mod private_fs;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticEvent {
    Attempt,
    SessionStarted,
    WindowOpened,
    WindowCloseRequested,
    WindowHidden,
    SettingsSaved,
    QuitRequested,
    HostFailure,
    GithubConnectionChecked,
    GithubConnectionFailed,
    GithubMetadataRead,
    GithubReadFailed,
    NotificationAccepted,
    NotificationFailed,
    NotificationOpened,
    NotificationActivated,
    ThreadSkipped,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub timestamp_secs: u64,
    pub event: DiagnosticEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<diagnostics::Record>,
}

fn diagnostic_time() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| "System clock precedes the Unix epoch.".into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub launch_at_login: bool,
    #[serde(default)]
    pub defaults: Policy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<Repository>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repository_authorizations: BTreeMap<String, Option<RepositoryAuthorization>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_folder: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<ReviewPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_review_preset: Option<String>,
    // An explicitly empty library is saved, never confused with uninitialized data.
    #[serde(default)]
    pub doctrines: Vec<Doctrine>,
    #[serde(default = "doctrine_catalog_version")]
    pub doctrine_catalog_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doctrine_reset: Option<DoctrineReset>,
    // Same library semantics as `doctrines`. Files without the version predate
    // Voices and are seeded once on load; an emptied library stays empty.
    #[serde(default)]
    pub voices: Vec<Voice>,
    #[serde(default = "voice_catalog_version")]
    pub voice_catalog_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_notice: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<Agent>,
    #[serde(default = "default_capacity")]
    pub capacity: u32,
    #[serde(default = "default_review_inactivity_minutes")]
    pub review_inactivity_minutes: u32,
    #[serde(default, skip_serializing_if = "DuckLimits::is_default")]
    pub rubber_duck: DuckLimits,
    /// Always written. Settings files without it predate admission rules and
    /// get the one-time migration on load; programmatic settings are current.
    #[serde(default = "admission_migration_version")]
    pub admission_migration_version: u32,
    /// Omitted when Light so default files stay readable by earlier versions.
    #[serde(default, skip_serializing_if = "Theme::is_light")]
    pub theme: Theme,
    /// IANA zone for every repository schedule. Always written. Files without
    /// it predate repository schedules and get `migrate_global_schedule` on load.
    #[serde(default = "utc")]
    pub time_zone: String,
}

/// Panel appearance. `Os` follows the current system light/dark appearance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
    Os,
}

impl Theme {
    fn is_light(&self) -> bool {
        *self == Theme::Light
    }
}

pub const ADMISSION_MIGRATION_VERSION: u32 = 1;

fn admission_migration_version() -> u32 {
    ADMISSION_MIGRATION_VERSION
}

fn utc() -> String {
    "UTC".into()
}

fn default_cron() -> String {
    crate::schedule::DEFAULT_CRON.into()
}

fn default_capacity() -> u32 {
    4
}

fn doctrine_catalog_version() -> u32 {
    1
}

fn voice_catalog_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctrineReset {
    pub previous_count: usize,
    pub removed_references: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctrineCatalog {
    pub source: String,
    pub source_revision: String,
    pub effective_revision: String,
    pub count: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            defaults: Policy::default(),
            repositories: Vec::new(),
            repository_authorizations: BTreeMap::new(),
            root_folder: None,
            presets: Vec::new(),
            default_review_preset: None,
            doctrines: Vec::new(),
            doctrine_catalog_version: doctrine_catalog_version(),
            doctrine_reset: None,
            voices: Vec::new(),
            voice_catalog_version: voice_catalog_version(),
            capability_notice: None,
            agents: Vec::new(),
            capacity: default_capacity(),
            review_inactivity_minutes: default_review_inactivity_minutes(),
            rubber_duck: DuckLimits::default(),
            admission_migration_version: admission_migration_version(),
            theme: Theme::Light,
            time_zone: utc(),
        }
    }
}

/// A review principle: stable `id` (UUID v4, never changed), display
/// `title` and plain-text `body` (~500 words recommended, never enforced).
/// Agents reference it by id. `id` is empty only on pre-id settings (migrated
/// on load) and pre-id job evidence (kept as recorded).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Doctrine {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub title: String,
    pub body: String,
}

/// A writing style for text an Agent writes for humans: stable `id`, `title`,
/// plain-text `body` and the fixed-guard flag. Agents reference it by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub title: String,
    pub body: String,
    /// The host adds its fixed tone guard to the packet. Default on.
    #[serde(default = "guardrails_default")]
    pub guardrails: bool,
}

fn guardrails_default() -> bool {
    true
}

/// A shared, id-keyed library entry (Doctrine, Voice).
trait LibraryEntry: Clone + PartialEq {
    fn id(&self) -> &str;
    fn id_mut(&mut self) -> &mut String;
    fn title(&self) -> &str;
    fn body(&self) -> &str;
}

impl LibraryEntry for Doctrine {
    fn id(&self) -> &str {
        &self.id
    }
    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn body(&self) -> &str {
        &self.body
    }
}

impl LibraryEntry for Voice {
    fn id(&self) -> &str {
        &self.id
    }
    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn body(&self) -> &str {
        &self.body
    }
}

fn same_title(left: &str, right: &str) -> bool {
    left.trim().to_lowercase() == right.trim().to_lowercase()
}

/// Library ids. Rejects invalid or repeated ids, empty or repeated
/// (case-insensitive) titles and unsafe text.
fn library_ids<T: LibraryEntry>(entries: &[T], plural: &str) -> Result<HashSet<String>, String> {
    let mut ids = HashSet::new();
    let mut titles = HashSet::new();
    for entry in entries {
        if uuid::Uuid::parse_str(entry.id()).is_err() || !ids.insert(entry.id().to_string()) {
            return Err(format!("{plural} need a unique stable identity."));
        }
        if entry.title().trim().is_empty() || !titles.insert(entry.title().trim().to_lowercase()) {
            return Err(format!("{plural} need a unique, nonempty title."));
        }
        crate::policy::validate_configuration_text(entry.title())?;
        crate::policy::validate_configuration_text(entry.body())?;
    }
    Ok(ids)
}

/// Compare-and-save one library entry by id. A rename is a title change only:
/// Agents reference the id and are never rewritten.
fn replace_entry<T: LibraryEntry>(
    library: &mut Vec<T>,
    id: &str,
    expected: Option<&T>,
    value: Option<T>,
    referenced: bool,
    noun: &str,
    conflict: &str,
) -> Result<(), String> {
    let index = library.iter().position(|e| e.id() == id);
    if index.map(|i| &library[i]) != expected {
        return Err(conflict.into());
    }
    if value.as_ref().is_some_and(|v| v.id() != id) {
        return Err(format!("A {noun}'s stable identity cannot be changed."));
    }
    let in_use =
        "is used by an Agent. Remove or replace its references explicitly before deleting it.";
    match (index, value) {
        (None, None) => Err(format!(
            "This {noun} is not saved. Cancel its draft instead."
        )),
        (Some(_), None) if referenced => Err(format!("This {noun} {in_use}")),
        (Some(index), None) => {
            library.remove(index);
            Ok(())
        }
        (Some(index), Some(value)) => {
            library[index] = value;
            Ok(())
        }
        (None, Some(value)) => {
            library.push(value);
            Ok(())
        }
    }
}

/// Give every id-less library entry a fresh UUID v4. True when any changed.
fn assign_ids<T: LibraryEntry>(entries: &mut [T]) -> bool {
    let mut changed = false;
    for entry in entries.iter_mut().filter(|e| e.id().is_empty()) {
        *entry.id_mut() = uuid::Uuid::new_v4().to_string();
        changed = true;
    }
    changed
}

/// A reusable review profile: model + ordered doctrines + optional Voice +
/// prompt + signature. Referenced by id from repository assignments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub model: String,
    /// Absent on legacy profiles until Intelligence or account/model is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intelligence: Option<AgentIntelligence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_account: Option<AiAccount>,
    /// Legacy title references (single, then ordered list). Converted to
    /// `doctrine_ids` on load; kept only in recorded job evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doctrine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doctrines: Option<Vec<String>>,
    /// Ordered Doctrine ids. A reference with no library entry is unavailable
    /// and stays until the user removes it; migrated unmatched titles keep
    /// their title as the reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub doctrine_ids: Vec<String>,
    /// Zero or one Voice id. Absent on Agents that predate Voices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    pub prompt: String,
    pub signature: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentIntelligence {
    pub reasoning_effort: Option<String>,
    pub context_tier: Option<String>,
}

/// Stable connection reference, never a credential or mutable login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiAccount {
    pub provider: String,
    pub account_id: String,
}

/// Permissions are scoped to this assignment, never to the reusable Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub id: String,
    pub agent_id: String,
    /// Legacy assignment schedule: readable, never consulted or written anew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    pub comment: bool,
    #[serde(default)]
    pub approve: bool,
    /// Only explicit vNext opt-ins. The legacy `approve` flag is never a grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<ActionPermissions>,
    /// Packet tool ids (`PACKET_TOOLS`). None = the role default (`default_tools`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// Rubber Duck Agent id. Honored on the primary only; inert elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rubber_duck: Option<String>,
}

/// Role default tool set: investigation tools, the granted exits, then `panic_exit`.
/// Primary: `ready_for_human` and `wait`, `leave_comments`/`approve_*` per grant.
/// Non-primary: `submit_report`. Mirrored by `defaultTools` in `src/settings.ts`.
pub fn default_tools(authority: &AssignmentAuthority) -> Vec<String> {
    let mut tools: Vec<&str> = crate::review::tools::INVESTIGATION.to_vec();
    if authority.primary {
        if authority.comment {
            tools.push("leave_comments");
        }
        tools.push("ready_for_human");
        tools.push("wait");
        if authority.approve {
            tools.push("approve_pull_request");
        }
        if authority.merge {
            tools.push("approve_and_merge");
        }
    } else {
        tools.push("submit_report");
    }
    tools.push("panic_exit");
    tools.into_iter().map(String::from).collect()
}

/// Investigation tools, `rubber_duck`, then exits. Mirrored by `PACKET_TOOLS` in `src/policy.ts`.
pub const PACKET_TOOLS: [&str; 15] = [
    "read_changes",
    "read_source",
    "get_thread",
    "get_checks",
    "read_check_log",
    "list_tree",
    "search_code",
    crate::review::duck::TOOL,
    "leave_comments",
    "ready_for_human",
    "wait",
    "approve_pull_request",
    "approve_and_merge",
    "panic_exit",
    "submit_report",
];

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPermissions {
    #[serde(default)]
    pub reply: bool,
    pub approve: bool,
    pub merge: bool,
    /// Approve & Merge may count the user's own PRs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentAuthority {
    pub primary: bool,
    pub comment: bool,
    #[serde(default)]
    pub reply: bool,
    pub approve: bool,
    pub merge: bool,
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
    pub doctrine_catalog: DoctrineCatalog,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalPreferences {
    pub defaults: Policy,
    pub capacity: u32,
    #[serde(default = "default_review_inactivity_minutes")]
    pub review_inactivity_minutes: u32,
    pub time_zone: String,
    pub root_folder: Option<String>,
    pub presets: Vec<ReviewPreset>,
    pub default_review_preset: Option<String>,
    #[serde(default)]
    pub rubber_duck: DuckLimits,
}

pub const fn default_review_inactivity_minutes() -> u32 {
    15
}

pub fn review_inactivity_duration(minutes: u32) -> Result<std::time::Duration, String> {
    if minutes == 0 {
        return Err("Review inactivity must be positive whole minutes.".into());
    }
    let duration = std::time::Duration::from_secs(u64::from(minutes) * 60);
    std::time::Instant::now()
        .checked_add(duration)
        .ok_or("Review inactivity duration overflows the system clock.")?;
    Ok(duration)
}

/// Saved Preferences bounding Rubber Duck use per primary attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuckLimits {
    pub max_calls: u32,
    pub call_timeout_seconds: u32,
    pub primary_window_minutes: u32,
}

impl DuckLimits {
    pub const MAX_CALLS: std::ops::RangeInclusive<u32> = 1..=32;
    pub const CALL_TIMEOUT_SECONDS: std::ops::RangeInclusive<u32> = 30..=600;
    pub const PRIMARY_WINDOW_MINUTES: std::ops::RangeInclusive<u32> = 5..=120;

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<(), String> {
        if !Self::MAX_CALLS.contains(&self.max_calls)
            || !Self::CALL_TIMEOUT_SECONDS.contains(&self.call_timeout_seconds)
            || !Self::PRIMARY_WINDOW_MINUTES.contains(&self.primary_window_minutes)
        {
            return Err("Rubber duck limits: calls 1-32, call timeout 30-600 seconds, primary window 5-120 minutes.".into());
        }
        Ok(())
    }
}

impl Default for DuckLimits {
    fn default() -> Self {
        Self {
            max_calls: 8,
            call_timeout_seconds: 180,
            primary_window_minutes: 15,
        }
    }
}

/// Compare-and-save only the addressed resource. None means create/delete,
/// not an instruction to replace the rest of the Settings window.
// A short-lived command value, one per save; boxing would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceEdit {
    Agent {
        id: String,
        expected: Option<Agent>,
        value: Option<Agent>,
    },
    Doctrine {
        id: String,
        expected: Option<Doctrine>,
        value: Option<Doctrine>,
    },
    Voice {
        id: String,
        expected: Option<Voice>,
        value: Option<Voice>,
    },
    Repository {
        id: String,
        expected: Option<Box<Repository>>,
        value: Option<Box<Repository>>,
    },
    Preferences {
        expected: GlobalPreferences,
        value: GlobalPreferences,
    },
}

#[derive(Debug, Serialize)]
pub struct RepositoryReadiness {
    pub repository_id: String,
    pub primary_assignment_id: Option<String>,
    pub assignments: Vec<(String, AssignmentAuthority)>,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ResourceReadiness {
    pub configuration_ready: bool,
    pub issues: Vec<String>,
    pub repositories: Vec<RepositoryReadiness>,
}

#[derive(Debug, Serialize)]
pub struct SavedResources {
    pub settings: Settings,
    /// Saved configuration only; account verification, scope confirmation
    /// and provider capabilities remain separate execution gates.
    pub readiness: ResourceReadiness,
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
    /// Five-field cron, evaluated in `Settings::time_zone`.
    #[serde(default = "default_cron")]
    pub cron: String,
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
    /// Ordered admission rules. Absent keeps the legacy watched-author,
    /// reviewer-assignment and mention behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<AdmissionConfig>,
    /// Legacy assignment schedules remain readable but have no vNext editor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<Assignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_assignment_id: Option<String>,
    /// Include draft pull requests in review.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub review_drafts: bool,
}

/// Committed with configuration, not with the recoverable polling cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryAuthorization {
    pub version: String,
    pub name: String,
    pub account_id: String,
    pub repository_id: String,
}

impl RepositoryAuthorization {
    pub fn matches(&self, repository: &Repository) -> bool {
        repository.enabled
            && repository.provider == ProviderId::Github
            && self.name == repository.name
            && Some(&self.account_id) == repository.provider_account_id.as_ref()
            && Some(&self.repository_id) == repository.provider_repository_id.as_ref()
    }
}

impl Settings {
    pub fn doctrine_catalog(&self) -> DoctrineCatalog {
        use sha2::{Digest, Sha256};
        // Title and body only, as before ids: stable across id assignment.
        #[derive(Serialize)]
        struct Content<'a> {
            title: &'a str,
            body: &'a str,
        }
        let revision = |doctrines: &[Doctrine]| {
            let content: Vec<_> = doctrines
                .iter()
                .map(|d| Content {
                    title: &d.title,
                    body: &d.body,
                })
                .collect();
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&content).expect("Doctrine strings serialize"))
            )
        };
        DoctrineCatalog {
            source: "config/settings.json; defaults: src-tauri/doctrines".into(),
            source_revision: revision(&crate::seeds::doctrines()),
            effective_revision: revision(&self.doctrines),
            count: self.doctrines.len(),
        }
    }

    fn materialize_presets(&mut self) {
        if let Some(id) = &self.default_review_preset {
            self.defaults.prompt = self
                .presets
                .iter()
                .find(|p| &p.id == id)
                .unwrap()
                .body
                .clone();
        }
        for repository in &mut self.repositories {
            if let Some(id) = &repository.review_preset {
                repository.overrides.prompt = Some(
                    self.presets
                        .iter()
                        .find(|p| &p.id == id)
                        .unwrap()
                        .body
                        .clone(),
                );
            }
        }
    }

    pub fn global_preferences(&self) -> GlobalPreferences {
        GlobalPreferences {
            defaults: self.defaults.clone(),
            capacity: self.capacity,
            review_inactivity_minutes: self.review_inactivity_minutes,
            time_zone: self.time_zone.clone(),
            root_folder: self.root_folder.clone(),
            presets: self.presets.clone(),
            default_review_preset: self.default_review_preset.clone(),
            rubber_duck: self.rubber_duck,
        }
    }

    pub fn readiness(&self) -> ResourceReadiness {
        let mut issues = Vec::new();
        if !self.repositories.iter().any(|r| r.enabled) {
            issues.push("Select at least one repository. Monitoring has not been enabled.".into());
        }
        let repositories: Vec<_> = self
            .repositories
            .iter()
            .map(|repository| {
                let mut issues = Vec::new();
                if repository.provider != ProviderId::Github
                    || repository.account_binding().is_none()
                {
                    issues.push("Bind a supported repository account.".into());
                }
                if repository.assignments.is_empty() {
                    issues.push("Assign at least one saved Agent.".into());
                }
                for assignment in &repository.assignments {
                    if self
                        .agents
                        .iter()
                        .find(|a| a.id == assignment.agent_id)
                        .is_none_or(|a| a.ai_account.is_none() || a.model.trim().is_empty())
                    {
                        issues.push(format!(
                            "Assignment {} needs an explicit AI account and model.",
                            assignment.id
                        ));
                    }
                }
                for assignment in &repository.assignments {
                    let Some(duck) = repository.assignment_duck(assignment) else {
                        continue;
                    };
                    if self
                        .agents
                        .iter()
                        .find(|a| a.id == duck)
                        .is_none_or(|a| a.ai_account.is_none() || a.model.trim().is_empty())
                    {
                        issues.push(format!(
                            "Assignment {} Rubber duck needs an explicit AI account and model.",
                            assignment.id
                        ));
                    }
                }
                for assignment in &repository.assignments {
                    let Some(tools) = &assignment.tools else {
                        continue;
                    };
                    issues.extend(tool_issues(
                        assignment,
                        repository.primary_assignment_id() == Some(assignment.id.as_str()),
                        tools,
                    ));
                }
                RepositoryReadiness {
                    repository_id: repository.id.clone(),
                    primary_assignment_id: repository.primary_assignment_id().map(str::to_string),
                    assignments: repository
                        .assignments
                        .iter()
                        .map(|a| (a.id.clone(), repository.assignment_authority(a)))
                        .collect(),
                    issues,
                }
            })
            .collect();
        let configuration_ready = issues.is_empty()
            && repositories.iter().all(|r| {
                !self
                    .repositories
                    .iter()
                    .any(|item| item.id == r.repository_id && item.enabled)
                    || r.issues.is_empty()
            });
        ResourceReadiness {
            configuration_ready,
            issues,
            repositories,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.admission_migration_version != ADMISSION_MIGRATION_VERSION {
            return Err(
                "Unsupported admission migration version. Use a compatible application before saving."
                    .into(),
            );
        }
        if self.doctrine_catalog_version != doctrine_catalog_version() {
            return Err(
                "Unsupported doctrine catalog version. Use a compatible application before saving."
                    .into(),
            );
        }
        if self.voice_catalog_version != voice_catalog_version() {
            return Err(
                "Unsupported voice catalog version. Use a compatible application before saving."
                    .into(),
            );
        }
        self.defaults.validate()?;
        if self.time_zone.parse::<chrono_tz::Tz>().is_err() {
            return Err(
                "Enter an explicit IANA time zone, such as UTC or America/New_York.".into(),
            );
        }
        if self.capacity == 0 {
            return Err("AI capacity must be a positive whole number.".into());
        }
        review_inactivity_duration(self.review_inactivity_minutes)?;
        self.rubber_duck.validate()?;
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
        library_ids(&self.doctrines, "Doctrines")?;
        library_ids(&self.voices, "Voices")?;
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
            // Unavailable references may persist (shown, never dropped); see
            // `Store::validate_resource` for the rule that they cannot be added.
            if agent.doctrine.is_some() || agent.doctrines.is_some() {
                return Err("Agent doctrine references must use ids.".into());
            }
            let mut selected = HashSet::new();
            if agent
                .doctrine_ids
                .iter()
                .any(|id| id.trim().is_empty() || !selected.insert(id))
                || agent
                    .voice_id
                    .as_ref()
                    .is_some_and(|id| id.trim().is_empty())
            {
                return Err("Selected doctrines must not repeat. Repair the Agent's doctrine references before saving.".into());
            }
            crate::policy::validate_configuration_text(&agent.name)?;
            crate::policy::validate_configuration_text(&agent.model)?;
            if let Some(intelligence) = &agent.intelligence {
                for value in [&intelligence.reasoning_effort, &intelligence.context_tier]
                    .into_iter()
                    .flatten()
                {
                    if value.trim().is_empty() || value.trim() != value {
                        return Err("Intelligence overrides must be nonempty advertised values; use Provider default instead.".into());
                    }
                    crate::policy::validate_configuration_text(value)?;
                }
            }
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
            crate::schedule::validate_cron(&repository.cron)
                .map_err(|error| format!("{}: {error}", repository.name))?;
            repository.overrides.effective(&self.defaults).validate()?;
            if let Some(admission) = &repository.admission {
                admission.validate()?;
            }
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
                if assignment
                    .rubber_duck
                    .as_ref()
                    .is_some_and(|id| !agent_ids.contains(id))
                {
                    return Err(
                        "The Rubber duck Agent no longer exists. Choose a local agent.".into(),
                    );
                }
                if let Some(schedule) = &assignment.schedule {
                    schedule.validate()?;
                }
                if assignment
                    .tools
                    .iter()
                    .flatten()
                    .any(|tool| !PACKET_TOOLS.contains(&tool.as_str()))
                {
                    return Err("Unknown packet tool.".into());
                }
                if repository.primary_assignment_id() != Some(assignment.id.as_str())
                    && assignment
                        .actions
                        .as_ref()
                        .is_some_and(|a| a.reply || a.approve || a.merge)
                {
                    return Err("Secondary assignments can only Publish Comment. Clear Reply, Approve and Merge permissions before saving.".into());
                }
                if assignment
                    .actions
                    .as_ref()
                    .is_some_and(|a| a.merge && !a.approve)
                {
                    return Err(
                        "Approve & Merge includes approval. Merge-only is not a valid permission."
                            .into(),
                    );
                }
            }
            if repository
                .primary_assignment_id
                .as_ref()
                .is_some_and(|id| !assignment_ids.contains(id))
            {
                return Err("The primary assignment no longer exists. Select a primary or clear the selection.".into());
            }
        }
        Ok(())
    }

    /// Moves a legacy global schedule onto every repository; its zone becomes
    /// the saved time zone. A cron that can run becomes each repository's
    /// cron. A legacy interval (it never polled) or a cron with no occurrence
    /// leaves every repository paused (monitoring off) on the default cron.
    /// An unparseable legacy schedule is rejected, never silently dropped.
    /// True when one was moved.
    pub fn migrate_global_schedule(&mut self) -> Result<bool, String> {
        if let Some(schedule) = &self.defaults.schedule {
            schedule.validate()?;
        }
        let Some(schedule) = self.defaults.schedule.take() else {
            return Ok(false);
        };
        let (cron, timezone) = match schedule {
            Schedule::Cron {
                expression,
                timezone,
            } if crate::schedule::validate_cron(&expression).is_ok() => {
                (Some(expression), timezone)
            }
            Schedule::Cron { timezone, .. } | Schedule::Interval { timezone, .. } => {
                (None, timezone)
            }
        };
        for repository in &mut self.repositories {
            match &cron {
                Some(cron) => repository.cron.clone_from(cron),
                None => {
                    repository.cron = default_cron();
                    repository.enabled = false;
                }
            }
        }
        self.time_zone = timezone;
        Ok(true)
    }

    pub fn effective_policy(&self, id: &str) -> Option<Policy> {
        self.repositories
            .iter()
            .find(|repository| repository.id == id)
            .map(|repository| repository.overrides.effective(&self.defaults))
    }
}

fn tool_issues(assignment: &Assignment, primary: bool, tools: &[String]) -> Vec<String> {
    let actions = assignment.actions.clone().unwrap_or_default();
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    for tool in tools {
        let id = tool.as_str();
        let problem = if !PACKET_TOOLS.contains(&id) {
            Some("is unknown")
        } else if !seen.insert(id) {
            Some("is duplicated")
        } else if matches!(
            id,
            "leave_comments"
                | "approve_pull_request"
                | "approve_and_merge"
                | "ready_for_human"
                | "wait"
        ) && !primary
        {
            Some("is primary-only")
        } else if id == "submit_report" && primary {
            Some("is non-primary only")
        } else if id == crate::review::duck::TOOL && !primary {
            Some("is primary-only")
        } else if id == crate::review::duck::TOOL && assignment.rubber_duck.is_none() {
            Some("needs a Rubber duck")
        } else if id == "leave_comments" && !assignment.comment {
            Some("needs Publish Comment")
        } else if id == "approve_pull_request" && !(actions.approve || actions.merge) {
            Some("needs Approve")
        } else if id == "approve_and_merge" && !actions.merge {
            Some("needs Approve & Merge")
        } else {
            None
        };
        if let Some(problem) = problem {
            issues.push(format!("Assignment {} tool {id} {problem}.", assignment.id));
        }
    }
    issues
}

impl Agent {
    /// Legacy title references in selection order.
    fn legacy_doctrine_titles(&self) -> Vec<&str> {
        match &self.doctrines {
            Some(titles) => titles.iter().map(String::as_str).collect(),
            None => self.doctrine.iter().map(String::as_str).collect(),
        }
    }

    /// References missing from the library: unavailable, never dropped.
    pub fn unavailable_references(&self, settings: &Settings) -> Vec<&str> {
        let doctrines = self
            .doctrine_ids
            .iter()
            .filter(|id| !settings.doctrines.iter().any(|d| &d.id == *id));
        let voice = self
            .voice_id
            .iter()
            .filter(|id| !settings.voices.iter().any(|v| &v.id == *id));
        doctrines.chain(voice).map(String::as_str).collect()
    }
}

impl Settings {
    /// Idempotent: ids for id-less library entries, then legacy Agent title
    /// references to ids (trimmed, case-insensitive). An unmatched title stays
    /// as its own reference, so it shows as unavailable. True when changed.
    pub fn migrate_library_ids(&mut self) -> bool {
        let mut changed = assign_ids(&mut self.doctrines) | assign_ids(&mut self.voices);
        for agent in &mut self.agents {
            if agent.doctrine.is_none() && agent.doctrines.is_none() {
                continue;
            }
            let mut ids: Vec<String> = Vec::new();
            for title in agent.legacy_doctrine_titles() {
                let id = self
                    .doctrines
                    .iter()
                    .find(|d| same_title(&d.title, title))
                    .map_or_else(|| title.to_string(), |d| d.id.clone());
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            // A legacy list never mixes with ids; ids written earlier win.
            if agent.doctrine_ids.is_empty() {
                agent.doctrine_ids = ids;
            }
            agent.doctrine = None;
            agent.doctrines = None;
            changed = true;
        }
        changed
    }
}

/// Legacy effective values: defaults plus overrides plus the repository list.
fn seeded_admission(repository: &Repository, defaults: &Policy) -> AdmissionConfig {
    let policy = repository.overrides.effective(defaults);
    AdmissionConfig::from_legacy(
        &crate::monitoring::effective_watched_authors(repository, &policy),
        policy.reviewer_assignment,
    )
}

/// One-time, idempotent: repositories without `admission` get it materialized
/// from their effective legacy values and monitoring is turned off (and its
/// authorization cleared) until a human reviews the result. Repositories that
/// already have `admission` are untouched and nothing is ever re-enabled.
fn migrate_admission_rules(settings: &mut Settings) -> bool {
    let defaults = settings.defaults.clone();
    let mut migrated = 0;
    for repository in &mut settings.repositories {
        if repository.admission.is_some() {
            continue;
        }
        repository.admission = Some(seeded_admission(repository, &defaults));
        repository.enabled = false;
        settings
            .repository_authorizations
            .insert(repository.id.clone(), None);
        migrated += 1;
    }
    migrated > 0
}

impl Repository {
    pub fn primary_assignment_id(&self) -> Option<&str> {
        if self.assignments.len() == 1 {
            Some(&self.assignments[0].id)
        } else {
            self.primary_assignment_id.as_deref()
        }
    }

    /// Configuration permission, not a provider capability or execution grant.
    pub fn assignment_authority(&self, assignment: &Assignment) -> AssignmentAuthority {
        let primary = self.primary_assignment_id() == Some(assignment.id.as_str());
        let assigned = self.assignments.iter().any(|item| item == assignment);
        AssignmentAuthority {
            primary: primary && assigned,
            comment: assigned && assignment.comment,
            reply: assigned && primary && assignment.actions.as_ref().is_some_and(|a| a.reply),
            approve: assigned
                && primary
                && assignment
                    .actions
                    .as_ref()
                    .is_some_and(|a| a.approve || a.merge),
            merge: assigned && primary && assignment.actions.as_ref().is_some_and(|a| a.merge),
        }
    }

    /// The tools an assignment runs with: its explicit set, else its role default.
    pub fn assignment_tools(&self, assignment: &Assignment) -> Vec<String> {
        match &assignment.tools {
            Some(tools) => tools.clone(),
            None => {
                let mut tools = default_tools(&self.assignment_authority(assignment));
                if self.assignment_duck(assignment).is_some() {
                    tools.insert(
                        crate::review::tools::INVESTIGATION.len(),
                        crate::review::duck::TOOL.into(),
                    );
                }
                tools
            }
        }
    }

    /// The assignment's Rubber Duck Agent id: primary only.
    pub fn assignment_duck<'a>(&self, assignment: &'a Assignment) -> Option<&'a str> {
        (self.primary_assignment_id() == Some(assignment.id.as_str()))
            .then_some(assignment.rubber_duck.as_deref())
            .flatten()
    }

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
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
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

/// Saved bytes by (directory, file name) in a frozen copy.
type FrozenFiles = BTreeMap<(String, String), std::sync::Arc<Vec<u8>>>;

/// Bytes a live frozen copy holds, by file, with the stamp they were read at;
/// concurrent copies of an unchanged file share them instead of re-reading.
type SharedFiles = BTreeMap<(String, String), (SharedStamp, std::sync::Weak<Vec<u8>>)>;
type SharedStamp = (u64, u64, std::time::SystemTime);

/// See `Store::stamp_of`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileStamp {
    writes: u64,
    files: Vec<Option<(u64, std::time::SystemTime)>>,
}

/// Saved-file counters; see `Store::generations`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counter {
    pub(crate) writes: u64,
    pub(crate) transitions: u64,
    fingerprint: Option<u64>,
}

pub struct Store {
    root: PathBuf,
    initial_time_zone: String,
    /// Per saved file: every write, and the writes that can give a
    /// coordinator work (transitions, not progress or bookkeeping).
    counters: std::sync::Mutex<BTreeMap<String, Counter>>,
    /// Read-only point-in-time copy for scans outside the Store lock.
    frozen: Option<std::sync::Arc<FrozenFiles>>,
    shared: std::sync::Mutex<SharedFiles>,
    #[cfg(test)]
    state_writes: std::sync::Mutex<BTreeMap<String, usize>>,
    #[cfg(test)]
    failed_state_write: std::sync::Mutex<Option<(String, usize)>>,
    #[cfg(test)]
    state_reads: std::sync::Mutex<BTreeMap<String, usize>>,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            initial_time_zone: "UTC".into(),
            counters: std::sync::Mutex::new(BTreeMap::new()),
            frozen: None,
            shared: std::sync::Mutex::new(BTreeMap::new()),
            #[cfg(test)]
            state_writes: std::sync::Mutex::new(BTreeMap::new()),
            #[cfg(test)]
            failed_state_write: std::sync::Mutex::new(None),
            #[cfg(test)]
            state_reads: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    /// Schedule zone written only when this store creates a brand-new profile;
    /// existing settings, including a saved UTC, are never changed.
    pub fn with_initial_time_zone(mut self, zone: String) -> Self {
        self.initial_time_zone = zone;
        self
    }

    #[cfg(test)]
    pub(crate) fn take_state_reads(&self) -> BTreeMap<String, usize> {
        std::mem::take(&mut *self.state_reads.lock().unwrap())
    }

    #[cfg(test)]
    pub(crate) fn take_state_writes(&self) -> BTreeMap<String, usize> {
        std::mem::take(&mut *self.state_writes.lock().unwrap())
    }

    /// Per-file write and transition counters since this Store opened. Idle
    /// scheduler ticks compare them instead of rebuilding derived snapshots.
    pub(crate) fn generations(&self) -> BTreeMap<String, Counter> {
        self.counters
            .lock()
            .map(|counters| counters.clone())
            .unwrap_or_default()
    }

    /// Identity of the saved `names`: this Store's write count plus each
    /// file's size and modification time, so a rewrite through this Store or
    /// out of band changes it. `None` for a frozen copy or an unreadable stamp.
    pub(crate) fn stamp_of(&self, names: &[&str]) -> Option<FileStamp> {
        if self.frozen.is_some() {
            return None;
        }
        let writes = {
            let counters = self.counters.lock().ok()?;
            names
                .iter()
                .filter_map(|name| counters.get(*name))
                .map(|counter| counter.writes)
                .sum()
        };
        let mut files = Vec::with_capacity(names.len());
        for name in names {
            let directory = if *name == "settings.json" {
                "config"
            } else {
                "state"
            };
            files.push(
                match std::fs::metadata(self.root.join(directory).join(name)) {
                    Ok(metadata) => Some((metadata.len(), metadata.modified().ok()?)),
                    Err(error) if error.kind() == ErrorKind::NotFound => None,
                    Err(_) => return None,
                },
            );
        }
        Some(FileStamp { writes, files })
    }

    /// Records a transition in `name` that its write alone did not signal.
    pub(crate) fn wake_file(&self, name: &str) {
        if let Ok(mut counters) = self.counters.lock() {
            counters.entry(name.into()).or_default().transitions += 1;
        }
    }

    /// Whether `fingerprint` equals the transition fingerprint of the last
    /// write to `name`, i.e. a new write would only record progress.
    pub(crate) fn same_transition(&self, name: &str, fingerprint: u64) -> bool {
        self.counters.lock().is_ok_and(|counters| {
            counters.get(name).and_then(|c| c.fingerprint) == Some(fingerprint)
        })
    }

    fn record_write(&self, name: &str, fingerprint: Option<u64>) {
        let Ok(mut counters) = self.counters.lock() else {
            return;
        };
        let counter = counters.entry(name.into()).or_default();
        counter.writes += 1;
        // Liveness probes and derived indexes are bookkeeping; they never
        // create work, so they must not wake the coordinators that wrote them.
        let bookkeeping = matches!(
            name,
            "connectivity.json"
                | "result-index.json"
                | "result-index-pending.json"
                | "queue-selection.json"
        );
        if !bookkeeping && (fingerprint.is_none() || counter.fingerprint != fingerprint) {
            counter.transitions += 1;
        }
        counter.fingerprint = fingerprint;
    }

    pub(crate) fn is_frozen(&self) -> bool {
        self.frozen.is_some()
    }

    /// A consistent read-only copy of saved state. Take it under the Store
    /// lock (cheap byte copies), then parse and project without holding it.
    pub(crate) fn freeze(&self) -> Result<Store, String> {
        if let Some(frozen) = &self.frozen {
            return Ok(self.frozen_with(frozen.clone()));
        }
        // Creates default settings first, as an ordinary read would.
        self.load_settings()?;
        let mut files = BTreeMap::new();
        let unreadable = |_| "Cannot snapshot local state.".to_string();
        let mut shared = self
            .shared
            .lock()
            .map_err(|_| "Cannot snapshot local state.".to_string())?;
        shared.retain(|_, (_, bytes)| bytes.strong_count() > 0);
        let key = ("config".to_string(), "settings.json".to_string());
        let settings = self
            .shared_read(&mut shared, &key, || {
                self.read_file("config", "settings.json")
            })
            .map_err(unreadable)?;
        files.insert(key, settings);
        let directory = self.root.join("state");
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries.collect::<Result<Vec<_>, _>>().map_err(unreadable)?,
            Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(unreadable(error)),
        };
        for entry in entries {
            let Some(name) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            if !name.ends_with(".json") || name.contains(".pr-sniper-write-") {
                continue;
            }
            let key = ("state".to_string(), name);
            match self.shared_read(&mut shared, &key, || {
                private_fs::read(&directory.join(&key.1))
            }) {
                Ok(bytes) => {
                    files.insert(key, bytes);
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(unreadable(error)),
            }
        }
        Ok(self.frozen_with(std::sync::Arc::new(files)))
    }

    /// `read`, or the bytes a live frozen copy already holds for this unchanged
    /// file (same write count through this Store, size and modification time).
    fn shared_read(
        &self,
        shared: &mut SharedFiles,
        key: &(String, String),
        read: impl FnOnce() -> std::io::Result<Vec<u8>>,
    ) -> std::io::Result<std::sync::Arc<Vec<u8>>> {
        let writes = self
            .counters
            .lock()
            .ok()
            .and_then(|counters| counters.get(&key.1).map(|c| c.writes))
            .unwrap_or(0);
        let stamp = std::fs::metadata(self.root.join(&key.0).join(&key.1))
            .and_then(|metadata| Ok((writes, metadata.len(), metadata.modified()?)))
            .ok();
        if let Some(stamp) = stamp {
            if let Some(bytes) = shared
                .get(key)
                .filter(|(seen, _)| *seen == stamp)
                .and_then(|(_, bytes)| bytes.upgrade())
            {
                return Ok(bytes);
            }
        }
        let bytes = std::sync::Arc::new(read()?);
        match stamp {
            Some(stamp) => {
                shared.insert(key.clone(), (stamp, std::sync::Arc::downgrade(&bytes)));
            }
            None => {
                shared.remove(key);
            }
        }
        Ok(bytes)
    }

    fn frozen_with(&self, files: std::sync::Arc<FrozenFiles>) -> Store {
        let mut view =
            Store::new(self.root.clone()).with_initial_time_zone(self.initial_time_zone.clone());
        view.frozen = Some(files);
        view
    }

    #[cfg(test)]
    pub(crate) fn frozen_bytes(&self, name: &str) -> Option<std::sync::Arc<Vec<u8>>> {
        self.frozen
            .as_ref()?
            .get(&("state".to_string(), name.to_string()))
            .cloned()
    }

    /// Names of saved state files in this frozen copy.
    pub(crate) fn frozen_state_files(&self) -> Vec<&str> {
        self.frozen
            .iter()
            .flat_map(|files| files.keys())
            .filter(|(directory, _)| directory == "state")
            .map(|(_, name)| name.as_str())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn fail_state_write(&self, name: &str, occurrence: usize) {
        assert!(occurrence > 0);
        *self.failed_state_write.lock().unwrap() = Some((name.into(), occurrence));
    }

    pub fn load_queue_selection(&self) -> Result<Option<String>, String> {
        match self.read_state("queue-selection.json") {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                "The saved queue destination is invalid; select an item explicitly.".into()
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(_) => Err(
                "Cannot read the saved queue destination. Check local storage permissions.".into(),
            ),
        }
    }

    pub fn save_queue_selection(&self, id: Option<&str>) -> Result<(), String> {
        self.write_state("queue-selection.json", &id)
    }

    pub(crate) fn write_state<T: Serialize + ?Sized>(
        &self,
        name: &str,
        value: &T,
    ) -> Result<(), String> {
        #[cfg(test)]
        {
            let mut fault = self.failed_state_write.lock().unwrap();
            if let Some((target, remaining)) = fault.as_mut() {
                if target == name {
                    *remaining -= 1;
                    if *remaining == 0 {
                        *fault = None;
                        return Err(format!("Injected {name} write failure."));
                    }
                }
            }
        }
        self.write_state_as(name, value, None)
    }

    /// `write_state` carrying a transition fingerprint: an unchanged
    /// fingerprint records progress without waking coordinators.
    pub(crate) fn write_state_as<T: Serialize + ?Sized>(
        &self,
        name: &str,
        value: &T,
        fingerprint: Option<u64>,
    ) -> Result<(), String> {
        if self.frozen.is_some() {
            return Err("A read-only state snapshot cannot be saved.".into());
        }
        let directory = self
            .directory("state")
            .map_err(|_| "Cannot create monitoring state directory.".to_string())?;
        let bytes =
            serde_json::to_vec_pretty(value).map_err(|_| "Cannot encode monitoring state.")?;
        #[cfg(test)]
        {
            *self
                .state_writes
                .lock()
                .unwrap()
                .entry(name.to_string())
                .or_default() += 1;
        }
        let result = private_fs::replace(&directory.join(name), &bytes, "monitoring state");
        self.record_write(name, fingerprint);
        result
    }

    fn directory(&self, name: &str) -> std::io::Result<PathBuf> {
        private_fs::directory(&self.root)?;
        let directory = self.root.join(name);
        private_fs::directory(&directory)?;
        Ok(directory)
    }

    pub(crate) fn read_state(&self, name: &str) -> std::io::Result<Vec<u8>> {
        #[cfg(test)]
        {
            *self
                .state_reads
                .lock()
                .unwrap()
                .entry(name.to_string())
                .or_default() += 1;
        }
        self.read_file("state", name)
    }

    /// Recover abandoned operational stages before restoring workers, with
    /// exclusive access to this Store. Configuration is deliberately excluded.
    pub fn recover_state_writes(&self) -> Result<(), String> {
        match private_fs::existing_directory(&self.root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("Cannot access state for staged-write recovery.".into()),
        }
        private_fs::recover(&self.root.join("state"), None).map_err(|_| {
            "Cannot recover owned state stages; operational recovery is blocked.".into()
        })
    }

    fn read_file(&self, directory: &str, name: &str) -> std::io::Result<Vec<u8>> {
        if let Some(files) = &self.frozen {
            return files
                .get(&(directory.to_string(), name.to_string()))
                .map(|bytes| bytes.as_ref().clone())
                .ok_or_else(|| ErrorKind::NotFound.into());
        }
        private_fs::existing_directory(&self.root)?;
        let directory = self.root.join(directory);
        private_fs::existing_directory(&directory)?;
        let path = directory.join(name);
        match private_fs::read(&path) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        // A committed file needs no housekeeping or writer lock. Only absence
        // requires recovery before callers may infer a fresh/default resource.
        private_fs::recover(&directory, Some(name)).map_err(|error| {
            std::io::Error::other(format!("Cannot recover staged storage: {error}"))
        })?;
        private_fs::read(&path)
    }

    pub fn has_saved_settings(&self) -> bool {
        self.root.join("config/settings.json").exists()
    }

    pub fn saved_resources(&self) -> Result<SavedResources, String> {
        let settings = self.load_settings()?;
        let readiness = settings.readiness();
        Ok(SavedResources {
            settings,
            readiness,
        })
    }

    pub fn validate_resource(&self, edit: ResourceEdit) -> Result<Settings, String> {
        let mut settings = self.load_settings()?;
        let conflict = "Resource changed in another window. Your draft has not been written; reload or explicitly repair it before saving.";
        match edit {
            ResourceEdit::Agent {
                id,
                expected,
                value,
            } => {
                let current = settings.agents.iter().find(|a| a.id == id);
                if current != expected.as_ref() {
                    return Err(conflict.into());
                }
                if current.is_none() && value.is_none() {
                    return Err("This Agent is not saved. Cancel its draft instead.".into());
                }
                if value.as_ref().is_some_and(|a| a.id != id) {
                    return Err("An Agent's stable identity cannot be changed.".into());
                }
                // Keeping an unavailable reference is allowed; adding one is not.
                if let Some(value) = &value {
                    let kept = current
                        .map(|a| a.unavailable_references(&settings))
                        .unwrap_or_default();
                    if value
                        .unavailable_references(&settings)
                        .iter()
                        .any(|id| !kept.contains(id))
                    {
                        return Err("Selected doctrines and Voice must exist. Choose saved library entries.".into());
                    }
                }
                if value.is_none()
                    && settings.repositories.iter().any(|r| {
                        r.assignments.iter().any(|a| {
                            a.agent_id == id || a.rubber_duck.as_deref() == Some(id.as_str())
                        })
                    })
                {
                    return Err("This Agent is assigned to a repository. Remove or replace its assignments explicitly before deleting it.".into());
                }
                if let Some(index) = settings.agents.iter().position(|a| a.id == id) {
                    if let Some(value) = value {
                        settings.agents[index] = value;
                    } else {
                        settings.agents.remove(index);
                    }
                } else if let Some(value) = value {
                    settings.agents.push(value);
                }
            }
            ResourceEdit::Doctrine {
                id,
                expected,
                value,
            } => {
                let referenced = settings.agents.iter().any(|a| a.doctrine_ids.contains(&id));
                replace_entry(
                    &mut settings.doctrines,
                    &id,
                    expected.as_ref(),
                    value,
                    referenced,
                    "doctrine",
                    conflict,
                )?;
            }
            ResourceEdit::Voice {
                id,
                expected,
                value,
            } => {
                let referenced = settings
                    .agents
                    .iter()
                    .any(|a| a.voice_id.as_ref() == Some(&id));
                replace_entry(
                    &mut settings.voices,
                    &id,
                    expected.as_ref(),
                    value,
                    referenced,
                    "voice",
                    conflict,
                )?;
            }
            ResourceEdit::Repository {
                id,
                expected,
                value,
            } => {
                let current = settings.repositories.iter().find(|r| r.id == id);
                if current != expected.as_deref() {
                    return Err(conflict.into());
                }
                if current.is_none() && value.is_none() {
                    return Err("This repository is not saved. Cancel its draft instead.".into());
                }
                if value.as_ref().is_some_and(|r| r.id != id) {
                    return Err(
                        "A repository configuration's stable identity cannot be changed.".into(),
                    );
                }
                if let Some(index) = settings.repositories.iter().position(|r| r.id == id) {
                    if let Some(value) = value {
                        settings.repositories[index] = *value;
                    } else {
                        settings.repositories.remove(index);
                    }
                } else if let Some(value) = value {
                    let mut value = *value;
                    if value.admission.is_none() {
                        value.admission = Some(seeded_admission(&value, &settings.defaults));
                    }
                    settings.repositories.push(value);
                }
            }
            ResourceEdit::Preferences { expected, value } => {
                if settings.global_preferences() != expected {
                    return Err(conflict.into());
                }
                if value.defaults.schedule.is_some() {
                    return Err("Schedules belong to repositories.".into());
                }
                settings.defaults = value.defaults;
                settings.capacity = value.capacity;
                settings.review_inactivity_minutes = value.review_inactivity_minutes;
                settings.time_zone = value.time_zone;
                settings.root_folder = value.root_folder;
                settings.presets = value.presets;
                settings.default_review_preset = value.default_review_preset;
                settings.rubber_duck = value.rubber_duck;
            }
        }
        settings.validate()?;
        settings.materialize_presets();
        Ok(settings)
    }

    pub fn save_resource(&self, edit: ResourceEdit) -> Result<Settings, String> {
        let enabled_repository_id = match &edit {
            ResourceEdit::Repository {
                id,
                value: Some(repository),
                ..
            } if repository.enabled => Some(id.clone()),
            _ => None,
        };
        let edited_existing_repository = match &edit {
            ResourceEdit::Repository {
                id,
                expected: Some(_),
                ..
            } => Some(id.clone()),
            _ => None,
        };
        let unchanged_repository = matches!(&edit,
            ResourceEdit::Repository { expected: Some(expected), value: Some(value), .. }
                if expected == value);
        let authorizing_id = match &edit {
            ResourceEdit::Repository {
                id,
                value: Some(value),
                ..
            } if value.enabled
                && value.provider == ProviderId::Github
                && value.account_binding().is_some()
                && !value.assignments.is_empty() =>
            {
                Some(id.clone())
            }
            _ => None,
        };
        let mut settings = self.validate_resource(edit)?;
        if let Some(id) = enabled_repository_id {
            let repository = settings
                .repositories
                .iter()
                .find(|repository| repository.id == id)
                .unwrap();
            if repository.provider != ProviderId::Github || repository.account_binding().is_none() {
                return Err(format!(
                    "{}: bind a supported GitHub account and repository before enabling monitoring.",
                    repository.name
                ));
            }
            if repository.assignments.is_empty() {
                return Err(format!(
                    "{}: assign at least one saved Agent before enabling repository monitoring.",
                    repository.name
                ));
            }
            if repository.assignments.iter().any(|assignment| {
                settings
                    .agents
                    .iter()
                    .find(|agent| agent.id == assignment.agent_id)
                    .is_none_or(|agent| agent.ai_account.is_none() || agent.model.trim().is_empty())
            }) {
                return Err(format!("{}: choose an explicit AI account and model for every assigned Agent before enabling repository monitoring.", repository.name));
            }
        }
        for (id, authorization) in &mut settings.repository_authorizations {
            if !settings
                .repositories
                .iter()
                .any(|r| r.id == *id && authorization.as_ref().is_some_and(|a| a.matches(r)))
            {
                *authorization = None;
            }
        }
        if authorizing_id.is_none() {
            if let Some(id) = edited_existing_repository {
                // A null receipt revokes legacy polling-cache authority even
                // when that cache cannot be written during this Save.
                settings.repository_authorizations.insert(id, None);
            }
        }
        if let Some(id) = authorizing_id.filter(|id| {
            !unchanged_repository
                || settings
                    .repository_authorizations
                    .get(id)
                    .is_none_or(Option::is_none)
        }) {
            let repository = settings.repositories.iter().find(|r| r.id == id).unwrap();
            let binding = repository
                .account_binding()
                .ok_or("Bind a connected GitHub account before enabling repository monitoring.")?;
            settings.repository_authorizations.insert(
                id,
                Some(RepositoryAuthorization {
                    version: uuid::Uuid::new_v4().to_string(),
                    name: repository.name.clone(),
                    account_id: binding.account.account_id,
                    repository_id: binding.repository.repository_id,
                }),
            );
        }
        // Callers hold the existing Host Store mutex across compare and commit.
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn save_preferences(
        &self,
        mut settings: Settings,
        expected: &Settings,
    ) -> Result<Settings, String> {
        // Whole-settings saves may carry pre-id library shapes.
        settings.migrate_library_ids();
        settings.migrate_global_schedule()?;
        let current = self.load_settings()?;
        if &current != expected {
            return Err("Settings changed in another window. Reload Settings before saving; your draft has not been written.".into());
        }
        if settings.launch_at_login != current.launch_at_login {
            return Err("Change launch at login using the separate startup control.".into());
        }
        if settings.repository_authorizations != current.repository_authorizations {
            return Err("Repository authorization changes require saving that repository.".into());
        }
        for old in &current.repositories {
            if !settings.repositories.iter().any(|r| {
                r.id == old.id
                    && (r.enabled || !old.enabled)
                    && r.name == old.name
                    && r.account_binding() == old.account_binding()
            }) {
                settings
                    .repository_authorizations
                    .insert(old.id.clone(), None);
            }
        }
        settings.validate()?;
        settings.materialize_presets();
        self.save_settings(&settings)?;
        Ok(settings)
    }

    pub fn load_settings(&self) -> Result<Settings, String> {
        let bytes = match self.read_file("config", "settings.json") {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let mut settings = Settings {
                    doctrines: crate::seeds::doctrines(),
                    voices: crate::seeds::voices(),
                    ..Settings::default()
                };
                settings.time_zone.clone_from(&self.initial_time_zone);
                settings.validate()?;
                self.write_settings(&settings)?;
                return Ok(settings);
            }
            Err(_) => return Err("Cannot read settings or recover staged settings writes. Check local file permissions; committed data was not rolled back.".into()),
        };
        let mut settings: Settings = serde_json::from_slice(&bytes)
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        #[derive(Deserialize)]
        struct SavedCatalogVersion {
            #[serde(default)]
            doctrine_catalog_version: Option<u32>,
            #[serde(default)]
            admission_migration_version: Option<u32>,
            #[serde(default)]
            voice_catalog_version: Option<u32>,
            #[serde(default)]
            time_zone: Option<serde::de::IgnoredAny>,
        }
        // API-created settings default to the current version; only unversioned
        // on-disk pre-alpha settings need the one-time reconciliation.
        let saved_version: SavedCatalogVersion = serde_json::from_slice(&bytes)
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        let reconcile_doctrines = saved_version.doctrine_catalog_version.is_none();
        let migrate_admission = saved_version.admission_migration_version.is_none();
        // Files without a saved time zone predate repository schedules; a file
        // with no saved global schedule ran on the then-default cron. Only a
        // real change is written, like the admission migration.
        let saved_schedule = settings.defaults.schedule.is_some();
        let schedules = |settings: &Settings| {
            let repositories: Vec<_> = settings
                .repositories
                .iter()
                .map(|r| (r.cron.clone(), r.enabled))
                .collect();
            (settings.time_zone.clone(), repositories)
        };
        let unmigrated = schedules(&settings);
        if saved_version.time_zone.is_none() {
            settings
                .defaults
                .schedule
                .get_or_insert_with(|| Schedule::Cron {
                    expression: crate::schedule::LEGACY_DEFAULT_CRON.into(),
                    timezone: "UTC".into(),
                });
        }
        settings
            .migrate_global_schedule()
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        let migrated_schedule = saved_schedule || schedules(&settings) != unmigrated;
        // One-time seed for files that predate Voices; never a top-up.
        let seed_voices = saved_version.voice_catalog_version.is_none();
        if seed_voices {
            for seed in crate::seeds::voices() {
                if !settings
                    .voices
                    .iter()
                    .any(|v| same_title(&v.title, &seed.title))
                {
                    settings.voices.push(seed);
                }
            }
        }
        if reconcile_doctrines {
            let canonical = crate::seeds::doctrines();
            let mut removed_references = 0;
            let valid = |title: &str| {
                canonical
                    .iter()
                    .any(|d| d.title.trim().eq_ignore_ascii_case(title.trim()))
            };
            for agent in &mut settings.agents {
                if let Some(titles) = &mut agent.doctrines {
                    titles.retain(|title| {
                        let keep = valid(title);
                        removed_references += usize::from(!keep);
                        keep
                    });
                }
                if agent.doctrine.as_deref().is_some_and(|title| !valid(title)) {
                    agent.doctrine = None;
                    removed_references += 1;
                }
            }
            let content = |d: &Doctrine| (d.title.clone(), d.body.clone());
            if !settings
                .doctrines
                .iter()
                .map(content)
                .eq(canonical.iter().map(content))
                || removed_references > 0
            {
                settings.doctrine_reset = Some(DoctrineReset {
                    previous_count: settings.doctrines.len(),
                    removed_references,
                });
            }
            settings.doctrines = canonical;
        }
        // Reconcile only current configuration. Historical execution and provider
        // receipts are separate resources and must never be rewritten.
        let mut reconciled_permissions = false;
        for repository in &mut settings.repositories {
            let primary = repository.primary_assignment_id().map(str::to_owned);
            for assignment in &mut repository.assignments {
                if let Some(actions) = &mut assignment.actions {
                    if primary.as_deref() != Some(&assignment.id) {
                        if actions.reply || actions.approve || actions.merge {
                            *actions = ActionPermissions::default();
                            reconciled_permissions = true;
                        }
                    } else if actions.merge && !actions.approve {
                        actions.approve = true;
                        reconciled_permissions = true;
                    }
                }
            }
        }
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
        if reconciled_permissions {
            settings.capability_notice = Some("Repository permissions reconciled: obsolete secondary Reply/Approve/Merge grants disabled; primary Approve & Merge includes approval. Primary selection grants no capabilities. Existing job history and provider receipts are unchanged.".into());
        }
        // Files without repositories to migrate are left byte-for-byte alone;
        // every later save stamps the version.
        let migrated_admission = migrate_admission && migrate_admission_rules(&mut settings);
        // After the pre-alpha reconciliation, which still reads legacy titles.
        let migrated_ids = settings.migrate_library_ids();
        settings
            .validate()
            .map_err(|_| "Settings are invalid. Repair config/settings.json before saving.")?;
        if migrated
            || reconcile_doctrines
            || seed_voices
            || migrated_ids
            || reconciled_permissions
            || migrated_admission
            || migrated_schedule
        {
            self.write_settings(&settings)?;
        }
        Ok(settings)
    }

    pub fn save_theme(&self, theme: Theme) -> Result<(), String> {
        let mut settings = self.load_settings()?;
        settings.theme = theme;
        self.save_settings(&settings)
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        // Callers may still hand in pre-id shapes (fixtures, imports).
        let mut settings = settings.clone();
        settings.migrate_library_ids();
        settings.migrate_global_schedule()?;
        let settings = &settings;
        settings.validate()?;
        // Never silently replace unreadable or corrupt existing configuration.
        self.load_settings()?;
        self.write_settings(settings)
    }

    fn write_settings(&self, settings: &Settings) -> Result<(), String> {
        if self.frozen.is_some() {
            return Err("A read-only state snapshot cannot be saved.".into());
        }
        let directory = self
            .directory("config")
            .map_err(|_| "Cannot create configuration directory.".to_string())?;
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|_| "Cannot encode settings.".to_string())?;
        let result = private_fs::replace(&directory.join("settings.json"), &bytes, "settings");
        self.record_write("settings.json", None);
        result
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
            review_drafts: false,
            id: uuid::Uuid::new_v4().to_string(),
            provider: ProviderId::Github,
            name,
            enabled: true,
            cron: default_cron(),
            provider_account_id: None,
            legacy_installation_id: None,
            provider_repository_id: None,
            overrides: PolicyOverrides::default(),
            review_preset: None,
            watched_authors: Vec::new(),
            admission: None,
            assignments: Vec::new(),
            primary_assignment_id: None,
        });
        let created = settings
            .repositories
            .last_mut()
            .expect("repository was pushed");
        created.admission = Some(seeded_admission(created, &settings.defaults));
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
        SavedSettings {
            doctrine_catalog: settings.doctrine_catalog(),
            settings,
            warning,
        }
    }

    pub fn record(&self, event: DiagnosticEvent) -> Result<(), String> {
        if event == DiagnosticEvent::Attempt {
            return Err("Attempt diagnostics require a typed attempt record.".into());
        }
        let directory = self
            .directory("state")
            .map_err(|_| "Cannot create diagnostics directory.".to_string())?;
        let diagnostic = Diagnostic {
            timestamp_secs: diagnostic_time()?,
            event,
            attempt: None,
        };
        let mut bytes = serde_json::to_vec(&diagnostic)
            .map_err(|_| "Cannot encode diagnostics.".to_string())?;
        bytes.push(b'\n');
        let path = directory.join("diagnostics.jsonl");
        match private_fs::existing_file(&path).and_then(|_| fs::metadata(&path)) {
            Ok(metadata) if metadata.len() + bytes.len() as u64 > MAX_DIAGNOSTICS_BYTES => {
                private_fs::rotate(&path, &directory.join("diagnostics.previous.jsonl"))
                    .map_err(|_| "Cannot rotate diagnostics.".to_string())?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot inspect diagnostics.".into()),
        }
        private_fs::append(&path)
            .and_then(|mut file| file.write_all(&bytes).and_then(|_| file.sync_data()))
            .map_err(|_| "Cannot write diagnostics.".into())
    }

    pub fn diagnostics(&self) -> Result<Vec<Diagnostic>, String> {
        let contents = match self.read_state("diagnostics.jsonl") {
            Ok(contents) => String::from_utf8(contents).map_err(|_| "Cannot read diagnostics.")?,
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(_) => return Err("Cannot read diagnostics.".into()),
        };
        if contents.len() as u64 > MAX_DIAGNOSTICS_BYTES {
            return Err("Diagnostics exceed the supported size.".into());
        }
        let mut records = contents
            .lines()
            .map(|line| {
                let record: Diagnostic = serde_json::from_str(line)
                    .map_err(|_| "Diagnostics contain invalid data.".to_string())?;
                if record.attempt.is_some() || record.event == DiagnosticEvent::Attempt {
                    return Err("Host diagnostics contain invalid attempt data.".into());
                }
                Ok(record)
            })
            .collect::<Result<Vec<Diagnostic>, String>>()?;
        records.extend(self.attempt_diagnostics()?);
        records.sort_by_key(|record| record.timestamp_secs);
        Ok(records)
    }
}
