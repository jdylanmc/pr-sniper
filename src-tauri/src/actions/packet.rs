//! Approve exits ride the existing action pipeline. The exit run is the final
//! evidence (synthesized sign-off, no findings); the pump does approve (with the
//! agent's comment as the review body) then merge. Gates are the exit gate table,
//! re-run against fresh provider state before every write.

use super::*;
use crate::{
    exits::{
        consume,
        gates::{self, Facts, GateInput, Grants, Offer},
        Exit, ExitOutcome,
    },
    github::provider::RemoteRepository,
    retention::Binding,
    review::RunMode,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketApproval {
    /// `JobOperation` id of the exit run this record executes.
    pub run_id: String,
    pub state_seq: u64,
    /// The agent's approval comment: the review body.
    pub comment: String,
    pub merge: bool,
    /// Every wanted action is done (or abandoned): nothing more to watch.
    #[serde(default)]
    pub settled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ready {
    Go,
    /// Our approval already stands on this head: success, no effect.
    AlreadyApproved,
}

pub const ALREADY_APPROVED: &str = "The acting account already has an approval on this head; another automated vote is unnecessary.";

/// A packet final whose state is consumed once its writes are confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settle {
    pub final_id: String,
    pub job: QueueJob,
    /// The exit run whose packet state is consumed.
    pub run_id: String,
}

fn exit_comment(run: &ReviewRun) -> Option<(String, bool)> {
    match run.result.as_ref()?.output.exit.as_ref()? {
        ExitOutcome::Approve { comment } => Some((comment.clone(), false)),
        ExitOutcome::ApproveAndMerge { comment } => Some((comment.clone(), true)),
        ExitOutcome::Lgtm {
            comment, action, ..
        } => match action {
            crate::exits::LgtmAction::Ready => None,
            crate::exits::LgtmAction::Approve => Some((comment.clone(), false)),
            crate::exits::LgtmAction::Merge => Some((comment.clone(), true)),
        },
        _ => None,
    }
}

/// A sign-off acts only after its own thread resolutions completed. `Err` while
/// they are pending (the action waits) or once they stopped (the final is abandoned).
fn sign_off_written(store: &Store, run: &FinalReview) -> Result<Option<String>, String> {
    use crate::exit_writes::Phase;
    if !matches!(
        run.execution
            .result
            .as_ref()
            .and_then(|r| r.output.exit.as_ref()),
        Some(ExitOutcome::Lgtm { .. })
    ) {
        return Ok(None);
    }
    let Some(write) = crate::exit_writes::plan(&run.execution, 0)
        .map(|_| crate::exit_writes::for_review(store, &run.execution.operation.id))
    else {
        return Ok(None);
    };
    Ok(match write?.map(|w| w.phase) {
        Some(Phase::Completed) => None,
        Some(Phase::Tossed | Phase::Failed) => {
            Some("The sign-off's thread resolutions stopped; nothing more is sent.".into())
        }
        _ => Some(PENDING_SIGN_OFF.into()),
    })
}

pub const PENDING_SIGN_OFF: &str = "The sign-off's own thread resolutions are not complete yet.";

/// The exit's state is still the newest and unconsumed, or consumed only by this
/// sign-off's own completed thread resolutions (which baseline just those writes).
fn live(
    states: &crate::pr_state::PrStates,
    writes: &[crate::exit_writes::ExitWrite],
    run: &ReviewRun,
) -> bool {
    let seq = consume::state_seq(&run.job);
    states.get(&Binding::job(&run.job)).is_some_and(|r| {
        r.state_seq == seq
            && r.exit.as_ref().is_none_or(|e| {
                e.state_seq != seq
                    || writes.iter().any(|w| {
                        w.review_operation_id == run.operation.id
                            && w.sign_off.is_some()
                            && w.phase == crate::exit_writes::Phase::Completed
                            && w.consumed
                    })
            })
    })
}

/// Record each completed approve exit of this item as a packet final. The state
/// must still be the one the exit read: a moved state is never executed.
pub fn adopt(store: &Store, item_id: &str) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    let Some(observation) = ledger
        .observations
        .iter()
        .find(|o| o.item_id == item_id)
        .and_then(|o| o.observation.clone())
    else {
        return Ok(());
    };
    if observation.state != "OPEN" {
        return Ok(());
    }
    let reviews = store.load_reviews()?;
    let states = store.load_pr_states()?;
    let writes = store.load_feedback()?.exit_writes;
    let settings = store.load_settings()?;
    let monitor = store.load_monitoring_state()?;
    let mut changed = false;
    for run in reviews.iter().filter(|r| {
        r.mode == RunMode::Packet
            && r.operation.state == OperationState::Completed
            && queue::item_id(&r.job) == item_id
    }) {
        let Some((comment, merge)) = exit_comment(run) else {
            continue;
        };
        if ledger.finals.iter().any(|f| {
            f.packet
                .as_ref()
                .is_some_and(|p| p.run_id == run.operation.id)
        }) {
            continue;
        }
        let seq = consume::state_seq(&run.job);
        let live = live(&states, &writes, run);
        let reviewed = run
            .result
            .as_ref()
            .and_then(|r| r.reviewed_base_sha.as_deref());
        if !live || observation.head != run.job.head_sha || reviewed != Some(&observation.base) {
            continue;
        }
        let (Some(repository), Some(iteration_id)) = (
            settings
                .repositories
                .iter()
                .find(|r| r.id == run.job.configuration_id),
            run.job.work.as_ref().map(|w| w.iteration_id.clone()),
        ) else {
            continue;
        };
        let Some(assignment) = repository
            .assignments
            .iter()
            .find(|a| a.id == run.assignment_id)
        else {
            continue;
        };
        let Some(activation) = monitor.activations.get(&repository.id) else {
            continue;
        };
        let grants = repository.assignment_authority(assignment);
        let permissions = ActionPermissions {
            include_self: assignment.actions.as_ref().is_some_and(|a| a.include_self),
            reply: false,
            // Self-authored: GitHub refuses the vote, so only the merge is attempted.
            approve: grants.approve && !(merge && observation.author_id == observation.account_id),
            merge: grants.merge && merge,
        };
        let id = format!("final-packet-{}", run.operation.id);
        ledger.finals.push(FinalReview {
            id,
            enqueue_order: store.allocate_enqueue_order()?,
            basis: Basis {
                activation_version: activation.version.clone(),
                item_id: item_id.into(),
                iteration_id,
                repository: repository.clone(),
                job: run.job.clone(),
                selection: run.selection.clone(),
                peers: vec![run.clone()],
                feedback: vec![],
                conversations: vec![],
                permissions,
                trust_confirmed: false,
            },
            observation: observation.clone(),
            execution: run.clone(),
            cancelled: false,
            attempts: vec![],
            packet: Some(PacketApproval {
                run_id: run.operation.id.clone(),
                state_seq: seq,
                comment,
                merge,
                settled: false,
            }),
        });
        changed = true;
    }
    if changed {
        store.save_actions(&ledger)?;
    }
    Ok(())
}

