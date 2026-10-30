pub(crate) mod host;
pub(crate) mod packet;

use crate::{
    feedback,
    github::actions::{Action, Observation, Receipt},
    monitoring::{self, JobOperation, OperationState, QueueJob},
    queue::{self, State},
    review::{Failure, ReviewRun, Selection},
    storage::{ActionPermissions, Repository, Store},
};
pub(crate) use packet::adoptable_runs;
pub use packet::{adopt as adopt_packet, PacketApproval, Ready};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Basis {
    pub activation_version: String,
    pub item_id: String,
    pub iteration_id: String,
    pub repository: Repository,
    pub job: QueueJob,
    pub selection: Selection,
    pub peers: Vec<ReviewRun>,
    pub feedback: Vec<feedback::Context>,
    pub conversations: Vec<crate::follow_up::FollowUp>,
    pub permissions: ActionPermissions,
    /// Retained for historical decoding only, not a permission.
    #[serde(default)]
    pub trust_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalReview {
    pub id: String,
    pub enqueue_order: u64,
    pub basis: Basis,
    pub observation: Observation,
    pub execution: ReviewRun,
    pub cancelled: bool,
    #[serde(default)]
    pub attempts: Vec<ReviewRun>,
    /// Set when a Packet-mode approve exit created this record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet: Option<PacketApproval>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
    Prepared,
    Uncertain,
    Confirmed,
    Rejected,
    Stale,
    ExternalMerge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub reconcile_attempts: u8,
    #[serde(default)]
    pub reconcile_requested: bool,
    pub id: String,
    pub final_id: String,
    pub item_id: String,
    pub action: Action,
    pub operation: JobOperation,
    pub observation: Observation,
    pub body: String,
    pub state: EffectState,
    pub error: Option<String>,
    pub receipt: Option<Receipt>,
}

impl Effect {
    pub fn needs_reconciliation(&self) -> bool {
        self.state == EffectState::Uncertain
            || self.state == EffectState::Confirmed && self.error.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observed {
    #[serde(default)]
    pub failures: u8,
    #[serde(default)]
    pub retry_at: Option<i64>,
    pub item_id: String,
    pub at: i64,
    pub observation: Option<Observation>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub finals: Vec<FinalReview>,
    pub effects: Vec<Effect>,
    pub observations: Vec<Observed>,
}

fn same_scope(effect: &Effect, job: &QueueJob) -> bool {
    effect.observation.account_id == job.account_id
        && effect.observation.repository_id == job.repository_id
        && effect.observation.pull_request_id == job.pull_request_id
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub item_id: String,
    pub final_valid: bool,
    pub provider_observed_at: Option<i64>,
    pub observation_retry_blocker: Option<String>,
    pub primary_assignment_id: Option<String>,
    pub machine_clear: bool,
    pub personal_review: &'static str,
    pub permissions: ActionPermissions,
    pub provider_approval: Option<crate::github::actions::ProviderReview>,
    pub final_review: Option<FinalReview>,
    pub effects: Vec<Effect>,
    pub blockers: Vec<String>,
}

fn context(observation: &Observation, effects: &[Effect]) -> serde_json::Value {
    let reviews: Vec<_> = observation
        .reviews
        .iter()
        .filter(|r| {
            !effects.iter().any(|e| {
                e.action == Action::Approve
                    && r.state == "APPROVED"
                    && e.observation.account_id == r.actor_id
                    && e.observation.head == r.head
                    && e.body == r.body
                    && e.receipt.as_ref().is_some_and(|receipt| receipt.id == r.id)
            })
        })
        .collect();
    serde_json::json!({"node_id":observation.node_id,"repository_id":observation.repository_id,"pull_request_id":observation.pull_request_id,
        "account_id":observation.account_id,"author_id":observation.author_id,"head_repository_id":observation.head_repository_id,"head":observation.head,"base":observation.base,
        "state":observation.state,"draft":observation.draft,"permission":observation.permission,"write_capability":observation.write_capability,"protection":observation.protection,
        "base_name":observation.base_name,"merge_rules":observation.merge_rules,
        "threads":observation.threads,"reviews":reviews,"comments":observation.comments})
}

pub fn observation_matches(run: &FinalReview, current: &Observation, effects: &[Effect]) -> bool {
    context(&run.observation, effects) == context(current, effects)
}

pub fn synchronize(
    store: &Store,
    item_id: &str,
    observation: Result<Observation, Failure>,
    now: i64,
) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let jobs = store.load_queue()?;
    let identity = jobs
        .iter()
        .find(|j| queue::item_id(j) == item_id)
        .map(|j| (&j.account_id, &j.repository_id, &j.pull_request_id))
        .or_else(|| {
            ledger
                .finals
                .iter()
                .find(|f| f.basis.item_id == item_id)
                .map(|f| {
                    (
                        &f.basis.job.account_id,
                        &f.basis.job.repository_id,
                        &f.basis.job.pull_request_id,
                    )
                })
        });
    let observation = observation.and_then(|value| {
        if identity.is_none_or(|(account, repository, pull)| {
            *account != value.account_id
                || *repository != value.repository_id
                || *pull != value.pull_request_id
        }) {
            return Err(Failure::permanent(
                "Provider observation does not match this recorded account/repository/PR.",
            ));
        }
        Ok(value)
    });
    let previous_failures = ledger
        .observations
        .iter()
        .find(|o| o.item_id == item_id)
        .map(|o| o.failures)
        .unwrap_or(0);
    let account = identity.map(|(account, _, _)| account.as_str());
    // A rate limit is a wait: no failure counted, no error stored.
    let paced = match (&observation, account) {
        (Err(error), Some(account)) if error.rate_limited() => Some(
            crate::backpressure::record(
                store,
                crate::backpressure::Scope::of(error.origin, account, None),
                error.retry_after_seconds,
                item_id,
                now,
            )
            .unwrap_or_else(|_| {
                now.saturating_add(crate::backpressure::delay_seconds(
                    item_id,
                    0,
                    error.retry_after_seconds,
                ))
            }),
        ),
        _ => None,
    };
    if let (Ok(_), Some(account)) = (&observation, account) {
        let _ = crate::backpressure::succeeded(store, &crate::backpressure::Scope::github(account));
    }
    let observed = Observed {
        failures: match (&observation, paced) {
            (Ok(_), _) => 0,
            (Err(_), Some(_)) => previous_failures,
            (Err(_), None) => previous_failures.saturating_add(1),
        },
        retry_at: paced.or_else(|| {
            observation.as_ref().err().and_then(|error| {
                let policy = crate::retry::TRANSIENT;
                (error.kind != monitoring::OperationFailure::Permanent
                    && !policy.exhausted(previous_failures.saturating_add(1)))
                .then(|| {
                    now.saturating_add(policy.wait(
                        item_id,
                        u32::from(previous_failures),
                        error.retry_after_seconds,
                    ))
                })
            })
        }),
        item_id: item_id.into(),
        at: now,
        observation: observation.as_ref().ok().cloned(),
        error: observation
            .as_ref()
            .err()
            .filter(|_| paced.is_none())
            .map(|e| e.message.clone()),
    };
    if let Some(previous) = ledger
        .observations
        .iter_mut()
        .find(|o| o.item_id == item_id)
    {
        *previous = observed
    } else {
        ledger.observations.push(observed);
    }
    let terminal = observation
        .as_ref()
        .ok()
        .filter(|o| o.state == "MERGED" || o.state == "CLOSED")
        .cloned();
    if let Ok(observation) = observation {
        for effect in ledger
            .effects
            .iter_mut()
            .filter(|e| e.item_id == item_id && matches!(e.state, EffectState::Uncertain))
        {
            reconcile(effect, &observation)?;
        }
        for effect in ledger.effects.iter_mut().filter(|e| {
            e.item_id == item_id && e.state == EffectState::Confirmed && e.error.is_some()
        }) {
            if effect.action == Action::Approve
                && observation.head == effect.observation.head
                && observation.base == effect.observation.base
                && effect.receipt.as_ref().is_some_and(|receipt| {
                    observation.reviews.iter().any(|r| {
                        r.id == receipt.id
                            && r.actor_id == receipt.actor_id
                            && r.head == receipt.head
                            && r.body == effect.body
                            && r.state == "APPROVED"
                    })
                })
            {
                effect.error = None;
            }
            if effect.action == Action::Merge
                && observation.state == "MERGED"
                && observation.head == effect.observation.head
                && effect.receipt.as_ref().is_some_and(|r| {
                    observation.merge_commit == r.merge_commit
                        && observation.merged_by.as_deref() == Some(r.actor_id.as_str())
                })
            {
                effect.error = None;
            }
        }
    }
    store.save_actions(&ledger)?;
    if let Some(observation) = terminal {
        record_terminal(store, item_id, &observation, now)?;
    }
    Ok(())
}

/// Legacy finals (no packet approval) are historical: they authorize nothing.
pub(crate) const LEGACY_FINAL_RETIRED: &str = "Legacy final review retired.";

pub fn validate_local(store: &Store, run: &FinalReview) -> Result<(), String> {
    match run.packet {
        Some(_) => packet::validate_local(store, run),
        None => Err(LEGACY_FINAL_RETIRED.into()),
    }
}

pub fn ready(
    store: &Store,
    run: &FinalReview,
    observation: &Observation,
    action: Action,
) -> Result<(), String> {
    if run.packet.is_none() {
        return Err(LEGACY_FINAL_RETIRED.into());
    }
    match packet::ready(store, run, observation, action)? {
        Ready::Go => Ok(()),
        Ready::AlreadyApproved => Err(packet::ALREADY_APPROVED.into()),
    }
}

pub(crate) fn provider_approval(
    observation: &Observation,
) -> Option<&crate::github::actions::ProviderReview> {
    let mut reviewers = std::collections::HashSet::new();
    observation.reviews.iter().rev().find(|review| {
        matches!(
            review.state.as_str(),
            "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED"
        ) && reviewers.insert(&review.actor_id)
            && review.state == "APPROVED"
            && review.actor_id != observation.author_id
            && review.head == observation.head
            && [&review.id, &review.actor_id].iter().all(|id| {
                id.parse::<u64>()
                    .is_ok_and(|value| value > 0 && value.to_string() == **id)
            })
            && review
                .submitted_at
                .as_deref()
                .is_some_and(|time| chrono::DateTime::parse_from_rfc3339(time).is_ok())
    })
}

pub fn prepare_effect(
    store: &Store,
    final_id: &str,
    action: Action,
    current: &Observation,
    now: i64,
) -> Result<Effect, String> {
    crate::capacity::publication_gate(store)?;
    let mut ledger = store.load_actions()?;
    let run = ledger
        .finals
        .iter()
        .find(|f| f.id == final_id)
        .ok_or("Final review unavailable.")?;
    ready(store, run, current, action)?;
    if ledger
        .effects
        .iter()
        .any(|e| e.item_id == run.basis.item_id && e.action == action)
    {
        return Err("This iteration/account already has an action intent. Reconcile the original effect; do not create another.".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let mut operation = JobOperation::review(&run.basis.job, now);
    operation.operation_type = if action == Action::Approve {
        "github_approve"
    } else {
        "github_merge"
    }
    .into();
    let effect=Effect{cancelled:false,reconcile_attempts:0,reconcile_requested:false,id:id.clone(),final_id:run.id.clone(),item_id:run.basis.item_id.clone(),action,operation,
        observation:current.clone(),body:match &run.packet {
            Some(_) => packet::approval_body(&id, &run.basis.repository.provider),
            None => format!("Automated primary final review by PR Sniper.\n\nThis is the acting account's automated vote, not personal review.\n\n<!-- pr-sniper:action:{id} -->\n\n{}", crate::exit_writes::SIGNATURE),
        },
        state:EffectState::Prepared,error:None,receipt:None};
    ledger.effects.push(effect.clone());
    store.save_actions(&ledger)?;
    Ok(effect)
}

pub fn mark_intent(store: &Store, id: &str, now: i64) -> Result<Effect, String> {
    crate::capacity::publication_gate(store)?;
    let mut ledger = store.load_actions()?;
    let effect = ledger
        .effects
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Action unavailable.")?;
    if effect.state != EffectState::Prepared || effect.cancelled {
        return Err(
            "An attempted or cancelled mutation can only be reconciled, never resent.".into(),
        );
    }
    if let Err(error) = effect.operation.begin_attempt(now) {
        effect.state = EffectState::Stale;
        effect.error = Some(error.clone());
        store.save_actions(&ledger)?;
        return Err(error);
    }
    effect.operation.attempted_mutation = Some(format!("{:?}", effect.action));
    effect.state = EffectState::Uncertain;
    let result = effect.clone();
    store.save_actions(&ledger)?;
    Ok(result)
}

pub fn cancel_effect(store: &Store, id: &str) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let effect = ledger
        .effects
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Action unavailable.")?;
    if matches!(
        effect.state,
        EffectState::Confirmed | EffectState::ExternalMerge
    ) {
        return Err("A confirmed provider effect cannot be undone.".into());
    }
    effect.cancelled = true;
    if effect.state == EffectState::Prepared {
        effect.state = EffectState::Stale;
    }
    effect.error=Some("Action cancelled. Any already-started request still requires reconciliation; no remote undo is promised.".into());
    store.save_actions(&ledger)
}

/// Whether `expire_prepared` would change anything; read-only.
pub fn prepared_expired(store: &Store, now: i64) -> Result<bool, String> {
    Ok(store
        .load_actions()?
        .effects
        .iter()
        .any(|e| e.state == EffectState::Prepared && now >= e.operation.retry_deadline))
}

pub fn expire_prepared(store: &Store, now: i64) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let mut changed = false;
    for effect in ledger
        .effects
        .iter_mut()
        .filter(|e| e.state == EffectState::Prepared && now >= e.operation.retry_deadline)
    {
        effect.state = EffectState::Stale;
        effect.operation.state = OperationState::ManualRetry;
        effect.operation.next_attempt_at = None;
        effect.error = Some(
            "Provider action deadline expired before transmission; no request was sent.".into(),
        );
        changed = true;
    }
    if changed {
        store.save_actions(&ledger)?;
    }
    Ok(())
}

pub fn reconcile(effect: &mut Effect, observation: &Observation) -> Result<(), String> {
    if observation.account_id != effect.observation.account_id
        || observation.pull_request_id != effect.observation.pull_request_id
        || observation.repository_id != effect.observation.repository_id
    {
        return Err("Action reconciliation identity changed.".into());
    }
    match effect.action {
        Action::Approve => {
            let matches: Vec<_> = observation
                .reviews
                .iter()
                .filter(|r| {
                    r.actor_id == effect.observation.account_id
                        && r.head == effect.observation.head
                        && r.body == effect.body
                        && r.state == "APPROVED"
                        // A vote with no text (Azure DevOps) is never pending.
                        && (r.submitted_at.is_some() || effect.body.is_empty())
                })
                .collect();
            if matches.len() > 1 {
                return Err(
                    "Multiple provider approvals match one intent; manual reconciliation required."
                        .into(),
                );
            }
            if let Some(review) = matches.first() {
                accept(
                    effect,
                    Receipt {
                        id: review.id.clone(),
                        actor_id: review.actor_id.clone(),
                        head: review.head.clone(),
                        action: Action::Approve,
                        merge_commit: None,
                    },
                );
            } else {
                effect.error = Some(
                    "Approval outcome remains unresolved; no replacement vote will be submitted."
                        .into(),
                );
            }
        }
        Action::Merge if observation.state == "MERGED" => {
            // A provider lifecycle read proves merge, not attribution to a lost mutation.
            effect.state = EffectState::ExternalMerge;
            effect.error=Some("GitHub confirms merged; attribution to this unconfirmed request is unknown. No replacement merge is sent.".into());
            effect.operation.state = OperationState::Completed;
            effect.operation.next_attempt_at = None;
        }
        Action::Merge => {
            effect.error = Some(
                "Merge outcome remains unresolved; no replacement request will be submitted."
                    .into(),
            )
        }
    }
    Ok(())
}

pub fn accept(effect: &mut Effect, receipt: Receipt) {
    effect.operation.confirmed_receipt = Some(receipt.id.clone());
    effect.operation.state = OperationState::Completed;
    effect.operation.next_attempt_at = None;
    effect.state = EffectState::Confirmed;
    effect.receipt = Some(receipt);
    effect.error = None;
}

pub fn finish_effect(
    store: &Store,
    id: &str,
    operation_id: &str,
    result: Result<Receipt, crate::publication::WriteFailure>,
) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let effect = ledger
        .effects
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Action disappeared.")?;
    if effect.state != EffectState::Uncertain || effect.operation.id != operation_id {
        return Err("Action outcome no longer owns this intent.".into());
    }
    match result {
        Ok(receipt) => {
            if receipt.action != effect.action
                || receipt.actor_id != effect.observation.account_id
                || receipt.head != effect.observation.head
            {
                return Err("Provider receipt does not match this frozen action.".into());
            }
            accept(effect, receipt);
        }
        Err(error) => {
            effect.state = if error.uncertain {
                EffectState::Uncertain
            } else {
                EffectState::Rejected
            };
            effect.operation.state = OperationState::ManualRetry;
            effect.operation.next_attempt_at = None;
            effect.error = Some(error.failure.message);
        }
    }
    let merged = effect
        .receipt
        .as_ref()
        .filter(|r| r.action == Action::Merge)
        .map(|receipt| {
            let mut observation = effect.observation.clone();
            observation.state = "MERGED".into();
            observation.merged_by = Some(receipt.actor_id.clone());
            observation.merge_commit = receipt.merge_commit.clone();
            observation
        });
    let terminal_item = effect.item_id.clone();
    store.save_actions(&ledger)?;
    if let Some(observation) = merged {
        record_terminal(store, &terminal_item, &observation, crate::now_seconds()?)?;
    }
    Ok(())
}

pub fn verify_after_effect(
    store: &Store,
    effect: &Effect,
    observed: Result<Observation, Failure>,
) -> Result<(), Failure> {
    let checked = observed.map_err(|e| e.message).and_then(|current| {
        if current.account_id != effect.observation.account_id
            || current.repository_id != effect.observation.repository_id
            || current.pull_request_id != effect.observation.pull_request_id
            || current.head != effect.observation.head
            || current.base != effect.observation.base
        {
            return Err("Provider identity or revision changed after the confirmed action.".into());
        }
        if effect.action == Action::Approve {
            let ledger = store.load_actions()?;
            let run = ledger
                .finals
                .iter()
                .find(|f| f.id == effect.final_id)
                .ok_or("Original final review unavailable.")?;
            ready(store, run, &current, Action::Approve)?;
        } else if current.state != "MERGED" {
            return Err(
                "Provider merge receipt could not be corroborated after the request.".into(),
            );
        }
        Ok(())
    });
    let mut ledger = store.load_actions().map_err(Failure::permanent)?;
    let record = ledger
        .effects
        .iter_mut()
        .find(|e| e.id == effect.id && e.operation.id == effect.operation.id)
        .ok_or_else(|| Failure::permanent("Post-action verification lost operation ownership."))?;
    if let Err(reason) = &checked {
        record.error = Some(format!(
            "Provider receipt retained; post-action validation failed: {reason}"
        ));
    }
    store.save_actions(&ledger).map_err(Failure::permanent)?;
    checked.map_err(Failure::permanent)
}

fn record_terminal(
    store: &Store,
    item_id: &str,
    observation: &Observation,
    now: i64,
) -> Result<(), String> {
    let mut queue = store.load_queue_state()?;
    let Some(scope) = queue
        .tracked
        .iter()
        .find(|p| {
            p.item_id == item_id
                && p.account_id == observation.account_id
                && p.repository_id == observation.repository_id
                && p.pull_request_id == observation.pull_request_id
                && p.head_sha == observation.head
        })
        .cloned()
    else {
        // An old action observation cannot close a new same-head incarnation.
        return Err("Terminal observation belongs to an unavailable or newer PR incarnation; no current work was retired.".into());
    };
    let binding = crate::retention::Binding::tracked(&scope);
    for tracked in queue
        .tracked
        .iter_mut()
        .filter(|p| crate::retention::Binding::tracked(p) == binding)
    {
        tracked.terminal_observed = true;
        tracked.lifecycle = if observation.state == "MERGED" {
            crate::provider::Lifecycle::Merged
        } else {
            crate::provider::Lifecycle::Closed
        };
        tracked.head_sha = observation.head.clone();
        tracked.observed_at = now;
        tracked.observed_sequence = crate::monitoring::next_observation();
    }
    for job in queue.jobs.iter_mut().filter(|j| binding.matches(j)) {
        if !matches!(
            job.waiting.as_str(),
            monitoring::WAITING_CLOSED | monitoring::WAITING_MERGED
        ) {
            job.waiting = if observation.state == "MERGED" {
                monitoring::WAITING_MERGED
            } else {
                monitoring::WAITING_CLOSED
            }
            .into();
        }
    }
    store.save_queue_state(&queue)
}

pub fn project(store: &Store, snapshot: &mut queue::Snapshot) -> Result<(), String> {
    let settings = store.load_settings()?;
    let ledger = store.load_actions()?;
    let paused = store.load_automation()?.paused;
    let now = crate::now_seconds()?;
    for item in &mut snapshot.items {
        let observation_retry_blocker =
            observation_retry_blocker(item, &settings, &ledger, paused, now);
        let Some(repository) = settings
            .repositories
            .iter()
            .find(|r| r.id == item.job.configuration_id)
        else {
            continue;
        };
        let primary = repository.primary_assignment_id();
        let permissions = ActionPermissions {
            include_self: false,
            reply: repository
                .assignments
                .iter()
                .any(|a| repository.assignment_authority(a).reply),
            approve: repository
                .assignments
                .iter()
                .any(|a| repository.assignment_authority(a).approve),
            merge: repository
                .assignments
                .iter()
                .any(|a| repository.assignment_authority(a).merge),
        };
        let final_review = ledger
            .finals
            .iter()
            .rev()
            .find(|f| f.basis.item_id == item.id && f.packet.is_some())
            .cloned();
        let effects: Vec<_> = ledger
            .effects
            .iter()
            .filter(|e| e.item_id == item.id)
            .cloned()
            .collect();
        let mut blockers = Vec::new();
        let mut final_valid = false;
        let observed = ledger.observations.iter().find(|o| o.item_id == item.id);
        if let Some(error) = observed.and_then(|o| o.error.clone()) {
            blockers.push(error);
            if item.state == State::MachineSignedOff
                && (permissions.approve
                    || permissions.merge
                    || ledger.effects.iter().any(|e| {
                        same_scope(e, &item.job)
                            && (e.state == EffectState::Prepared || e.needs_reconciliation())
                    }))
            {
                item.state = State::Failed;
                // Retry refreshes the provider evidence; it never reposts an action.
                item.failed_operation = Some(queue::FailedOperation::ProviderAction);
                item.remedy = Some(queue::Remedy::Retry);
                item.summary="Fresh provider review/policy evidence is unavailable; no action readiness is asserted.";
            }
        }
        if let Some(observation) = observed.and_then(|o| o.observation.as_ref()) {
            for (action, enabled) in [
                (
                    Action::Approve,
                    permissions.approve
                        && !(permissions.merge && provider_approval(observation).is_some()),
                ),
                (Action::Merge, permissions.merge),
            ] {
                if enabled {
                    if let Some(reason) = observation.blocker(action) {
                        blockers.push(reason);
                    }
                }
            }
            if observation.state == "MERGED" && item.state != State::Closed {
                item.state = State::Merged;
                item.summary="GitHub confirms this PR merged. Provider receipts identify any confirmed PR Sniper action; personal review is not inferred.";
            } else if item.state == State::MachineSignedOff {
                // Others' threads never block a packet approve exit (AC-017).
                let packet_approve = matches!(
                    item.exit,
                    Some(
                        crate::exits::Exit::ApprovePullRequest
                            | crate::exits::Exit::ApproveAndMerge
                    )
                );
                if observation.head != item.job.head_sha
                    || item.job.observed_base_sha.as_deref() != Some(&observation.base)
                {
                    item.state = State::Stale;
                    item.summary="Provider head/base changed; this saved review does not establish current clearance.";
                } else if observation.draft {
                    item.state = State::Queued;
                    item.summary = "The PR is draft; automatic approval and merge are blocked.";
                } else if !packet_approve && observation.discussion_blocker().is_some() {
                    item.state = State::WaitingForHuman;
                    item.summary="Provider review or discussion concerns remain outstanding; no whole-cycle clearance is asserted.";
                }
            }
        }
        if primary.is_none()
            && repository
                .assignments
                .iter()
                .any(|a| a.actions.as_ref().is_some_and(|p| p.approve || p.merge))
        {
            blockers
                .push("No repository primary selected; automatic actions are unavailable.".into());
        }
        if let Some(run) = &final_review {
            final_valid = validate_local(store, run).is_ok()
                && run.execution.operation.state == OperationState::Completed
                && run.execution.result.as_ref().is_some_and(|r| {
                    r.output.decision == crate::review::Decision::MachineSignOff
                        && r.output.findings.is_empty()
                        && r.output.held_findings.is_empty()
                        && !r.output.feedback_conflict
                })
                && observed.is_some_and(|o| {
                    o.error.is_none()
                        && o.observation.as_ref().is_some_and(|current| {
                            observation_matches(run, current, &ledger.effects)
                        })
                });
            if let Err(reason) = validate_local(store, run) {
                blockers.push(reason);
            }
            if let Some(reason) = &run.execution.error {
                blockers.push(reason.clone());
            }
            if run.execution.operation.state == OperationState::Completed {
                if let Some(observation) = observed.and_then(|o| o.observation.as_ref()) {
                    for (action, enabled) in [
                        (
                            Action::Approve,
                            permissions.approve
                                && !(permissions.merge && provider_approval(observation).is_some()),
                        ),
                        (Action::Merge, permissions.merge),
                    ] {
                        if enabled && !effects.iter().any(|e| e.action == action) {
                            if let Err(reason) = ready(store, run, observation, action) {
                                blockers.push(reason);
                            }
                        }
                    }
                }
            }
        }
        if effects
            .iter()
            .any(|e| e.state == EffectState::Confirmed && e.action == Action::Merge)
        {
            item.state = State::Merged;
            item.summary="GitHub confirmed the recorded PR Sniper merge. Personal human review is not inferred.";
        } else if ledger
            .effects
            .iter()
            .any(|e| same_scope(e, &item.job) && e.state == EffectState::Uncertain)
            && item.state != State::Merged
            && item.state != State::Closed
        {
            item.state = State::Failed;
            // A lost response is reconciled against its original intent, never reposted.
            item.failed_operation = Some(queue::FailedOperation::ProviderAction);
            item.remedy = Some(queue::Remedy::Reconcile);
            item.summary="A provider action outcome is unresolved. Reconcile its original intent; no replacement request is authorized.";
            blockers.push("An action on this PR has an unresolved provider outcome, possibly from an earlier iteration.".into());
        }
        // The packet final of the item's own exit run: its progress, or a parked toss
        // (a toss under budget re-queued a newer run, so it no longer matches).
        if let Some((run, approval)) = final_review.as_ref().and_then(|f| {
            let packet = f.packet.as_ref()?;
            (!f.cancelled && Some(&packet.run_id) == item.exit_run_id.as_ref())
                .then_some((f, packet))
        }) {
            let own = |e: &&Effect| e.final_id == run.id;
            let tossed = effects
                .iter()
                .filter(own)
                .any(|e| matches!(e.state, EffectState::Rejected | EffectState::Stale));
            let approved = !run.basis.permissions.approve
                || effects
                    .iter()
                    .filter(own)
                    .any(|e| e.action == Action::Approve && e.state == EffectState::Confirmed);
            if approval.settled && tossed && !matches!(item.state, State::Merged | State::Closed) {
                item.state = State::Failed;
                item.failure_class = Some(monitoring::FailureClass::Other);
                item.failure_cause = Some(queue::CAUSE_TOSSED);
                item.failed_operation = Some(queue::FailedOperation::ProviderAction);
                item.remedy = Some(queue::Remedy::Retry);
                item.summary = State::Failed.summary();
            } else if item.progress.is_some() {
                item.progress = if approval.settled || (approved && !run.basis.permissions.merge) {
                    None
                } else if approved {
                    Some(match approval.merge_pending.as_ref().map(|p| p.reason) {
                        Some(crate::exits::gates::MergeBlock::BehindBase) => {
                            queue::Progress::MergePendingBehindBase
                        }
                        Some(crate::exits::gates::MergeBlock::Conflicts) => {
                            queue::Progress::MergePendingConflicts
                        }
                        None => queue::Progress::Merging,
                    })
                } else {
                    Some(queue::Progress::Approving)
                };
            }
        }
        if item.state != State::MachineSignedOff
            && matches!(
                item.progress,
                Some(
                    queue::Progress::Approving
                        | queue::Progress::Merging
                        | queue::Progress::MergePendingBehindBase
                        | queue::Progress::MergePendingConflicts
                )
            )
        {
            item.progress = None;
        }
        let machine_clear = item.state == State::MachineSignedOff;
        // A confirmed approval of the current head ends the handoff; a new head or
        // any unsettled effect (Failed above) keeps or returns the PR to normal flow.
        let approved = machine_clear
            && effects.iter().any(|e| {
                e.action == Action::Approve
                    && e.state == EffectState::Confirmed
                    && !e.needs_reconciliation()
                    && e.receipt
                        .as_ref()
                        .is_some_and(|r| r.head == item.job.head_sha)
            });
        item.action_status = Some(Status {
            item_id: item.id.clone(),
            final_valid,
            provider_observed_at: observed.map(|o| o.at),
            observation_retry_blocker,
            primary_assignment_id: primary.map(str::to_string),
            machine_clear,
            personal_review: if item.state == State::Merged {
                "Not inferred; PR merged."
            } else if machine_clear {
                "Required: review this unmerged PR personally, including after automated approval."
            } else {
                "Not inferred."
            },
            permissions,
            provider_approval: observed
                .and_then(|o| o.observation.as_ref())
                .and_then(provider_approval)
                .cloned(),
            final_review,
            effects,
            blockers,
        });
        if approved {
            item.state = State::Approved;
            item.summary = State::Approved.summary();
        }
    }
    Ok(())
}

pub fn restore(store: &Store) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let before = ledger.clone();
    for run in &mut ledger.finals {
        if run.packet.is_none()
            && !run.cancelled
            && run.execution.operation.state != OperationState::Completed
        {
            run.cancelled = true;
            run.execution.operation.state = OperationState::Failed;
            run.execution.operation.failure = Some(monitoring::OperationFailure::Permanent);
            run.execution.operation.next_attempt_at = None;
            run.execution.operation.ai_attempt = None;
            run.execution.operation.interruption = None;
            run.execution.operation.resume_at = None;
            run.execution.phase = "Cancelled".into();
            run.execution.error = Some(LEGACY_FINAL_RETIRED.into());
            continue;
        }
        if run.execution.operation.state == OperationState::Running {
            if run.execution.operation.interruption.is_some() {
                run.execution.operation.requeue_intentional(0);
            } else {
                run.execution.operation.state = OperationState::Interrupted;
                run.execution.operation.next_attempt_at = Some(0);
            }
        }
    }
    if ledger != before {
        store.save_actions(&ledger)?;
    }
    Ok(())
}

