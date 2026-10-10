pub(crate) mod host;

use crate::{
    feedback,
    github::actions::{Action, Observation, Receipt},
    monitoring::{self, JobOperation, OperationState, QueueJob},
    queue::{self, State},
    review::{Failure, ReviewRun, Selection},
    storage::{ActionPermissions, Repository, Store},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

impl Basis {
    fn same_execution(&self, other: &Self) -> bool {
        // Preserve the full action basis except retired automation preferences and poll cadence.
        let mut current = self.clone();
        current.selection.policy.automatic_agent_start =
            other.selection.policy.automatic_agent_start;
        current.selection.policy.automatic_comment_publication =
            other.selection.policy.automatic_comment_publication;
        current.selection.policy.schedule = other.selection.policy.schedule.clone();
        if let (Some(left), Some(right)) = (
            current.selection.configuration.as_mut(),
            other.selection.configuration.as_ref(),
        ) {
            left.repository.overrides.automatic_agent_start =
                right.repository.overrides.automatic_agent_start;
            left.repository.overrides.automatic_comment_publication =
                right.repository.overrides.automatic_comment_publication;
            left.repository.overrides.schedule = right.repository.overrides.schedule.clone();
        }
        current.repository.overrides.automatic_agent_start =
            other.repository.overrides.automatic_agent_start;
        current.repository.overrides.automatic_comment_publication =
            other.repository.overrides.automatic_comment_publication;
        current.repository.overrides.schedule = other.repository.overrides.schedule.clone();
        if monitoring::actionable(&current.job) && monitoring::actionable(&other.job) {
            current.job.waiting = other.job.waiting.clone();
        }
        current == *other
    }
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

pub fn basis(store: &Store, item_id: &str) -> Result<Basis, String> {
    let settings = store.load_settings()?;
    let snapshot = queue::normal_snapshot(store, vec![])?;
    let item = snapshot
        .items
        .iter()
        .find(|i| i.id == item_id)
        .ok_or("Current PR iteration unavailable.")?;
    if item.state != State::MachineSignedOff {
        return Err("All current normal passes and outstanding concerns must be clear before final review or action.".into());
    }
    let tracked = snapshot
        .tracked
        .iter()
        .find(|p| p.item_id == item_id && p.lifecycle == crate::github::metadata::Lifecycle::Open)
        .ok_or("This is not the currently tracked open iteration.")?;
    let repository = settings
        .repositories
        .iter()
        .find(|r| r.id == item.job.configuration_id)
        .ok_or("Repository removed.")?
        .clone();
    let activation_version = store
        .load_monitoring_state()?
        .activations
        .get(&repository.id)
        .ok_or("Monitoring scope is not active.")?
        .version
        .clone();
    let primary = repository
        .primary_assignment_id()
        .ok_or("No primary assigned; automatic approval and merge are unavailable.")?;
    let mut peers = Vec::new();
    for assignment in &repository.assignments {
        let candidate = snapshot
            .reviews
            .iter()
            .find(|r| {
                r.assignment_id == assignment.id
                    && queue::item_id(&r.job) == item_id
                    && r.job.work.as_ref().is_some_and(|w| {
                        w.agent_id == assignment.agent_id && w.iteration_id == tracked.iteration_id
                    })
            })
            .ok_or("A current assignment is awaiting its normal pass at the next global scan.")?;
        let run = candidate
            .run
            .as_ref()
            .filter(|r| r.operation.state == OperationState::Completed && r.result.is_some())
            .ok_or("A current assignment has not completed its normal pass.")?;
        if run.selection.agent.id != assignment.agent_id {
            return Err(
                "An assignment changed its Agent; old evidence cannot authorize an action.".into(),
            );
        }
        peers.push(run.clone());
    }
    let selected = peers
        .iter()
        .find(|r| r.assignment_id == primary)
        .ok_or("The primary normal pass is missing.")?;
    let job = snapshot
        .jobs
        .iter()
        .find(|j| selected.matches_job(j))
        .ok_or("Primary job unavailable.")?
        .clone();
    let selection = Selection::resolve(&settings, &job, primary)?;
    let authority = repository
        .assignments
        .iter()
        .find(|a| a.id == primary)
        .map(|a| repository.assignment_authority(a))
        .ok_or("Primary unavailable.")?;
    let permissions = ActionPermissions {
        reply: authority.reply,
        approve: authority.approve,
        merge: authority.merge,
    };
    let feedback = feedback::contexts(store, &job, &selection.agent.id).map_err(|e| e.message)?;
    let trust_confirmed = selected.trust_confirmed;
    let conversations = store
        .load_follow_ups()?
        .into_iter()
        .filter(|r| feedback::same_pr(&r.context.job, &job))
        .collect();
    Ok(Basis {
        activation_version,
        item_id: item_id.into(),
        iteration_id: tracked.iteration_id.clone(),
        repository,
        job,
        selection,
        peers,
        feedback,
        conversations,
        permissions,
        trust_confirmed,
    })
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

fn fingerprint(
    basis: &Basis,
    observation: &Observation,
    effects: &[Effect],
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(basis, context(observation, effects)))
        .map_err(|_| "Cannot encode final-review identity.")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(
            "Final review context exceeds the supported bound; evidence was not truncated.".into(),
        );
    }
    Ok(format!("final-{:x}", Sha256::digest(bytes)))
}

pub fn validate_observation(basis: &Basis, observation: &Observation) -> Result<(), String> {
    if observation.account_id != basis.job.account_id
        || observation.repository_id != basis.job.repository_id
        || observation.pull_request_id != basis.job.pull_request_id
        || observation.head != basis.job.head_sha
        || basis.job.observed_base_sha.as_deref() != Some(&observation.base)
    {
        return Err("Provider identity or head/base differs from the cleared iteration.".into());
    }
    if let Some(reason) = observation.common_blocker() {
        return Err(reason);
    }
    if basis.feedback.iter().any(|c| {
        !c.closed
            && c.thread.as_ref().is_some_and(|t| {
                observation
                    .threads
                    .iter()
                    .any(|current| current.id == t.id && current.resolved)
            })
    }) {
        return Err("Owned discussion closure needs the full verified thread observation at the next repository scan before final review.".into());
    }
    Ok(())
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
    let observed = Observed {
        failures: if observation.is_err() {
            ledger
                .observations
                .iter()
                .find(|o| o.item_id == item_id)
                .map(|o| o.failures)
                .unwrap_or(0)
                .saturating_add(1)
        } else {
            0
        },
        retry_at: observation.as_ref().err().and_then(|error| {
            let failures = ledger
                .observations
                .iter()
                .find(|o| o.item_id == item_id)
                .map(|o| o.failures)
                .unwrap_or(0);
            (error.kind != monitoring::OperationFailure::Permanent && failures < 3).then(|| {
                now.saturating_add(error.retry_after_seconds.unwrap_or(5 * (1_i64 << failures)))
            })
        }),
        item_id: item_id.into(),
        at: now,
        observation: observation.as_ref().ok().cloned(),
        error: observation.as_ref().err().map(|e| e.message.clone()),
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
        if let Ok(basis) = basis(store, item_id) {
            let scope = monitoring::Monitor::restore_readonly(store.load_monitoring_state()?)
                .activation_status(&store.load_settings()?, &basis.job.configuration_id)
                .active;
            let needs_action = (basis.permissions.approve
                && !ledger
                    .effects
                    .iter()
                    .any(|e| e.item_id == item_id && e.action == Action::Approve))
                || (basis.permissions.merge
                    && !ledger
                        .effects
                        .iter()
                        .any(|e| e.item_id == item_id && e.action == Action::Merge));
            let human_final = ledger.finals.iter().any(|f| {
                f.basis.item_id == item_id
                    && f.execution.result.as_ref().is_some_and(|r| {
                        r.output.decision != crate::review::Decision::MachineSignOff
                            || r.output.feedback_conflict
                            || !r.output.findings.is_empty()
                            || !r.output.held_findings.is_empty()
                    })
            });
            if !ledger
                .effects
                .iter()
                .any(|e| same_scope(e, &basis.job) && e.state == EffectState::Uncertain)
                && !human_final
                && scope
                && needs_action
                && validate_observation(&basis, &observation).is_ok()
                && (basis.permissions.merge || observation.blocker(Action::Approve).is_none())
            {
                let id = fingerprint(&basis, &observation, &ledger.effects)?;
                if !ledger.finals.iter().any(|f| {
                    f.id == id
                        || (f.basis.same_execution(&basis)
                            && observation_matches(f, &observation, &ledger.effects))
                }) {
                    let operation = {
                        let mut o = JobOperation::review(&basis.job, now);
                        o.operation_type = "primary_final_review".into();
                        o
                    };
                    let execution = ReviewRun {
                        feedback_context: Some(basis.feedback.clone()),
                        key: id.clone(),
                        assignment_id: basis
                            .job
                            .assignment_id
                            .clone()
                            .ok_or("Primary assignment unavailable.")?,
                        job: basis.job.clone(),
                        selection: basis.selection.clone(),
                        operation,
                        manual_start: false,
                        trust_confirmed: basis.trust_confirmed,
                        phase: "Waiting for primary final full review".into(),
                        error: None,
                        result: None,
                    };
                    ledger.finals.push(FinalReview {
                        id,
                        enqueue_order: store.allocate_enqueue_order()?,
                        basis,
                        observation,
                        execution,
                        cancelled: false,
                        attempts: Vec::new(),
                    });
                }
            }
        }
    }
    store.save_actions(&ledger)?;
    if let Some(observation) = terminal {
        record_terminal(store, item_id, &observation, now)?;
    }
    Ok(())
}

pub fn validate_local(store: &Store, run: &FinalReview) -> Result<(), String> {
    if run.cancelled {
        return Err("Final review was cancelled.".into());
    }
    if !basis(store, &run.basis.item_id)?.same_execution(&run.basis) {
        return Err(
            "Current review, feedback, assignment or permissions changed after the final snapshot."
                .into(),
        );
    }
    let monitor = store.load_monitoring_state()?;
    let settings = store.load_settings()?;
    let monitor = monitoring::Monitor::restore_readonly(monitor);
    if !monitor
        .activation_status(&settings, &run.basis.job.configuration_id)
        .active
    {
        return Err("Monitoring scope is not active.".into());
    }
    Ok(())
}

pub fn ready(
    store: &Store,
    run: &FinalReview,
    observation: &Observation,
    action: Action,
) -> Result<(), String> {
    validate_local(store, run)?;
    validate_observation(&run.basis, observation)?;
    if store.load_actions()?.effects.iter().any(|e| {
        e.final_id == run.id
            && e.action != action
            && e.state == EffectState::Confirmed
            && e.error.is_some()
    }) {
        return Err("The preceding confirmed action still needs post-action verification before another action.".into());
    }
    if store.load_actions()?.effects.iter().any(|e| {
        same_scope(e, &run.basis.job)
            && e.state == EffectState::Uncertain
            && !(e.final_id == run.id
                && e.action == action
                && e.operation.state == OperationState::Running)
    }) {
        return Err("Another provider action has an unresolved outcome.".into());
    }
    if !observation_matches(run, observation, &store.load_actions()?.effects) {
        return Err("Provider review/discussion state changed after the final review.".into());
    }
    if action == Action::Approve {
        let existing = observation.reviews.iter().rev().find(|r| {
            r.actor_id == observation.account_id
                && matches!(
                    r.state.as_str(),
                    "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED"
                )
        });
        if let Some(review) =
            existing.filter(|r| r.state == "APPROVED" && r.head == observation.head)
        {
            if !store.load_actions()?.effects.iter().any(|e| {
                e.receipt
                    .as_ref()
                    .is_some_and(|r| r.action == Action::Approve && r.id == review.id)
            }) {
                return Err("The acting account already has an approval on this head; another automated vote is unnecessary.".into());
            }
        }
    }
    let result = run
        .execution
        .result
        .as_ref()
        .filter(|_| run.execution.operation.state == OperationState::Completed)
        .ok_or("The primary final full review has not completed.")?;
    if result.reviewed_base_sha.as_deref() != Some(&observation.base)
        || result.output.decision != crate::review::Decision::MachineSignOff
        || !result.output.findings.is_empty()
        || !result.output.held_findings.is_empty()
        || result.output.feedback_conflict
    {
        return Err("The primary final review requires human input or found new concerns.".into());
    }
    if !match action {
        Action::Approve => run.basis.permissions.approve,
        Action::Merge => run.basis.permissions.merge,
    } {
        return Err("This provider action is not opted in.".into());
    }
    if action == Action::Merge && provider_approval(observation).is_none() {
        return Err(
            "Merge requires confirmed current-revision provider approval before transmission."
                .into(),
        );
    }
    if let Some(reason) = observation.blocker(action) {
        return Err(reason);
    }
    Ok(())
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
        observation:current.clone(),body:format!("Automated primary final review by PR Sniper.\n\nThis is the acting account's automated vote, not personal review.\n\n<!-- pr-sniper:action:{id} -->\n\nPR Sniper"),
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
                        && r.submitted_at.is_some()
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
            crate::github::metadata::Lifecycle::Merged
        } else {
            crate::github::metadata::Lifecycle::Closed
        };
        tracked.head_sha = observation.head.clone();
        tracked.observed_at = now;
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
            .find(|f| f.basis.item_id == item.id)
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
        if final_review.is_none() && (permissions.approve || permissions.merge) {
            if let Some(observation) = observed.and_then(|o| o.observation.as_ref()) {
                if let Err(reason) =
                    basis(store, &item.id).and_then(|b| validate_observation(&b, observation))
                {
                    blockers.push(reason);
                }
            }
        }
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
                item.state = State::Blocked;
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
                if observation.head != item.job.head_sha
                    || item.job.observed_base_sha.as_deref() != Some(&observation.base)
                {
                    item.state = State::Stale;
                    item.summary="Provider head/base changed; this saved review does not establish current clearance.";
                } else if observation.draft {
                    item.state = State::Blocked;
                    item.summary = "The PR is draft; automatic approval and merge are blocked.";
                } else if observation.discussion_blocker().is_some() {
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
            if item.state == State::MachineSignedOff
                && run.execution.result.as_ref().is_some_and(|r| {
                    r.output.decision != crate::review::Decision::MachineSignOff
                        || r.output.feedback_conflict
                        || !r.output.held_findings.is_empty()
                })
            {
                item.state = State::WaitingForHuman;
                item.summary="The primary final review found concerns or needs human judgment. Automated actions are blocked.";
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
            item.summary="A provider action outcome is unresolved. Reconcile its original intent; no replacement request is authorized.";
            blockers.push("An action on this PR has an unresolved provider outcome, possibly from an earlier iteration.".into());
        }
        let machine_clear = item.state == State::MachineSignedOff;
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
    }
    Ok(())
}

pub fn restore(store: &Store) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let before = ledger.clone();
    for run in &mut ledger.finals {
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

pub fn request_final(store: &Store, id: &str, now: i64) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let run = ledger
        .finals
        .iter_mut()
        .find(|f| f.id == id)
        .ok_or("Final review unavailable.")?;
    run.cancelled = false;
    validate_local(store, run)?;
    if run.execution.operation.state == OperationState::Running {
        return Err("Final review is still running or stopping.".into());
    }
    if run.execution.operation.state == OperationState::Completed {
        return Err("This final review is complete; changed state creates new evidence.".into());
    }
    if matches!(
        run.execution.operation.state,
        OperationState::Failed | OperationState::ManualRetry
    ) {
        run.attempts.push(run.execution.clone());
        run.execution.operation = JobOperation::review(&run.basis.job, now);
        run.execution.operation.operation_type = "primary_final_review".into();
    }
    run.execution.manual_start = true;
    store.save_actions(&ledger)
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
    }) || item.state == State::MachineSignedOff
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
