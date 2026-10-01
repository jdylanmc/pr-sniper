pub(crate) mod host;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
mod windows_native;
pub use host::{view as snapshot, Snapshot};

use crate::{queue, storage::Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Confirmation,
    HumanInput,
    Ready,
    Failure,
    Test,
    Approved,
    Merged,
    FinalReviewCleared,
}

impl Category {
    pub fn title(self) -> &'static str {
        match self {
            Self::Confirmation => "Review needs your confirmation",
            Self::HumanInput => "Review needs your input",
            Self::Ready => "Ready for your final review",
            Self::Failure => "PR Sniper needs attention",
            Self::Test => "PR Sniper test notification",
            Self::Approved => "Automated approval confirmed; personal review remains",
            Self::Merged => "PR merged on GitHub",
            Self::FinalReviewCleared => "Primary final full review cleared",
        }
    }

    pub fn body(self) -> &'static str {
        match self {
            Self::Confirmation => "Open the exact queue item to confirm the next action. Opening this alert does not start or publish a review.",
            Self::HumanInput => "Automated review needs your judgment. Open the exact queue item for the conversation and evidence.",
            Self::Ready => "Automated review completed. Open the exact queue item for your final review and merge decision. This is not approval.",
            Self::Failure => "A review or monitoring operation needs recovery. Open the saved destination for details; no success is implied.",
            Self::Test => "Open to verify the selected in-app destination. No review will start and nothing will be published.",
            Self::Approved => "GitHub recorded the acting account's automated approval. This unmerged PR still needs your personal review; one account is one vote.",
            Self::Merged => "GitHub confirmed merge. Open saved evidence for action attribution; human review is not inferred.",
            Self::FinalReviewCleared => "The primary completed its final full review. Provider approval/merge remain separately gated; an unmerged PR still needs personal review.",
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Test => 0,
            Self::HumanInput
            | Self::Ready
            | Self::Approved
            | Self::Merged
            | Self::FinalReviewCleared => 1,
            Self::Confirmation => 2,
            Self::Failure => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    QueueItem { item_id: String },
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub category: Category,
    pub destination: Destination,
    pub cause: String,
    pub group: String,
}

