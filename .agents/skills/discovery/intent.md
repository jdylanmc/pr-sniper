# Intent: discovery

![Discovery knowledge acquisition, alignment, domain modeling, frontier mapping, and compaction flow](https://github.com/user-attachments/assets/8e735137-b759-422b-bf31-4852e5eb1b62)

Discovery exists to carry an unclear product, engineering, or workflow question
through an evidence-preserving loop until the next action is justified. It is
for work that is too unsettled for specification, ticket breakdown, or
implementation, but concrete enough that reading evidence and recording open
questions can make progress.

It should gather sources, preserve what each source actually says, track
questions and decisions, and keep a visible frontier of what is known, unknown,
blocked, and ready. It may maintain a discovery tracker only through an
explicit approval gate; the cycle body itself must stay read-only.

Every cycle preserves this exact order: acquire knowledge, document the
findings, align those findings with the human, model the aligned domain, map the
remaining frontier from that model, persist the full foundation, reread the
full foundation, compact and persist the handoff, reread the compact handoff,
then continue or exit.

Before domain modeling or any handoff, discovery must align with the human. The
agent describes what was found, what was uncovered, what is still unknown, and
the current state of discovery, then waits for the human to confirm or correct
that understanding. Only aligned context can be modeled or persisted.

Automatic domain modeling exists only inside Discovery. It converts aligned
findings into concepts, actors, systems, terms, states, events, boundaries, and
relationships, then gives that evidence back to Discovery for frontier mapping.
It does not take alignment, persistence, handoff, tracker, specification, or
implementation authority.

Discovery must not absorb interrogation, specification, ticketing, or
implementation. Mapping issues, tickets, work items, backlog dependencies,
critical paths, delivery sequences, roadmaps, or ready work is not domain
modeling and must route elsewhere.
