use super::*;
use crate::{
    actions::{EffectState, Ledger as Actions},
    follow_up::FollowUp,
    github::metadata::Lifecycle,
    monitoring::{OperationState, QueueState},
    notifications::{Destination, Ledger as Notifications, Phase as NoticePhase},
    publication::{Publication, RemoteState},
    review::ReviewRun,
};

struct State {
    queue: QueueState,
    reviews: Vec<ReviewRun>,
    publications: Vec<Publication>,
    follow_ups: Vec<FollowUp>,
    actions: Actions,
    feedback: crate::feedback::Ledger,
    notifications: Notifications,
}

fn read<T: serde::de::DeserializeOwned + Default>(store: &Store, name: &str) -> Result<T, String> {
    match store.read_state(name) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| format!("Cannot recover cleanup: invalid {name}.")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(_) => Err(format!("Cannot recover cleanup: unreadable {name}.")),
    }
}

impl State {
    fn read(store: &Store) -> Result<Self, String> {
        let publications: Vec<Publication> = read(store, "publications.json")?;
        let follow_ups = match store.read_state("follow-ups.json") {
            Ok(bytes) => crate::follow_up::decode_with_origins(&bytes, &publications)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err("Cannot recover cleanup: unreadable follow-ups.json.".into()),
        };
        let queue = match store.read_state("queue.json") {
            Ok(bytes) => crate::storage_state::decode_queue(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => QueueState::default(),
            Err(_) => return Err("Cannot recover cleanup: unreadable queue.json.".into()),
        };
        let notifications: Notifications = read(store, "notifications.json")?;
        notifications.validate()?;
        Ok(Self {
            queue,
            reviews: read(store, "reviews.json")?,
            publications,
            follow_ups,
            actions: read(store, "actions.json")?,
            feedback: read(store, "feedback.json")?,
            notifications,
        })
    }

    fn receipt(&self, scope: &TrackedPullRequest) -> Result<Option<Receipt>, String> {
        let binding = Binding::tracked(scope);
        let matches = |job: &QueueJob| binding.matches(job);
        let running = |op: &JobOperation| op.state == OperationState::Running;
        let publications: Vec<_> = self
            .publications
            .iter()
            .filter(|p| matches(&p.review.job))
            .collect();
        let follows: Vec<_> = self
            .follow_ups
            .iter()
            .filter(|f| matches(&f.context.job))
            .collect();
        let review_origins: BTreeMap<_, _> = publications
            .iter()
            .map(|p| &p.review)
            .chain(follows.iter().filter_map(|f| match &f.target {
                crate::follow_up::ConversationTarget::Owned(origin) => Some(&origin.review),
                _ => None,
            }))
            .chain(self.reviews.iter().filter(|r| matches(&r.job)))
            .map(|r| (r.operation.id.clone(), r))
            .collect();
        let reviews: Vec<_> = review_origins.into_values().collect();
        let finals: Vec<_> = self
            .actions
            .finals
            .iter()
            .filter(|f| matches(&f.basis.job))
            .collect();
        let mut items: BTreeSet<_> = self
            .queue
            .jobs
            .iter()
            .filter(|j| matches(j))
            .map(crate::queue::item_id)
            .collect();
        items.insert(scope.item_id.clone());
        for job in self.queue.jobs.iter().filter(|j| matches(j)) {
            if let Some(alias) = job.work.as_ref().and_then(|w| w.legacy_item_id.clone()) {
                items.insert(alias);
            }
        }
        for job in reviews
            .iter()
            .map(|r| &r.job)
            .chain(publications.iter().map(|p| &p.review.job))
            .chain(follows.iter().map(|f| &f.context.job))
            .chain(finals.iter().map(|f| &f.basis.job))
        {
            items.insert(crate::queue::item_id(job));
        }
        let effects: Vec<_> = self
            .actions
            .effects
            .iter()
            .filter(|e| items.contains(&e.item_id))
            .collect();
        // Stopped local jobs are not proof that a provider intent settled.
        if reviews.iter().any(|r| running(&r.operation))
            || finals.iter().any(|f| running(&f.execution.operation))
            || publications.iter().any(|p| running(&p.operation) || p.uncertain
                || p.receipts.last().is_some_and(|r| r.state == RemoteState::Pending))
            || follows.iter().any(|f| f.analysis.as_ref().is_some_and(running)
                || f.publication.as_ref().is_some_and(running)
                || f.uncertain)
            || effects.iter().any(|e| running(&e.operation) || e.needs_reconciliation()
                || e.state == EffectState::Prepared && e.operation.attempted_mutation.is_some())
            || self.notifications.notices.iter().any(|n|
                matches!(&n.event.destination, Destination::QueueItem { item_id } if items.contains(item_id))
                    && n.phase == NoticePhase::Submitting)
        {
            return Ok(None);
        }
        let mut owned = Vec::new();
        for publication in publications.iter().filter(|p| {
            p.receipts
                .last()
                .is_some_and(|r| r.state == RemoteState::Commented && !r.comment_ids.is_empty())
        }) {
            let mut receipt = OwnedReceipt::from_publication(publication)?;
            receipt.closed_roots.extend(
                self.feedback
                    .records
                    .iter()
                    .filter(|r| r.context.publication_id == publication.id && r.context.closed)
                    .map(|r| r.context.root_id.clone()),
            );
            receipt.human_input_threads.extend(
                follows
                    .iter()
                    .filter(|f| f.phase == crate::follow_up::Phase::HumanInputRequired)
                    .filter_map(|f| f.thread().ok())
                    .filter(|t| t.owned_by(&receipt.proof))
                    .map(|t| t.id.clone()),
            );
            owned.push(receipt);
        }
        let mut work = BTreeSet::new();
        work.extend(reviews.iter().map(|r| WorkId {
            kind: crate::capacity::Kind::Normal,
            id: r.key.clone(),
        }));
        let mut pass_ordinals: BTreeMap<String, u64> = BTreeMap::new();
        let mut reply_ordinals: BTreeMap<String, u64> = BTreeMap::new();
        for job in self.queue.jobs.iter().filter(|j| matches(j)) {
            if let Some(work) = &job.work {
                let ordinal = pass_ordinals.entry(work.agent_id.clone()).or_default();
                *ordinal = (*ordinal).max(work.pass_ordinal);
            }
            if let Some(assignment) = &job.assignment_id {
                work.insert(WorkId {
                    kind: crate::capacity::Kind::Normal,
                    id: crate::review::key(job, assignment),
                });
            }
        }
        work.extend(finals.iter().map(|f| WorkId {
            kind: crate::capacity::Kind::PrimaryFinal,
            id: f.id.clone(),
        }));
        work.extend(follows.iter().map(|f| WorkId {
            kind: f.kind(),
            id: f.id.clone(),
        }));
        for follow in &follows {
            let ordinal = reply_ordinals
                .entry(follow.context.selection.agent.id.clone())
                .or_default();
            *ordinal = (*ordinal).max(follow.reply_ordinal.unwrap_or(0));
        }
        work.extend(
            self.feedback
                .mentions
                .iter()
                .filter(|m| {
                    self.queue
                        .jobs
                        .iter()
                        .any(|j| matches(j) && m.binding.matches(j))
                })
                .map(|m| WorkId {
                    kind: crate::capacity::Kind::Mention,
                    id: m.work_id.clone(),
                }),
        );
        let mut operations = BTreeMap::new();
        for op in reviews
            .iter()
            .map(|r| &r.operation)
            .chain(publications.iter().flat_map(|p| {
                std::iter::once(&p.review.operation)
                    .chain(std::iter::once(&p.operation))
                    .chain(p.history.iter())
            }))
            .chain(follows.iter().flat_map(|f| {
                f.analysis
                    .iter()
                    .chain(f.publication.iter())
                    .chain(f.history.iter())
            }))
            .chain(finals.iter().flat_map(|f| {
                std::iter::once(&f.execution.operation)
                    .chain(f.attempts.iter().map(|r| &r.operation))
            }))
            .chain(effects.iter().map(|e| &e.operation))
        {
            operations.insert(op.id.clone(), op.clone());
        }
        Ok(Some(Receipt {
            scope: scope.clone(),
            enqueue_watermark: self.queue.next_enqueue_order,
            items,
            work,
            operations: operations.into_values().collect(),
            publications: publications
                .iter()
                .map(|p| (p.id.clone(), p.review.key.clone(), p.receipts.clone()))
                .collect(),
            effects: effects
                .iter()
                .map(|e| EffectReceipt {
                    id: e.id.clone(),
                    item_id: e.item_id.clone(),
                    action: e.action,
                    state: e.state.clone(),
                    operation_id: e.operation.id.clone(),
                    receipt: e.receipt.clone(),
                })
                .collect(),
            completions: reviews
                .iter()
                .map(|r| {
                    (
                        WorkId {
                            kind: crate::capacity::Kind::Normal,
                            id: r.key.clone(),
                        },
                        r.operation.id.clone(),
                    )
                })
                .chain(finals.iter().map(|f| {
                    (
                        WorkId {
                            kind: crate::capacity::Kind::PrimaryFinal,
                            id: f.id.clone(),
                        },
                        f.execution.operation.id.clone(),
                    )
                }))
                .chain(follows.iter().flat_map(|f| {
                    f.analysis.iter().chain(f.publication.iter()).map(|o| {
                        (
                            WorkId {
                                kind: f.kind(),
                                id: f.id.clone(),
                            },
                            o.id.clone(),
                        )
                    })
                }))
                .collect(),
            pass_ordinals,
            reply_ordinals,
            follow_up_keys: follows
                .iter()
                .map(|f| f.key.clone())
                .chain(
                    self.feedback
                        .mentions
                        .iter()
                        .filter(|m| {
                            self.queue
                                .jobs
                                .iter()
                                .any(|j| matches(j) && m.binding.matches(j))
                        })
                        .map(|m| m.key.clone()),
                )
                .collect(),
            owned,
            notices: BTreeMap::new(),
        }))
    }

    fn compact(mut self, store: &Store, receipt: &Receipt) -> Result<(), String> {
        mark_activity_pending(store)?;
        let binding = Binding::tracked(&receipt.scope);
        self.follow_ups.retain(|f| !binding.matches(&f.context.job));
        self.publications
            .retain(|p| !binding.matches(&p.review.job));
        self.reviews.retain(|r| !binding.matches(&r.job));
        self.actions
            .finals
            .retain(|f| !binding.matches(&f.basis.job));
        self.actions
            .effects
            .retain(|e| !receipt.items.contains(&e.item_id));
        self.actions
            .observations
            .retain(|o| !receipt.items.contains(&o.item_id));
        self.feedback.records.retain(|r| !binding.matches(&r.job));
        self.feedback.mentions.retain(|m| {
            !(m.binding.configuration_id == binding.configuration_id
                && m.binding.account_id == binding.account_id
                && m.binding.repository_id == binding.repository_id
                && m.binding.pull_request_id == binding.pull_request_id)
        });
        self.notifications
            .notices
            .retain(|n| !receipt.notices.contains_key(&n.id));
        self.notifications.observations.retain(|source, _| {
            !receipt
                .items
                .iter()
                .any(|item| source == &format!("queue:{item}"))
        });
        self.queue.jobs.retain(|j| !binding.matches(j));
        // Follow-ups first: legacy decoding may need the original publication.
        // Repeating every write is intentional; the journal is the recovery fence.
        store.write_state("follow-ups.json", &self.follow_ups)?;
        store.write_state("publications.json", &self.publications)?;
        store.write_state("reviews.json", &self.reviews)?;
        store.write_state("actions.json", &self.actions)?;
        store.write_state("feedback.json", &self.feedback)?;
        store.write_state("notifications.json", &self.notifications)?;
        store.write_state("queue.json", &self.queue)?;
        super::paging::discard(store, &receipt.items)
    }
}

/// Startup-only, before any operational restore or worker. An applying journal
/// is a local serialization fence, not a claim of provider/server atomicity.
pub fn recover(store: &Store) -> Result<(), String> {
    store.recover_state_writes()?;
    let mut ledger = load(store)?;
    let Some(pending) = ledger.pending.clone() else {
        return Ok(());
    };
    if !pending.applying {
        ledger.pending = None;
        return save(store, &ledger);
    }
    State::read(store)?.compact(store, &pending.receipt)?;
    ledger.receipts.push(pending.receipt);
    ledger.pending = None;
    save(store, &ledger)?;
    refresh(store)
}

/// The caller owns the Store lock and must exclude live workers/scan snapshots.
/// A false quiescence proof never starts or advances destructive cleanup.
pub fn maintain(store: &Store, workers_quiescent: bool) -> Result<usize, String> {
    if !workers_quiescent {
        return Ok(0);
    }
    recover(store)?;
    if store
        .load_monitoring_state()?
        .health
        .values()
        .any(|h| h.in_flight)
    {
        return Ok(0);
    }
    let scopes = store.load_queue_state()?.tracked;
    let mut count = 0;
    for scope in scopes {
        if !scope.terminal_observed || scope.lifecycle == Lifecycle::Open {
            continue;
        }
        let mut ledger = load(store)?;
        if ledger.receipts.iter().any(|r| {
            r.scope.iteration_id == scope.iteration_id
                && Binding::tracked(&r.scope) == Binding::tracked(&scope)
        }) {
            continue;
        }
        let state = State::read(store)?;
        let binding = Binding::tracked(&scope);
        let has_origin = |id: &str| {
            state.publications.iter().any(|p| p.id == id)
                || ledger
                    .receipts
                    .iter()
                    .flat_map(|r| &r.owned)
                    .any(|o| o.proof.publication_id == id)
        };
        if state
            .feedback
            .records
            .iter()
            .filter(|r| binding.matches(&r.job))
            .any(|r| !has_origin(&r.context.publication_id))
            || state
                .follow_ups
                .iter()
                .filter(|f| binding.matches(&f.context.job))
                .any(|f| match &f.target {
                    crate::follow_up::ConversationTarget::Owned(origin) => {
                        !has_origin(&origin.publication_id)
                    }
                    crate::follow_up::ConversationTarget::Retained(origin) => {
                        !has_origin(&origin.proof.publication_id)
                    }
                    _ => false,
                })
        {
            return Err("Terminal cleanup needs the missing owned-publication provenance. Detail was retained; repair the original receipt.".into());
        }
        let Some(mut receipt) = state.receipt(&scope)? else {
            continue;
        };
        for notice in &state.notifications.notices {
            if let Destination::QueueItem { item_id } = &notice.event.destination {
                if receipt.items.contains(item_id) {
                    receipt.notices.insert(notice.id.clone(), item_id.clone());
                }
            }
        }
        // Carry forward sticky human closures without retaining active bodies.
        for old in &mut ledger.receipts {
            for origin in &mut old.owned {
                origin.human_input_threads.extend(
                    state
                        .follow_ups
                        .iter()
                        .filter(|f| f.phase == crate::follow_up::Phase::HumanInputRequired)
                        .filter_map(|f| f.thread().ok())
                        .filter(|t| t.owned_by(&origin.proof))
                        .map(|t| t.id.clone()),
                );
                origin.closed_roots.extend(
                    state
                        .feedback
                        .records
                        .iter()
                        .filter(|r| {
                            r.context.publication_id == origin.proof.publication_id
                                && r.context.closed
                        })
                        .map(|r| r.context.root_id.clone()),
                );
            }
        }
        ledger.pending = Some(Pending {
            receipt,
            applying: false,
        });
        save(store, &ledger)?;
        apply_prepared(store)?;
        count += 1;
    }
    Ok(count)
}

fn apply_prepared(store: &Store) -> Result<(), String> {
    let mut ledger = load(store)?;
    let pending = ledger
        .pending
        .as_mut()
        .ok_or("Cleanup intent disappeared.")?;
    let current = store
        .load_queue_state()?
        .tracked
        .into_iter()
        .find(|p| Binding::tracked(p) == Binding::tracked(&pending.receipt.scope));
    if current.as_ref().is_none_or(|p| {
        !p.terminal_observed
            || p.lifecycle == Lifecycle::Open
            || p.iteration_id != pending.receipt.scope.iteration_id
    }) {
        ledger.pending = None;
        return save(store, &ledger);
    }
    pending.applying = true;
    save(store, &ledger)?;
    recover(store)
}
