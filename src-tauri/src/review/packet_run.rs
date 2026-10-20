//! Packet-mode preparation: fresh reads -> gates -> packet -> exit setup.
//! Runs blocking, outside the Store lock, inside the review preparation step.

use super::{
    runtime::{ExitGate, ExitSetup, Refusal},
    tools, Failure, ReviewRun,
};
use crate::{
    exits::gates::{self, Facts, GateInput, Grants},
    github::{
        actions::Observation,
        provider::{GithubClient, RemoteRepository, Transport},
        review::ReviewContext,
        state::PrSnapshot,
        threads::QueryTransport,
    },
    packet::{self, reads, Directive, Packet, PacketInput, Role},
};
use std::sync::Arc;

pub(crate) struct Prepared {
    pub packet: Packet,
    pub setup: ExitSetup,
    /// Present when the run offers `rubber_duck`.
    pub duck: Option<super::runtime::DuckPrep>,
}

fn gate_input(run: &ReviewRun, facts: Facts) -> Result<GateInput, Failure> {
    let config = run
        .selection
        .configuration
        .as_ref()
        .ok_or_else(|| Failure::permanent("Execution configuration is missing."))?;
    let assignment = config
        .repository
        .assignments
        .iter()
        .find(|a| a.id == run.assignment_id)
        .ok_or_else(|| Failure::permanent("The assignment is no longer configured."))?;
    let actions = assignment.actions.clone().unwrap_or_default();
    Ok(GateInput {
        tools: config.repository.assignment_tools(assignment),
        primary: config.authority.primary,
        grants: Grants {
            comment: config.authority.comment,
            reply: config.authority.reply,
            approve: config.authority.approve,
            merge: config.authority.merge,
            include_self: actions.include_self,
        },
        review_drafts: config.repository.review_drafts,
        facts,
    })
}

/// Who builds the packet, and the local facts the packet needs beyond the PR read.
pub(crate) struct Caller<'a> {
    pub acting_login: &'a str,
    pub write_capability: bool,
    pub teammates: Vec<crate::packet::TeammateEntry>,
    /// The PR's known (head, local iteration) pairs, for targeted revisions.
    pub iterations: &'a [(String, u64)],
    /// When this PR's state was last consumed, for `new_since_last_review`.
    pub last_consumed_at: Option<i64>,
    pub ownership: Vec<crate::github::threads::Ownership>,
}

