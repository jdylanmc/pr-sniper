# Intent: status-report

## What this is for

Give the user a highly concise overview of the current agent's objective and
progress. When an agent has been working for hours and the user has forgotten
what it is doing or where things stand, this report brings them back up to
speed without making them reconstruct the conversation.

## What it must show

- The current objective, in the agent's own words, in no more than three
  sentences.
- Short bulleted lists of what has been completed and what remains to do for
  that objective.
- When the snapshot was taken and how long the current objective has been
  running, rather than the age of the entire agent session.
- How many tool calls the current agent has made for that objective, without
  adding its subagents' tool calls to that count.
- How many subagents are currently running on that objective and what each
  is working on, including nested subagents but excluding unrelated agents
  working in the same repository.
- Both the ticket number and the ticket title whenever a ticket is referenced,
  including in progress bullets and subagent assignments. Bare numbers and
  chains of unexplained ticket references are not useful to the user.

## How it behaves

The report is on demand and read-only. It may inspect existing progress and
runtime information to describe the work, but it does not change tickets,
direct subagents, or advance the underlying task.

Use observable timing and activity to make how long the agent has been
churning understandable. If a time, count, assignment, or ticket title cannot
be established, explicitly mark that information unavailable rather than
guessing. An incomplete view must not masquerade as a complete count.

## The judgement worth preserving

The purpose is quick human reorientation, not a detailed session history.
Keep the report short, concrete, and readable without requiring the user to
remember what ticket numbers mean.