pub fn request_observation_retry(store: &Store, item_id: &str, now: i64) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let snapshot = queue::normal_snapshot(store, vec![])?;
    let item = snapshot
        .items
        .iter()
        .find(|i| i.id == item_id)
        .ok_or("Current PR iteration unavailable.")?;
    if let Some(reason) = observation_retry_blocker(
        item,
        &store.load_settings()?,
        &ledger,
        store.load_automation()?.paused,
        now,
    ) {
        return Err(reason);
    }
    reset_observation_retry(&mut ledger, item_id, now)?;
    store.save_actions(&ledger)
}

fn observation_pending(
    item: &queue::Item,
    settings: &crate::storage::Settings,
    ledger: &Ledger,
    now: i64,
) -> bool {
    ledger.effects.iter().any(|e| {
        e.item_id == item.id
            && (e.state == EffectState::Prepared && now < e.operation.retry_deadline
                || e.needs_reconciliation()
                    && (e.reconcile_requested
                        || e.reconcile_attempts < 3 && now < e.operation.retry_deadline))
    }) || packet::open(ledger, &item.id)
        || item.state == State::MachineSignedOff
            && settings
                .repositories
                .iter()
                .find(|r| r.id == item.job.configuration_id)
                .is_some_and(|r| {
                    r.assignments.iter().any(|a| {
                        let p = r.assignment_authority(a);
                        p.approve
                            && !ledger
                                .effects
                                .iter()
                                .any(|e| e.item_id == item.id && e.action == Action::Approve)
                            || p.merge
                                && !ledger
                                    .effects
                                    .iter()
                                    .any(|e| e.item_id == item.id && e.action == Action::Merge)
                    })
                })
            && !ledger
                .effects
                .iter()
                .any(|e| same_scope(e, &item.job) && e.state == EffectState::Uncertain)
}

