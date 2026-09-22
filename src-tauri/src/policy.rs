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
    pub schedule: Schedule,
    pub watched_authors: Vec<WatchedIdentity>,
    pub reviewer_assignment: bool,
    pub adapter: Adapter,
    pub selector: Selector,
    pub prompt: String,
    pub automatic_agent_start: bool,
    pub automatic_comment_publication: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            schedule: Schedule::Interval {
                minutes: 15,
                timezone: "UTC".into(),
            },
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
        let timezone = match &self.schedule {
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
            schedule: self
                .schedule
                .clone()
                .unwrap_or_else(|| defaults.schedule.clone()),
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
            automatic_agent_start: self
                .automatic_agent_start
                .unwrap_or(defaults.automatic_agent_start),
            automatic_comment_publication: self
                .automatic_comment_publication
                .unwrap_or(defaults.automatic_comment_publication),
        }
    }
}
