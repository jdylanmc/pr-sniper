use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Schedule {
    Interval {
        minutes: u32,
        timezone: String,
    },
    Cron {
        expression: String,
        timezone: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchedIdentity {
    pub id: String,
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Adapter {
    Copilot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", from = "SelectorInput")]
pub enum Selector {
    Default,
    Model { value: String },
    Agent { value: String },
}

// Serde's internally tagged unit variants otherwise ignore extra fields.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SelectorInput {
    Default {},
    Model { value: String },
    Agent { value: String },
}

impl From<SelectorInput> for Selector {
    fn from(value: SelectorInput) -> Self {
        match value {
            SelectorInput::Default {} => Self::Default,
            SelectorInput::Model { value } => Self::Model { value },
            SelectorInput::Agent { value } => Self::Agent { value },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Legacy global schedule. Moved onto every repository on load and on
    /// whole-settings saves (`Settings::migrate_global_schedule`); never written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    pub watched_authors: Vec<WatchedIdentity>,
    pub reviewer_assignment: bool,
    pub adapter: Adapter,
    pub selector: Selector,
    pub prompt: String,
    /// Legacy configuration/evidence only; eligible reviews always start automatically.
    pub automatic_agent_start: bool,
    pub automatic_comment_publication: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            schedule: None,
            watched_authors: Vec::new(),
            reviewer_assignment: true,
            adapter: Adapter::Copilot,
            selector: Selector::Default,
            prompt: "Review this pull request for actionable defects.".into(),
            automatic_agent_start: false,
            automatic_comment_publication: false,
        }
    }
}

impl Schedule {
    pub fn validate(&self) -> Result<(), String> {
        let timezone = match self {
            Schedule::Interval { minutes, timezone } => {
                if *minutes == 0 {
                    return Err("Interval must be a positive whole number of minutes.".into());
                }
                timezone
            }
            Schedule::Cron {
                expression,
                timezone,
            } => {
                if expression.split_whitespace().count() != 5
                    || expression.parse::<croner::Cron>().is_err()
                {
                    return Err(
                        "Enter a valid five-field cron expression (minute hour day month weekday)."
                            .into(),
                    );
                }
                timezone
            }
        };
        if timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err(
                "Enter an explicit IANA time zone, such as UTC or America/New_York.".into(),
            );
        }
        Ok(())
    }
}

fn contains_credential(value: &str) -> bool {
    [
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "-----BEGIN PRIVATE KEY",
        "-----BEGIN RSA PRIVATE KEY",
    ]
    .iter()
    .any(|prefix| value.contains(prefix))
}

pub fn validate_configuration_text(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("Review instructions and preset names must not be empty.".into());
    }
    if contains_credential(value) {
        return Err("Credentials and tokens must not be stored in configuration.".into());
    }
    Ok(())
}

impl Policy {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(schedule) = &self.schedule {
            schedule.validate()?;
        }
        let mut identities = HashSet::new();
        for identity in &self.watched_authors {
            let valid_id = identity
                .id
                .parse::<u64>()
                .ok()
                .filter(|id| *id > 0 && id.to_string() == identity.id);
            if valid_id.is_none() || !identities.insert(&identity.id) {
                return Err(
                    "Watched GitHub account IDs must be unique positive decimal numbers.".into(),
                );
            }
            if identity.login.trim().is_empty() || identity.login.chars().any(char::is_whitespace) {
                return Err("Each watched GitHub identity needs a nonempty login display label without whitespace.".into());
            }
            if contains_credential(&identity.login) {
                return Err("Credentials and tokens must not be stored in configuration.".into());
            }
        }
        if let Selector::Model { value } | Selector::Agent { value } = &self.selector {
            if value.trim().is_empty() {
                return Err(
                    "Enter a model or named-agent identifier, or select Adapter default.".into(),
                );
            }
            if contains_credential(value) {
                return Err("Credentials and tokens must not be stored in configuration.".into());
            }
        }
        if self.prompt.trim().is_empty() {
            return Err("Review prompt must not be empty.".into());
        }
        if contains_credential(&self.prompt) {
            return Err("Credentials and tokens must not be stored in configuration.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watched_authors: Option<Vec<WatchedIdentity>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_assignment: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<Adapter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<Selector>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Readable legacy preference, not an execution gate.
    pub automatic_agent_start: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automatic_comment_publication: Option<bool>,
}

impl PolicyOverrides {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub fn effective(&self, defaults: &Policy) -> Policy {
        Policy {
            schedule: self.schedule.clone().or_else(|| defaults.schedule.clone()),
            watched_authors: self
                .watched_authors
                .clone()
                .unwrap_or_else(|| defaults.watched_authors.clone()),
            reviewer_assignment: self
                .reviewer_assignment
                .unwrap_or(defaults.reviewer_assignment),
            adapter: self
                .adapter
                .clone()
                .unwrap_or_else(|| defaults.adapter.clone()),
            selector: self
                .selector
                .clone()
                .unwrap_or_else(|| defaults.selector.clone()),
            prompt: self
                .prompt
                .clone()
                .unwrap_or_else(|| defaults.prompt.clone()),
            automatic_agent_start: true,
            automatic_comment_publication: self
                .automatic_comment_publication
                .unwrap_or(defaults.automatic_comment_publication),
        }
    }
}

/// Every IANA zone `Schedule::validate` accepts, in alphabetical order.
pub fn time_zones() -> Vec<&'static str> {
    let mut zones: Vec<_> = chrono_tz::TZ_VARIANTS
        .iter()
        .map(|zone| zone.name())
        .collect();
    zones.sort_unstable();
    zones
}

/// Zone for a brand-new profile, std-only: a valid IANA `TZ` value (leading
/// `:` stripped), else the zone named by the `/etc/localtime` link target,
/// else UTC. Every candidate must parse as `chrono_tz::Tz`, the same authority
/// as `Schedule::validate`.
pub fn initial_time_zone(tz: Option<&str>, localtime: Option<&std::path::Path>) -> String {
    let valid = |zone: &str| {
        zone.parse::<chrono_tz::Tz>()
            .ok()
            .map(|zone| zone.name().to_string())
    };
    tz.map(|value| value.strip_prefix(':').unwrap_or(value))
        .and_then(valid)
        .or_else(|| {
            let target = localtime?.to_str()?;
            let (_, zone) = target.rsplit_once("zoneinfo/")?;
            valid(zone)
        })
        .unwrap_or_else(|| "UTC".into())
}

/// `initial_time_zone` for this host. Windows has no `/etc/localtime`; without
/// a valid `TZ` it falls back to UTC.
pub fn host_time_zone() -> String {
    let tz = std::env::var("TZ").ok();
    #[cfg(unix)]
    let localtime = std::fs::read_link("/etc/localtime").ok();
    #[cfg(not(unix))]
    let localtime: Option<std::path::PathBuf> = None;
    initial_time_zone(tz.as_deref(), localtime.as_deref())
}

#[cfg(test)]
mod time_zone_tests {
    use super::{initial_time_zone, time_zones, Schedule};
    use std::path::Path;

    #[test]
    fn time_zones_are_sorted_unique_and_each_validates() {
        let zones = time_zones();
        assert!(zones.windows(2).all(|pair| pair[0] < pair[1]));
        for zone in ["UTC", "America/New_York", "Asia/Tokyo", "Pacific/Auckland"] {
            assert!(zones.contains(&zone), "{zone}");
        }
        assert!(!zones.contains(&"Mars/Olympus_Mons"));
        for zone in zones {
            Schedule::Cron {
                expression: "*/15 * * * *".into(),
                timezone: zone.into(),
            }
            .validate()
            .unwrap();
        }
    }

    #[test]
    fn initial_time_zone_prefers_valid_tz_then_the_localtime_link_then_utc() {
        let mac = Path::new("/var/db/timezone/zoneinfo/America/New_York");
        let linux = Path::new("/usr/share/zoneinfo/Europe/Paris");
        let relative = Path::new("../usr/share/zoneinfo/Asia/Tokyo");
        let cases: [(Option<&str>, Option<&Path>, &str); 11] = [
            (Some("Asia/Tokyo"), Some(linux), "Asia/Tokyo"),
            (Some(":Pacific/Auckland"), None, "Pacific/Auckland"),
            (Some("Mars/Olympus_Mons"), Some(linux), "Europe/Paris"),
            (Some(""), Some(mac), "America/New_York"),
            (None, Some(mac), "America/New_York"),
            (None, Some(linux), "Europe/Paris"),
            (None, Some(relative), "Asia/Tokyo"),
            (None, Some(Path::new("/etc/custom-localtime")), "UTC"),
            (
                None,
                Some(Path::new("/usr/share/zoneinfo/Mars/Olympus_Mons")),
                "UTC",
            ),
            (Some("not a zone"), None, "UTC"),
            (None, None, "UTC"),
        ];
        for (tz, link, expected) in cases {
            assert_eq!(initial_time_zone(tz, link), expected, "{tz:?} {link:?}");
        }
    }
}