fn observation_retry_blocker(
    item: &queue::Item,
    settings: &crate::storage::Settings,
    ledger: &Ledger,
    paused: bool,
    now: i64,
) -> Option<String> {
    let Some(observed) = ledger.observations.iter().find(|o| o.item_id == item.id) else {
        return Some("No action observation is recorded for this iteration.".into());
    };
    if paused {
        return Some("Automation is paused; resume before refreshing provider evidence.".into());
    }
    if observed.retry_at.is_some_and(|at| at > now) {
        return Some(
            "Provider retry backoff is still active; retry after its recorded delay.".into(),
        );
    }
    if !observation_pending(item, settings, ledger, now) {
        return Some(
            "No enabled provider action or pending reconciliation is eligible for refresh.".into(),
        );
    }
    None
}

pub fn request_reconciliation(store: &Store, id: &str, now: i64) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let effect = ledger
        .effects
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Action unavailable.")?;
    if !effect.needs_reconciliation() {
        return Err("Only an unresolved original action needs reconciliation.".into());
    }
    effect.reconcile_requested = true;
    let item_id = effect.item_id.clone();
    reset_observation_retry(&mut ledger, &item_id, now)?;
    store.save_actions(&ledger)
}

fn reset_observation_retry(ledger: &mut Ledger, item_id: &str, now: i64) -> Result<(), String> {
    let observed = ledger
        .observations
        .iter_mut()
        .find(|o| o.item_id == item_id)
        .ok_or("No action observation is recorded for this iteration.")?;
    if observed.retry_at.is_some_and(|at| at > now) {
        return Err(
            "Provider retry backoff is still active; retry after its recorded delay.".into(),
        );
    }
    observed.failures = 0;
    observed.retry_at = Some(now);
    Ok(())
}

#[cfg(test)]
mod tests;
