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
        threads::QueryTransport,
    },
    packet::{self, reads, Directive, Packet, PacketInput, Role},
};
use std::sync::Arc;

pub(crate) struct Prepared {
    pub packet: Packet,
    pub setup: ExitSetup,
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
        tools: assignment.tools.clone().unwrap_or_default(),
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

/// Build the packet and the run's tool set from state read right now.
/// A head that moved since the job was queued is a superseded state, not a failure.
pub(crate) fn prepare<T>(
    client: &Arc<GithubClient<T>>,
    repo: &RemoteRepository,
    run: &ReviewRun,
    context: &ReviewContext,
    acting_login: &str,
    write_capability: bool,
) -> Result<Prepared, Failure>
where
    T: Transport + QueryTransport + Send + Sync + 'static,
{
    let mut observation = client
        .action_observation(repo, run.job.number)
        .map_err(Failure::from)?;
    observation.write_capability = write_capability;
    if observation.head != context.pull.head_sha || observation.base != context.pull.base_sha {
        return Err(Failure::superseded(
            "The PR state moved before the packet was built.",
        ));
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
    let tool_set = tools::tool_set(&base.tools, &offered);
    let config = run
        .selection
        .configuration
        .as_ref()
        .ok_or_else(|| Failure::permanent("Execution configuration is missing."))?;
    let agent = &run.selection.agent;
    let admission = run.job.work.as_ref().map(|w| &w.admission);
    let conversation = reads::conversation(&observation);
    let checks = reads::checks(&observation);
    let description = None;
    let input = PacketInput {
        state_id: format!("{}:{}", context.pull.head_sha, context.pull.base_sha),
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
        role: if base.primary {
            Role::Primary
        } else {
            Role::Teammate
        },
        tools: tool_set.clone(),
        exits,
        pr: reads::pr_facts(context, &observation, description.clone(), acting_login),
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
        },
        teammates: vec![],
        conversation: conversation.clone(),
    };
    let packet = packet::build(&input);
    let facts = Arc::new(reads::tool_facts(description, &conversation, &checks));
    let gate = exit_gate(
        client.clone(),
        repo.clone(),
        run,
        context,
        base,
        write_capability,
    )?;
    Ok(Prepared {
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
) -> Result<ExitGate, Failure>
where
    T: Transport + QueryTransport + Send + Sync + 'static,
{
    let number = run.job.number;
    let (head, target) = (context.pull.head_sha.clone(), context.pull.base_sha.clone());
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
        gates::check_call(&input, call).map_err(Refusal::Denied)
    }))
}