/// Every finding this exit relied on as settled without a fresh check (accepted
/// author won't-fix, carried forward) still reads exactly as accepted on the provider.
fn settled_threads_hold(run: &FinalReview, observation: &Observation) -> Result<(), String> {
    let result = run
        .execution
        .result
        .as_ref()
        .ok_or("Final review result unavailable.")?;
    settled_hold(
        &result.finding_dispositions,
        &result.carried_findings,
        observation,
    )
}

pub(super) fn settled_hold(
    dispositions: &[crate::review::findings::Assessment],
    carried: &[crate::review::findings::Assessment],
    observation: &Observation,
) -> Result<(), String> {
    use crate::review::findings::{self, Disposition, Seen};
    let changed = || "A settled finding's thread changed after review; a fresh review is needed.";
    for evidence in dispositions
        .iter()
        .filter(|a| {
            matches!(
                a.disposition,
                Disposition::AuthorWontFix | Disposition::SignedOff
            )
        })
        .chain(carried)
    {
        let anchor = evidence.anchor.as_ref().ok_or_else(changed)?;
        let thread = observation
            .threads
            .iter()
            .find(|t| t.id == evidence.thread_id)
            .ok_or_else(changed)?;
        let ids: Vec<(String, Option<String>)> = thread
            .comments
            .iter()
            .map(|c| {
                let id = |v: &serde_json::Value| match v {
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    serde_json::Value::String(s) => Some(s.clone()),
                    _ => None,
                };
                (
                    id(&c["fullDatabaseId"]).unwrap_or_default(),
                    id(&c["author"]["databaseId"]),
                )
            })
            .collect();
        let comments: Vec<Seen<'_>> = thread
            .comments
            .iter()
            .zip(&ids)
            .map(|(c, (id, author))| Seen {
                id,
                author_id: author.as_deref(),
                body: c["body"].as_str().unwrap_or_default(),
            })
            .collect();
        let holds = if evidence.disposition == Disposition::AuthorWontFix {
            findings::author_reply_holds(
                evidence,
                &comments,
                Some(&observation.author_id),
                &observation.account_id,
            )
        } else {
            findings::snapshot_holds(anchor, &comments, |c| {
                findings::own_marked(c, &observation.account_id)
            })
        };
        if !holds || anchor.base_sha != observation.base {
            return Err(changed().into());
        }
    }
    Ok(())
}

