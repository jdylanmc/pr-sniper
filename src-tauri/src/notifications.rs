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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
            Self::Test => "PR Sniper notifications are on",
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
            Self::Test => "Alerts for review events will appear here.",
            Self::Approved => "GitHub recorded the acting account's automated approval. This unmerged PR still needs your personal review; one account is one vote.",
            Self::Merged => "GitHub confirmed merge. Open saved evidence for action attribution; human review is not inferred.",
            Self::FinalReviewCleared => "The primary completed its final full review. Provider approval/merge remain separately gated; an unmerged PR still needs personal review.",
        }
    }

    /// Event categories a user can turn off; `Test` confirms opt-in and always sends.
    pub const SELECTABLE: [Self; 7] = [
        Self::Confirmation,
        Self::HumanInput,
        Self::Ready,
        Self::Failure,
        Self::Approved,
        Self::Merged,
        Self::FinalReviewCleared,
    ];

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
        if self.category == Category::Merged {
            // Saved evidence and receipt availability may change; a PR merges only once.
            return digest(&serde_json::json!([self.category, self.group]).to_string());
        }
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
    let mut merged = BTreeMap::<String, &queue::Item>::new();
    for item in &snapshot.items {
        if item.state == queue::State::Merged && item.action_status.is_some() {
            let identity = digest(
                &serde_json::json!([
                    item.job.provider,
                    item.job.account_id,
                    item.job.repository_id,
                    item.job.pull_request_id
                ])
                .to_string(),
            );
            let candidate = merged.entry(identity).or_insert(item);
            let rank = |item: &queue::Item| {
                (
                    item.job.work.as_ref().map_or(0, |work| work.iteration),
                    item.job.detected_at,
                    item.id.clone(),
                )
            };
            if rank(item) > rank(candidate) {
                *candidate = item;
            }
            continue;
        }
        let mut category = match item.state {
            queue::State::ConfirmationRequired => Some(Category::Confirmation),
            queue::State::WaitingForHuman => Some(Category::HumanInput),
            queue::State::MachineSignedOff => Some(Category::Ready),
            queue::State::Failed | queue::State::StaleAfterPublication => Some(Category::Failure),
            _ => None,
        };
        if item.state == queue::State::MachineSignedOff
            && item.action_status.as_ref().is_some_and(|s| s.final_valid)
        {
            category = Some(Category::FinalReviewCleared);
        }
        if item.state == queue::State::Approved {
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
    for (identity, item) in merged {
        let merge_commit = item.action_status.as_ref().and_then(|status| {
            status.effects.iter().find_map(|effect| {
                effect
                    .receipt
                    .as_ref()
                    .filter(|receipt| {
                        receipt.action == crate::github::actions::Action::Merge
                            && effect.observation.account_id == item.job.account_id
                            && effect.observation.repository_id == item.job.repository_id
                            && effect.observation.pull_request_id == item.job.pull_request_id
                    })
                    .and_then(|receipt| receipt.merge_commit.as_deref())
            })
        });
        frames.push(Frame {
            source: format!("merged:{identity}"),
            event: Some(Event {
                category: Category::Merged,
                destination: Destination::QueueItem {
                    item_id: item.id.clone(),
                },
                cause: digest(&serde_json::json!([identity, merge_commit]).to_string()),
                group: identity,
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
                    queue::State::Failed | queue::State::StaleAfterPublication
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
    /// Turned-off categories; absent in older files, so every category defaults on.
    #[serde(default)]
    pub muted: BTreeSet<Category>,
    /// Durable merge tombstones, independent of retained queue/notice detail.
    /// Missing in legacy ledgers: first observation silently adopts merge history.
    #[serde(default)]
    pub merged_sources: Option<BTreeSet<String>>,
    pub observations: BTreeMap<String, Observation>,
    pub notices: Vec<Notice>,
}

#[cfg(windows)]
pub(crate) const WINDOWS_SETUP_SOURCE: &str = "windows-permission-setup";
pub(crate) const ENABLED_SOURCE: &str = "notifications-enabled";

impl Default for Ledger {
    fn default() -> Self {
        Self {
            profile_id: uuid::Uuid::new_v4().to_string(),
            enabled: false,
            muted: BTreeSet::new(),
            merged_sources: Some(BTreeSet::new()),
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
        if self.muted.contains(&Category::Test) {
            return Err("Notification preferences are invalid; no notices were sent.".into());
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
        self.record(frames, now, true)
    }

    /// Tracks every source's current event. Muted categories still advance the
    /// baseline, so turning one back on never replays what happened while off.
    fn record(&mut self, frames: &[Frame], now: i64, notify: bool) -> Result<bool, String> {
        let mut changed = false;
        if self.merged_sources.is_none() {
            self.merged_sources = Some(
                frames
                    .iter()
                    .filter(|frame| {
                        frame
                            .event
                            .as_ref()
                            .is_some_and(|event| event.category == Category::Merged)
                    })
                    .map(|frame| frame.source.clone())
                    .collect(),
            );
            self.drop_queued(
                |notice| notice.event.category == Category::Merged,
                "Historical merge notifications were adopted without replay.",
            );
            changed = true;
        }
        let mut sources = BTreeSet::new();
        for frame in frames {
            if !sources.insert(frame.source.clone()) {
                return Err("Duplicate notification source.".into());
            }
            let fingerprint = frame.event.as_ref().map(Event::fingerprint);
            let first_merge = if frame
                .event
                .as_ref()
                .is_some_and(|event| event.category == Category::Merged)
            {
                let inserted = self
                    .merged_sources
                    .as_mut()
                    .unwrap()
                    .insert(frame.source.clone());
                changed |= inserted;
                inserted
            } else {
                true
            };
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
                if !notify || !first_merge || self.muted.contains(&event.category) {
                    continue;
                }
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

    /// Turns notifications on from now forward: current events become a silent
    /// baseline and one confirmation is queued. Returns `None` when already on.
    pub fn enable(&mut self, frames: &[Frame], now: i64) -> Result<Option<String>, String> {
        if self.enabled {
            return Ok(None);
        }
        self.drop_queued(
            |_| true,
            "Notifications were turned off before this was sent.",
        );
        self.record(frames, now, false)?;
        self.enabled = true;
        Ok(Some(self.push_test(
            Destination::Settings,
            now,
            ENABLED_SOURCE,
            Phase::Queued,
        )))
    }

    /// Returns whether the preference changed.
    pub fn disable(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        self.enabled = false;
        self.drop_queued(
            |_| true,
            "Notifications were turned off before this was sent.",
        );
        true
    }

    /// Returns whether the preference changed. Turning a category off drops its
    /// unsent notices; turning it on relies on the continuously kept baseline.
    pub fn set_category(&mut self, category: Category, enabled: bool) -> Result<bool, String> {
        if !Category::SELECTABLE.contains(&category) {
            return Err("This notification type cannot be changed.".into());
        }
        if enabled {
            return Ok(self.muted.remove(&category));
        }
        if !self.muted.insert(category) {
            return Ok(false);
        }
        self.drop_queued(
            |notice| notice.event.category == category,
            "This notification type was turned off before this was sent.",
        );
        Ok(true)
    }

    fn drop_queued(&mut self, matches: impl Fn(&Notice) -> bool, reason: &str) {
        for notice in self
            .notices
            .iter_mut()
            .filter(|n| n.phase == Phase::Queued && matches(n))
        {
            notice.phase = Phase::NotSent;
            notice.error = Some(reason.into());
        }
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
            && !self.muted.contains(&notice.event.category)
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
    if let Some(notice) = ledger.notices.iter().find(|n| n.id == id) {
        return Ok(notice.event.destination.clone());
    }
    if let Some(item_id) = crate::retention::load(store)?
        .receipts
        .iter()
        .find_map(|r| r.notices.get(id))
    {
        return Ok(Destination::QueueItem {
            item_id: item_id.clone(),
        });
    }
    Err("This notification belongs to another profile or is no longer retained. No substitute destination was opened.".into())
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

pub(crate) const SUPERSEDED: &str = "A newer notification preference replaced this request.";

/// Saves the opt-in preference only if `current()` still holds once storage is
/// held, so a superseded request neither persists nor queues a confirmation.
pub(crate) fn save_preference(
    store: &Store,
    enabled: bool,
    baseline: &[Frame],
    now: i64,
    current: impl Fn() -> bool,
) -> Result<Option<String>, String> {
    if !current() {
        return Err(SUPERSEDED.into());
    }
    let mut ledger = store.load_notifications()?;
    let confirmation = if enabled {
        ledger.enable(baseline, now)?
    } else {
        ledger.disable();
        None
    };
    store.save_notifications(&ledger)?;
    Ok(confirmation)
}

/// Per-type counterpart of [`save_preference`].
pub(crate) fn save_category(
    store: &Store,
    category: Category,
    enabled: bool,
    current: impl Fn() -> bool,
) -> Result<(), String> {
    if !current() {
        return Err(SUPERSEDED.into());
    }
    let mut ledger = store.load_notifications()?;
    if ledger.set_category(category, enabled)? {
        store.save_notifications(&ledger)?;
    }
    Ok(())
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
