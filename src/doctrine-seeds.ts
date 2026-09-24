// Generated 2026-09-24 from .agents/skills/doctrine/doctrines/*.doctrine.md.
// Seeds every fresh install with the existing local doctrine catalog. Users
// can freely create, edit, and delete doctrines afterward; this is initial
// content, not a locked or managed set.
export interface DoctrineSeed {
  title: string;
  body: string;
}

export const doctrineSeeds: DoctrineSeed[] = [
  { title: "boundaries", body: `## Prime directive

A model means what it means inside the boundary that owns it.

## Position

A bounded context is like a dollhouse. The things inside belong to one coherent little world: the kitchen, dining table, and bathtub make sense together because the house gives them a place and a purpose. A road sign or a tree belongs to a different world.

The walls protect that coherence. Doors and windows permit deliberate exchange. A missing wall is not openness; it is an uncontrolled place where foreign meanings, assumptions, and ownership leak in until the model no longer explains itself.

Enterprise software cannot sustain one universal model for every business purpose. The same word may carry different data, behavior, and obligations in different contexts. Those differences are not duplication to eliminate. They are meaning to own.

## Principles

- **Keep meaning local.** Inside one boundary, language, behavior, invariants, and lifecycle should agree. A term has one owned meaning there, even when another context uses the same word differently.
- **Put only belonging things inside.** A model should contain what its business purpose needs. Foreign concerns, framework shapes, transport formats, and neighboring models do not become native merely because they are convenient to reuse.
- **Build walls with care.** Boundaries should make ownership and authority visible. Gaps create shared state, ambiguous responsibility, and accidental coupling.
- **Design doors and windows.** Contexts collaborate through explicit contracts and deliberate translation. Crossing a boundary should reveal that meaning may change rather than pretending both sides share one model.
- **Protect autonomy without demanding isolation.** A boundary permits independent evolution while preserving intentional relationships with the larger enterprise.
- **Align structure when useful, not by definition.** A context may align with a team, service, repository, or database, but none of those automatically defines the model boundary.
- **Avoid both universal models and shattered ones.** One model for everything collapses distinct meanings. Too many tiny contexts replace coherence with translation overhead.
- **Evolve boundaries deliberately.** When language, ownership, or business purpose changes, revisit the walls and contracts instead of letting the model drift across them.

## Boundary

The dollhouse is a lens, not a complete architecture. Boundaries exist to preserve meaning and ownership while enabling controlled collaboration—not to wall every component away from the rest of the system.` },
  { title: "code", body: `## Prime directive

Choose the implementation that carries less defect risk and costs the next reader less effort to understand.

## Position

Working once is not finished construction. Before substantial coding, understand the requirement, architectural fit, language constraints, conventions, error policy, representation, reusable parts, integration path, and verification approach. When the ground is uncertain, build the smallest real slice that can expose the uncertainty.

Code is read more often than it is written. Prefer explicit behavior, visible control flow, related concepts kept together, and familiar project idioms over cleverness or compressed syntax.

## Principles

- **Make routines coherent.** A routine does one nameable thing, exposes a small interface, and resists incorrect use. Separate validation, computation, coordination, and effects when they represent different responsibilities.
- **Make data carry meaning.** Use names, types, units, ranges, and structures that reveal purpose. Keep scope small and initialization deliberate. Use a Boolean only for genuinely binary meaning.
- **Keep control flow visible.** Favor a clear normal path, shallow nesting, named conditions, plain loops, and explicit side effects. Table-driven logic earns its place only when the table makes the rule easier to inspect and validate.
- **Guard boundaries deliberately.** Validate where trust changes hands. Use assertions for programmer invariants and domain results for expected failure. Handle errors at the level that can interpret them while preserving diagnostic context.
- **Keep modules cohesive.** Hide representation and internal bookkeeping. Do not let unrelated persistence, formatting, business logic, and integration accumulate behind one name.
- **Simplify before extending.** Remove proven redundancy and accidental indirection before building on them. Once a replacement is proven, finish the refactor by removing superseded paths and temporary bridges whose obligations have ended.
- **Test the contract.** Cover normal behavior, boundaries, invalid input, defensive checks, promised outcomes, and edge cases suggested by the data. Tests should protect behavior rather than freeze implementation shape.
- **Refactor with evidence.** Place protection around risky or poorly understood behavior before restructuring it. Keep behavior changes separate when reviewability benefits.
- **Tune measured problems.** Set a performance target, measure the baseline, change one thing, and measure again. Keep the clearer form unless the demonstrated gain earns the complexity.

## Boundary

Code Doctrine owns local construction. Documentation owns durable explanation; Cyclomatic Complexity owns path pressure; Debugging owns causal investigation; Testing owns test economics; Laziness and Sequencing own change size and order. Do not duplicate their full authority here.` },
  { title: "context", body: `## Prime directive

Spend active context on the current decision. Keep durable evidence retrievable outside it.

## Position

A context window is not a warehouse. Filling it with every available file, transcript, result, and explanation reduces the attention left for reasoning. Persisting bytes elsewhere does not restore attention already spent.

Context economy and evidence fidelity are equal duties. Saving space by dropping the source, qualifier, contradiction, uncertainty, or exact revision needed for correctness is not economy. It is evidence loss.

## Principles

- **Admit context for a reason.** Keep information active when it supports the current decision, carries authority, or makes required evidence findable. Do not retain material merely because it was available.
- **Discover before filtering.** Learn enough about instructions, ownership, topology, dependencies, and trust boundaries to know what matters. Selective reading without discovery is confident omission.
- **Isolate deliberately.** Move large payloads or independent investigations elsewhere only when the work is separable and the parent can decide from a faithful result. Delegation that duplicates grounding or hides decisive evidence saves nothing.
- **Summaries point back to evidence.** Preserve sources, revisions, confidence, contradictions, and unresolved questions. A summary is a map, not a replacement for the terrain.
- **Persist continuity before attention expires.** Durable state should let another agent recover the objective, scope, decisions, evidence, validation state, open questions, and next action without replaying everything.
- **Keep one authority.** Frequently needed guidance may stay close to the work, but duplicated content needs a canonical source or deterministic derivation. Convenience does not excuse drift.
- **Treat budgets as signals.** Token, file, turn, and delegation limits should trigger splitting, checkpointing, retrieval, or handoff. They never make incomplete evidence sufficient or unfinished work complete.
- **Isolation does not widen authority.** Moving content must not expand access, retention, exposure, or the power of untrusted evidence to become instruction.

## Boundary

Do not optimize context by deleting unique knowledge, skipping dependency discovery, or delegating material the decision-maker must inspect. The goal is not fewer tokens. It is enough trustworthy context to make the right decision, with everything else recoverable when needed.` },
  { title: "cyclomatic-complexity", body: `## Prime directive

Keep cyclomatic complexity at five or less. Do not exceed ten without explicit human approval.

## Position

Cyclomatic complexity counts independent control-flow paths through a routine. For a connected routine, it is roughly the number of decisions plus one, though exact treatment of cases, compound conditions, and exceptions depends on the measuring tool.

Each path adds another condition a maintainer must understand and another behavior the system may need to prove. Humans and agents may fail differently, but they share the same code. Do not grant agents a higher tolerance merely because they can enumerate more branches; hidden state, side effects, vague names, and dispersed context still obscure meaning.

## Principles

- **Use one shared standard.** Code should be easy for a human to understand and an agent to reason about. Optimize for their shared need: visible, cohesive behavior.
- **Target five or less.** A routine at or below five usually leaves enough room to understand its decisions without building a mental simulator.
- **Treat ten as the ceiling.** Complexity above ten requires an explicit human exception grounded in a cohesive domain rule, invariant, safety boundary, compatibility obligation, or other evidence that decomposition would make the code worse.
- **Investigate increases.** New paths should trigger a design conversation. Look for several decisions trapped in one routine, tangled policy and mechanism, or a missing domain concept.
- **Decompose meaningfully.** Split responsibilities and concepts. Moving branches into tiny forwarding functions lowers a score without reducing cognitive burden.
- **Protect semantic quality.** Never weaken correctness, cohesion, types, validation, diagnostics, security, accessibility, compatibility, or tests to satisfy the number.
- **Explain necessary complexity.** A justified exception remains visible and reviewable. Measurement is pressure toward clarity, not permission to stop thinking.

## Boundary

Cyclomatic complexity measures paths, not understanding. Use the number to expose risk and prompt better design. Never confuse a low score with good code or a high score with automatic failure.` },
  { title: "data-processing", body: `## Prime directive

Assume every durable computation will run again.

## Position

Batch jobs and stream processors live across time. They restart, fall behind, see duplicates, consume old and new records together, and revisit history after the world has changed. A pipeline that only works once, in order, at the present moment is not durable processing.

Replayability is not merely rerunning code. The same inputs, checkpoints, reference data, and external effects must have deliberate meaning when execution resumes or history is processed again.

## Principles

- **Name the unit of ordering.** Preserve order only where the business requires it, and state whether that scope is a record, entity, key, partition, stream, window, or whole history.
- **Separate facts from instructions and views.** Events record what happened. Commands request work. Streams carry records. Materialized views are derived copies. Each has different replay and ownership obligations.
- **Make restart a normal path.** Declare inputs, outputs, checkpoints, intermediate state, and the point at which a sink accepts work. A checkpoint that advances before its effects are durable creates invisible loss; one that advances afterward may create duplicates.
- **Distinguish time.** When something happened, when it arrived, and when it was processed are different facts. Windows, joins, and late arrivals must choose which time they mean.
- **Design for history.** Retention defines how far recovery and recomputation can reach. Processing must tolerate records written by older and newer versions while history remains mixed.
- **Make lag visible.** Falling behind is a system state, not a private implementation detail. Expose backlog, watermark, staleness, failed recovery, and the point beyond which results are incomplete.
- **Keep derived results rebuildable.** A projection, index, or aggregate needs a known source, replay path, and repair strategy. If it cannot be rebuilt, it is an authority and must be treated as one.
- **Protect effects outside the pipeline.** Reprocessing must not silently repeat irreversible actions. Stable operation identity and compensation belong to Idempotency doctrine; this doctrine ensures the processing path exposes where they are needed.

## Boundary

Data Processing owns computation across replay and time. Data owns storage, schema, and consistency contracts. Distributed Data owns cross-node coordination. Idempotency owns logical operation identity and recovery from duplicate effects.` },
  { title: "data", body: `## Prime directive

State what owns the truth and what every read and write is allowed to promise.

## Position

Stored data outlives the operation that produced it. A design must distinguish request acceptance, committed state, reader visibility, external effects, and survival through failure. Treating them as one event creates guarantees the system never actually made.

Storage choices follow the data’s relationships, access patterns, consistency needs, evolution pressure, and measured workload—not fashion or a diagram drawn before the evidence.

## Principles

- **Name the authority.** Identify the owning store, who may change it, which copies are derived, and what each consumer may assume about freshness and consistency.
- **Define durability and visibility.** State when a write survives restart, when readers can observe it, whether stale reads are allowed, and how conflicts are detected and settled.
- **Measure before reshaping.** Understand volume, read and write patterns, latency, throughput, contention, growth, and the slowest important paths before changing engines, indexes, or layouts.
- **Fit representation to access.** Model relationships and fields according to how they change and are queried. Select storage and indexes according to observed use and required guarantees.
- **Own every second copy.** Caches, indexes, projections, search data, warehouses, and denormalized fields need a source, update path, acceptable lag, visibility into drift, and a way to rebuild or repair them.
- **Evolve contracts while versions coexist.** Schemas, encodings, interfaces, messages, and stored records must survive old and new readers, writers, and data during rollout.
- **Use transactions for named invariants.** Define what commits atomically, which anomalies are unacceptable, and which isolation or concurrency control protects the invariant. Weaker guarantees require an explicit compensating mechanism.
- **Draw service boundaries around ownership.** Keep data that must remain tightly consistent under one authority. Avoid splitting a business concept merely to create another service.
- **Make operational state visible.** Expose stale copies, failed rebuilds, conflicting writes, migration state, and repair paths. Hidden inconsistency is not eventual consistency.

## Boundary

Data owns storage and single-store consistency. Data Processing owns replay and computation through time. Distributed Data owns coordination across nodes and stores. Idempotency owns logical retries and ambiguous effects. Domain owns business meaning; Sequencing owns migration order.` },
  { title: "debugging", body: `## Prime directive

The symptom is evidence. It is not automatically the defect.

## Position

A failure becomes understandable when an explanation accounts for what happened, why it became possible, and why the proposed change prevents it from happening again. Debugging is the work of earning that explanation.

Reproduce the failure when practical. When direct reproduction is impossible, use the strongest available observations and state the uncertainty honestly. A confident guess is not stronger than incomplete evidence.

## Principles

- **Seek the mechanism.** Distinguish what triggered the failure from the defect that made it possible, the conditions that helped it occur, and the missing containment that made it worse. These distinctions clarify thought; they are not ceremony.
- **Preserve evidence before changing state.** A restart, cache clear, retry, rollback, or reset may restore service while destroying the best clue. Recover urgently when needed, but do not mistake recovery for understanding.
- **Separate mitigation from correction.** Containment can stop damage before the cause is known. It remains a workaround until the mechanism is repaired.
- **Instrument instead of guessing.** Add observation when evidence is weak, but remember that instrumentation can change timing, cost, privacy, and behavior. Gather only what the investigation can justify.
- **Use guards to protect, not conceal.** A guard is sound when it enforces a contract, contains damage, exposes invalid state, or fails safely. It is dangerous when it converts a violated invariant into apparent success.
- **Generalize by cause, not appearance.** Similar text and similar symptoms do not prove the same defect. Broaden a repair only where the same mechanism and contract are established.
- **Prove more than disappearance.** A fix is complete when evidence supports the causal explanation, the regression is prevented or made observable, and adjacent behavior still holds.

## Boundary

Do not delay urgent containment while pursuing perfect certainty. Do not destroy evidence, hide degraded behavior, or call a cleared symptom a root-cause repair. Several causes may coexist; debugging should reduce uncertainty rather than force a tidy story.` },
  { title: "distributed-data", body: `## Prime directive

Distribution turns assumptions into failure modes. Name the guarantees and pay their costs deliberately.

## Position

A remote value is not a local variable. Messages arrive late or twice. Replicas disagree. Processes pause. Clocks drift. Owners lose authority while still running. Networks divide systems precisely when coordination matters most.

Distributed design begins by admitting these conditions. Availability, latency, consistency, durability, and agreement cannot all be maximized at once. The system must state which guarantees matter for each operation and what callers observe when those guarantees cannot be met.

## Principles

- **Name replica visibility.** State whether reads may be stale, move backward, see their own writes, or require a consistent prefix. Expose lag and define how divergent copies converge.
- **Partition for real access.** Choose boundaries from locality, consistency, and workload. Account for hot keys, skew, routing, secondary indexes, rebalancing, and operations that must cross partitions.
- **Treat failover as a state transition.** Define who may become authoritative, what happens to in-flight work, and how clients distinguish retryable failure from unknown completion.
- **Fence stale owners.** Expiration alone does not stop an old leader from writing. Leases, ownership epochs, and fencing must make superseded authority unable to corrupt current state.
- **State the fault model.** Design for delayed and dropped messages, partitions, duplicate delivery, process pauses, clock uncertainty, and conflicting writers according to the environment that actually exists.
- **Use coordination only for named invariants.** Locks, quorum, consensus, ordered broadcast, and atomic commitment impose availability and latency costs. Buy them only when participants truly must agree.
- **Make cross-boundary atomicity honest.** A transaction inside one store does not make several stores or external effects atomic. Define partial outcomes, reconciliation, compensation, or explicit refusal.
- **Keep failure visible.** Replica lag, lost leadership, blocked quorum, repair, and convergence are operational states. A distributed system that hides them cannot be trusted when degraded.

## Boundary

Distributed Data owns guarantees across nodes, stores, and partitions. Data owns storage and single-store transaction semantics. Data Processing owns replay over time. Idempotency owns logical operation identity and duplicate-effect recovery. Domain and Boundaries own business meaning and ownership seams.` },
  { title: "documentation", body: `## Position

Documentation should preserve knowledge, not cast a prose shadow over the code.

Source code and executable behavior are the authority for what an implementation currently does. A handwritten walkthrough of classes, functions, files, or control flow creates a second account of the same fact. The prose is not compiled with the code, so it drifts. When the two disagree, both humans and agents waste time deciding which story to trust.

Code cannot carry every truth. It rarely explains why an alternative lost, what a product intends, what a public contract promises, which words a domain owns, how an operator recovers a system, or which obligations require evidence. Preserve those truths in artifacts suited to them.

## Principles

- **One concern, one authority.** Name the artifact that owns each durable fact. Other representations are generated, validated, or clearly derived from it.
- **Let code explain behavior.** Make ownership, boundaries, contracts, and entry points discoverable in the implementation. Comments preserve intent, invariants, constraints, and surprising decisions; they do not narrate syntax.
- **Document what code cannot.** Product intent, architecture rationale, domain language, public contracts, user guidance, operations, migrations, and governance have legitimate homes outside implementation code.
- **Make every document earn its maintenance cost.** A permanent document needs a unique purpose, an owner or canonical source, and a credible way to stay current. Generate reference material from validated sources when practical.
- **Treat disagreement as drift.** Identify which concern owns the disputed fact, trust that authority, and repair the other artifact. Never average conflicting accounts or choose the convenient one.
- **Optimize retrieval, not document count.** Keep entry points small and navigable. Link to focused authority and load detail only when relevant. Deleting unique knowledge is not context economy.

## Boundary

“Self-explanatory code” never excuses missing rationale, public contracts, recovery procedures, user guidance, or required security, privacy, accessibility, licensing, compliance, and audit records.` },
  { title: "domain", body: `## Prime directive

Model the business, not the machinery surrounding it.

## Position

A domain model is the shared understanding of a business problem expressed in language and running code. Tables, screens, wire formats, frameworks, and diagrams may represent pieces of that understanding, but none of them becomes the model merely by existing.

The model grows through a repeated conversation between people who know the business and people changing the software. Concrete scenarios reveal missing concepts. Implementation exposes weak explanations. Refactoring gives important meaning a name and a home.

## Principles

- **Speak one local language.** Inside one bounded context, code, tests, written material, planning, and business conversation should use the same words for the same concepts.
- **Put business decisions in the model.** Presentation, storage, messaging, frameworks, and workflow coordination may carry a decision, but they should not secretly own it.
- **Choose building blocks by meaning.** Identity suggests an entity. Descriptive immutable attributes suggest a value object. A business operation with no natural object may be a domain service. Concepts understood together form a module.
- **Protect invariants at the right boundary.** Objects that must remain mutually consistent belong behind one aggregate root. Outside code refers to that root rather than reaching through it.
- **Create whole objects.** Construction should establish valid state before an object becomes reachable. Retrieval should hide storage mechanics and answer questions in business terms.
- **Make infrastructure adapt.** Persistence and transport preserve domain identity, value semantics, invariants, and retrieval needs. Their convenient shapes must not leak back into the model.
- **Design interfaces around purpose.** Name operations for business intent. Separate questions from state-changing commands. Make preconditions, postconditions, and invariant obligations visible.
- **Refactor toward understanding.** When a policy, calculation, constraint, process, or criterion carries business meaning, model it directly instead of leaving it buried in procedural branches.
- **Test in business language.** Prove legal construction, required invariants, allowed transitions, rejected transitions, and meaningful outcomes before testing supporting plumbing.
- **Defend the distinctive model.** Commodity mechanisms and reusable infrastructure should support the business model without crowding it out.

## Boundary

This doctrine owns meaning and behavior inside one bounded context. Boundaries doctrine owns where contexts begin, how their models relate, and how meaning crosses between them. Data doctrine owns storage guarantees; Testing doctrine owns test economics and mechanics.` },
  { title: "idempotency", body: `## Prime directive

Design every mutation so repetition and interruption have deliberate outcomes.

## Position

Retries happen after timeouts, crashes, lost responses, duplicate delivery, and uncertain completion. Treating them as exceptional leaves the most dangerous state transitions undefined.

Idempotency is one answer, not a synonym for recovery. Some operations can repeat with the same observable effect. Others must reconcile toward desired state, deduplicate effects, compensate, or stop for human recovery. The mechanism follows the guarantees the system can actually make.

## Principles

- **Define invariants before mutation.** Name the desired state, authoritative source, admissible starting states, ownership, commit boundary, and effects that escape it. Recovery cannot be clearer than the contract it recovers.
- **Preserve logical identity during retry.** Repeating one command keeps its original identity and intent. Recomputing from current authority is reconciliation, not retry. Confusing them defeats deduplication and changes what “same operation” means.
- **Classify durable state before acting.** After interruption, distinguish complete, partial, stale, conflicting, corrupt, and ambiguous state. Resume, repair, replace, or refuse according to evidence—not creation order or optimism.
- **Prove authority before writing or deleting.** Locks, leases, tokens, and ownership records matter only when they demonstrate who may act now. Never clean up state whose ownership or equivalence is uncertain.
- **Protect external effects separately.** A local transaction cannot promise that a remote effect happened once. Use only guarantees the complete path supports, and name when outcomes are duplicated, compensatable, manually reconcilable, or unknowable.
- **Prefer convergence over repeated hope.** A recovery loop should move classifiable state toward one intended outcome, stop on permanent failure, and refuse to spin forever through uncertainty.
- **Expose ambiguity.** Unknown completion, violated invariants, incompatible versions, or unprovable ownership are real states. Surface them rather than manufacturing a success-shaped answer.

## Boundary

Do not promise exactly-once behavior from systems that cannot provide it. Do not replace available atomicity with eventual repair, or availability with silent corruption. Safety and evidence outrank the appearance of self-healing progress.` },
  { title: "integration-testing", body: `## Prime directive

Use real boundaries where substitutes would hide the failure you need to prevent.

## Position

Integration tests prove that separately correct parts still honor their contracts when connected to real persistence, shared state, processes, protocols, or external systems. Their value comes from fidelity unavailable at lower scopes; their cost comes from slowness, coordination, environmental dependence, and cleanup.

Use the maximum necessary reality, not the maximum available reality. Every real dependency should protect an important behavior that a smaller test cannot prove.

## Principles

- **Cross the boundary deliberately.** Name which real dependency matters and which contract the test proves. Do not build an end-to-end environment merely because integration is involved.
- **Reload persisted truth.** Verify durable results through an independent read rather than trusting the objects or responses that performed the write.
- **Specify exact effects.** Assert the values sent across unmanaged boundaries and maintain a permitted-call baseline that rejects unexpected effects.
- **Preserve production fidelity.** Database and protocol behavior should match the production technology where its semantics affect correctness. A convenient substitute that behaves differently proves the wrong system.
- **Separate Arrange, Act, and Assert contexts.** Setup, operation, and independent observation may require different connections or processes so the assertion does not inherit hidden state from the action.
- **Own shared-state cleanup.** Preserve immutable reference data, clear scenario-owned state before each test, and serialize tests whose correctness depends on shared mutable resources. Teardown or rollback alone cannot protect the next run after interruption.
- **Combine expensive operations only when natural.** Consecutive actions may share one test when splitting them would create greater external cost and the combined failure remains diagnosable.
- **Prove safety before omitting an edge case.** Skip a real-boundary scenario only when lower-scope evidence shows the failure occurs before persistent mutation or external effect.
- **Use end-to-end tests for irreducible gaps.** Reserve the broadest scope for critical behavior that unit and focused integration tests cannot protect.

## Boundary

Integration Testing owns evidence across real boundaries. Testing owns value, scope, and economics. Test Seams owns substitution and collaborator observation. Data doctrines own the guarantees being tested; this doctrine owns proving that the assembled system actually provides them.` },
  { title: "laziness", body: `## Prime directive

Keep it simple. Keep it minimal. Don't add things you won't need, and don't modify things you don't need to.

## Position

Code is cheap for an agent to write and expensive for a human to inherit. Seek the most useful complete result with the least code, indirection, and coordination burden that can safely deliver it. Think like a tired maintainer.

Laziness means refusing unnecessary work. It never means skipping investigation, correctness, validation, security, accessibility, compatibility, diagnostics, or proof.

## Rules

- **Delete before adding.** First look for redundant behavior, obsolete paths, purposeless pass-throughs, duplicated choices, and representation leaks that evidence shows can be removed.
- **Make the smallest complete change.** Solve the root problem and every directly affected contract, error path, test, migration, documentation surface, and safeguard. Authorized scope always wins over cleanup opportunity. Report unrelated cleanup separately.
- **Keep the cognitive path flat.** Maintainers should find ownership and follow behavior without crossing layers that only forward, rename, or translate. More than three files or layers is a warning to inspect for accidental indirection, not an automatic violation. A rich interface that hides substantial work is not inherently a deep call chain.
- **Question long data paths.** When a signal crosses several functions, types, schemas, pipelines, or services, compare that design with a direct route. Preserve ownership, validation, type safety, authorization, observability, consistency, lifecycle semantics, and compatibility.
- **Make structure earn its cost.** Keep a layer, abstraction, wrapper, schema, or generalized mechanism when it enforces an invariant, owns a responsibility, isolates change, stabilizes a contract, exposes failure, preserves compatibility, or improves safe testing. Speculative flexibility is not evidence.
- **Optimize maintenance, not line count.** Smaller diffs and fewer lines are useful evidence, never the definition of simplicity.

## Boundaries

Do not flatten architecture by mixing responsibilities, bypassing trust boundaries, weakening types, introducing hidden coupling, or deleting safeguards. Existing adjacent leaks stay outside scope unless the authorized result requires their removal. Generated or framework-mandated structure may be necessary.` },
  { title: "machine", body: `## Prime directive

Before repeating non-trivial work, compare careful manual execution with the smallest trustworthy machine that could perform or prove it.

## Position

Repetition invites inconsistency, forgotten steps, weak evidence, and expensive review. A small script, generator, query, codemod, or deterministic check can turn private effort into something replayable and inspectable.

Automation is not automatically economical. Building the lever has a cost. So do validating it, maintaining it, explaining it, and eventually removing it. Choose the lever only when its total lifecycle cost and risk are lower than doing the work carefully by hand.

## Principles

- **Build the smallest trustworthy lever.** Automate only the stable mechanical core. Leave uncertain work exploratory until its shape is understood.
- **Rerunnable is not correct.** Determinism can reproduce the same mistake perfectly. Validate the lever against independent requirements, tests, or evidence rather than trusting its first output.
- **Bound the blast radius.** A machine should know what it may touch, what it must preserve, when it should stop, and what evidence it leaves behind. Broad mutation without clear scope is merely fast damage.
- **Automate mechanics; delegate judgment.** Do not fan out humans or agents to repeat what one checked tool can do. Use independent minds for review, domain judgment, and challenges the machine cannot settle.
- **Make authority visible.** Generated and derived artifacts need one named source of truth. A machine must not create a second owner for the same fact.
- **Choose a lifecycle deliberately.** Some levers belong in the repository. Others should be temporary and discarded after verified use. Artifact creation is not evidence that permanent ownership is worthwhile.
- **Let laziness govern.** Tool-building is over-engineering when the tool costs more to construct, trust, and maintain than the work it saves.

## Boundary

Urgent, one-off, low-confidence, or genuinely judgment-heavy work may be safer by hand. Machines amplify both discipline and error; use them where replayability improves trust, not where automation merely looks systematic.` },
  { title: "nimble", body: `## Prime directive

Optimize for uninterrupted safe progress, not uninterrupted activity.

## Position

Human attention is scarce and asynchronous. Spending it on routine permission prompts slows work without improving judgment. An agent should continue through bounded work when scope, authority, evidence, and recovery are already established.

Autonomy is not inferred from confidence, technical reversibility, or the ability to restore bytes. It comes from delegated authority combined with consequences that remain bounded, reviewable, and economical to correct.

## Principles

- **Continue within established authority.** Make routine tactical choices when requirements and boundaries already determine the acceptable space. Return evidence and assumptions at a useful checkpoint rather than interrupting each step.
- **Notify without pretending notification is approval.** Already-authorized visible work may be surfaced afterward only while review can still change course cheaply and dependent work has not hardened the choice.
- **Ask when judgment matters.** Stop before product direction, architecture, accepted risk, shared authority, or materially different interpretations. A human decision is valuable when the available paths produce meaningfully different consequences.
- **Measure consequence, not theoretical reversibility.** Blast radius, sensitivity, disruption, cost, recovery burden, and contamination of dependent work matter more than whether an action can technically be undone.
- **Preserve economical correction.** Work in bounded increments. Leave decisions, evidence, validation, and recovery visible enough that asynchronous review remains real rather than ceremonial.
- **Stop the line when harm compounds.** Do not defer a known correctness, security, privacy, data-integrity, or authority failure when continued work would spread or depend on it.
- **Do not manufacture questions.** Uncertainty inside delegated tactical scope is work to resolve. Uncertainty about authority or materially different outcomes belongs to the human.

## Boundary

Nimble does not mean silent, careless, permissionless, or always running. It never bypasses an existing human gate. The aim is to spend human judgment where it changes the outcome and let authorized execution proceed everywhere else.` },
  { title: "pragmatic", body: `## Prime directive

Answer for the result, not for having followed a ritual.

## Position

Engineering happens under incomplete information, changing conditions, and limited time. A responsible engineer chooses methods according to the actual users, risks, evidence, and codebase instead of repeating a practice because it is familiar.

Calibration never means lowering a binding standard. Requirements, doctrine, safety, and explicit human decisions remain constraints. Pragmatism decides how best to satisfy them.

## Principles

- **Own the outcome.** State tradeoffs, risks, unknowns, and avoidable future costs. Tools, inherited design, and schedule pressure explain conditions; they do not become responsible for the result.
- **Calibrate effort to consequence.** Spend process, proof, precision, and time where they improve this outcome. Reject ceremony that produces no useful signal.
- **Price the future inside the scope.** A cheap edit that makes every later change harder is usually expensive. Within the authorized implementation scope, improve directly related surroundings when the improvement is small, low-risk, and cheaper now than later.
- **Preserve reversibility while evidence is weak.** Avoid welding uncertain vendors, platforms, environments, policies, or requirements into the design before the decision earns that commitment.
- **Charge shared state honestly.** Globals, ambient context, mutable shared data, ordering, locks, and asynchronous behavior impose coordination costs. Make ownership, synchronization, cleanup, and failure visible.
- **Excavate the requirement.** Separate durable needs and constraints from today’s implementation detail, proposed solutions disguised as needs, and disagreements nobody has stated yet.
- **Classify failure before handling it.** Expected domain failure, violated contract, impossible state, transient fault, recoverable damage, and permanent failure require different responses. Preserve diagnostic context and place recovery where it can make sense of the failure.
- **Own what you acquire.** Memory, handles, locks, temporary state, and external effects create cleanup obligations across success and failure paths.
- **Make uncertainty visible.** Estimates and plans are provisional. Use early evidence and feedback to correct them rather than defending expired confidence.
- **Keep accountability shared.** Teams should make expectations, quality, remaining risk, and the evidence behind completion visible enough that everyone can stand behind the work.

## Boundary

Pragmatism is not permission to bypass doctrine, weaken proof, or accept hidden debt. It is disciplined judgment about how to reach the required outcome under real conditions.` },
  { title: "scout", body: `## Prime directive

When a consequential decision is genuinely uncertain, scout the terrain before committing to a route.

## Position

The first plausible path is rarely proof that no better path exists. Familiarity, implementation momentum, and a polished prototype can make one direction feel inevitable before its consequences or alternatives are understood.

Discovery should reveal the terrain: viable routes, hidden constraints, likely consequences, and uncertainty that matters. A scout does not choose the destination or build the road. The scout returns evidence so the accountable human can choose deliberately.

Scouting is valuable when a decision is novel, expensive to reverse, disputed by informed people, or likely to create a durable product or architectural commitment. Mechanical work and settled decisions need no ceremonial expedition.

## Principles

- **Name the decision before scouting it.** Separate hard constraints from assumptions, and distinguish what must remain true from what may be challenged.
- **Seek meaningfully different routes.** Different names, layouts, or implementations of the same underlying choice are one option. Alternatives matter when they expose distinct hypotheses or consequences.
- **Set judgment before attachment.** Decide which constraints are non-negotiable and which qualities distinguish better outcomes before a favorite solution gathers momentum.
- **Buy the cheapest useful evidence.** Use thought, research, sketches, prototypes, benchmarks, or experiments according to the uncertainty being reduced. The artifact is disposable; the knowledge is the product.
- **Scale the expedition to the terrain.** More uncertainty, impact, and irreversibility justify more breadth and evidence. Strong constraints, low consequence, or cheap reversibility justify less.
- **Return when information stops paying.** Commit when evidence distinguishes a route, hard constraints leave one viable path, remaining uncertainty is acceptable, or further scouting has little expected value.
- **Keep selection human-owned.** Evidence sharpens product and architecture judgment. It does not acquire authority to choose the route.

## Boundary

Do not scout to satisfy a count, present cosmetic variants as choice, or let experimental code quietly become production code. Scouting should prevent premature commitment, not postpone commitment indefinitely.` },
  { title: "sequencing", body: `## Prime directive

Order work so each coherent step makes a claim, proves it, and leaves the next step safer.

## Position

Sequence is part of correctness. The same changes performed in a careless order can hide causes, create unsafe intermediate states, invalidate evidence, and make review harder than the work itself.

The goal is not the smallest possible step. It is the smallest coherent step: enough change to produce a meaningful state, little enough change to localize failure and understand why progress is justified.

## Principles

- **Subtract before adding.** Remove obsolete paths, duplicated choices, and accidental complexity before building new behavior on top of them. A smaller foundation makes later evidence clearer.
- **Pair claims with evidence.** Each step should say what became true, how that claim was tested, and what must be true before dependent work begins.
- **Separate execution from delivery.** Execution order helps locate failure and maintain valid states. Delivery order helps another person understand the problem, transformation, and proof. One sequence may serve both, but they are not the same concern.
- **Preserve coherent states.** Do not split generated output, migrations, protocols, or tightly coupled behavior merely to produce smaller artifacts. A step that cannot stand meaningfully is not small; it is incomplete.
- **Batch only what fails together.** Homogeneous mechanical work may move as one bounded unit when splitting adds cost without improving fault isolation. Semantic differences deserve separate proof.
- **Treat evidence as revision-bound.** Rebases, regenerated output, changed dependencies, or later edits can invalidate earlier confidence. Revalidate the state that will actually continue or land.
- **Distinguish expected evidence from unexplained failure.** A deliberate failing observation can prove an absence or defect. An unexpected red state is not progress and must not become a foundation.
- **Prove the assembled result.** Focused checks localize confidence. They do not replace validation of the complete system after the pieces meet.
- **Build an argument, not an artifact count.** More commits, phases, or change requests are useful only when they clarify dependency, risk, or proof. Fragmentation without information is ceremony.

## Boundary

Sequencing must not invent dependencies, preserve intentionally broken landing states, or demand costly validation after every trivial edit. Order work according to risk and evidence, not ritual.` },
  { title: "solid", body: `## Prime directive

Preserve trustworthy behavior while making change local, deliberate, and unsurprising.

## Position

Spaghetti code is not merely code with many lines. It is code where responsibilities overlap, changes ripple unpredictably, contracts lie, clients depend on things they do not use, and important policy is trapped inside volatile mechanisms.

SOLID provides five equal lenses for finding those pressures. They are not rituals and do not require an interface, subclass, or abstraction everywhere. Apply them where they reduce change cost and clarify ownership.

## Principles

- **Single Responsibility Principle.** Give a module, component, or class one coherent source of change. Keep behavior together when it changes for the same business reason; separate behavior when different owners, policies, or timelines pull it apart.
- **Open/Closed Principle.** Protect stable behavior behind a deliberate extension boundary when real variation exists. Add a new case without repeatedly rewriting trusted logic, but do not predict hypothetical extensions or preserve obsolete paths.
- **Liskov Substitution Principle.** An interchangeable implementation must preserve the behavioral expectations of the contract it claims to satisfy. Inputs, outputs, invariants, side effects, and failure behavior matter more than matching a type signature.
- **Interface Segregation Principle.** Give clients focused contracts containing what they actually need. Do not force consumers to understand, implement, mock, or depend on unrelated capabilities. Small interfaces are valuable when they represent cohesive client needs, not when fragmentation creates forwarding ceremony.
- **Dependency Inversion Principle.** Important policy should define the stable contracts it needs instead of depending directly on volatile mechanisms. Infrastructure may implement those contracts, but an abstraction must express real policy and variation rather than hide one concrete dependency behind another name.

## Working together

The principles reinforce one another: coherent responsibility reveals the right contract; focused contracts make substitution honest; honest substitution enables safe extension; dependency direction keeps policy stable while mechanisms change.

## Boundary

SOLID does not guarantee scalability, reuse, testability, or maintainability. It supplies questions for design judgment. Code, Domain, Boundaries, Laziness, and Cyclomatic Complexity remain authoritative for their concerns. Use SOLID to reduce coupling—not to manufacture layers.` },
  { title: "tactical-strategic", body: `## Prime directive

Agents execute tactics. Humans own strategy.

## Position

Agents are exceptionally effective at bounded programming work. Within an explicit objective and scope, they can implement, validate, refine, investigate, and correct faster than a human should need to supervise step by step.

That tactical strength does not create strategic authority. Humans decide where the capability should be applied, why the work matters, how it fits the larger system, and which consequences are acceptable.

## Principles

- **Delegate an objective and a boundary.** Tactical authority begins with a named outcome, constraints, and scope. It does not expand because adjacent work appears useful or technically possible.
- **Trust execution inside the boundary.** Agents should resolve ordinary implementation choices, gather evidence, validate results, and refine the work without returning every local decision to a human.
- **Keep strategy human-owned.** Product direction, architecture, priorities, system boundaries, enduring tradeoffs, accepted risk, and the shape of the delegation remain human responsibilities.
- **Escalate changes of meaning.** A choice becomes strategic when it changes the objective, crosses a boundary, commits the system to a durable direction, or requires context and authority not contained in the delegation.
- **Make authority visible.** Workflows should reveal who may decide, what the agent may change, which evidence supports continuation, and where human judgment resumes.
- **Use Nimble inside the delegation.** This doctrine answers who owns the decision. Nimble answers whether authorized tactical work should continue, notify, ask, or stop as consequences emerge.
- **Preserve the next layer.** Strategic stewardship means leaving a system that can continue evolving. Tactical success is incomplete when it quietly makes future direction harder to change.

## Boundary

Human ownership does not require humans to type the solution or approve every implementation detail. Agent autonomy does not grant authority over purpose. The partnership works when humans choose the direction and agents are trusted to make disciplined progress within it.` },
  { title: "test-seams", body: `## Prime directive

Test through a boundary that reveals behavior and hides irrelevant implementation detail.

## Position

A test seam is where controlled input enters and observable behavior leaves. A good seam makes failures meaningful and refactoring cheap. A bad seam forces tests to know private structure, reproduce production logic, or orchestrate collaborators the system itself has not separated cleanly.

Isolation is not the goal. Trustworthy evidence is. Use real collaborators when they are fast, deterministic, and owned by the application. Substitute dependencies when crossing them would make the test slow, unstable, destructive, unavailable, or unable to exercise required failure behavior.

## Principles

- **Prefer outcomes over conversations.** Verify returned output first, then observable state, then collaborator communication only when the communication itself is the contract.
- **Test behavior, not construction.** Do not freeze call order, private methods, object layout, or incidental collaborator counts unless the public contract makes them meaningful.
- **Choose doubles by role.** Stubs provide answers. Mocks verify expected commands. Spies record interaction for later assertions. A substitute should model only the contract the test needs.
- **Respect command and query semantics.** Queries return information without changing observable state. Commands perform effects. Verification should match that distinction.
- **Know the application edge.** Managed dependencies under application control can often remain real. Unmanaged systems, shared state, clocks, networks, and irreversible effects need an explicit seam.
- **Treat mock pain as design feedback.** Large setup graphs and fragile interaction assertions often reveal mixed responsibilities, hidden effects, or contracts drawn at the wrong level.
- **Move decisions inward and effects outward.** Represent external state as values, decide using explicit inputs, return intended effects, and apply them at an outer boundary when that makes behavior easier to prove.
- **Reject test-only architecture.** Avoid partial mocks, production switches used only by tests, ambient time, wrappers created solely for mocking, and exposing private members merely to assert them.
- **Allow controlled exceptions.** Reflection or a narrow adapter may be justified for a non-public external contract when no safer observable seam exists. Keep the exception explicit.

## Boundary

Test Seams owns substitution and observation choices. Testing owns value, economics, and test scope. Integration Testing owns evidence across real persistence, shared state, and external boundaries. SOLID and Code own the production design those seams reveal.` },
  { title: "testing", body: `## Prime directive

Test behavior worth protecting at the smallest scope that can provide trustworthy evidence.

## Position

A test earns its lifetime cost by detecting a meaningful regression, resisting harmless refactoring, failing close to the cause, and remaining understandable enough to maintain. Test count and coverage percentage are evidence about a suite, not measures of its value.

The unit under test is a behavior, not a class or line. Choose scope according to the evidence required: a unit test protects a small fast behavior without shared-state dependence; an integration test crosses a meaningful real boundary; an end-to-end test protects a critical gap no smaller scope can prove.

## Principles

- **Protect observable behavior.** Write expectations from requirements, client goals, and independent domain knowledge. Use structural knowledge to find gaps, not as the oracle.
- **Keep the oracle independent.** Do not calculate expected results with the production logic being tested or infer persistence from the objects that performed the write.
- **Prefer the smallest sufficient scope.** Broad tests cost more and localize less. Use them when lower scopes cannot faithfully exercise the contract.
- **Test one meaningful behavior.** Several assertions may prove one outcome, but multiple Acts and branches usually hide several scenarios inside one test.
- **Make Arrange, Act, and Assert visible.** Setup should expose the facts that matter, invoke the behavior once, and verify relevant explicit and implicit outcomes.
- **Use Test-Driven Development at the right time.** Write the failing behavior before the implementation exists, implement minimally, then refactor while the tests remain green. Tests written after behavior exists are regression work, not retroactive Test-Driven Development.
- **Balance four qualities.** Judge regression protection, refactoring resistance, feedback speed, and maintainability together. A test with no meaningful protection has no value however fast or tidy it is.
- **Treat the pyramid as context, not quota.** Prefer many fast focused tests, fewer real-boundary tests, and the fewest end-to-end tests only when that shape matches the system’s risks.
- **Keep setup explicit.** Use focused factories with meaningful parameters when setup repeats. Keep scenario facts in the test and split parameterized cases once their differences become opaque.
- **Use fixtures deliberately.** Shared fixtures help when they preserve clear stable context; they harm when they hide facts, couple unrelated tests, or make failures depend on execution order.
- **Listen to failures.** Repeatedly ignored, retried, disabled, or flaky tests signal weak evidence or uncontrolled dependencies. Repair the cause rather than normalizing distrust.

## Boundary

Testing owns value, behavior, scope, and economics. Test Seams owns doubles, substitution, and collaborator observation. Integration Testing owns persistence, shared-state, and external-boundary evidence. Domain and Data doctrines own the behavior and guarantees being proved.` },
  { title: "worktrees", body: `## Prime directive

Every pull request starts from an isolated, owned workspace. Documentation and
specification changes deserve the same protection as product code.

## Position

A branch name separates history, not concurrent working state. Agents sharing
a checkout also share its index, uncommitted changes, and accidental consequences.
Isolation makes ownership visible and keeps one delivery from rewriting another.

An existing appropriate worktree is already useful isolation. Creating another
one by ritual adds confusion rather than safety.

## Principles

- **Inspect before creating.** Establish the repository, worktree, branch, base,
  and current owner. Account for submodules and harness-managed workspaces.
  Do not infer isolation from a directory name or a single Git signal.
- **Reuse only compatible ownership.** Reuse an isolated workspace for the same
  delivery when no competing writer owns it. Do not share a mutable checkout
  between independent writing agents or repurpose the default-branch workspace.
- **Respect the environment.** Prefer an available harness-native mechanism
  when it supplies the needed isolation; use Git worktrees otherwise. Honor
  approved placement and verify the actual destination is excluded from source
  control when it lives inside the repository.
- **Keep failure visible.** Failure to establish isolation is a reason to seek
  direction, not permission to work in the user's main checkout. A fresh
  directory is not proof of a clean baseline or working dependencies.
- **Preserve custody through delivery.** Record the workspace owner and starting
  state. Serialize integration, retain unrelated changes, and keep the delivery
  worktree available while its pull request needs maintenance.
- **Clean up only what is finished and owned.** Confirm integration, absence of
  active writers, and preservation of uncommitted work before removing a
  run-owned worker worktree.

## Boundary

Isolation does not authorize dependency installation, ignore-file edits,
commits, history rewriting, publication, or merging. The calling workflow owns
those permissions and the human owns exceptions. Do not turn cleanup into
deleting another person's work.` },
];
