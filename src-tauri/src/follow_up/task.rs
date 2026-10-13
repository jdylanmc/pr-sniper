//! Stored shape of a legacy Reply/Mention analysis. Read from old records only.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyDecision {
    Reply,
    Quiet,
    HumanInputRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub path: String,
    pub side: String,
    pub line: u64,
    pub quote: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyOutput {
    #[serde(default)]
    pub feedback_assessments: Vec<crate::feedback::Assessment>,
    pub decision: ReplyDecision,
    pub body: String,
    pub new_information: String,
    pub reason: String,
    pub evidence: Vec<Evidence>,
}