pub fn validate_local(store: &Store, run: &FinalReview) -> Result<(), String> {
    if run.cancelled {
        return Err("Final review was cancelled.".into());
    }
    crate::review::findings::action_clearance(
        &run.execution
            .result
            .as_ref()
            .ok_or("Final review result unavailable.")?
            .finding_dispositions,
    )?;
    let monitor = monitoring::Monitor::restore_readonly(store.load_monitoring_state()?);
    if !monitor
        .activation_status(&store.load_settings()?, &run.basis.job.configuration_id)
        .active
    {
        return Err("Monitoring scope is not active.".into());
    }
    Ok(())
}

/// Whether `action` may be sent now. Gates: CI red or conflicts deny both; CI
/// pending allows approve only; unmet required reviews deny merge; others'
/// threads deny merge, never approve; self-authored: no approve, merge needs
/// include self.
pub fn ready(
    store: &Store,
    run: &FinalReview,
    observation: &Observation,
    action: Action,
) -> Result<Ready, String> {
    validate_local(store, run)?;
    let job = &run.basis.job;
    if observation.account_id != job.account_id
        || observation.repository_id != job.repository_id
        || observation.pull_request_id != job.pull_request_id
    {
        return Err("Provider identity differs from the exit's PR.".into());
    }
    let reviewed = run
        .execution
        .result
        .as_ref()
        .and_then(|r| r.reviewed_base_sha.as_deref());
    if observation.head != job.head_sha || reviewed != Some(&observation.base) {
        return Err("The PR moved after the exit.".into());
    }
    let ledger = store.load_actions()?;
    if ledger.effects.iter().any(|e| {
        e.final_id == run.id
            && e.action != action
            && e.state == EffectState::Confirmed
            && e.error.is_some()
    }) {
        return Err("The preceding confirmed action still needs post-action verification before another action.".into());
    }
    if ledger.effects.iter().any(|e| {
        same_scope(e, job)
            && e.state == EffectState::Uncertain
            && !(e.final_id == run.id
                && e.action == action
                && e.operation.state == OperationState::Running)
    }) {
        return Err("Another provider action has an unresolved outcome.".into());
    }
    let permissions = &run.basis.permissions;
    if !match action {
        Action::Approve => permissions.approve,
        Action::Merge => permissions.merge,
    } {
        return Err("This provider action is not opted in.".into());
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
            let ours = ledger.effects.iter().any(|e| {
                e.receipt
                    .as_ref()
                    .is_some_and(|r| r.action == Action::Approve && r.id == review.id)
            });
            if !ours {
                return Ok(Ready::AlreadyApproved);
            }
        }
    }
    if let Some(reason) = sign_off_written(store, run)? {
        return Err(reason);
    }
    if action == Action::Merge {
        settled_threads_hold(run, observation)?;
    }
    let facts = Facts::from_observation(observation);
    let self_authored = facts.author_is_acting;
    let exit = match action {
        Action::Approve => Exit::ApprovePullRequest,
        Action::Merge => Exit::ApproveAndMerge,
    };
    let input = GateInput {
        tools: vec![exit.id().into()],
        primary: true,
        grants: Grants {
            comment: false,
            reply: false,
            approve: permissions.approve,
            merge: permissions.merge,
            include_self: permissions.include_self,
        },
        review_drafts: false,
        facts,
    };
    if let Some((_, Offer::Denied(reason))) = gates::offered(&input)
        .into_iter()
        .find(|(offered, _)| *offered == exit)
    {
        return Err(reason);
    }
    if action == Action::Merge && !self_authored && provider_approval(observation).is_none() {
        return Err(
            "Merge requires confirmed current-revision provider approval before transmission."
                .into(),
        );
    }
    Ok(Ready::Go)
}