/// Build the packet and the run's tool set from state read right now.
/// A head that moved since the job was queued is a superseded state, not a failure;
/// any other change since enqueue is simply read fresh from `snapshot`.
pub(crate) fn prepare<T>(
    client: &Arc<GithubClient<T>>,
    repo: &RemoteRepository,
    run: &ReviewRun,
    context: &ReviewContext,
    snapshot: &PrSnapshot,
    caller: Caller<'_>,
) -> Result<Prepared, Failure>
where
    T: Transport + QueryTransport + Send + Sync + 'static,
{
    let Caller {
        acting_login,
        write_capability,
        teammates,
        iterations,
        last_consumed_at,
        ownership,
    } = caller;
    let mut observation = client
        .action_observation(repo, run.job.number)
        .map_err(Failure::from)?;
    observation.write_capability = write_capability;
    if observation.head != context.pull.head_sha || observation.base != context.pull.base_sha {
        return Err(
            Failure::superseded("The PR state moved before the packet was built.")
                .after(super::Resume::NextPoll),
        );
    }
    let base = gate_input(run, Facts::from_observation(&observation))?;
    let exits = gates::offered(&base);
    let offered: Vec<_> = exits
        .iter()
        .filter(|(_, offer)| offer.is_offered())
        .map(|(exit, _)| *exit)
        .collect();
    if offered
        .iter()
        .all(|exit| *exit == crate::exits::Exit::PanicExit)
    {
        return Err(Failure::permanent(
            "No exit is offered to this assignment. Grant a usable exit tool.",
        ));
    }
    let config = run
        .selection
        .configuration
        .as_ref()
        .ok_or_else(|| Failure::permanent("Execution configuration is missing."))?;
    let tool_set = tools::tool_set(&base.tools, config.rubber_duck.is_some(), &offered);
    let agent = &run.selection.agent;
    let admission = run.job.work.as_ref().map(|w| &w.admission);
    let mut conversation = reads::conversation(&observation);
    super::findings::stamp_ownership(
        &mut conversation.threads,
        &ownership,
        &run.assignment_id,
        &run.job.account_id,
    );
    reads::stamp_targeted_revisions(&mut conversation, &observation, iterations);
    reads::stamp_new_since(&mut conversation, &observation, last_consumed_at);
    let checks = reads::checks(&observation);
    let description = Some(snapshot.body.clone());
    let state_hash = snapshot.normalize().hash();
    let input = PacketInput {
        state_id: format!(
            "{}:{}",
            run.job.work.as_ref().map_or(0, |w| w.state_seq),
            &state_hash[..16]
        ),
        revision: run
            .job
            .work
            .as_ref()
            .map(|w| u32::try_from(w.pass_ordinal.max(1)).unwrap_or(u32::MAX))
            .unwrap_or(1),
        created_at: chrono::Utc::now().to_rfc3339(),
        admitted_by: admission.map(|a| {
            if a.watched_author {
                "watched_author"
            } else if a.requested_reviewer {
                "requested_reviewer"
            } else {
                "all_authors"
            }
            .to_string()
        }),
        acting_login: acting_login.to_string(),
        include_self: base.grants.include_self,
        role: if base.primary {
            Role::Primary
        } else {
            Role::Teammate
        },
        tools: tool_set.clone(),
        exits,
        pr: reads::pr_facts(context, &observation, snapshot, acting_login),
        files: reads::files(context),
        delta: None,
        directive: Directive {
            doctrines: config
                .doctrines
                .iter()
                .map(|d| (d.title.clone(), d.body.clone()))
                .collect(),
            agent_prompt: crate::prompt_tokens::expand(&agent.prompt, &agent.name, context),
            repository_prompt: crate::prompt_tokens::expand(
                &run.selection.policy.prompt,
                &agent.name,
                context,
            ),
            preset: run.selection.preset.clone(),
            signature: agent.signature.clone(),
            // Captured for primaries only (Selection::resolve).
            voice: config.voice.clone(),
        },
        teammates: if base.primary { teammates } else { vec![] },
        conversation: conversation.clone(),
    };
    let duck_input = input.clone();
    let mut packet = packet::build(&input);
    packet.record.state_hash = Some(state_hash);
    packet.record.wait_snapshot = offered
        .contains(&crate::exits::Exit::Wait)
        .then(|| snapshot.normalize());
    packet.record.state_marks = Some(crate::github::state::StateMarks::of(snapshot, acting_login));
    let duck_tools = tools::duck_tools(&base.tools);
    let duck = config
        .rubber_duck
        .as_ref()
        .filter(|_| tool_set.iter().any(|t| t == super::duck::TOOL))
        .map(|duck| super::runtime::DuckPrep {
            primary_hash: packet.hash.clone(),
            directive: Directive {
                doctrines: duck
                    .doctrines
                    .iter()
                    .map(|d| (d.title.clone(), d.body.clone()))
                    .collect(),
                agent_prompt: crate::prompt_tokens::expand(
                    &duck.agent.prompt,
                    &duck.agent.name,
                    context,
                ),
                repository_prompt: duck_input.directive.repository_prompt.clone(),
                preset: None,
                signature: String::new(),
                voice: None,
            },
            tools: duck_tools,
            changed: context.files.iter().map(|f| f.path.clone()).collect(),
            threads: conversation.threads.iter().map(|t| t.id.clone()).collect(),
            base: duck_input,
        });
    let facts = Arc::new(reads::tool_facts(description, &conversation, &checks));
    let gate = exit_gate(
        client.clone(),
        repo.clone(),
        run,
        context,
        base,
        write_capability,
        ownership,
    )?;
    Ok(Prepared {
        duck,
        packet,
        setup: ExitSetup {
            tool_set,
            offered,
            facts,
            gate,
        },
    })
}

