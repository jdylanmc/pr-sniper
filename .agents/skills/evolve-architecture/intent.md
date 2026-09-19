# Intent: evolve-architecture

Evolve architecture exists to turn demonstrated architectural friction into a
bounded, agreed evolution. It helps decide what should change and why before
anyone starts restructuring code.

Architecture matters because it changes the cost and safety of future work.
More layers, deeper modules, cleaner diagrams, or fashionable patterns are not
outcomes by themselves. The useful starting point is evidence: changes that
spread unnecessarily, responsibilities that are hard to locate, fragile tests,
or boundaries that repeatedly make ordinary work difficult.

The workflow should start from the concern the human named. When no concern is
named, recent change patterns can suggest where to investigate, but frequency
alone does not prove a problem. It should challenge its diagnosis, compare
meaningfully different alternatives when the decision is uncertain, and keep
retaining the current design, simplifying, or deleting in the available space.

The human receives a decision-ready proposal: the actual friction, the proposed
boundary change, what must remain true, meaningful tradeoffs, how the change
could be introduced safely, and what evidence would establish improvement.
Before-and-after visuals are useful when they explain those things, not as a
mandatory report format.

Existing decisions deserve context rather than automatic reopening. A new
decision is recorded only when it is consequential enough to need a durable
explanation and the human authorizes recording it.

This is not another interview, generic discovery engine, or delivery controller.
It uses those capabilities without taking over their responsibilities.
Its default result is a proposal, not changed product code. Agreement with a
direction is not permission to implement it. When implementation is authorized,
the agreed scope and evidence pass to the existing execution and delivery
owners.

A finding that no evolution is justified is a valid result. The workflow must
not invent architectural work merely to have something to hand off.