/// Review body for the approval: the agent's comment plus the intent marker that
/// lets a lost response be reconciled.
pub fn approval_body(approval: &PacketApproval, effect_id: &str) -> String {
    format!(
        "{}\n\n<!-- pr-sniper:action:{effect_id} -->\n\nPR Sniper",
        approval.comment
    )
}

/// The packet finals of `item_id` whose wanted writes are all confirmed (or whose
/// approval already stood): their state is ready to consume. Abandoned finals
/// (state moved, PR closed) are settled here with no consume; a failed write is a
/// toss that re-queues the review through the budget.
pub fn settle(
    store: &Store,
    item_id: &str,
    observation: &Observation,
    now: i64,
) -> Result<Vec<Settle>, String> {
    let mut ledger = store.load_actions()?;
    let states = store.load_pr_states()?;
    let mut ready_to_consume = Vec::new();
    let mut changed = false;
    let mut failed = Vec::new();
    for index in 0..ledger.finals.len() {
        let run = &ledger.finals[index];
        let Some(approval) = run.packet.as_ref().filter(|p| !p.settled) else {
            continue;
        };
        if run.basis.item_id != item_id || run.cancelled {
            continue;
        }
        let job = run.basis.job.clone();
        let moved = states
            .get(&Binding::job(&job))
            .is_none_or(|r| r.state_seq != approval.state_seq)
            || observation.head != job.head_sha;
        if observation.state != "OPEN" || moved {
            ledger.finals[index].packet.as_mut().unwrap().settled = true;
            changed = true;
            continue;
        }
        if sign_off_written(store, run)?.is_some_and(|reason| reason != PENDING_SIGN_OFF) {
            // The exit write tosses and re-queues itself; nothing more to watch here.
            ledger.finals[index].packet.as_mut().unwrap().settled = true;
            changed = true;
            continue;
        }
        let effects: Vec<_> = ledger
            .effects
            .iter()
            .filter(|e| e.final_id == run.id)
            .collect();
        if effects
            .iter()
            .any(|e| matches!(e.state, EffectState::Rejected | EffectState::Stale))
        {
            failed.push((index, job));
            continue;
        }
        let permissions = &run.basis.permissions;
        if !permissions.approve && !permissions.merge {
            ledger.finals[index].packet.as_mut().unwrap().settled = true;
            changed = true;
            continue;
        }
        // Any current-head approval by the acting account is an idempotent success,
        // including one an earlier exit of ours placed (no second vote is prepared).
        let approve_done = !permissions.approve
            || effects
                .iter()
                .any(|e| e.action == Action::Approve && e.state == EffectState::Confirmed)
            || Facts::from_observation(observation).already_approved
            || ready(store, run, observation, Action::Approve) == Ok(Ready::AlreadyApproved);
        let merge_done = !permissions.merge
            || effects.iter().any(|e| {
                e.action == Action::Merge
                    && matches!(e.state, EffectState::Confirmed | EffectState::ExternalMerge)
            });
        if approve_done && merge_done {
            ready_to_consume.push(Settle {
                final_id: run.id.clone(),
                job,
                run_id: approval.run_id.clone(),
            });
        }
    }
    for (index, job) in failed {
        ledger.finals[index].packet.as_mut().unwrap().settled = true;
        changed = true;
        consume::toss_and_requeue(store, &job, &ledger.finals[index].execution, now)?;
    }
    if changed {
        store.save_actions(&ledger)?;
    }
    Ok(ready_to_consume)
}