/// Re-read the PR at exit time: a moved state tosses the run, a gate may deny the exit.
fn exit_gate<T>(
    client: Arc<GithubClient<T>>,
    repo: RemoteRepository,
    run: &ReviewRun,
    context: &ReviewContext,
    base: GateInput,
    write_capability: bool,
    ownership: Vec<crate::github::threads::Ownership>,
) -> Result<ExitGate, Failure>
where
    T: Transport + QueryTransport + Send + Sync + 'static,
{
    let number = run.job.number;
    let pull_request_id = run.job.pull_request_id.clone();
    let (head, target) = (context.pull.head_sha.clone(), context.pull.base_sha.clone());
    let files = context.files.clone();
    Ok(Arc::new(move |call| {
        let mut observation: Observation = client
            .action_observation(&repo, number)
            .map_err(|_| Refusal::Denied("PR state unreadable now. Try again.".into()))?;
        observation.write_capability = write_capability;
        if observation.head != head || observation.base != target {
            return Err(Refusal::Moved(
                "PR state moved. Review discarded. Stop.".into(),
            ));
        }
        if observation.state != "OPEN" {
            return Err(Refusal::Moved("PR is no longer open. Stop.".into()));
        }
        let input = GateInput {
            facts: Facts::from_observation(&observation),
            ..base.clone()
        };
        gates::check_call(&input, call).map_err(Refusal::Denied)?;
        // A reply or resolve must name a thread of this PR now. Caught here, the agent
        // fixes the id in the loop instead of the host tossing the whole exit later.
        if let crate::exits::ExitCall::LeaveComments {
            reply_comments,
            resolve_own_threads,
            ..
        } = call
        {
            let unknown = reply_comments
                .iter()
                .map(|r| r.thread_id.as_str())
                .chain(resolve_own_threads.iter().map(String::as_str))
                .any(|id| !observation.threads.iter().any(|t| t.id == id));
            if unknown {
                return Err(Refusal::Denied(
                    "A reply or resolve names an unknown thread. Copy thread ids exactly from the Conversation section."
                        .into(),
                ));
            }
            for id in resolve_own_threads {
                let thread = client
                    .review_thread(&repo.id, &pull_request_id, id)
                    .map_err(|_| {
                        Refusal::Denied("Thread ownership unreadable now. Try again.".into())
                    })?
                    .ok_or_else(|| Refusal::Denied("Thread unavailable now.".into()))?;
                if !thread.owned_by_any(&ownership) {
                    return Err(Refusal::Denied(
                        "Only this assignment's verified own threads may be resolved.".into(),
                    ));
                }
            }
        }
        if let Some(reason) = out_of_diff(&files, call) {
            return Err(Refusal::Denied(reason));
        }
        Ok(())
    }))
}

/// New comments on lines the run's diff cannot address, as a directive the agent
/// can act on in the loop. Same mapping as the publication batch
/// ([`crate::publication::DiffLines`]), so an accepted exit always maps.
pub(crate) fn out_of_diff(
    files: &[crate::github::review::ReviewFile],
    call: &crate::exits::ExitCall,
) -> Option<String> {
    let crate::exits::ExitCall::LeaveComments { new_comments, .. } = call else {
        return None;
    };
    let lines = crate::publication::DiffLines::of(files);
    let bad: Vec<_> = new_comments
        .iter()
        .filter(|c| lines.position(c).is_none())
        .map(|c| {
            format!(
                "{}:{} ({})",
                c.path,
                c.line,
                c.side.as_deref().unwrap_or("head")
            )
        })
        .collect();
    const SHOWN: usize = 5;
    (!bad.is_empty()).then(|| {
        let more = bad.len().saturating_sub(SHOWN);
        format!(
            "{}{} not in diff. Comment on a changed line, or put it in body.",
            bad[..bad.len().min(SHOWN)].join(", "),
            if more > 0 {
                format!(" (+{more} more)")
            } else {
                String::new()
            }
        )
    })
}