impl Event {
    fn fingerprint(&self) -> String {
        digest(&serde_json::json!([self.category, self.destination, self.cause]).to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub source: String,
    pub event: Option<Event>,
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn frames(snapshot: &queue::Snapshot) -> Vec<Frame> {
    let mut frames = Vec::new();
    for item in &snapshot.items {
        let mut category = match item.state {
            queue::State::ConfirmationRequired => Some(Category::Confirmation),
            queue::State::WaitingForHuman => Some(Category::HumanInput),
            queue::State::MachineSignedOff => Some(Category::Ready),
            queue::State::Failed | queue::State::Blocked | queue::State::StaleAfterPublication => {
                Some(Category::Failure)
            }
            _ => None,
        };
        if item.state == queue::State::MachineSignedOff
            && item.action_status.as_ref().is_some_and(|s| s.final_valid)
        {
            category = Some(Category::FinalReviewCleared);
        }
        if item.state == queue::State::Merged && item.action_status.is_some() {
            category = Some(Category::Merged);
        }
        if item.state == queue::State::MachineSignedOff
            && item.action_status.as_ref().is_some_and(|s| {
                s.effects.iter().any(|e| {
                    e.action == crate::github::actions::Action::Approve
                        && e.state == crate::actions::EffectState::Confirmed
                })
            })
        {
            category = Some(Category::Approved);
        }
        if item.state != queue::State::Merged
            && item.action_status.as_ref().is_some_and(|s| {
                s.effects.iter().any(|e| {
                    e.error.is_some()
                        || matches!(
                            e.state,
                            crate::actions::EffectState::Rejected
                                | crate::actions::EffectState::Uncertain
                        )
                }) || s.final_review.as_ref().is_some_and(|f| {
                    matches!(
                        f.execution.operation.state,
                        crate::monitoring::OperationState::Failed
                            | crate::monitoring::OperationState::ManualRetry
                    )
                })
            })
        {
            category = Some(Category::Failure);
        }
        let mut causes = BTreeSet::new();
        if let Some(status) = &item.action_status {
            if let Some(final_review) = &status.final_review {
                if matches!(
                    final_review.execution.operation.state,
                    crate::monitoring::OperationState::Completed
                        | crate::monitoring::OperationState::Failed
                        | crate::monitoring::OperationState::ManualRetry
                ) {
                    causes.insert(final_review.execution.operation.id.clone());
                }
            }
            for effect in &status.effects {
                causes.insert(format!("{}:{:?}", effect.id, effect.state));
            }
        }
        for review in snapshot
            .reviews
            .iter()
            .filter(|r| item.review_keys.contains(&r.key))
        {
            causes.insert(
                review
                    .run
                    .as_ref()
                    .map_or(review.key.clone(), |r| r.operation.id.clone()),
            );
            if let Some(run) = &review.run {
                for publication in snapshot
                    .publications
                    .iter()
                    .filter(|p| p.review_operation_id == run.operation.id)
                {
                    if let Some(p) = &publication.publication {
                        causes.insert(p.operation.id.clone());
                    }
                }
            }
        }
        for follow in snapshot
            .follow_ups
            .iter()
            .filter(|f| item.follow_up_ids.contains(&f.run.id))
        {
            causes.insert(follow.run.id.clone());
            for operation in [&follow.run.analysis, &follow.run.publication]
                .into_iter()
                .flatten()
            {
                causes.insert(operation.id.clone());
            }
        }
        if category == Some(Category::Failure) {
            for health in snapshot.health.iter().filter(|h| {
                h.repository_id == item.job.configuration_id
                    && h.provider_account_id.as_deref() == Some(&item.job.account_id)
                    && h.last_failure.is_some()
            }) {
                if let Some(operation) = &health.operation {
                    causes.insert(operation.id.clone());
                }
                if let Some(failure) = &health.last_failure {
                    causes.insert(failure.clone());
                }
            }
        }
        frames.push(Frame {
            source: format!("queue:{}", item.id),
            event: category.map(|category| Event {
                category,
                destination: Destination::QueueItem {
                    item_id: item.id.clone(),
                },
                cause: digest(&serde_json::json!(causes).to_string()),
                group: digest(
                    &serde_json::json!([
                        item.job.provider,
                        item.job.account_id,
                        item.job.configuration_id
                    ])
                    .to_string(),
                ),
            }),
        });
    }
    for health in &snapshot.health {
        let source = digest(
            &serde_json::json!([
                health.repository_id,
                health.provider_account_id,
                health.assignment_id
            ])
            .to_string(),
        );
        let covered = snapshot.items.iter().any(|item| {
            item.job.configuration_id == health.repository_id
                && Some(&item.job.account_id) == health.provider_account_id.as_ref()
                && matches!(
                    item.state,
                    queue::State::Failed
                        | queue::State::Blocked
                        | queue::State::StaleAfterPublication
                )
        });
        frames.push(Frame {
            source: format!("schedule:{source}"),
            event: health
                .last_failure
                .as_ref()
                .filter(|_| !covered)
                .map(|failure| Event {
                    category: Category::Failure,
                    destination: Destination::Settings,
                    cause: digest(
                        &serde_json::json!([health.operation.as_ref().map(|o| &o.id), failure])
                            .to_string(),
                    ),
                    group: source,
                }),
        });
    }
    frames
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Submitting,
    AcceptedUnconfirmed,
    OutcomeUnknown,
    PermissionDenied,
    Failed,
    NotSent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub id: String,
    pub source: String,
    pub fingerprint: String,
    pub episode: u64,
    pub event: Event,
    pub phase: Phase,
    pub created_at: i64,
    pub error: Option<String>,
    pub opened_at: Option<i64>,
    pub navigation_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    fingerprint: Option<String>,
    episode: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub profile_id: String,
    pub enabled: bool,
    pub observations: BTreeMap<String, Observation>,
    pub notices: Vec<Notice>,
}

#[cfg(windows)]
pub(crate) const WINDOWS_SETUP_SOURCE: &str = "windows-permission-setup";

impl Default for Ledger {
    fn default() -> Self {
        Self {
            profile_id: uuid::Uuid::new_v4().to_string(),
            enabled: false,
            observations: BTreeMap::new(),
            notices: vec![],
        }
    }
}

impl Ledger {
    pub fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.profile_id).is_err()
            || self
                .notices
                .iter()
                .any(|n| !n.id.starts_with(&format!("pr-sniper:{}:", self.profile_id)))
        {
            return Err("Notification identities are invalid; no notices were sent.".into());
        }
        let unique: BTreeSet<_> = self.notices.iter().map(|n| &n.id).collect();
        if unique.len() != self.notices.len() {
            return Err("Duplicate notification identities; no notices were sent.".into());
        }
        Ok(())
    }

    pub fn observe(&mut self, frames: &[Frame], now: i64) -> Result<bool, String> {
        if !self.enabled {
            return Ok(false);
        }
        let mut changed = false;
        let mut sources = BTreeSet::new();
        for frame in frames {
            if !sources.insert(frame.source.clone()) {
                return Err("Duplicate notification source.".into());
            }
            let fingerprint = frame.event.as_ref().map(Event::fingerprint);
            let previous = self.observations.entry(frame.source.clone()).or_default();
            if previous.fingerprint == fingerprint {
                continue;
            }
            previous.fingerprint = fingerprint.clone();
            changed = true;
            if let Some(event) = &frame.event {
                previous.episode = previous
                    .episode
                    .checked_add(1)
                    .ok_or("Notification episode limit reached.")?;
                self.notices.push(Notice {
                    id: format!("pr-sniper:{}:{}", self.profile_id, uuid::Uuid::new_v4()),
                    source: frame.source.clone(),
                    fingerprint: fingerprint.unwrap(),
                    episode: previous.episode,
                    event: event.clone(),
                    phase: Phase::Queued,
                    created_at: now,
                    error: None,
                    opened_at: None,
                    navigation_error: None,
                });
            }
        }
        for (source, observation) in &mut self.observations {
            if !sources.contains(source) && observation.fingerprint.take().is_some() {
                changed = true;
            }
        }
        Ok(changed)
    }

    pub fn enqueue_test(&mut self, destination: Destination, now: i64) -> Result<String, String> {
        if !self.enabled {
            return Err("Enable notifications before sending a test.".into());
        }
        Ok(self.push_test(destination, now, "test", Phase::Queued))
    }

    #[cfg(windows)]
    pub(crate) fn begin_permission_setup(&mut self, now: i64) -> Result<Notice, String> {
        if self.enabled {
            return Err(
                "Turn notifications off before retrying Windows notification setup.".into(),
            );
        }
        if self.notices.iter().any(|notice| {
            notice.source == WINDOWS_SETUP_SOURCE
                && !matches!(notice.phase, Phase::Failed | Phase::NotSent)
        }) {
            return Err("Windows notification setup already had a send attempt. Its outcome remains in history; it will not be resent.".into());
        }
        self.push_test(
            Destination::Settings,
            now,
            WINDOWS_SETUP_SOURCE,
            Phase::Submitting,
        );
        Ok(self.notices.last().unwrap().clone())
    }

    fn push_test(
        &mut self,
        destination: Destination,
        now: i64,
        source: &str,
        phase: Phase,
    ) -> String {
        let id = format!("pr-sniper:{}:{}", self.profile_id, uuid::Uuid::new_v4());
        self.notices.push(Notice {
            id: id.clone(),
            source: source.into(),
            fingerprint: id.clone(),
            episode: 1,
            event: Event {
                category: Category::Test,
                destination,
                cause: id.clone(),
                group: self.profile_id.clone(),
            },
            phase,
            created_at: now,
            error: None,
            opened_at: None,
            navigation_error: None,
        });
        id
    }

    pub fn next(&self) -> Option<&Notice> {
        self.notices
            .iter()
            .filter(|n| n.phase == Phase::Queued)
            .min_by_key(|n| (n.event.category.priority(), n.created_at, &n.id))
    }

    pub fn current(&self, notice: &Notice) -> bool {
        self.enabled
            && (notice.event.category == Category::Test
                || self.observations.get(&notice.source).is_some_and(|o| {
                    o.fingerprint.as_ref() == Some(&notice.fingerprint)
                        && o.episode == notice.episode
                }))
    }

    pub fn begin(&mut self, id: &str) -> Result<Notice, String> {
        let notice = self
            .notices
            .iter()
            .find(|n| n.id == id)
            .ok_or("Notification disappeared.")?;
        if notice.phase != Phase::Queued {
            return Err(
                "This notification already had its send attempt; it will not be repeated.".into(),
            );
        }
        let current = self.current(notice);
        let notice = self.notices.iter_mut().find(|n| n.id == id).unwrap();
        notice.phase = if current {
            Phase::Submitting
        } else {
            Phase::NotSent
        };
        if !current {
            notice.error = Some(
                "The transition changed or notifications were disabled before submission.".into(),
            );
        }
        Ok(notice.clone())
    }

    pub fn finish(&mut self, id: &str, phase: Phase, error: Option<String>) -> Result<(), String> {
        if !matches!(
            phase,
            Phase::AcceptedUnconfirmed
                | Phase::OutcomeUnknown
                | Phase::PermissionDenied
                | Phase::Failed
                | Phase::NotSent
        ) {
            return Err("Invalid notification completion state.".into());
        }
        let notice = self
            .notices
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Notification disappeared.")?;
        if notice.phase != Phase::Submitting {
            return Err("Notification send attempt is no longer active.".into());
        }
        notice.phase = phase;
        notice.error = error;
        Ok(())
    }

    pub fn restore(&mut self) -> bool {
        let mut changed = false;
        for notice in &mut self.notices {
            if notice.phase == Phase::Submitting {
                notice.phase = Phase::OutcomeUnknown;
                notice.error = Some("Interrupted during native submission. Delivery is unconfirmed; this transition will not be resent.".into());
                changed = true;
            }
        }
        changed
    }
}

pub fn restore(store: &Store) -> Result<(), String> {
    let mut ledger = store.load_notifications()?;
    ledger.restore();
    store.save_notifications(&ledger)
}

#[cfg(windows)]
pub(crate) fn persisted_ledger(store: &Store) -> Result<Ledger, String> {
    let bytes = store.read_state("notifications.json").map_err(|_| {
        "Notification history is not persisted or cannot be read. Notifications remain unavailable."
    })?;
    let ledger: Ledger = serde_json::from_slice(&bytes)
        .map_err(|_| "Notification history is invalid; no identity can be registered.")?;
    ledger.validate()?;
    Ok(ledger)
}

#[derive(Debug, Clone, Serialize)]
pub struct Permission {
    pub authorization: String,
    pub alerts_enabled: Option<bool>,
    pub center_enabled: Option<bool>,
}

impl Permission {
    pub fn allowed(&self) -> bool {
        self.authorization == "authorized_aggregate"
            || (matches!(self.authorization.as_str(), "authorized" | "provisional")
                && (self.alerts_enabled == Some(true) || self.center_enabled == Some(true)))
    }
}

#[derive(Debug)]
pub struct SendError {
    pub message: String,
    pub uncertain: bool,
}

pub trait Adapter: Send + Sync {
    fn permission(&self) -> Result<Permission, String>;
    fn request_permission(&self) -> Result<Permission, String>;
    fn send(&self, notice: &Notice) -> Result<(), SendError>;
}

pub fn submit(adapter: &dyn Adapter, notice: &Notice) -> (Phase, Option<String>) {
    if notice.phase != Phase::Submitting {
        return (
            Phase::Failed,
            Some("Native notification intent must be persisted before submission.".into()),
        );
    }
    match adapter.send(notice) {
        Ok(()) => (Phase::AcceptedUnconfirmed, None),
        Err(error) => (
            if error.uncertain {
                Phase::OutcomeUnknown
            } else {
                Phase::Failed
            },
            Some(error.message),
        ),
    }
}

pub fn destination(store: &Store, id: &str) -> Result<Destination, String> {
    let ledger = store.load_notifications()?;
    Ok(ledger.notices.iter().find(|n| n.id == id).ok_or(
        "This notification belongs to another profile or is no longer retained. No substitute destination was opened.",
    )?.event.destination.clone())
}

pub fn prepare(store: &Store, frames: &[Frame], id: &str, now: i64) -> Result<Notice, String> {
    let mut ledger = store.load_notifications()?;
    ledger.observe(frames, now)?;
    let mut notice = ledger.begin(id)?;
    if notice.phase == Phase::Submitting {
        if let Destination::QueueItem { item_id } = &notice.event.destination {
            if let Err(error) = queue::destination(store, item_id, None) {
                ledger.finish(id, Phase::NotSent, Some(error))?;
                notice = ledger.notices.iter().find(|n| n.id == id).unwrap().clone();
            }
        }
    }
    store.save_notifications(&ledger)?;
    Ok(notice)
}

pub fn complete(
    store: &Store,
    id: &str,
    phase: Phase,
    error: Option<String>,
) -> Result<(), String> {
    let mut ledger = store.load_notifications()?;
    ledger.finish(id, phase, error)?;
    store.save_notifications(&ledger)
}

#[cfg(test)]
mod tests;