pub fn mark_settled(store: &Store, final_id: &str) -> Result<(), String> {
    let mut ledger = store.load_actions()?;
    if let Some(approval) = ledger
        .finals
        .iter_mut()
        .find(|f| f.id == final_id)
        .and_then(|f| f.packet.as_mut())
    {
        approval.settled = true;
        store.save_actions(&ledger)?;
    }
    Ok(())
}

/// Items whose completed approve exit has no packet final yet and that [`adopt`]
/// could still adopt: a closed PR or a removed assignment or scope is never observed again.
pub fn unadopted(
    store: &Store,
    ledger: &Ledger,
) -> Result<std::collections::HashSet<String>, String> {
    let states = store.load_pr_states()?;
    let writes = store.load_feedback()?.exit_writes;
    let settings = store.load_settings()?;
    let monitor = store.load_monitoring_state()?;
    let adoptable = |r: &ReviewRun| {
        let item = queue::item_id(&r.job);
        let closed = ledger
            .observations
            .iter()
            .find(|o| o.item_id == item)
            .and_then(|o| o.observation.as_ref())
            .is_some_and(|o| o.state != "OPEN");
        !closed
            && r.job.work.is_some()
            && settings
                .repositories
                .iter()
                .find(|repo| repo.id == r.job.configuration_id)
                .is_some_and(|repo| {
                    repo.assignments.iter().any(|a| a.id == r.assignment_id)
                        && monitor.activations.contains_key(&repo.id)
                })
    };
    Ok(store
        .load_reviews()?
        .iter()
        .filter(|r| {
            adoptable(r)
                && r.mode == RunMode::Packet
                && r.operation.state == OperationState::Completed
                && exit_comment(r).is_some()
                && !ledger.finals.iter().any(|f| {
                    f.packet
                        .as_ref()
                        .is_some_and(|p| p.run_id == r.operation.id)
                })
                && live(&states, &writes, r)
        })
        .map(|r| queue::item_id(&r.job))
        .collect())
}

/// A packet final still has writes to watch.
pub fn open(ledger: &Ledger, item_id: &str) -> bool {
    ledger.finals.iter().any(|f| {
        f.basis.item_id == item_id && !f.cancelled && f.packet.as_ref().is_some_and(|p| !p.settled)
    })
}

/// Consume `job`'s state after its writes: re-read, baseline, mark settled.
pub fn consume_settled(
    app_store: &std::sync::Mutex<Store>,
    client: &crate::github::provider::GithubClient<crate::github::http::HttpTransport>,
    settle: &Settle,
) -> Result<(), String> {
    let repo = RemoteRepository {
        id: settle.job.repository_id.clone(),
        name: settle.job.repository_name.clone(),
    };
    consume::consume_after(app_store, client, &repo, &settle.job, &settle.run_id)?;
    let store = app_store.lock().map_err(|_| "Storage unavailable.")?;
    mark_settled(&store, &settle.final_id)
}
