mod cleanup;
mod paging;
mod reopening;

pub use cleanup::{maintain, recover};
pub use paging::{detail, page, DetailResult, Page, PageRequest};
pub(crate) use reopening::contexts_checked as retained_contexts_checked;
pub(crate) use reopening::known_key;
pub(crate) use reopening::{admit_observations, scan_origins, Observed};

use crate::{
    capacity::WorkId,
    github::threads::{Ownership, Provenance},
    monitoring::{JobOperation, QueueJob, TrackedPullRequest},
    publication::Publication,
    storage::Store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub provider: String,
    pub configuration_id: String,
    pub account_id: String,
    pub repository_id: String,
    pub pull_request_id: String,
}

impl Binding {
    pub fn job(job: &QueueJob) -> Self {
        Self {
            provider: job.provider.clone(),
            configuration_id: job.configuration_id.clone(),
            account_id: job.account_id.clone(),
            repository_id: job.repository_id.clone(),
            pull_request_id: job.pull_request_id.clone(),
        }
    }
    pub fn tracked(pr: &TrackedPullRequest) -> Self {
        Self {
            provider: pr.provider.clone(),
            configuration_id: pr.configuration_id.clone(),
            account_id: pr.account_id.clone(),
            repository_id: pr.repository_id.clone(),
            pull_request_id: pr.pull_request_id.clone(),
        }
    }
    pub fn matches(&self, job: &QueueJob) -> bool {
        *self == Self::job(job)
            || (job.configuration_id.is_empty()
                && job.work.is_none()
                && self.provider == job.provider
                && self.account_id == job.account_id
                && self.repository_id == job.repository_id
                && self.pull_request_id == job.pull_request_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedReceipt {
    pub proof: Ownership,
    /// Root bodies are not retained. Hints preserve the existing closed-file
    /// overlap guard, including a root first observed after reopening.
    pub hints: BTreeMap<String, (String, String)>,
    pub closed_roots: BTreeSet<String>,
    pub human_input_threads: BTreeSet<String>,
}

impl OwnedReceipt {
    pub fn from_publication(p: &Publication) -> Result<Self, String> {
        let proof = p.ownership().map_err(|_| {
            "Cannot compact published roots without verified original provenance.".to_string()
        })?;
        let mut hints = BTreeMap::new();
        for comment in &p
            .batch
            .as_ref()
            .ok_or("Original batch unavailable.")?
            .comments
        {
            let title = p
                .review
                .result
                .as_ref()
                .and_then(|r| {
                    r.output
                        .findings
                        .iter()
                        .find(|f| f.path == comment.path && comment.body.contains(&f.title))
                })
                .map(|f| f.title.clone())
                .unwrap_or_default();
            hints
                .entry(hash(&comment.body))
                .or_insert((comment.path.clone(), title));
        }
        Ok(Self {
            proof,
            hints,
            closed_roots: BTreeSet::new(),
            human_input_threads: BTreeSet::new(),
        })
    }
    pub fn matches(&self, job: &QueueJob) -> bool {
        job.provider == "github"
            && self.proof.account_id == job.account_id
            && self.proof.repository_id == job.repository_id
            && self.proof.pull_request_id == job.pull_request_id
            && (self.proof.configuration_id == job.configuration_id
                || self.proof.configuration_id.is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub scope: TrackedPullRequest,
    pub enqueue_watermark: u64,
    pub items: BTreeSet<String>,
    pub work: BTreeSet<WorkId>,
    pub completions: Vec<(WorkId, String)>,
    pub pass_ordinals: BTreeMap<String, u64>,
    pub reply_ordinals: BTreeMap<String, u64>,
    pub operations: Vec<JobOperation>,
    pub publications: Vec<(String, String, Vec<crate::publication::Receipt>)>,
    pub effects: Vec<EffectReceipt>,
    pub follow_up_keys: BTreeSet<String>,
    pub owned: Vec<OwnedReceipt>,
    pub notices: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectReceipt {
    pub id: String,
    pub item_id: String,
    pub action: crate::github::actions::Action,
    pub state: crate::actions::EffectState,
    pub operation_id: String,
    pub receipt: Option<crate::github::actions::Receipt>,
}

pub(crate) fn ordinal(
    store: &Store,
    binding: &Binding,
    agent: &str,
    reply: bool,
) -> Result<u64, String> {
    Ok(load(store)?
        .receipts
        .iter()
        .filter(|r| &Binding::tracked(&r.scope) == binding)
        .filter_map(|r| {
            if reply {
                r.reply_ordinals.get(agent)
            } else {
                r.pass_ordinals.get(agent)
            }
        })
        .copied()
        .max()
        .unwrap_or(0))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub receipt: Receipt,
    pub applying: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub receipts: Vec<Receipt>,
    pub pending: Option<Pending>,
}

pub(crate) fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn load(store: &Store) -> Result<Ledger, String> {
    match store.read_state("retention.json") {
        Ok(bytes) => {
            let ledger: Ledger = serde_json::from_slice(&bytes)
                .map_err(|_| "Retention receipts are invalid; cleanup and work are blocked.")?;
            if ledger
                .receipts
                .iter()
                .chain(ledger.pending.iter().map(|p| &p.receipt))
                .any(|r| {
                    !r.scope.terminal_observed
                        || r.scope.lifecycle == crate::github::metadata::Lifecycle::Open
                        || r.scope.iteration == 0
                        || r.scope.iteration_id.is_empty()
                })
            {
                return Err("Retention receipt lacks a confirmed terminal iteration; cleanup and work are blocked.".into());
            }
            Ok(ledger)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
        Err(_) => Err("Cannot read retention receipts; cleanup and work are blocked.".into()),
    }
}

pub(crate) fn save(store: &Store, ledger: &Ledger) -> Result<(), String> {
    store.write_state("retention.json", ledger)
}

pub(crate) fn guard(store: &Store) -> Result<(), String> {
    if load(store)?.pending.is_some_and(|p| p.applying) {
        Err("Terminal cleanup is incomplete. Operational state is unavailable until recovery succeeds.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn reject_cleaned(
    store: &Store,
    jobs: impl IntoIterator<Item = QueueJob>,
) -> Result<(), String> {
    let receipts = load(store)?.receipts;
    for job in jobs {
        if receipts.iter().any(|r| {
            Binding::tracked(&r.scope).matches(&job)
                && (r.items.contains(&crate::queue::item_id(&job))
                    || job
                        .work
                        .as_ref()
                        .is_some_and(|w| w.iteration <= r.scope.iteration))
        }) {
            return Err("A stale worker attempted to restore cleaned terminal detail.".into());
        }
    }
    Ok(())
}

pub(crate) fn reject_items<'a>(
    store: &Store,
    items: impl IntoIterator<Item = &'a str>,
) -> Result<(), String> {
    let receipts = load(store)?.receipts;
    if items.into_iter().any(|id| {
        receipts
            .iter()
            .any(|r| r.items.contains(id) || r.notices.contains_key(id))
    }) {
        Err("A stale operation attempted to restore cleaned terminal detail.".into())
    } else {
        Ok(())
    }
}

pub(crate) fn cleaned(
    store: &Store,
    destination: &crate::panel::Detail,
) -> Result<Option<&'static str>, String> {
    guard(store)?;
    Ok(load(store)?.receipts.iter().any(|r| match destination {
        crate::panel::Detail::Item {item_id} => r.items.contains(item_id),
        crate::panel::Detail::Job {kind, id} => r.work.contains(&WorkId {kind:*kind,id:id.clone()}),
        _ => false,
    }).then_some("Detail for this exact destination was cleaned after provider-confirmed closure or merge. No other PR or iteration was selected."))
}

pub(crate) fn refresh(store: &Store) -> Result<(), String> {
    paging::refresh(store)
}

pub(crate) fn mark_activity_pending(store: &Store) -> Result<(), String> {
    paging::mark_pending(store)
}

#[cfg(test)]
mod tests;
