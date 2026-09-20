use serde::{Deserialize, Serialize};

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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selector {
    Default,
    Model { value: String },
    Agent { value: String },
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
